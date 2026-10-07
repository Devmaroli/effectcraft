//! Composition ▸ Add to EncodeCraft Queue (After Effects' Add to Adobe Media Encoder Queue).
//!
//! Sends the active composition and the **saved** project path to EncodeCraft:
//! 1. `GET /health` (unauthenticated) to see if EncodeCraft is up
//! 2. `POST /v1/enqueue` with `X-EncodeCraft-Token` (loopback HTTP/1.1, no extra HTTP crate)
//! 3. if that is down, the same JSON is written to EncodeCraft's inbox and the app is launched
//!
//! Desktop only. The web build returns a clear "desktop app" error (no localhost encoder).
//!
//! Security: loopback only (never DNS); IPC token required when the encoder is running;
//! no Origin / Sec-Fetch-Site headers; inbox `create_new`; launch from absolute existing
//! paths only (never `PATH`; macOS uses `/usr/bin/open`).

use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use encodecraft_job::{
    DEFAULT_ENQUEUE_URL, Job, MAX_CONTROL_BODY, TOKEN_HEADER, discover_ipc_token, enqueue_error_message, is_health_ok, preset_id_from_hint, sanitize_token,
};
use serde_json::{Value, json};

use super::{CommandSpec, has_comp, str_p};
use crate::{EngineError, Event, Result, Session};

const CMD: &str = "encodecraft.queue";
const UNSAVED: &str =
    "Save the project before adding it to the EncodeCraft Queue (File ▸ Save). EncodeCraft renders the file on disk, so unsaved changes would be missing.";
const SETUP_TOKEN: &str = "Open EncodeCraft once so it can set up the connection";
#[cfg(target_arch = "wasm32")]
const DESKTOP_ONLY: &str = "Add to EncodeCraft Queue is only available in the desktop app";
#[cfg(not(target_arch = "wasm32"))]
const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);
#[cfg(not(target_arch = "wasm32"))]
const HTTP_TIMEOUT: Duration = Duration::from_millis(1500);
#[cfg(not(target_arch = "wasm32"))]
const LAUNCH_RETRIES: u32 = 4;
#[cfg(not(target_arch = "wasm32"))]
const LAUNCH_RETRY_WAIT: Duration = Duration::from_millis(200);
#[cfg(not(target_arch = "wasm32"))]
const MAX_RESPONSE: usize = 64 * 1024;
const MAX_RESULT_BODY: usize = 1024;
const MAX_INBOX_PATH: usize = 4096;
const MAX_INBOX_NAME_TRIES: u32 = 32;

