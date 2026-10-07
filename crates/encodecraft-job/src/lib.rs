//! Shared EncodeCraft queue-job schema (vendored from EncodeCraft `crates/job`).
//!
//! A job is JSON. EffectCraft POSTs it to `http://127.0.0.1:9878/v1/enqueue` and, when
//! EncodeCraft is not listening, writes the same object into EncodeCraft's inbox.
//!
//! Provenance: copied from <https://cursor.com/codebase/devmaroli/encodecraft> (`crates/job`),
//! MIT OR Apache-2.0. This crate is serde-only so this repo builds without a sibling checkout.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::{Deserialize, Serialize};

/// Schema id written on every job this crate produces.
pub const SCHEMA_V1: &str = "encodecraft.job/v1";

/// EncodeCraft's default enqueue endpoint (loopback only).
pub const DEFAULT_ENQUEUE_URL: &str = "http://127.0.0.1:9878/v1/enqueue";

/// Default TCP port EncodeCraft listens on.
pub const DEFAULT_PORT: u16 = 9878;

/// App id EffectCraft writes in [`Job::source`].
pub const SOURCE_EFFECTCRAFT: &str = "effectcraft";

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
    pub fn effectcraft(project: impl Into<String>, name: impl Into<String>, id: Option<u64>) -> Self {
        Self {
            schema: SCHEMA_V1.into(),
            id: None,
            project: project.into(),
            composition: name.into(),
            composition_id: id,
            output: None,
            format: None,
            source: SOURCE_EFFECTCRAFT.into(),
            source_version: Some(env!("CARGO_PKG_VERSION").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

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
}
