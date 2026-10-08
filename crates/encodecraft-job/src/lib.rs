//! Shared EncodeCraft queue-job schema (vendored from EncodeCraft `crates/job`).
//!
//! A job is JSON. EffectCraft POSTs it to `http://127.0.0.1:9878/v1/enqueue` with
//! `X-EncodeCraft-Token` and, when EncodeCraft is not listening, writes the same object
//! into EncodeCraft's inbox (`<data dir>/inbox/`).
//!
//! Wire format (EncodeCraft 0.1.0): `schema` is integer `1`; `source` is tagged by
//! lowercase `kind` (`file` | `effectcraft`); `preset_id` is required. Unknown fields
//! are ignored on read.
//!
//! Provenance: EncodeCraft `crates/job` / `docs/job-format.md`, MIT OR Apache-2.0.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Schema version written on every job this crate produces (`schema: 1`).
pub const SCHEMA_V1: u32 = 1;

/// EncodeCraft's default enqueue endpoint (loopback only).
pub const DEFAULT_ENQUEUE_URL: &str = "http://127.0.0.1:9878/v1/enqueue";

/// Unauthenticated liveness probe. Body: `{"ok":true,"product":"EncodeCraft"}`.
pub const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:9878/health";

/// Default TCP port EncodeCraft listens on.
pub const DEFAULT_PORT: u16 = 9878;

/// `source.kind` for an EffectCraft project + composition.
pub const SOURCE_KIND_EFFECTCRAFT: &str = "effectcraft";

/// Default H.264 MP4 system preset.
pub const DEFAULT_PRESET_ID: &str = "system.h264-mp4";

/// Default mezzanine EncodeCraft renders from EffectCraft (`prores`).
pub const DEFAULT_MEZZANINE: &str = "prores";

/// Env var that supplies the IPC token, overriding the on-disk file.
pub const TOKEN_ENV: &str = "ENCODECRAFT_TOKEN";

/// HTTP header carrying the IPC token (`POST /v1/enqueue`, `GET /v1/queue`, `POST /v1/control`).
pub const TOKEN_HEADER: &str = "X-EncodeCraft-Token";

/// Filename EncodeCraft writes on first launch (`<data dir>/ipc-token`).
pub const TOKEN_FILE: &str = "ipc-token";

/// Env var that overrides EncodeCraft's data directory.
pub const HOME_ENV: &str = "ENCODECRAFT_HOME";

/// `directories::ProjectDirs::from` qualifier / organization / application EncodeCraft uses.
pub const PROJECT_QUALIFIER: &str = "dev";
/// Organization folder on Windows (`%APPDATA%\<org>\<app>\data`).
pub const PROJECT_ORGANIZATION: &str = "EncodeCraft";
/// Application folder; Linux XDG uses the lowercased form `encodecraft`.
pub const PROJECT_APPLICATION: &str = "EncodeCraft";

/// Maximum JSON body for `/v1/enqueue` and `/v1/control`.
pub const MAX_CONTROL_BODY: usize = 256 * 1024;

/// Maximum number of size entries on a job.
pub const MAX_SIZES: usize = 64;

fn schema_v1() -> u32 {
    SCHEMA_V1
}

fn default_preset_id() -> String {
    DEFAULT_PRESET_ID.into()
}

fn default_mezzanine() -> String {
    DEFAULT_MEZZANINE.into()
}

fn default_start_queue() -> bool {
    true
}

/// Input EncodeCraft should encode: a file on disk, or a saved EffectCraft composition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// An already-rendered file (`{"kind":"file","path":"…"}`).
    File { path: String },
    /// A saved `.ecproj` / `.ecprojx` plus composition name (or id as a string).
    Effectcraft {
        project: String,
        /// Composition name as shown in the Project panel, or the item id as a decimal string.
        comp: String,
        #[serde(default = "default_mezzanine")]
        mezzanine: String,
        #[serde(default)]
        work_area: bool,
    },
}

/// One named output size (`sizes` on a job; at most [`MAX_SIZES`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSize {
    pub name: String,
    pub width: u32,
    pub height: u32,
}