fn queue(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        return Err(EngineError::Other(DESKTOP_ONLY.into()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        queue_native(s, p)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn queue_native(s: &mut Session, p: &Value) -> Result<Value> {
    let cid = super::comp_id(s, p)?;
    let name = s.project.item(cid).map(|i| i.name.clone()).ok_or(EngineError::NoComp)?;
    let path = s.path.as_deref().filter(|p| !p.is_empty()).ok_or_else(|| EngineError::Other(UNSAVED.into()))?;
    if s.is_dirty() {
        return Err(EngineError::Other(UNSAVED.into()));
    }
    let project = abs_path(path);
    let mut job = Job::effectcraft(&project, &name);
    if let Some(out) = str_p(p, "output") {
        validate_output_path(out).map_err(EngineError::Other)?;
    }
    // Never set output_dir: EncodeCraft only accepts paths inside its configured output folder.
    job.output_dir = None;
    if let Some(preset) = str_p(p, "presetId").or_else(|| str_p(p, "preset_id")) {
        validate_preset_id(preset).map_err(EngineError::Other)?;
        job.preset_id = preset.to_string();
    } else if let Some(fmt) = str_p(p, "format") {
        validate_format(fmt).map_err(EngineError::Other)?;
        job.preset_id = preset_id_from_hint(fmt);
    }
    if let Some(m) = str_p(p, "mezzanine") {
        validate_format(m).map_err(EngineError::Other)?;
        job.set_mezzanine(m);
    }
    if let Some(w) = p.get("workArea").or_else(|| p.get("work_area")).and_then(Value::as_bool) {
        job.set_work_area(w);
    }
    if let Some(b) = p.get("startQueue").or_else(|| p.get("start_queue")).and_then(Value::as_bool) {
        job.start_queue = b;
    }
    if let Some(n) = str_p(p, "naming") {
        job.naming = Some(n.to_string());
    }
    let url = str_p(p, "url").map(str::to_string).or_else(|| std::env::var("ENCODECRAFT_URL").ok()).unwrap_or_else(|| DEFAULT_ENQUEUE_URL.into());
    parse_loopback_http_url(&url).map_err(EngineError::Other)?;
    let health = health_url_for(&url).map_err(EngineError::Other)?;
    let inbox = match str_p(p, "inbox") {
        Some(p) => Some(validate_inbox_dir(Path::new(p)).map_err(EngineError::Other)?),
        None => inbox_dir(),
    };
    let launch = b_launch(p);
    let token = match str_p(p, "token") {
        Some(raw) => Some(sanitize_token(raw).ok_or_else(|| EngineError::Other(SETUP_TOKEN.into()))?),
        None => discover_ipc_token(),
    };
    let body = serde_json::to_string(&job).map_err(|e| EngineError::Other(format!("cannot encode the EncodeCraft job: {e}")))?;
    if body.len() > MAX_CONTROL_BODY {
        return Err(EngineError::Other("EncodeCraft job is too large to send".into()));
    }

    let running = encoder_is_up(&health);
    if running {
        let Some(tok) = token.as_deref() else {
            return Err(EngineError::Other(SETUP_TOKEN.into()));
        };
        return match post_json(&url, &body, Some(tok), CONNECT_TIMEOUT, HTTP_TIMEOUT) {
            Ok((401, _)) => Err(EngineError::Other(SETUP_TOKEN.into())),
            Ok((status, resp)) if (200..300).contains(&status) => {
                s.toast(format!("Queued “{name}” in EncodeCraft"));
                Ok(queued_json(&job, "http", Some(status), Some(resp), None))
            }
            Ok((status, resp)) => Err(refuse(s, status, &resp)),
            Err(http_err) => queue_via_inbox(s, &job, &body, &url, inbox.as_deref(), launch, &name, Some(http_err.as_str())),
        };
    }
    queue_via_inbox(s, &job, &body, &url, inbox.as_deref(), launch, &name, None)
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn queue_via_inbox(s: &mut Session, job: &Job, body: &str, url: &str, inbox: Option<&Path>, launch: bool, name: &str, http_err: Option<&str>) -> Result<Value> {
    let inbox_path = match inbox {
        Some(dir) => write_inbox(dir, &new_job_id(), body)?,
        None => {
            let why = http_err.unwrap_or("EncodeCraft is not running");
            return Err(EngineError::Other(format!(
                "{SETUP_TOKEN}. {why} at {url}, and no inbox folder was found. Install EncodeCraft, or set ENCODECRAFT_HOME / ENCODECRAFT_INBOX."
            )));
        }
    };
    let mut launched = false;
    if launch {
        launched = launch_encodecraft();
        if launched {
            for _ in 0..LAUNCH_RETRIES {
                std::thread::sleep(LAUNCH_RETRY_WAIT);
                let tok = discover_ipc_token();
                let health = health_url_for(url).unwrap_or_else(|_| String::new());
                if !health.is_empty() && encoder_is_up(&health) {
                    let Some(tok) = tok.as_deref() else {
                        return Err(EngineError::Other(SETUP_TOKEN.into()));
                    };
                    match post_json(url, body, Some(tok), CONNECT_TIMEOUT, HTTP_TIMEOUT) {
                        Ok((401, _)) => return Err(EngineError::Other(SETUP_TOKEN.into())),
                        Ok((status, resp)) if (200..300).contains(&status) => {
                            s.toast(format!("Queued “{name}” in EncodeCraft"));
                            return Ok(queued_json(job, "http-after-launch", Some(status), Some(resp), Some(inbox_path.to_string_lossy().into_owned())));
                        }
                        Ok((status, resp)) => return Err(refuse(s, status, &resp)),
                        _ => {}
                    }
                }
            }
        }
    }
    let how = if launched {
        format!("EncodeCraft was opened; the job is in the inbox ({})", inbox_path.display())
    } else if let Some(err) = http_err {
        format!("the job was saved to the inbox ({}). Open EncodeCraft to pick it up — it was not reachable at {url} ({err})", inbox_path.display())
    } else {
        format!("the job was saved to the inbox ({}). {SETUP_TOKEN} (it was not running at {url})", inbox_path.display())
    };
    s.toast(how);
    Ok(queued_json(job, "inbox", None, None, Some(inbox_path.to_string_lossy().into_owned())))
}

fn queued_json(job: &Job, via: &str, status: Option<u16>, response: Option<String>, inbox: Option<String>) -> Value {
    json!({
        "queued": true,
        "via": via,
        "project": job.project_path(),
        "composition": job.composition(),
        "presetId": job.preset_id,
        "httpStatus": status,
        "response": response.map(|r| truncate(&r, MAX_RESULT_BODY)),
        "inbox": inbox,
    })
}

fn refuse(s: &mut Session, status: u16, body: &str) -> EngineError {
    let msg = enqueue_error_message(status, body);
    s.events.push(Event::Toast { message: msg.clone(), error: true });
    EngineError::Other(msg)
}

fn b_launch(p: &Value) -> bool {
    match p.get("launch").and_then(Value::as_bool) {
        Some(b) => b,
        None => std::env::var("ENCODECRAFT_LAUNCH").ok().is_none_or(|v| v != "0" && !v.eq_ignore_ascii_case("false")),
    }
}

fn abs_path(path: &str) -> String {
    Path::new(path).canonicalize().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string())
}

fn new_job_id() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("ec-{ms:x}-{:x}", std::process::id())
}

fn truncate(s: &str, n: usize) -> String {
    let mut t = s.chars().take(n).collect::<String>();
    if s.chars().count() > n {
        t.push('…');
    }
    t
}

fn has_ctrl(s: &str) -> bool {
    s.bytes().any(|b| b < 0x20 || b == 0x7f)
}

/// EncodeCraft's drop folder. `ENCODECRAFT_INBOX` wins when it is a valid absolute path;
/// otherwise `<data dir>/inbox/` from [`encodecraft_job::inbox_dir`].
pub fn inbox_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ENCODECRAFT_INBOX")
        && !p.is_empty()
    {
        return validate_inbox_dir(Path::new(&p)).ok();
    }
    encodecraft_job::inbox_dir().and_then(|p| validate_inbox_dir(&p).ok())
}

fn validate_inbox_dir(dir: &Path) -> std::result::Result<PathBuf, String> {
    let raw = dir.to_string_lossy();
    if raw.len() > MAX_INBOX_PATH || raw.contains('\0') || has_ctrl(&raw) {
        return Err("EncodeCraft inbox path is not allowed".into());
    }
    if dir.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("EncodeCraft inbox path must not contain '..'".into());
    }
    if !dir.is_absolute() {
        return Err("EncodeCraft inbox path must be absolute".into());
    }
    Ok(dir.to_path_buf())
}

