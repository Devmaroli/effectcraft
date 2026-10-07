//! Shared EncodeCraft queue-job schema (vendored from EncodeCraft `crates/job`).
//!
//! A job is JSON. EffectCraft POSTs it to `http://127.0.0.1:9878/v1/enqueue` with
//! `X-EncodeCraft-Token` and, when EncodeCraft is not listening, writes the same object
//! into EncodeCraft's inbox (`<data dir>/inbox/`).
//!
//! Provenance: copied from <https://cursor.com/codebase/devmaroli/encodecraft> (`crates/job`
//! and `docs/job-format.md`), MIT OR Apache-2.0.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Schema id written on every job this crate produces.
pub const SCHEMA_V1: &str = "encodecraft.job/v1";

/// EncodeCraft's default enqueue endpoint (loopback only).
pub const DEFAULT_ENQUEUE_URL: &str = "http://127.0.0.1:9878/v1/enqueue";

/// Unauthenticated liveness probe. Body: `{"ok":true,"product":"EncodeCraft"}`.
pub const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:9878/health";

/// Default TCP port EncodeCraft listens on.
pub const DEFAULT_PORT: u16 = 9878;

/// App id EffectCraft writes in [`Job::source`].
pub const SOURCE_EFFECTCRAFT: &str = "effectcraft";

/// Env var that supplies the IPC token, overriding the on-disk file.
pub const TOKEN_ENV: &str = "ENCODECRAFT_TOKEN";

/// HTTP header carrying the IPC token (`POST /v1/enqueue`, `GET /v1/queue`, `POST /v1/control`).
pub const TOKEN_HEADER: &str = "X-EncodeCraft-Token";

/// Filename EncodeCraft writes on first launch (`<data dir>/ipc-token`).
pub const TOKEN_FILE: &str = "ipc-token";

/// Env var that overrides EncodeCraft's data directory.
pub const HOME_ENV: &str = "ENCODECRAFT_HOME";

/// Maximum JSON body for `/v1/enqueue` and `/v1/control`.
pub const MAX_CONTROL_BODY: usize = 256 * 1024;

/// Maximum number of size presets on a control request.
pub const MAX_SIZES: usize = 64;

fn schema_v1() -> String {
    SCHEMA_V1.into()
}

fn source_effectcraft() -> String {
    SOURCE_EFFECTCRAFT.into()
}

/// One composition to render, pointed at a **saved** EffectCraft project file.
///
/// EncodeCraft reads `project` from disk and calls `effectcraft-cli` to render
/// `composition` (by name, falling back to `compositionId`).
///
/// Do **not** set [`Job::output_dir`] unless the path is inside EncodeCraft's configured
/// output folder; leave it unset and EncodeCraft chooses a unique name (`-2`, `-3`, …).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// `encodecraft.job/v1`.
    #[serde(default = "schema_v1")]
    pub schema: String,
    /// Sender-generated id (inbox file stem, HTTP `id` echo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Absolute path of a saved `.ecproj` / `.ecprojx` file.
    #[serde(alias = "project_path", alias = "projectPath", alias = "path")]
    pub project: String,
    /// Composition name as shown in the Project panel.
    #[serde(alias = "comp", alias = "compName", alias = "composition_name")]
    pub composition: String,
    /// EffectCraft composition item id, when the sender knows it.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "composition_id", alias = "compId")]
    pub composition_id: Option<u64>,
    /// Optional destination file EncodeCraft should encode to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Optional output folder. Must be inside EncodeCraft's configured output dir; omit by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    /// Optional container/codec hint (`h264`, `hevc`, `prores`, `webm`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Sending application (`effectcraft`).
    #[serde(default = "source_effectcraft")]
    pub source: String,
    /// `CARGO_PKG_VERSION` of the sender, when known.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "source_version")]
    pub source_version: Option<String>,
}

impl Job {
    /// A job for the composition `name` / `id` in the saved project at `project`.
    ///
    /// `output_dir` is left unset so EncodeCraft writes under its own output folder.
    pub fn effectcraft(project: impl Into<String>, name: impl Into<String>, id: Option<u64>) -> Self {
        Self {
            schema: SCHEMA_V1.into(),
            id: None,
            project: project.into(),
            composition: name.into(),
            composition_id: id,
            output: None,
            output_dir: None,
            format: None,
            source: SOURCE_EFFECTCRAFT.into(),
            source_version: Some(env!("CARGO_PKG_VERSION").into()),
        }
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
/// Data dirs, in order: `$ENCODECRAFT_HOME`; Windows `%APPDATA%\EncodeCraft\EncodeCraft`;
/// macOS `~/Library/Application Support/dev.EncodeCraft.EncodeCraft`; elsewhere
/// `$XDG_DATA_HOME/encodecraft` then `~/.local/share/encodecraft`.
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
pub fn data_dirs() -> Vec<PathBuf> {
    data_dirs_from(
        std::env::var(HOME_ENV).ok().as_deref(),
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
    )
}

/// Resolve data dirs from the same env names EncodeCraft reads.
#[allow(unused_variables)]
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
    #[cfg(target_os = "windows")]
    {
        if let Some(app) = appdata.map(str::trim).filter(|a| !a.is_empty()) {
            push(PathBuf::from(app).join("EncodeCraft").join("EncodeCraft"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = user_home.map(str::trim).filter(|h| !h.is_empty()) {
            push(PathBuf::from(home).join("Library/Application Support/dev.EncodeCraft.EncodeCraft"));
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(xdg) = xdg_data_home.map(str::trim).filter(|x| !x.is_empty()) {
            push(PathBuf::from(xdg).join("encodecraft"));
        }
        if let Some(home) = user_home.map(str::trim).filter(|h| !h.is_empty()) {
            push(PathBuf::from(home).join(".local/share/encodecraft"));
        }
    }
    out
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
    fn round_trip_camel_case() {
        let mut j = Job::effectcraft("/tmp/demo.ecproj", "Main", Some(2));
        j.id = Some("ec-1".into());
        j.output = Some("/tmp/out.mp4".into());
        j.format = Some("h264".into());
        let v: Value = serde_json::to_value(&j).unwrap();
        assert_eq!(v["schema"], SCHEMA_V1);
        assert_eq!(v["project"], "/tmp/demo.ecproj");
        assert_eq!(v["composition"], "Main");
        assert_eq!(v["compositionId"], 2);
        assert_eq!(v["source"], "effectcraft");
        assert_eq!(v["sourceVersion"], env!("CARGO_PKG_VERSION"));
        assert!(v.get("outputDir").is_none(), "output_dir must stay unset by default: {v}");
        assert_eq!(serde_json::from_value::<Job>(v).unwrap(), j);
    }

    #[test]
    fn aliases_from_snake_and_short_names() {
        let j: Job = serde_json::from_value(json!({
            "project_path": "C:/work/t.ecproj",
            "comp": "Title",
            "composition_id": 9,
            "source_version": "0.4.0"
        }))
        .unwrap();
        assert_eq!(j.project, "C:/work/t.ecproj");
        assert_eq!(j.composition, "Title");
        assert_eq!(j.composition_id, Some(9));
        assert_eq!(j.source, "effectcraft");
        assert_eq!(j.schema, SCHEMA_V1);
        assert!(j.output_dir.is_none());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let j: Job = serde_json::from_value(json!({
            "project": "/a.ecproj",
            "composition": "A",
            "extraFutureField": true
        }))
        .unwrap();
        assert_eq!(j.composition, "A");
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
}