/// Optional trim, in seconds. `out_sec` null means through the end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trim {
    pub in_sec: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_sec: Option<f64>,
}

/// How EncodeCraft scales into a size preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScaleMode {
    Fit,
    Fill,
    Stretch,
    Scale,
}

/// One composition (or file) to add to EncodeCraft's queue.
///
/// Do **not** set [`Job::output_dir`] unless the path is inside EncodeCraft's configured
/// output folder; leave it unset and EncodeCraft chooses a unique name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Integer schema version. EncodeCraft 0.1.0 expects `1` (not a string).
    #[serde(default = "schema_v1")]
    pub schema: u32,
    pub source: Source,
    /// System or user preset, e.g. `system.h264-mp4`.
    #[serde(default = "default_preset_id")]
    pub preset_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_override: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    /// e.g. `{name}_{width}x{height}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub naming: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<OutputSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<Trim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<ScaleMode>,
    /// Start EncodeCraft's queue after enqueue. EffectCraft sends `true`.
    #[serde(default = "default_start_queue")]
    pub start_queue: bool,
}

impl Job {
    /// A job for composition `comp` (name, or id as a string) in the saved project at `project`.
    ///
    /// `output_dir` is left unset so EncodeCraft writes under its own output folder.
    /// `preset_id` is [`DEFAULT_PRESET_ID`]; `mezzanine` is ProRes; `start_queue` is true.
    pub fn effectcraft(project: impl Into<String>, comp: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_V1,
            source: Source::Effectcraft { project: project.into(), comp: comp.into(), mezzanine: DEFAULT_MEZZANINE.into(), work_area: false },
            preset_id: DEFAULT_PRESET_ID.into(),
            preset_override: None,
            output_dir: None,
            naming: None,
            sizes: Vec::new(),
            trim: None,
            scale: None,
            start_queue: true,
        }
    }

    /// Project path when the source is EffectCraft or a file.
    pub fn project_path(&self) -> &str {
        match &self.source {
            Source::File { path } => path,
            Source::Effectcraft { project, .. } => project,
        }
    }

    /// Composition name/id when the source is EffectCraft.
    pub fn composition(&self) -> Option<&str> {
        match &self.source {
            Source::Effectcraft { comp, .. } => Some(comp),
            Source::File { .. } => None,
        }
    }

    /// Set `source.mezzanine` when this is an EffectCraft job.
    pub fn set_mezzanine(&mut self, mezzanine: impl Into<String>) {
        if let Source::Effectcraft { mezzanine: slot, .. } = &mut self.source {
            *slot = mezzanine.into();
        }
    }

    /// Set `source.work_area` when this is an EffectCraft job.
    pub fn set_work_area(&mut self, work_area: bool) {
        if let Source::Effectcraft { work_area: slot, .. } = &mut self.source {
            *slot = work_area;
        }
    }
}

/// `POST /v1/enqueue` reply: `{ok, ids[], error?}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnqueueReply {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub ids: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Plain-language error when EncodeCraft refuses a job (HTTP 4xx/5xx body).
pub fn enqueue_error_message(status: u16, body: &str) -> String {
    match enqueue_error_detail(body) {
        Some(detail) => format!("EncodeCraft could not add this composition to the queue: {detail}"),
        None => format!("EncodeCraft could not add this composition to the queue (HTTP {status})."),
    }
}

/// The `error` (or `message` / `reason`) string from an enqueue JSON body, if any.
pub fn enqueue_error_detail(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(r) = serde_json::from_str::<EnqueueReply>(trimmed)
        && let Some(e) = r.error
    {
        let e = e.trim();
        if !e.is_empty() {
            return Some(e.to_string());
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in ["error", "message", "detail", "reason"] {
            if let Some(s) = v.get(key).and_then(json_error_text) {
                return Some(s);
            }
        }
    } else if !trimmed.starts_with('{') && trimmed.len() <= 512 && !trimmed.bytes().any(|b| b < 0x20) {
        return Some(trimmed.to_string());
    }
    None
}

fn json_error_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) => None,
        other => {
            let s = other.to_string();
            if s.is_empty() { None } else { Some(s) }
        }
    }
}