fn validate_output_path(path: &str) -> std::result::Result<(), String> {
    if path.is_empty() || path.len() > MAX_INBOX_PATH || path.contains('\0') || has_ctrl(path) {
        return Err("EncodeCraft output path is not allowed".into());
    }
    let p = Path::new(path);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("EncodeCraft output path must not contain '..'".into());
    }
    if !p.is_absolute() {
        return Err("EncodeCraft output path must be absolute".into());
    }
    Ok(())
}

fn validate_format(fmt: &str) -> std::result::Result<(), String> {
    validate_preset_id(fmt)
}

fn validate_preset_id(id: &str) -> std::result::Result<(), String> {
    if id.is_empty() || id.len() > 64 || has_ctrl(id) || id.contains("..") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("EncodeCraft preset must be a short id such as system.h264-mp4".into());
    }
    Ok(())
}

fn write_inbox(dir: &Path, id: &str, body: &str) -> Result<PathBuf> {
    let dir = validate_inbox_dir(dir).map_err(EngineError::Other)?;
    ensure_inbox_dir(&dir)?;
    let stem: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(80).collect();
    let stem = if stem.is_empty() { "job".into() } else { stem };
    for n in 0..MAX_INBOX_NAME_TRIES {
        let name = if n == 0 { format!("{stem}.json") } else { format!("{stem}-{n}.json") };
        let path = dir.join(&name);
        // The file name is only the sanitized id; refuse if join somehow escaped `dir`.
        if path.parent() != Some(dir.as_path()) {
            return Err(EngineError::Other("EncodeCraft inbox path resolved outside the inbox folder".into()));
        }
        match open_inbox_new(&path) {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(body.as_bytes()).map_err(|e| EngineError::Other(format!("cannot write {}: {e}", path.display())))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(EngineError::Other(format!("cannot write {}: {e}", path.display()))),
        }
    }
    Err(EngineError::Other("cannot allocate a unique EncodeCraft inbox file name".into()))
}

fn ensure_inbox_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true).mode(0o700);
        b.create(dir).map_err(|e| EngineError::Other(format!("cannot create EncodeCraft inbox {}: {e}", dir.display())))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir).map_err(|e| EngineError::Other(format!("cannot create EncodeCraft inbox {}: {e}", dir.display())))?;
    }
    Ok(())
}

