//! Screen Manager paste matching: size-based (aliases) vs screen-specific (90% name + size, no aliases).

use serde::{Deserialize, Serialize};

use crate::combiner::{CombinerLayout, DupWarning, active_combiners};
use crate::fuzzy::{names_match_90, similarity};
use crate::inventory::{Library, Preset};
use crate::normalize::{compact_size, format_size, normalize, split_paste};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum JobMode {
    #[default]
    BySize,
    ScreenSpecific,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerMatch {
    pub asked: String,
    pub preset: String,
    pub width: u32,
    pub height: u32,
    pub score: f32,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSelection {
    pub mode: JobMode,
    pub names: Vec<String>,
    pub selected: Vec<String>,
    pub show_selected_only: bool,
    pub matches: Vec<ManagerMatch>,
    pub combiners: Vec<CombinerLayout>,
    pub warnings: Vec<DupWarning>,
}

/// Paste aliases used in **size** mode only (Screen Manager `PASTE_SELECTION_ALIASES`).
fn paste_alias(lib: &Library, token: &str) -> Vec<String> {
    let q = normalize(token);
    if q.is_empty() {
        return Vec::new();
    }
    let compact = compact_size(token);
    if compact == "26" || q == "2 6 screen" {
        return vec!["2.6 Screen".into()];
    }
    if compact == "17hd" {
        return vec!["1.7HD".into()];
    }
    if q.contains("thuraya") && q.contains("combined") {
        return lib.presets.iter().filter(|p| normalize(&p.name).contains("thuraya")).map(|p| p.name.clone()).collect();
    }
    if q == "marina palms full" || q == "marina palm trees" || q.contains("palms full") {
        if let Some(p) = lib.presets.iter().find(|p| crate::inventory::is_palm_trees(&p.name)) {
            return vec![p.name.clone()];
        }
        return vec!["Marina - Palm Trees".into()];
    }
    if q.contains("piccadilly") && q.contains("sync") {
        return vec!["Piccadilly".into()];
    }
    if q.contains("topaz") {
        return lib.presets.iter().filter(|p| normalize(&p.name).contains("topaz")).map(|p| p.name.clone()).collect();
    }
    Vec::new()
}

pub fn select_pasted(lib: &Library, paste: &[String], mode: JobMode) -> ManagerSelection {
    let mut names = Vec::new();
    for raw in paste {
        for token in split_paste(raw, true) {
            match mode {
                JobMode::BySize => {
                    let aliased = paste_alias(lib, &token);
                    if !aliased.is_empty() {
                        names.extend(aliased);
                    } else {
                        names.push(token);
                    }
                }
                JobMode::ScreenSpecific => names.push(token),
            }
        }
    }
    names = unique_keep(names);
    let matches: Vec<ManagerMatch> = names.iter().map(|n| match_one(lib, n, mode)).collect();
    // Keep the asked / cover name (Top Gear, Quartz, Grand Avenues) so comps are
    // named after what was booked, not the SM pool stand-in.
    let selected: Vec<String> = matches.iter().filter(|m| m.status == "ok").map(|m| m.asked.clone()).collect();
    let combiners = if mode == JobMode::BySize { active_combiners(lib, &selected) } else { Vec::new() };
    let warnings: Vec<DupWarning> = combiners.iter().flat_map(|c| c.warnings.clone()).collect();
    ManagerSelection { mode, names: names.clone(), selected, show_selected_only: true, matches, combiners, warnings }
}

fn unique_keep(v: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for x in v {
        if !out.iter().any(|y: &String| normalize(y) == normalize(&x)) {
            out.push(x);
        }
    }
    out
}

fn match_one(lib: &Library, asked: &str, mode: JobMode) -> ManagerMatch {
    match mode {
        JobMode::BySize => match_by_size(lib, asked),
        JobMode::ScreenSpecific => match_screen_specific(lib, asked),
    }
}

fn match_by_size(lib: &Library, asked: &str) -> ManagerMatch {
    let q = normalize(asked);
    if let Some(p) = lib.presets.iter().find(|p| normalize(&p.name) == q) {
        return ok(asked, p, 1.0, "Exact preset name");
    }
    // Unique partial ≥4 chars.
    if q.chars().count() >= 4 {
        let hits: Vec<&Preset> = lib.presets.iter().filter(|p| normalize(&p.name).contains(&q) || q.contains(&normalize(&p.name))).collect();
        if hits.len() == 1
            && let Some(p) = hits.first()
        {
            return ok(asked, p, 0.95, "Unique partial name");
        }
    }
    if let Some(s) = lib.screen_by_name(asked).or_else(|| lib.screens.iter().find(|s| crate::fuzzy::token_score(asked, &s.name) >= 0.68)) {
        if let Some(alias) = s.format_alias()
            && let Some(p) = lib.preset_by_name(alias)
        {
            return ok(asked, p, 0.9, &format!("Size pool {alias}"));
        }
        if let Some(p) = lib.presets.iter().find(|p| p.width == s.width && p.height == s.height) {
            return ok(asked, p, 0.85, "Same pixel size");
        }
    }
    not_found(asked, "Not found in Screen Manager. Nothing was substituted.")
}

fn match_screen_specific(lib: &Library, asked: &str) -> ManagerMatch {
    // No aliases: Baitak is not Top Gear; Diamond is not Quartz.
    let mut best: Option<(&Preset, f32)> = None;
    for p in &lib.presets {
        let sc = similarity(asked, &p.name);
        if names_match_90(asked, &p.name) && best.as_ref().is_none_or(|(_, b)| sc > *b) {
            best = Some((p, sc));
        }
    }
    // Inventory screens that are not SM presets still count (each is its own screen).
    let inv = lib
        .screens
        .iter()
        .filter(|s| names_match_90(asked, &s.name) || s.aliases.iter().any(|a| names_match_90(asked, a)))
        .max_by(|a, b| similarity(asked, &a.name).partial_cmp(&similarity(asked, &b.name)).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((p, sc)) = best {
        if let Some(s) = inv
            && (s.width != p.width || s.height != p.height)
            && names_match_90(asked, &s.name)
        {
            return ManagerMatch {
                asked: asked.into(),
                preset: s.name.clone(),
                width: s.width,
                height: s.height,
                score: sc,
                status: "sizeMismatch".into(),
                detail: format!(
                    "The name matches “{}”, but the size does not: booked {}×{} vs Screen Manager {}×{}. Nothing was substituted.",
                    s.name, s.width, s.height, p.width, p.height
                ),
            };
        }
        return ok(asked, p, sc, "Screen-specific name (≥90%)");
    }
    if let Some(s) = inv {
        // Use the inventory screen itself (no alias substitution).
        return ManagerMatch {
            asked: asked.into(),
            preset: s.name.clone(),
            width: s.width,
            height: s.height,
            score: similarity(asked, &s.name),
            status: "ok".into(),
            detail: "Screen-specific inventory match (no aliases)".into(),
        };
    }
    not_found(asked, "Not found as its own screen (screen-specific: no aliases, no size-pool stand-in).")
}

fn ok(asked: &str, p: &Preset, score: f32, detail: &str) -> ManagerMatch {
    ManagerMatch { asked: asked.into(), preset: p.name.clone(), width: p.width, height: p.height, score, status: "ok".into(), detail: detail.into() }
}

fn not_found(asked: &str, detail: &str) -> ManagerMatch {
    ManagerMatch { asked: asked.into(), preset: String::new(), width: 0, height: 0, score: 0.0, status: "notFound".into(), detail: detail.into() }
}

pub fn size_mismatch_report_line(m: &ManagerMatch) -> String {
    format!("{}: {} ({})", m.asked, m.detail, format_size(m.width, m.height))
}
