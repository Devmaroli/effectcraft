//! Streaming render: raw frames to stdout / a pipe / a file, optional WAV, JSON progress.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use effectcraft_engine::Session;
use effectcraft_export::render_queue::{AudioOutput, Channels, OutputModule, RenderQuality, RenderSettings, TimeSpan};
use effectcraft_export::{Job, PixFmt, export_raw, export_wav, stream_info};
use effectcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};

use crate::gpu_slot::GpuSlot;
use crate::progress::{self, header_json};
use crate::{Args, Failure};

pub fn is_stream_request(args: &Args) -> bool {
    if args.flag("--audio-only") {
        return true;
    }
    if args.opt("--out") == Some("-") {
        match args.opt("--format") {
            None | Some("raw") | Some("yuv420p") | Some("yuv420") | Some("rgb24") | Some("rgba") | Some("wav") | Some("wave") => return true,
            _ => {}
        }
    }
    let Some(fmt) = args.opt("--format") else {
        return false;
    };
    PixFmt::parse(fmt).is_some()
}

pub fn pix_fmt_from_args(args: &Args) -> Result<PixFmt, Failure> {
    if let Some(p) = args.opt("--pix-fmt") {
        return PixFmt::parse(p).ok_or_else(|| Failure::Usage("--pix-fmt: yuv420p|rgb24|rgba".into()));
    }
    match args.opt("--format") {
        Some("wav" | "wave" | "aiff") => Ok(PixFmt::Yuv420p),
        Some(f) => PixFmt::parse(f).ok_or_else(|| Failure::Usage(format!("unknown stream format `{f}`"))),
        None => Ok(PixFmt::Yuv420p),
    }
}

/// One-shot `render` streaming path (keeps `--format prores` on the Render Queue).
pub fn cli_render(args: &Args, json_out: bool) -> Result<(), Failure> {
    let mut s = crate::session_cpu()?;
    match &args.project {
        Some(p) => s.execute("file.open", json!({ "path": p })).map_err(|e| Failure::Error(e.to_string()))?,
        None => s.execute("file.openDemoProject", json!({})).map_err(|e| Failure::Error(e.to_string()))?,
    };
    let cancel = AtomicBool::new(false);
    let mut events = |v: Value| progress::emit(&v);
    let slot = GpuSlot::attach(&mut s, args.flag("--gpu"), &mut events);
    let req = request_from_args(args)?;
    let result = run(&mut s, &req, &slot, &cancel, &mut events)?;
    if json_out {
        crate::emit(&result, true);
    }
    Ok(())
}

pub struct StreamRequest {
    pub comp: Option<String>,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub work_area: bool,
    pub fps: Option<f64>,
    pub resolution: f64,
    pub draft: bool,
    pub pix_fmt: PixFmt,
    pub out: String,
    pub audio_out: Option<String>,
    pub sidecar: Option<String>,
    pub audio_only: bool,
}

pub fn request_from_args(args: &Args) -> Result<StreamRequest, Failure> {
    let pix_fmt = pix_fmt_from_args(args)?;
    let audio_only = args.flag("--audio-only") || args.opt("--format").is_some_and(|f| matches!(f, "wav" | "wave"));
    let out = args.opt("--out").map(str::to_string).ok_or_else(|| Failure::Usage("render: --out FILE or --out - is required".into()))?;
    let audio_out = args.opt("--audio-out").map(str::to_string);
    if !audio_only && out == "-" && audio_out.as_deref() == Some("-") {
        return Err(Failure::Usage("video and audio cannot both go to stdout; pass --audio-out FILE".into()));
    }
    let resolution = match args.opt("--resolution").or(args.opt("--scale")) {
        Some("full") | None => 1.0,
        Some("half") => 0.5,
        Some("third") => 1.0 / 3.0,
        Some("quarter") => 0.25,
        Some(s) => s.parse::<f64>().map_err(|_| Failure::Usage(format!("--resolution: not a number: {s}")))?,
    };
    Ok(StreamRequest {
        comp: args.opt("--comp").map(str::to_string),
        start: args.num("--start")?,
        end: args.num("--end")?,
        work_area: args.flag("--work-area"),
        fps: args.num("--fps")?,
        resolution,
        draft: args.opt("--quality").is_some_and(|q| q.eq_ignore_ascii_case("draft")),
        pix_fmt,
        out,
        audio_out,
        sidecar: args.opt("--sidecar").map(str::to_string),
        audio_only,
    })
}