/// Map a short codec hint (`h264`) or a full preset id (`system.h264-mp4`) to `preset_id`.
pub fn preset_id_from_hint(hint: &str) -> String {
    let h = hint.trim();
    if h.is_empty() {
        return DEFAULT_PRESET_ID.into();
    }
    if h.contains('.') {
        return h.to_string();
    }
    match h {
        "h264" | "mp4" | "h264-mp4" => DEFAULT_PRESET_ID.into(),
        other => format!("system.{other}"),
    }
}

/// `POST /v1/control` body. The HTTP header still carries the token; this field is for
/// JSON-lines `{id, method, params, token}` as well.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// One JSON-lines control-channel request (`{id, method, params, token}`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonLine {
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Trimmed, non-empty token with no control characters (so it is safe in an HTTP header).
pub fn sanitize_token(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 2048 || t.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    Some(t.to_string())
}

/// IPC token: `ENCODECRAFT_TOKEN` (trimmed, non-empty), else `ipc-token` in a data dir.
///
/// Data dirs, in order: `$ENCODECRAFT_HOME`; EncodeCraft's
/// `directories::ProjectDirs::from("dev","EncodeCraft","EncodeCraft").data_dir()`
/// (Windows `%APPDATA%\EncodeCraft\EncodeCraft\data`, macOS
/// `~/Library/Application Support/dev.EncodeCraft.EncodeCraft`, Linux
/// `$XDG_DATA_HOME/encodecraft` or `~/.local/share/encodecraft`); then the
/// pre-directories-6 Windows folder `%APPDATA%\EncodeCraft\EncodeCraft`.
pub fn discover_ipc_token() -> Option<String> {
    discover_ipc_token_in(std::env::var(TOKEN_ENV).ok().as_deref(), &data_dirs())
}

/// Token discovery with explicit env value and data dirs (tests do not mutate process env).
pub fn discover_ipc_token_in(env_token: Option<&str>, dirs: &[PathBuf]) -> Option<String> {
    if let Some(v) = env_token
        && let Some(t) = sanitize_token(v)
    {
        return Some(t);
    }
    for dir in dirs {
        let path = dir.join(TOKEN_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Some(t) = sanitize_token(&text) {
            return Some(t);
        }
    }
    None
}

/// EncodeCraft data directories to search, first existing-or-configured first.
///
/// On native hosts this also inserts `ProjectDirs::data_dir()` after `$ENCODECRAFT_HOME`
/// so discovery stays aligned with EncodeCraft if the `directories` crate changes a platform
/// path. Wasm keeps the constructed paths only (this crate is L0).
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = data_dirs_from(
        std::env::var(HOME_ENV).ok().as_deref(),
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
    );
    #[cfg(not(target_arch = "wasm32"))]
    insert_native_project_dirs(&mut dirs);
    dirs
}

/// `%APPDATA%\EncodeCraft\EncodeCraft\data` — `ProjectDirs::data_dir()` on Windows (directories 6+).
pub fn windows_project_dirs_data(appdata: impl AsRef<Path>) -> PathBuf {
    appdata.as_ref().join(PROJECT_ORGANIZATION).join(PROJECT_APPLICATION).join("data")
}

/// `%APPDATA%\EncodeCraft\EncodeCraft` — Windows folder used before directories 6 appended `\data`.
pub fn windows_project_dirs_legacy(appdata: impl AsRef<Path>) -> PathBuf {
    appdata.as_ref().join(PROJECT_ORGANIZATION).join(PROJECT_APPLICATION)
}

/// `~/Library/Application Support/dev.EncodeCraft.EncodeCraft`.
pub fn macos_project_dirs_data(user_home: impl AsRef<Path>) -> PathBuf {
    user_home.as_ref().join("Library/Application Support/dev.EncodeCraft.EncodeCraft")
}

/// `$XDG_DATA_HOME/encodecraft` or `~/.local/share/encodecraft`.
pub fn linux_project_dirs_data(xdg_or_share: impl AsRef<Path>) -> PathBuf {
    xdg_or_share.as_ref().join("encodecraft")
}