fn open_inbox_new(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn health_url_for(enqueue: &str) -> std::result::Result<String, String> {
    let (_, port, _) = parse_loopback_http_url(enqueue)?;
    Ok(format!("http://127.0.0.1:{port}/health"))
}

#[cfg(not(target_arch = "wasm32"))]
fn encoder_is_up(health_url: &str) -> bool {
    matches!(http_exchange("GET", health_url, None, None, CONNECT_TIMEOUT, HTTP_TIMEOUT), Ok((status, body)) if (200..300).contains(&status) && is_health_ok(&body))
}

/// Loopback-only HTTP/1.1 POST. Never performs DNS. Rejects CR/LF so the request line cannot be smuggled.
/// `connect` is time-bounded so a dropped SYN cannot freeze the UI. No Origin / Sec-Fetch-Site.
#[cfg(not(target_arch = "wasm32"))]
fn post_json(url: &str, body: &str, token: Option<&str>, connect: Duration, rw: Duration) -> std::result::Result<(u16, String), String> {
    http_exchange("POST", url, Some(body), token, connect, rw)
}

#[cfg(not(target_arch = "wasm32"))]
fn http_exchange(
    method: &str,
    url: &str,
    body: Option<&str>,
    token: Option<&str>,
    connect: Duration,
    rw: Duration,
) -> std::result::Result<(u16, String), String> {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    if !matches!(method, "GET" | "POST") {
        return Err("unsupported HTTP method".into());
    }
    let (host, port, path) = parse_loopback_http_url(url)?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, connect).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(rw)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(rw)).map_err(|e| e.to_string())?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n");
    if let Some(tok) = token {
        let tok = sanitize_token(tok).ok_or_else(|| SETUP_TOKEN.to_string())?;
        req.push_str(TOKEN_HEADER);
        req.push_str(": ");
        req.push_str(&tok);
        req.push_str("\r\n");
    }
    if let Some(body) = body {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str("Content-Length: ");
        req.push_str(&body.len().to_string());
        req.push_str("\r\n\r\n");
        req.push_str(body);
    } else {
        req.push_str("\r\n");
    }
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let _ = stream.flush();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                let n = n.min(MAX_RESPONSE.saturating_sub(buf.len()));
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() >= MAX_RESPONSE {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    parse_http_response(&buf)
}

