//! Warm render server: one process, projects and decoders stay loaded between jobs.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use effectcraft_engine::Session;
use effectcraft_export::PixFmt;
use serde_json::{Value, json};

use crate::gpu_slot::GpuSlot;
use crate::stream::{self, StreamRequest};
use crate::{Args, Failure};

const MAX_LINE: usize = 4 * 1024 * 1024;
const DEFAULT_PORT: u16 = 9878;
const DEFAULT_IDLE: f64 = 120.0;

struct Server {
    session: Mutex<Session>,
    gpu: GpuSlot,
    cancel: AtomicBool,
    rendering: AtomicBool,
    want_gpu: bool,
    last_activity: Mutex<Instant>,
}

pub fn run(args: &Args) -> Result<(), Failure> {
    let port: u16 = match args.opt("--control") {
        Some(p) => p.parse().map_err(|_| Failure::Usage("--control: not a port number".into()))?,
        None => DEFAULT_PORT,
    };
    let idle = match args.num("--idle-exit")? {
        Some(s) => s.max(0.0),
        None => DEFAULT_IDLE,
    };
    let mut session = crate::session_cpu()?;
    if let Some(p) = &args.project {
        session.execute("file.open", json!({ "path": p })).map_err(|e| Failure::Error(e.to_string()))?;
    }
    let mut events = |v: Value| crate::progress::emit(&v);
    let gpu = GpuSlot::attach(&mut session, args.flag("--gpu"), &mut events);
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| Failure::Error(format!("cannot bind 127.0.0.1:{port}: {e}")))?;
    listener.set_nonblocking(true).map_err(|e| Failure::Error(e.to_string()))?;
    let bound = listener.local_addr().map_err(|e| Failure::Error(e.to_string()))?;
    crate::progress::emit(&json!({
        "event": "listening",
        "host": "127.0.0.1",
        "port": bound.port(),
        "pid": std::process::id(),
        "gpu": args.flag("--gpu"),
        "idleExit": idle,
    }));
    let server = Arc::new(Server {
        session: Mutex::new(session),
        gpu,
        cancel: AtomicBool::new(false),
        rendering: AtomicBool::new(false),
        want_gpu: args.flag("--gpu"),
        last_activity: Mutex::new(Instant::now()),
    });
    let stop = Arc::new(AtomicBool::new(false));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                touch(&server);
                stream.set_nodelay(true).ok();
                let server = Arc::clone(&server);
                let stop = Arc::clone(&stop);
                match std::thread::Builder::new().name("ec-serve".into()).spawn(move || {
                    if let Err(e) = handle_conn(&server, stream, &stop) {
                        crate::progress::emit(&json!({"event":"error","message": e}));
                    }
                    touch(&server);
                }) {
                    Ok(_) => {}
                    Err(e) => crate::progress::emit(&json!({"event":"error","message": format!("cannot spawn connection thread: {e}")})),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if idle > 0.0 && !server.rendering.load(Ordering::Relaxed) {
                    let last = lock(&server.last_activity);
                    if last.elapsed() >= Duration::from_secs_f64(idle) {
                        crate::progress::emit(&json!({"event":"idle-exit","seconds": idle}));
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Failure::Error(format!("accept: {e}"))),
        }
    }
    Ok(())
}

fn touch(s: &Server) {
    *lock(&s.last_activity) = Instant::now();
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn handle_conn(server: &Arc<Server>, stream: TcpStream, stop: &Arc<AtomicBool>) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let n = match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                touch(server);
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        if n > MAX_LINE || line.len() > MAX_LINE {
            let _ = writeln!(writer, "{}", json!({"ok": false, "error": "line too long, closing the connection"}));
            return Err("line too long".into());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                let _ = writeln!(writer, "{}", json!({"ok": false, "error": "not JSON, closing the connection"}));
                return Err("not JSON".into());
            }
        };
        let Some(method) = req.get("method").and_then(Value::as_str) else {
            let _ = writeln!(writer, "{}", json!({"ok": false, "error": "missing method, closing the connection"}));
            return Err("missing method".into());
        };
        let id = req.get("id").cloned();
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
        touch(server);
        match dispatch(server, method, &params, stop, &mut writer) {
            Ok(result) => {
                let mut reply = json!({"ok": true, "result": result});
                if let Some(id) = id {
                    reply["id"] = id;
                }
                write_line(&mut writer, &reply)?;
            }
            Err(e) => {
                let mut reply = json!({"ok": false, "error": e});
                if let Some(id) = id {
                    reply["id"] = id;
                }
                write_line(&mut writer, &reply)?;
            }
        }
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
    }
}