/// Resolve data dirs from the same env names EncodeCraft reads.
///
/// Windows / macOS / Linux candidates are all constructed whenever the matching env value is
/// provided, so the Windows `\data` suffix can be tested on Linux CI. Duplicate paths are skipped.
pub fn data_dirs_from(encodecraft_home: Option<&str>, xdg_data_home: Option<&str>, user_home: Option<&str>, appdata: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if is_plausible_data_dir(&p) && !out.contains(&p) {
            out.push(p);
        }
    };
    if let Some(h) = encodecraft_home.map(str::trim).filter(|h| !h.is_empty()) {
        push(PathBuf::from(h));
    }
    if let Some(app) = appdata.map(str::trim).filter(|a| !a.is_empty()) {
        // directories 6: `{RoamingAppData}\{org}\{app}\data`. Legacy: without `\data`.
        push(windows_project_dirs_data(app));
        push(windows_project_dirs_legacy(app));
    }
    if let Some(home) = user_home.map(str::trim).filter(|h| !h.is_empty()) {
        push(macos_project_dirs_data(home));
        push(PathBuf::from(home).join(".local/share").join("encodecraft"));
    }
    if let Some(xdg) = xdg_data_home.map(str::trim).filter(|x| !x.is_empty()) {
        push(linux_project_dirs_data(xdg));
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
fn insert_native_project_dirs(dirs: &mut Vec<PathBuf>) {
    let Some(pd) = directories::ProjectDirs::from(PROJECT_QUALIFIER, PROJECT_ORGANIZATION, PROJECT_APPLICATION) else {
        return;
    };
    let p = pd.data_dir().to_path_buf();
    if !is_plausible_data_dir(&p) || dirs.iter().any(|d| d == &p) {
        return;
    }
    let home_set = std::env::var(HOME_ENV).ok().as_deref().map(str::trim).is_some_and(|h| !h.is_empty());
    let at = if home_set { 1.min(dirs.len()) } else { 0 };
    dirs.insert(at, p);
}

fn is_plausible_data_dir(dir: &Path) -> bool {
    let raw = dir.to_string_lossy();
    !raw.is_empty() && raw.len() <= 4096 && !raw.contains('\0') && dir.is_absolute() && !dir.components().any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Inbox folder: `<first data dir>/inbox/`.
pub fn inbox_dir() -> Option<PathBuf> {
    data_dirs().into_iter().next().map(|d| d.join("inbox"))
}

/// True when a `/health` JSON body is EncodeCraft.
pub fn is_health_ok(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    v.get("ok").and_then(serde_json::Value::as_bool) == Some(true) && v.get("product").and_then(serde_json::Value::as_str) == Some("EncodeCraft")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ec-job-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn golden_effectcraft_enqueue_body() {
        let j = Job::effectcraft("/work/spot.ecproj", "Main");
        let s = serde_json::to_string(&j).unwrap();
        assert_eq!(
            s,
            r#"{"schema":1,"source":{"kind":"effectcraft","project":"/work/spot.ecproj","comp":"Main","mezzanine":"prores","work_area":false},"preset_id":"system.h264-mp4","start_queue":true}"#
        );
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["schema"], 1);
        assert!(v["schema"].is_u64(), "schema must be an integer, not a string: {v}");
        assert_eq!(serde_json::from_str::<Job>(&s).unwrap(), j);
    }

    #[test]
    fn schema_is_integer_not_string() {
        let j = Job::effectcraft("/tmp/demo.ecproj", "Main");
        let v: Value = serde_json::to_value(&j).unwrap();
        assert_eq!(v["schema"], json!(1));
        assert!(v["schema"].as_u64().is_some());
        assert!(v.get("project").is_none(), "project lives under source, not at the root: {v}");
        assert!(v.get("composition").is_none(), "{v}");
        assert_eq!(v["source"]["kind"], "effectcraft");
        assert_eq!(v["source"]["comp"], "Main");
        assert_eq!(v["preset_id"], DEFAULT_PRESET_ID);
        assert_eq!(v["start_queue"], true);
        assert!(v.get("output_dir").is_none(), "output_dir must stay unset by default: {v}");
    }

    #[test]
    fn file_source_and_optional_fields_round_trip() {
        let j = Job {
            schema: SCHEMA_V1,
            source: Source::File { path: "/tmp/in.mov".into() },
            preset_id: "system.h264-mp4".into(),
            preset_override: Some(json!({"crf": 18})),
            output_dir: None,
            naming: Some("{name}_{width}x{height}".into()),
            sizes: vec![OutputSize { name: "HD".into(), width: 1920, height: 1080 }],
            trim: Some(Trim { in_sec: 1.0, out_sec: None }),
            scale: Some(ScaleMode::Fit),
            start_queue: true,
        };
        let v: Value = serde_json::to_value(&j).unwrap();
        assert_eq!(v["source"]["kind"], "file");
        assert_eq!(v["source"]["path"], "/tmp/in.mov");
        assert_eq!(v["scale"], "fit");
        assert_eq!(v["sizes"][0]["width"], 1920);
        assert!(v["trim"]["out_sec"].is_null() || v["trim"].get("out_sec").is_none());
        assert_eq!(serde_json::from_value::<Job>(v).unwrap(), j);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let j: Job = serde_json::from_value(json!({
            "schema": 1,
            "source": {"kind": "effectcraft", "project": "/a.ecproj", "comp": "A"},
            "preset_id": "system.h264-mp4",
            "extraFutureField": true
        }))
        .unwrap();
        assert_eq!(j.composition(), Some("A"));
        assert_eq!(j.schema, 1);
        match &j.source {
            Source::Effectcraft { mezzanine, work_area, .. } => {
                assert_eq!(mezzanine, DEFAULT_MEZZANINE);
                assert!(!*work_area);
            }
            Source::File { .. } => panic!("expected effectcraft source"),
        }
    }

    #[test]
    fn control_request_skips_missing_token() {
        let c = ControlRequest::default();
        let v = serde_json::to_value(&c).unwrap();
        assert!(v.as_object().unwrap().is_empty());
        let c2: ControlRequest = serde_json::from_value(json!({"token": "abc"})).unwrap();
        assert_eq!(c2.token.as_deref(), Some("abc"));
    }

    #[test]
    fn json_line_includes_token() {
        let line = JsonLine { id: json!(1), method: "enqueue".into(), params: json!({}), token: Some("t".into()) };
        let v = serde_json::to_value(&line).unwrap();
        assert_eq!(v["token"], "t");
        assert_eq!(v["method"], "enqueue");
    }

    #[test]
    fn health_ok_requires_product() {
        assert!(is_health_ok(r#"{"ok":true,"product":"EncodeCraft"}"#));
        assert!(!is_health_ok(r#"{"ok":true}"#));
        assert!(!is_health_ok(r#"{"ok":false,"product":"EncodeCraft"}"#));
        assert!(!is_health_ok("not json"));
    }

    #[test]
    fn sanitize_token_rejects_injection() {
        assert_eq!(sanitize_token("  abc  ").as_deref(), Some("abc"));
        assert!(sanitize_token("").is_none());
        assert!(sanitize_token("   ").is_none());
        assert!(sanitize_token("ab\r\nX").is_none());
    }

    #[test]
    fn enqueue_error_uses_plain_language() {
        assert_eq!(
            enqueue_error_message(400, r#"{"ok":false,"ids":[],"error":"the output folder is not configured"}"#),
            "EncodeCraft could not add this composition to the queue: the output folder is not configured"
        );
        assert_eq!(
            enqueue_error_message(400, r#"invalid type: string "encodecraft.job/v1", expected u32 at line 1 column 30"#),
            r#"EncodeCraft could not add this composition to the queue: invalid type: string "encodecraft.job/v1", expected u32 at line 1 column 30"#
        );
        assert_eq!(enqueue_error_message(503, ""), "EncodeCraft could not add this composition to the queue (HTTP 503).");
        assert_eq!(preset_id_from_hint("h264"), DEFAULT_PRESET_ID);
        assert_eq!(preset_id_from_hint("system.hevc-mp4"), "system.hevc-mp4");
    }

    #[test]
    fn discover_ipc_token_env_wins_over_file() {
        let dir = tmp();
        std::fs::write(dir.join(TOKEN_FILE), "from-file\n").unwrap();
        let dirs = [dir.clone()];
        assert_eq!(discover_ipc_token_in(None, &dirs).as_deref(), Some("from-file"));
        assert_eq!(discover_ipc_token_in(Some("  from-env  "), &dirs).as_deref(), Some("from-env"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_ipc_token_searches_home_then_xdg() {
        let home = tmp();
        let xdg_root = tmp();
        let xdg = xdg_root.join("encodecraft");
        std::fs::create_dir_all(&xdg).unwrap();
        std::fs::write(xdg.join(TOKEN_FILE), "from-xdg").unwrap();
        let dirs = data_dirs_from(Some(&home.to_string_lossy()), Some(&xdg_root.to_string_lossy()), None, None);
        assert_eq!(dirs.first(), Some(&home));
        assert_eq!(discover_ipc_token_in(None, &dirs).as_deref(), Some("from-xdg"), "empty HOME dir falls through");
        std::fs::write(home.join(TOKEN_FILE), "from-home").unwrap();
        assert_eq!(discover_ipc_token_in(None, &dirs).as_deref(), Some("from-home"));
        assert_eq!(dirs[0].join("inbox"), home.join("inbox"));
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&xdg_root);
    }

    #[test]
    fn data_dirs_home_env_is_first() {
        let dirs = data_dirs_from(Some("/tmp/ec-home-test"), Some("/tmp/xdg-test"), Some("/home/user"), None);
        assert_eq!(dirs[0], PathBuf::from("/tmp/ec-home-test"));
        assert!(dirs.iter().any(|d| d.ends_with("encodecraft")));
    }

    #[test]
    fn windows_appdata_uses_projectdirs_data_suffix() {
        let appdata = "/tmp/Roaming";
        let dirs = data_dirs_from(None, None, None, Some(appdata));
        let data = windows_project_dirs_data(appdata);
        let legacy = windows_project_dirs_legacy(appdata);
        assert_eq!(dirs.first(), Some(&data), "ProjectDirs data_dir must come before the legacy folder: {dirs:?}");
        assert!(dirs.iter().any(|d| d == &legacy), "legacy %APPDATA%\\EncodeCraft\\EncodeCraft fallback missing: {dirs:?}");
        assert!(data.ends_with("EncodeCraft/EncodeCraft/data") || data.ends_with(r"EncodeCraft\EncodeCraft\data"));
    }

    #[test]
    fn home_env_beats_windows_appdata() {
        let dirs = data_dirs_from(Some("/tmp/ec-home-test"), None, None, Some("/tmp/Roaming"));
        assert_eq!(dirs[0], PathBuf::from("/tmp/ec-home-test"));
        assert!(dirs.iter().any(|d| d == &windows_project_dirs_data("/tmp/Roaming")));
    }

    #[test]
    fn macos_and_linux_match_projectdirs() {
        let mac = macos_project_dirs_data("/Users/dev");
        assert_eq!(mac, PathBuf::from("/Users/dev/Library/Application Support/dev.EncodeCraft.EncodeCraft"));
        let linux = linux_project_dirs_data("/home/dev/.local/share");
        assert_eq!(linux, PathBuf::from("/home/dev/.local/share/encodecraft"));
        let dirs = data_dirs_from(None, Some("/tmp/xdg"), Some("/Users/dev"), None);
        assert!(dirs.iter().any(|d| d == &mac));
        assert!(dirs.iter().any(|d| d == &PathBuf::from("/tmp/xdg/encodecraft")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_project_dirs_crate_matches_constructed_path() {
        let Some(pd) = directories::ProjectDirs::from(PROJECT_QUALIFIER, PROJECT_ORGANIZATION, PROJECT_APPLICATION) else {
            return;
        };
        let data = pd.data_dir().to_path_buf();
        if !is_plausible_data_dir(&data) {
            return;
        }
        let dirs = data_dirs();
        assert!(dirs.iter().any(|d| d == &data), "data_dirs {dirs:?} must include ProjectDirs {}", data.display());
    }
}