fn parse_loopback_http_url(url: &str) -> std::result::Result<(String, u16, String), String> {
    if url.len() > 2048 || has_ctrl(url) || url.contains(' ') || url.contains('@') || url.contains('\\') {
        return Err("EncodeCraft URL must be a plain http://127.0.0.1:… address".into());
    }
    let rest = url.strip_prefix("http://").ok_or("EncodeCraft URL must be http://127.0.0.1:…")?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    if authority.is_empty() || authority.contains('/') {
        return Err("EncodeCraft URL host is missing".into());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| format!("bad port `{p}`"))?),
        None => (authority, 80u16),
    };
    if port == 0 {
        return Err("EncodeCraft URL port must not be 0".into());
    }
    let loopback = host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost");
    if !loopback {
        return Err("EncodeCraft URL must be localhost (127.0.0.1)".into());
    }
    if !path.starts_with('/') || path.contains("..") || !path.bytes().all(|b| matches!(b, b'/' | b'.' | b'-' | b'_' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'))
    {
        return Err("EncodeCraft URL path is not allowed".into());
    }
    Ok(("127.0.0.1".into(), port, path))
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_http_response(buf: &[u8]) -> std::result::Result<(u16, String), String> {
    let text = String::from_utf8_lossy(buf);
    let (head, body) = text.split_once("\r\n\r\n").or_else(|| text.split_once("\n\n")).unwrap_or((text.as_ref(), ""));
    let status_line = head.lines().next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("not an HTTP response: {}", truncate(status_line, 80)))?;
    Ok((status, body.trim().to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
fn launch_encodecraft() -> bool {
    for (bin, trust) in encodecraft_bins() {
        if spawn_detached(&bin, trust) {
            return true;
        }
    }
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn encodecraft_bins() -> Vec<(PathBuf, LaunchTrust)> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("ENCODECRAFT_BIN") {
        let p = PathBuf::from(p);
        if p.is_absolute() && !p.as_os_str().is_empty() {
            out.push((p, LaunchTrust::Explicit));
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in ["encodecraft", "EncodeCraft", "encodecraft.exe", "EncodeCraft.exe"] {
            out.push((dir.join(name), LaunchTrust::Sibling));
        }
        #[cfg(target_os = "macos")]
        out.push((dir.join("EncodeCraft.app"), LaunchTrust::Sibling));
    }
    #[cfg(target_os = "macos")]
    {
        out.push((PathBuf::from("/Applications/EncodeCraft.app"), LaunchTrust::Install));
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            out.push((PathBuf::from(pf).join("EncodeCraft").join("EncodeCraft.exe"), LaunchTrust::Install));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            out.push((PathBuf::from(local).join("Programs").join("EncodeCraft").join("EncodeCraft.exe"), LaunchTrust::Install));
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        out.push((PathBuf::from("/usr/local/bin/encodecraft"), LaunchTrust::Install));
        out.push((PathBuf::from("/usr/bin/encodecraft"), LaunchTrust::Install));
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchTrust {
    /// `ENCODECRAFT_BIN`: user-supplied absolute path.
    Explicit,
    /// Same directory as this EffectCraft binary; must be same-uid on Unix.
    Sibling,
    /// Well-known install location.
    Install,
}

#[cfg(not(target_arch = "wasm32"))]
fn is_launchable_with(bin: &Path, trust: LaunchTrust) -> bool {
    if !bin.is_absolute() || bin.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        if bin.extension().and_then(|e| e.to_str()) == Some("app") {
            return bin.is_dir() && owner_ok(bin, trust);
        }
    }
    bin.is_file() && owner_ok(bin, trust)
}

#[cfg(not(target_arch = "wasm32"))]
fn owner_ok(bin: &Path, trust: LaunchTrust) -> bool {
    match trust {
        LaunchTrust::Explicit | LaunchTrust::Install => true,
        LaunchTrust::Sibling => same_uid_as_current_exe(bin),
    }
}

#[cfg(all(not(target_arch = "wasm32"), unix))]
fn same_uid_as_current_exe(bin: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(bin_meta) = bin.metadata() else {
        return false;
    };
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(exe_meta) = exe.metadata() else {
        return false;
    };
    bin_meta.uid() == exe_meta.uid()
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn same_uid_as_current_exe(_bin: &Path) -> bool {
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_detached(bin: &Path, trust: LaunchTrust) -> bool {
    use std::process::{Command, Stdio};
    if !is_launchable_with(bin, trust) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        if bin.extension().and_then(|e| e.to_str()) == Some("app") {
            let open = Path::new("/usr/bin/open");
            if !open.is_file() {
                return false;
            }
            let mut cmd = Command::new(open);
            cmd.arg(bin).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            detach_stdio(&mut cmd);
            return cmd.spawn().is_ok();
        }
    }
    let mut cmd = Command::new(bin);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach_stdio(&mut cmd);
    cmd.spawn().is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn detach_stdio(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Add to EncodeCraft Queue",
        menu: &["Composition"],
        shortcut: Some("Cmd+Alt+M"),
        params: "{comp?: id|name, presetId?: system.h264-mp4, format?: h264|hevc|… (mapped to presetId), mezzanine?: prores, workArea?: bool, startQueue?: bool (default true), naming?: {name}_{width}x{height}, output?: path (validated, not sent — EncodeCraft chooses the output folder), url?: http://127.0.0.1:port/v1/enqueue, inbox?: absolute folder, launch?: bool (default true; open EncodeCraft when the HTTP server is down), token?: IPC token (default: ENCODECRAFT_TOKEN or <data dir>/ipc-token)}",
        enabled: has_comp,
        run: queue,
        journal: false,
    }]
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use serde_json::json;

    use super::{LaunchTrust, is_launchable_with, parse_loopback_http_url, validate_format, validate_inbox_dir, validate_output_path, write_inbox};
    use crate::Session;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "ec-ame-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    fn session_saved(dir: &Path) -> Session {
        let mut s = Session::default();
        s.execute("file.openDemoProject", json!({})).unwrap();
        let path = dir.join("demo.ecproj").to_string_lossy().to_string();
        s.execute("file.saveAs", json!({"path": path})).unwrap();
        s
    }

    struct Server {
        url: String,
        hits: Arc<Mutex<Vec<String>>>,
        enqueue_status: u16,
        _join: thread::JoinHandle<()>,
    }

    /// Mock EncodeCraft: GET /health is open; POST /v1/enqueue requires the token header,
    /// a loopback Host, and rejects non-loopback Origin / cross-site fetch.
    fn serve_encodecraft(expect_token: &'static str, enqueue_status: u16) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let h = hits.clone();
        let join = thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        let _ = s.set_nonblocking(false);
                        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(400)));
                        let mut buf = vec![0u8; 16384];
                        let n = s.read(&mut buf).unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                        h.lock().unwrap_or_else(|e| e.into_inner()).push(req.clone());
                        let resp = mock_encodecraft_response(&req, expect_token, enqueue_status);
                        let _ = s.write_all(resp.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Server { url: format!("http://127.0.0.1:{port}/v1/enqueue"), hits, enqueue_status, _join: join }
    }

    fn header_value(req: &str, name: &str) -> Option<String> {
        let want = name.to_ascii_lowercase();
        for line in req.lines().skip(1) {
            if line.is_empty() || line == "\r" {
                break;
            }
            let (k, v) = line.split_once(':')?;
            if k.trim().eq_ignore_ascii_case(&want) {
                return Some(v.trim().to_string());
            }
        }
        None
    }

    fn host_is_loopback(host: &str) -> bool {
        let h = host.trim().trim_matches(|c| c == '[' || c == ']');
        let h = h.rsplit_once(':').map(|(n, p)| if p.chars().all(|c| c.is_ascii_digit()) { n } else { h }).unwrap_or(h);
        h == "127.0.0.1" || h.eq_ignore_ascii_case("localhost")
    }

    fn origin_is_forbidden(origin: &str) -> bool {
        let o = origin.trim();
        if o.eq_ignore_ascii_case("null") || o.is_empty() {
            return true;
        }
        let rest = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")).unwrap_or(o);
        let host = rest.split('/').next().unwrap_or(rest);
        !host_is_loopback(host)
    }

    fn mock_encodecraft_response(req: &str, expect_token: &str, enqueue_status: u16) -> String {
        let first = req.lines().next().unwrap_or("");
        let path = first.split_whitespace().nth(1).unwrap_or("");
        let host = header_value(req, "Host").unwrap_or_default();
        if !host_is_loopback(&host) {
            return http_json(403, r#"{"error":"host"}"#);
        }
        if let Some(origin) = header_value(req, "Origin")
            && origin_is_forbidden(&origin)
        {
            return http_json(403, r#"{"error":"origin"}"#);
        }
        if header_value(req, "Sec-Fetch-Site").is_some_and(|v| v.eq_ignore_ascii_case("cross-site")) {
            return http_json(403, r#"{"error":"site"}"#);
        }
        if path == "/health" || path.starts_with("/health?") {
            return http_json(200, r#"{"ok":true,"product":"EncodeCraft"}"#);
        }
        let tok = header_value(req, "X-EncodeCraft-Token");
        if tok.as_deref() != Some(expect_token) {
            return http_json(401, r#"{"error":"unauthorized"}"#);
        }
        if first.starts_with("POST /v1/enqueue") {
            let body = if (200..300).contains(&enqueue_status) {
                r#"{"ok":true,"ids":["q1"]}"#
            } else if enqueue_status == 400 {
                r#"{"ok":false,"ids":[],"error":"the preset is not installed"}"#
            } else {
                r#"{"ok":false,"ids":[],"error":"refused"}"#
            };
            return http_json(enqueue_status, body);
        }
        http_json(404, r#"{"error":"not found"}"#)
    }

    fn http_json(status: u16, body: &str) -> String {
        let reason = match status {
            200 => "OK",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            _ => "Error",
        };
        format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn serve(status: u16) -> Server {
        serve_encodecraft("test-token", status)
    }

    #[test]
    fn unsaved_project_is_a_clear_error() {
        let mut s = Session::default();
        s.execute("file.openDemoProject", json!({})).unwrap();
        let e = s.execute("encodecraft.queue", json!({"launch": false})).unwrap_err().to_string();
        assert!(e.contains("Save the project"), "{e}");
    }

    #[test]
    fn dirty_saved_project_is_a_clear_error() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        s.execute("comp.new", json!({"name": "Extra"})).unwrap();
        let e = s.execute("encodecraft.queue", json!({"launch": false})).unwrap_err().to_string();
        assert!(e.contains("Save the project"), "{e}");
    }

    #[test]
    fn posts_job_to_loopback_server() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let srv = serve(200);
        let r = s.execute("encodecraft.queue", json!({"url": srv.url, "launch": false, "token": "test-token"})).unwrap();
        assert_eq!(r["via"], "http");
        assert_eq!(r["queued"], true);
        let name = s.project.item(s.active_comp_id().unwrap()).unwrap().name.clone();
        assert_eq!(r["composition"], name);
        let req = srv.hits.lock().unwrap().join("\n");
        assert!(req.contains("GET /health"), "{req}");
        assert!(req.contains("POST /v1/enqueue"), "{req}");
        assert!(req.contains("X-EncodeCraft-Token: test-token"), "{req}");
        assert!(!req.to_ascii_lowercase().contains("\r\norigin:"), "must not send Origin\n{req}");
        assert!(!req.to_ascii_lowercase().contains("sec-fetch-site"), "{req}");
        assert!(req.contains(&name), "{req}");
        assert!(req.contains(r#""schema":1"#), "schema must be integer 1\n{req}");
        assert!(!req.contains("encodecraft.job/v1"), "must not send the old string schema\n{req}");
        assert!(req.contains(r#""kind":"effectcraft""#), "{req}");
        assert!(req.contains(r#""preset_id":"system.h264-mp4""#), "{req}");
        assert!(req.contains(r#""start_queue":true"#), "{req}");
        assert!(!req.contains("output_dir") && !req.contains("outputDir"), "{req}");
        assert_eq!(srv.enqueue_status, 200);
        assert!(s.drain_events().iter().any(|e| matches!(e, crate::Event::Toast { error: false, .. })));
    }

    #[test]
    fn http_error_status_is_not_queued() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let srv = serve(503);
        let e = s.execute("encodecraft.queue", json!({"url": srv.url, "launch": false, "token": "test-token"})).unwrap_err().to_string();
        assert!(e.contains("refused") || e.contains("503"), "{e}");
        assert!(
            s.drain_events().iter().any(|ev| matches!(ev, crate::Event::Toast { error: true, message } if message.contains("refused"))),
            "refusal must be an in-app error toast"
        );
    }

    #[test]
    fn http_400_shows_encodecraft_error_in_toast() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let srv = serve(400);
        let e = s.execute("encodecraft.queue", json!({"url": srv.url, "launch": false, "token": "test-token"})).unwrap_err().to_string();
        assert!(e.contains("the preset is not installed"), "{e}");
        assert!(!e.contains("HTTP 400") || e.contains("preset"), "{e}");
        assert!(
            s.drain_events().iter().any(|ev| matches!(ev, crate::Event::Toast { error: true, message } if message.contains("the preset is not installed"))),
            "EncodeCraft's error must appear in-app, not only in logs"
        );
    }

    #[test]
    fn http_401_asks_to_open_encodecraft() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let srv = serve_encodecraft("real-token", 200);
        let e = s.execute("encodecraft.queue", json!({"url": srv.url, "launch": false, "token": "wrong-token"})).unwrap_err().to_string();
        assert!(e.contains("Open EncodeCraft once"), "{e}");
    }

    #[test]
    fn running_encoder_without_token_is_a_clear_error() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let srv = serve(200);
        let e = s.execute("encodecraft.queue", json!({"url": srv.url, "launch": false})).unwrap_err().to_string();
        assert!(e.contains("Open EncodeCraft once"), "{e}");
        let hits = srv.hits.lock().unwrap().join("\n");
        assert!(hits.contains("GET /health"), "{hits}");
        assert!(!hits.contains("POST /v1/enqueue"), "must not POST without a token\n{hits}");
    }

    #[test]
    fn http_down_writes_inbox_without_launch() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let inbox = t.0.join("inbox");
        let r = s
            .execute(
                "encodecraft.queue",
                json!({
                    "url": "http://127.0.0.1:1/v1/enqueue",
                    "inbox": inbox.to_string_lossy(),
                    "launch": false
                }),
            )
            .unwrap();
        assert_eq!(r["via"], "inbox");
        let path = r["inbox"].as_str().unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let job: encodecraft_job::Job = serde_json::from_str(&text).unwrap();
        assert_eq!(job.composition(), r["composition"].as_str());
        assert!(job.project_path().contains("demo.ecproj"));
        assert!(job.output_dir.is_none());
        assert_eq!(job.schema, 1);
        assert_eq!(job.preset_id, "system.h264-mp4");
        assert!(path.starts_with(&*inbox.to_string_lossy()));
    }

    #[test]
    fn refuses_non_loopback_url() {
        let t = tmp();
        let mut s = session_saved(&t.0);
        let e = s.execute("encodecraft.queue", json!({"url": "http://example.com/v1/enqueue", "launch": false})).unwrap_err().to_string();
        assert!(e.contains("localhost") || e.contains("127.0.0.1"), "{e}");
    }

    #[test]
    fn refuses_crlf_in_url() {
        assert!(parse_loopback_http_url("http://127.0.0.1:9878/v1/enqueue\r\nX: 1").is_err());
        assert!(parse_loopback_http_url("http://127.0.0.1:9878/v1/enqueue HTTP/1.1").is_err());
        assert!(parse_loopback_http_url("http://127.0.0.1@evil.example/v1/enqueue").is_err());
        assert!(parse_loopback_http_url("http://127.0.0.1:9878/v1/../secret").is_err());
        let t = tmp();
        let mut s = session_saved(&t.0);
        let e = s.execute("encodecraft.queue", json!({"url": "http://127.0.0.1:9878/v1/enqueue\r\nHost: 127.0.0.1", "launch": false})).unwrap_err().to_string();
        assert!(e.contains("URL") || e.contains("plain") || e.contains("127.0.0.1"), "{e}");
    }

    #[test]
    fn refuses_non_http_and_ipv6_urls() {
        assert!(parse_loopback_http_url("https://127.0.0.1:9878/v1/enqueue").is_err());
        assert!(parse_loopback_http_url("file:///etc/passwd").is_err());
        assert!(parse_loopback_http_url("http://[::1]:9878/v1/enqueue").is_err());
        assert!(parse_loopback_http_url("http://0.0.0.0:9878/v1/enqueue").is_err());
        assert!(parse_loopback_http_url("http://127.0.0.1:0/v1/enqueue").is_err());
        assert!(parse_loopback_http_url("http://127.0.0.1:9878/v1/enqueue?x=1").is_err());
    }

    #[test]
    fn inbox_path_rejects_parent_dir_and_relative() {
        assert!(validate_inbox_dir(Path::new("inbox")).is_err());
        assert!(validate_inbox_dir(Path::new("/tmp/ec-inbox/../etc")).is_err());
        let t = tmp();
        let mut s = session_saved(&t.0);
        let sneaky = t.0.join("inbox").join("..").join("outside");
        let e = s
            .execute(
                "encodecraft.queue",
                json!({
                    "url": "http://127.0.0.1:1/v1/enqueue",
                    "inbox": sneaky.to_string_lossy(),
                    "launch": false
                }),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("..") || e.contains("inbox"), "{e}");
        assert!(!t.0.join("outside").exists());
    }

    #[test]
    fn inbox_does_not_overwrite_existing_job_file() {
        let t = tmp();
        let dir = t.0.join("inbox");
        let first = write_inbox(&dir, "ec-1", "{\"a\":1}").unwrap();
        let second = write_inbox(&dir, "ec-1", "{\"a\":2}").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{\"a\":1}");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "{\"a\":2}");
        assert!(second.file_name().unwrap().to_string_lossy().contains("ec-1-"));
    }

    #[test]
    fn output_and_format_reject_injection() {
        assert!(validate_output_path("/tmp/out.mp4").is_ok());
        assert!(validate_output_path("out.mp4").is_err());
        assert!(validate_output_path("/tmp/../etc/out.mp4").is_err());
        assert!(validate_output_path("/tmp/out.mp4\r\nX").is_err());
        assert!(validate_format("h264").is_ok());
        assert!(validate_format("h264\r\n").is_err());
        assert!(validate_format("../../x").is_err());
        let t = tmp();
        let mut s = session_saved(&t.0);
        let e =
            s.execute("encodecraft.queue", json!({"url": "http://127.0.0.1:1/v1/enqueue", "output": "../out.mp4", "launch": false})).unwrap_err().to_string();
        assert!(e.contains("output") || e.contains(".."), "{e}");
    }

    #[test]
    fn launchable_requires_absolute_existing_file() {
        assert!(!is_launchable_with(Path::new("encodecraft"), LaunchTrust::Explicit));
        assert!(!is_launchable_with(Path::new("/no/such/encodecraft-binary-ec-test"), LaunchTrust::Explicit));
        assert!(!is_launchable_with(Path::new("/tmp/../usr/bin/encodecraft"), LaunchTrust::Explicit));
        let t = tmp();
        let bin = t.0.join("encodecraft");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&bin).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&bin, p).unwrap();
        }
        assert!(is_launchable_with(&bin, LaunchTrust::Explicit));
    }

    #[test]
    fn needs_a_composition() {
        let mut s = Session::default();
        let e = s.execute("encodecraft.queue", json!({})).unwrap_err().to_string();
        assert!(e.contains("not available") || e.contains("composition"), "{e}");
    }

    #[test]
    fn loopback_url_rewrites_localhost_without_dns() {
        let (host, port, path) = parse_loopback_http_url("http://localhost:9878/v1/enqueue").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 9878);
        assert_eq!(path, "/v1/enqueue");
    }
}
