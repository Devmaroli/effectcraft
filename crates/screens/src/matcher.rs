//! Size Matcher: pre-render completeness against selected comps (and optional export files).

use serde::{Deserialize, Serialize};

use crate::combiner::{AcceptedSize, accepted_sizes_for, extra_stacked, size_matches_accepted};
use crate::inventory::Library;
use crate::manager::JobMode;
use crate::normalize::{format_size, normalize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompProbe {
    pub name: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub duration_s: f64,
    #[serde(default)]
    pub fps: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRow {
    pub screen: String,
    pub needs: String,
    pub status: String,
    pub detail: String,
    pub duplicated: bool,
    pub found: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReport {
    pub pass: bool,
    pub mode: JobMode,
    pub rows: Vec<MatchRow>,
    pub missing: Vec<String>,
    pub mismatches: Vec<String>,
    pub duplicated_ok: Vec<String>,
    pub oversized: Vec<String>,
    pub summary: String,
}

pub fn check_comps(lib: &Library, required: &[String], comps: &[CompProbe], mode: JobMode) -> MatchReport {
    let mut rows = Vec::new();
    let mut missing = Vec::new();
    let mut mismatches = Vec::new();
    let mut duplicated_ok = Vec::new();
    let mut oversized = Vec::new();
    for name in required {
        let accepted = accepted_sizes_for(lib, name);
        let accepted = if accepted.is_empty() { fallback_accepted(lib, name) } else { accepted };
        let lang_ok = bilingual_pair(comps, name);
        let named: Vec<&CompProbe> = comps.iter().filter(|c| name_fits(name, &c.name, mode)).collect();
        let over: Vec<&CompProbe> = named.iter().copied().filter(|c| extra_stacked(&accepted, c.width, c.height).is_some()).collect();
        if !over.is_empty() {
            for c in over {
                let Some((ew, eh, extra)) = extra_stacked(&accepted, c.width, c.height) else { continue };
                let piece = if extra == 1 { "piece" } else { "pieces" };
                let msg = format!(
                    "Look out — “{}” is larger than the normal size for this screen. Expected {ew}×{eh}, actual {}×{} ({extra} extra {piece} stacked vertically).",
                    c.name, c.width, c.height
                );
                oversized.push(msg.clone());
                mismatches.push(msg.clone());
                rows.push(MatchRow {
                    screen: name.clone(),
                    needs: needs_label(&accepted),
                    status: "sizeMismatch".into(),
                    detail: msg,
                    duplicated: false,
                    found: Some(c.name.clone()),
                });
            }
            continue;
        }
        let hit = named.iter().copied().find(|c| size_matches_accepted(&accepted, c.width, c.height).is_some());
        if let Some(c) = hit {
            let kind = size_matches_accepted(&accepted, c.width, c.height);
            let duplicated = kind.is_some_and(|k| k.duplicated);
            let detail =
                if duplicated { kind.map(|k| k.label.clone()).unwrap_or_default() } else { format!("Found {} ({})", c.name, format_size(c.width, c.height)) };
            if duplicated {
                duplicated_ok.push(format!("{}: {}", name, detail));
            }
            let mut status = "pass".to_string();
            let mut extra = detail;
            if c.fps > 0.0 && (c.fps - 25.0).abs() > 0.01 {
                status = "flag".into();
                extra = format!("{extra} · frame rate is {} (studio rule: 25 fps)", c.fps);
            }
            if lang_ok.as_ref().is_some_and(|s| s != "both") {
                extra = format!("{extra} · {}", lang_ok.unwrap_or_default());
            }
            rows.push(MatchRow { screen: name.clone(), needs: needs_label(&accepted), status, detail: extra, duplicated, found: Some(c.name.clone()) });
        } else if let Some(c) = named.first().copied() {
            let msg = format!(
                "Name matches “{}” but the size is {}×{} (needed {}). Nothing was skipped silently.",
                c.name,
                c.width,
                c.height,
                needs_label(&accepted)
            );
            mismatches.push(msg.clone());
            rows.push(MatchRow {
                screen: name.clone(),
                needs: needs_label(&accepted),
                status: "sizeMismatch".into(),
                detail: msg,
                duplicated: false,
                found: Some(c.name.clone()),
            });
        } else {
            let msg = format!("Missing — no composition for “{name}” at {}.", needs_label(&accepted));
            missing.push(msg.clone());
            rows.push(MatchRow { screen: name.clone(), needs: needs_label(&accepted), status: "missing".into(), detail: msg, duplicated: false, found: None });
        }
    }
    let pass = missing.is_empty() && mismatches.is_empty() && rows.iter().all(|r| r.status == "pass" || r.status == "flag");
    let summary = if pass {
        format!("PASS — {} / {} screens ready", rows.len(), rows.len())
    } else {
        format!("Not ready — {} missing, {} size mismatch. Fix and check again before EncodeCraft.", missing.len(), mismatches.len())
    };
    MatchReport { pass, mode, rows, missing, mismatches, duplicated_ok, oversized, summary }
}

fn fallback_accepted(lib: &Library, name: &str) -> Vec<AcceptedSize> {
    if let Some(p) = lib.preset_by_name(name) {
        return vec![AcceptedSize { width: p.width, height: p.height, duplicated: false, copies: 1, label: format_size(p.width, p.height) }];
    }
    if let Some(s) = lib.screen_by_name(name) {
        return vec![AcceptedSize { width: s.width, height: s.height, duplicated: false, copies: 1, label: format_size(s.width, s.height) }];
    }
    Vec::new()
}

fn needs_label(accepted: &[AcceptedSize]) -> String {
    accepted
        .iter()
        .map(|a| if a.duplicated { format!("{}×{} (duplicated deliverable)", a.width, a.height) } else { format!("{}×{}", a.width, a.height) })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn name_fits(screen: &str, comp: &str, mode: JobMode) -> bool {
    let (s, c) = (normalize(screen), normalize(comp));
    if s.is_empty() {
        return true;
    }
    if c.contains(&s) || s.contains(&c) {
        return true;
    }
    if mode == JobMode::BySize {
        if crate::normalize::compact_size(screen) == "17hd"
            && (c.contains("1.7") || c.contains("1920") || c.contains("jahra") || c.contains("salmiya") || crate::normalize::compact_size(comp) == "17hd")
        {
            return true;
        }
        if s == "2.6" && c.contains("2.6") {
            return true;
        }
        if (s.contains("baitak") || s.contains("top gear")) && (c.contains("baitak") || c.contains("4.3")) {
            return true;
        }
        if (s.contains("diamond") || s.contains("quartz")) && (c.contains("diamond") || c.contains("quartz")) {
            return true;
        }
        return crate::fuzzy::token_score(screen, comp) >= 0.5;
    }
    // Screen-specific: 90% name, no aliases.
    crate::fuzzy::names_match_90(screen, comp) || c.split('_').any(|part| crate::fuzzy::names_match_90(screen, part))
}

fn bilingual_pair(comps: &[CompProbe], screen: &str) -> Option<String> {
    let s = normalize(screen);
    let hits: Vec<&CompProbe> = comps.iter().filter(|c| normalize(&c.name).contains(&s) || name_fits(screen, &c.name, JobMode::BySize)).collect();
    if hits.is_empty() {
        return None;
    }
    let ar = hits.iter().any(|c| lang_tag(&c.name) == "ar");
    let en = hits.iter().any(|c| lang_tag(&c.name) == "en");
    if ar && en {
        Some("both".into())
    } else if ar {
        Some("AR only".into())
    } else if en {
        Some("EN only".into())
    } else {
        Some("both".into()) // untagged = general
    }
}

fn lang_tag(name: &str) -> &'static str {
    let n = name.to_ascii_uppercase();
    if n.contains("_AR") || n.contains("/AR/") || n.ends_with("_AR") {
        "ar"
    } else if n.contains("_EN") || n.contains("_ENG") || n.contains("/EN/") {
        "en"
    } else {
        "general"
    }
}