fn write_line(w: &mut TcpStream, v: &Value) -> Result<(), String> {
    writeln!(w, "{v}").map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

fn dispatch(server: &Arc<Server>, method: &str, params: &Value, stop: &Arc<AtomicBool>, writer: &mut TcpStream) -> Result<Value, String> {
    match method {
        "hello" | "server.info" => {
            let s = lock(&server.session);
            Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "gpuRequested": server.want_gpu,
                "gpu": server.gpu.used(&s),
                "project": s.path,
                "rendering": server.rendering.load(Ordering::Relaxed),
            }))
        }
        "project.open" => {
            let path = params.get("path").and_then(Value::as_str).ok_or("project.open needs path")?;
            let mut s = lock(&server.session);
            if s.path.as_deref() == Some(path) {
                return Ok(json!({"path": path, "reopened": false}));
            }
            s.execute("file.open", json!({ "path": path })).map_err(|e| e.to_string())?;
            Ok(json!({"path": path, "reopened": true}))
        }
        "project.close" => {
            let mut s = lock(&server.session);
            s.execute("file.newProject", json!({})).map_err(|e| e.to_string())?;
            Ok(json!({"ok": true}))
        }
        "render.cancel" => {
            server.cancel.store(true, Ordering::Relaxed);
            Ok(json!({"cancelling": server.rendering.load(Ordering::Relaxed)}))
        }
        "render.status" => {
            let s = lock(&server.session);
            Ok(json!({
                "rendering": server.rendering.load(Ordering::Relaxed),
                "project": s.path,
                "gpu": server.gpu.used(&s),
            }))
        }
        "app.quit" | "server.shutdown" => {
            server.cancel.store(true, Ordering::Relaxed);
            stop.store(true, Ordering::Relaxed);
            Ok(json!({"bye": true}))
        }
        "render.start" => render_start(server, params, writer),
        other => Err(format!("unknown method `{other}`")),
    }
}

fn render_start(server: &Arc<Server>, params: &Value, writer: &mut TcpStream) -> Result<Value, String> {
    if server.rendering.swap(true, Ordering::SeqCst) {
        return Err("a render is already running".into());
    }
    server.cancel.store(false, Ordering::Relaxed);
    let result = (|| {
        let req = request_from_params(params)?;
        let mut writer_events = writer.try_clone().map_err(|e| e.to_string())?;
        let mut s = lock(&server.session);
        if let Some(path) = params.get("project").and_then(Value::as_str)
            && s.path.as_deref() != Some(path)
        {
            s.execute("file.open", json!({ "path": path })).map_err(|e| e.to_string())?;
        }
        let cancel = &server.cancel;
        let gpu = &server.gpu;
        let mut events = |v: Value| {
            crate::progress::emit(&v);
            let _ = writeln!(writer_events, "{v}");
            let _ = writer_events.flush();
        };
        stream::run(&mut s, &req, gpu, cancel, &mut events).map_err(|e| match e {
            Failure::Usage(m) | Failure::Error(m) => m,
        })
    })();
    server.rendering.store(false, Ordering::Relaxed);
    result
}

fn request_from_params(p: &Value) -> Result<StreamRequest, String> {
    let pix = p.get("pixFmt").or_else(|| p.get("pix_fmt")).or_else(|| p.get("format")).and_then(Value::as_str).unwrap_or("yuv420p");
    let pix_fmt = PixFmt::parse(pix).ok_or_else(|| format!("pixFmt: yuv420p|rgb24|rgba (got {pix})"))?;
    let out = p.get("out").or_else(|| p.get("output")).and_then(Value::as_str).unwrap_or("-").to_string();
    let audio_only = p.get("audioOnly").or_else(|| p.get("audio_only")).and_then(Value::as_bool).unwrap_or(false) || pix.eq_ignore_ascii_case("wav");
    let resolution = match p.get("resolution").or_else(|| p.get("scale")) {
        Some(Value::String(s)) if s == "full" => 1.0,
        Some(Value::String(s)) if s == "half" => 0.5,
        Some(Value::String(s)) if s == "third" => 1.0 / 3.0,
        Some(Value::String(s)) if s == "quarter" => 0.25,
        Some(v) => v.as_f64().unwrap_or(1.0),
        None => 1.0,
    };
    Ok(StreamRequest {
        comp: p.get("comp").and_then(Value::as_str).map(str::to_string),
        start: p.get("start").and_then(Value::as_f64),
        end: p.get("end").and_then(Value::as_f64),
        work_area: p.get("workArea").or_else(|| p.get("work_area")).and_then(Value::as_bool).unwrap_or(false),
        fps: p.get("fps").or_else(|| p.get("frameRate")).and_then(Value::as_f64),
        resolution,
        draft: p.get("quality").and_then(Value::as_str).is_some_and(|q| q.eq_ignore_ascii_case("draft")),
        pix_fmt,
        out,
        audio_out: p.get("audioOut").or_else(|| p.get("audio_out")).and_then(Value::as_str).map(str::to_string),
        sidecar: p.get("sidecar").and_then(Value::as_str).map(str::to_string),
        audio_only,
    })
}