pub fn run(s: &mut Session, req: &StreamRequest, slot: &GpuSlot, cancel: &AtomicBool, events: &mut dyn FnMut(Value)) -> Result<Value, Failure> {
    if let Some(c) = &req.comp {
        s.execute("comp.open", json!({ "comp": crate::reference(c) })).map_err(|e| Failure::Error(e.to_string()))?;
    }
    let cid = s.active_comp_id().ok_or_else(|| Failure::Error("no composition".into()))?;
    let settings = render_settings(req)?;
    let (est_w, est_h) = s
        .project
        .comp(cid)
        .map(|c| {
            let w = ((c.width as f64 * settings.resolution).round() as u32).max(1);
            let h = ((c.height as f64 * settings.resolution).round() as u32).max(1);
            (w, h)
        })
        .ok_or_else(|| Failure::Error("no composition".into()))?;
    let gpu = slot.fit(s, est_w, est_h, events);
    let output = OutputModule {
        channels: if req.pix_fmt == PixFmt::Rgba { Channels::Rgba } else { Channels::Rgb },
        audio: if req.audio_only { AudioOutput::On } else { AudioOutput::Auto },
        quality: if req.draft { 50 } else { 90 },
        ..Default::default()
    };
    let dummy = req.out.clone();
    let job = Job {
        project: &s.project,
        footage: s.footage.as_ref(),
        expr: s.expr.as_deref(),
        accel: s.accel.as_deref(),
        comp: cid,
        settings: &settings,
        output: &output,
        path: &dummy,
        sink: None,
        options: Default::default(),
        nested_switches: s.prefs.general.switches_affect_nested_comps,
    };
    let info = stream_info(&job, req.pix_fmt).map_err(|e| Failure::Error(e.to_string()))?;
    let header = header_json(&info, gpu, &req.out, req.audio_out.as_deref());
    events(header.clone());
    if let Some(path) = &req.sidecar {
        write_sidecar(path, &header)?;
    }
    if cancel.load(Ordering::Relaxed) {
        events(json!({"event":"cancelled"}));
        return Err(Failure::Error("cancelled".into()));
    }

    let mut audio_bytes = 0u64;
    if req.audio_only || req.audio_out.is_some() {
        let dest = req.audio_out.as_deref().unwrap_or(&req.out);
        let mut w = open_sink(dest)?;
        let wav = {
            let mut on_audio = progress::on_frame(cancel, &mut *events);
            export_wav(&job, &mut *w, &mut on_audio)
        };
        match wav {
            Ok(r) => audio_bytes = r.bytes,
            Err(effectcraft_export::ExportError::Cancelled) => {
                events(json!({"event":"cancelled"}));
                return Err(Failure::Error("cancelled".into()));
            }
            Err(e) => return Err(map_io(e)),
        }
        if req.audio_only {
            let done = json!({
                "event": "done",
                "frames": info.frames,
                "width": info.width,
                "height": info.height,
                "bytes": audio_bytes,
                "audioBytes": audio_bytes,
                "gpu": gpu,
                "out": dest,
            });
            events(done.clone());
            return Ok(done);
        }
    }

    let mut w = open_sink(&req.out)?;
    let raw = {
        let mut on_video = progress::on_frame(cancel, &mut *events);
        export_raw(&job, req.pix_fmt, &mut *w, &mut on_video)
    };
    let report = match raw {
        Ok(r) => r,
        Err(effectcraft_export::ExportError::Cancelled) => {
            events(json!({"event":"cancelled"}));
            return Err(Failure::Error("cancelled".into()));
        }
        Err(e) => return Err(map_io(e)),
    };
    let done = json!({
        "event": "done",
        "frames": report.frames,
        "width": report.width,
        "height": report.height,
        "seconds": (report.seconds * 1000.0).round() / 1000.0,
        "bytes": report.bytes,
        "audioBytes": audio_bytes,
        "gpu": gpu,
        "pixFmt": req.pix_fmt.as_str(),
        "out": req.out,
    });
    events(done.clone());
    Ok(done)
}

fn render_settings(req: &StreamRequest) -> Result<RenderSettings, Failure> {
    let time_span = match (req.start, req.end, req.work_area) {
        (None, None, true) => TimeSpan::WorkArea,
        (None, None, false) => TimeSpan::LengthOfComp,
        (a, b, _) => TimeSpan::Custom { start: Tick::from_seconds_f64(a.unwrap_or(0.0).max(0.0)), end: Tick::from_seconds_f64(b.unwrap_or(f64::MAX).max(0.0)) },
    };
    Ok(RenderSettings {
        quality: if req.draft { RenderQuality::Draft } else { RenderQuality::Best },
        resolution: req.resolution.clamp(0.01, 4.0),
        time_span,
        frame_rate: req.fps.map(FrameRate::from_f64),
        ..Default::default()
    })
}

fn write_sidecar(path: &str, header: &Value) -> Result<(), Failure> {
    if let Some(dir) = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Failure::Error(format!("cannot create sidecar dir: {e}")))?;
    }
    std::fs::write(path, format!("{header}\n")).map_err(|e| Failure::Error(format!("cannot write sidecar {path}: {e}")))
}

pub fn open_sink(path: &str) -> Result<Box<dyn Write + Send>, Failure> {
    if path == "-" || path.eq_ignore_ascii_case("stdout") {
        return Ok(Box::new(std::io::stdout()));
    }
    let p = Path::new(path);
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Failure::Error(format!("cannot create {}: {e}", dir.display())))?;
    }
    let file = match p.metadata() {
        Ok(m) if m.is_file() => File::create(p),
        Ok(_) => OpenOptions::new().write(true).open(p),
        Err(_) => File::create(p),
    }
    .map_err(|e| Failure::Error(format!("cannot open {path}: {e}")))?;
    Ok(Box::new(std::io::BufWriter::with_capacity(1 << 20, file)))
}

fn map_io(e: effectcraft_export::ExportError) -> Failure {
    let msg = e.to_string();
    if msg.contains("Broken pipe") || msg.contains("broken pipe") || msg.contains("os error 32") {
        progress::emit(&json!({"event":"cancelled","reason":"pipe closed"}));
        return Failure::Error("cancelled: pipe closed".into());
    }
    Failure::Error(msg)
}
