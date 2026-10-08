//! Size sorter: paste booking names → match inventory → unique production sizes + flags.

use serde::{Deserialize, Serialize};

use crate::combiner::accepted_sizes_for;
use crate::fuzzy::{similarity, token_score};
use crate::inventory::{Library, Screen, cover_export_name, is_al_salam_sync, is_avenues_entrance, is_palm_trees, is_thuraya, is_yaal_slayel, size_mode_alias};
use crate::normalize::{compact_size, format_size, normalize, parse_pixel_size, split_paste};

pub const FLEXIBLE_MIN: f32 = 0.45;
pub const REVIEW_MIN: f32 = 0.68;
pub const DUPLICATE_MIN: f32 = 0.88;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MatchMode {
    #[default]
    Flexible,
    Strict,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterFilters {
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub governorate: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub search: String,
}

impl SorterFilters {
    pub fn is_empty(&self) -> bool {
        self.group.is_empty() && self.kind.is_empty() && self.governorate.is_empty() && self.category.is_empty() && self.search.is_empty()
    }

    pub fn allows(&self, s: &Screen) -> bool {
        let hit = |want: &str, got: &str| want.is_empty() || normalize(got) == normalize(want);
        if !hit(&self.group, &s.group) || !hit(&self.kind, &s.kind) || !hit(&self.governorate, &s.governorate) || !hit(&self.category, &s.category) {
            return false;
        }
        if self.search.is_empty() {
            return true;
        }
        let q = normalize(&self.search);
        if compact_size(&self.search) == "17hd" {
            return s.width == 1920 && s.height == 1080;
        }
        if compact_size(&self.search) == "26" {
            return s.width == 1536 && s.height == 576;
        }
        if q == "square" {
            return s.width == s.height;
        }
        if q == "vertical" {
            return s.height > s.width;
        }
        if q == "ultra wide" || q == "ultrawide" {
            return s.width as f32 / s.height.max(1) as f32 >= 3.0;
        }
        let blob = normalize(&format!("{} {} {} {}", s.name, s.location, s.group, format_size(s.width, s.height)));
        blob.contains(&q) || parse_pixel_size(&self.search).is_some_and(|(w, h)| (s.width.abs_diff(w) <= 10) && (s.height.abs_diff(h) <= 10))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterHit {
    pub line: String,
    pub screen: String,
    pub width: u32,
    pub height: u32,
    pub score: f32,
    pub reason: String,
    pub alternates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterFlag {
    pub id: String,
    pub kind: String,
    pub message: String,
    pub line: String,
    pub suggestion: String,
    pub alternates: Vec<String>,
    #[serde(default)]
    pub answered: bool,
    #[serde(default)]
    pub pick: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterRow {
    pub size: String,
    pub width: u32,
    pub height: u32,
    pub use_name: String,
    pub covers: Vec<String>,
    pub count: usize,
    pub confidence: u32,
    pub reason: String,
    pub needs_review: bool,
    /// Filters only hide rows. Hidden rows are still sent.
    #[serde(default)]
    pub hidden: bool,
    /// Combined deliverable when it differs from the face (Al Salam 3072×576, Marina 960×960).
    #[serde(default)]
    pub prod_width: u32,
    #[serde(default)]
    pub prod_height: u32,
    /// Covers cell: `← 3 screens`, `← 1 screen`, `Left + Right`, `4 screens`.
    #[serde(default)]
    pub covers_label: String,
    /// Library name when the Entry column shows the pasted name instead.
    #[serde(default)]
    pub library_hint: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterResult {
    pub rows: Vec<SorterRow>,
    pub hits: Vec<SorterHit>,
    pub unmatched: Vec<String>,
    pub flags: Vec<SorterFlag>,
    pub paste_names_by_size: Vec<String>,
    pub paste_names_screen_specific: Vec<String>,
}

pub fn sort_lines(lib: &Library, paste: &str, cleanup: bool, mode: MatchMode, filters: &SorterFilters, _send_all: bool) -> SorterResult {
    let lines = split_paste(paste, cleanup);
    // Match against the full inventory. Filters only hide rows in the panel;
    // hidden screens are still sent to Screen Manager / Size Matcher.
    let inventory: Vec<&Screen> = lib.screens.iter().collect();
    let mut hits = Vec::new();
    let mut unmatched = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let found = match_line(&inventory, line, mode);
        if found.is_empty() {
            unmatched.push(line.clone());
        } else {
            for mut h in found {
                h.line = line.clone();
                let _ = i;
                hits.push(h);
            }
        }
    }
    let mut flags = Vec::new();
    for (i, line) in unmatched.iter().enumerate() {
        flags.push(SorterFlag {
            id: format!("unmatched-{i}"),
            kind: "unmatched".into(),
            message: format!("No screen matched “{line}”. Confirm a screen or pick another — building stays locked until every flag has an answer."),
            line: line.clone(),
            suggestion: String::new(),
            alternates: Vec::new(),
            answered: false,
            pick: String::new(),
        });
    }
    // Duplicates: same inventory screen from more than one line at ≥88%.
    let mut by_screen: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, h) in hits.iter().enumerate() {
        if h.score < DUPLICATE_MIN {
            continue;
        }
        if let Some((_, idx)) = by_screen.iter_mut().find(|(n, _)| n == &h.screen) {
            idx.push(i);
        } else {
            by_screen.push((h.screen.clone(), vec![i]));
        }
    }
    for (name, idx) in by_screen {
        if idx.len() > 1 {
            flags.push(SorterFlag {
                id: format!("dup-{}", normalize(&name).replace(' ', "-")),
                kind: "duplicate".into(),
                message: format!("“{name}” appears more than once in the paste (≥88% match). Remove the extra line or confirm it is really booked twice."),
                line: name.clone(),
                suggestion: name,
                alternates: Vec::new(),
                answered: false,
                pick: String::new(),
            });
        }
    }
    for (i, h) in hits.iter().enumerate() {
        if h.score < REVIEW_MIN {
            flags.push(SorterFlag {
                id: format!("low-{i}"),
                kind: "lowConfidence".into(),
                message: format!("“{}” matched “{}” at {:.0}% — is this the booked screen?", h.line, h.screen, h.score * 100.0),
                line: h.line.clone(),
                suggestion: h.screen.clone(),
                alternates: h.alternates.clone(),
                answered: false,
                pick: String::new(),
            });
        }
        if !h.alternates.is_empty() {
            flags.push(SorterFlag {
                id: format!("amb-{i}"),
                kind: "ambiguous".into(),
                message: format!("“{}” could be “{}” or {}.", h.line, h.screen, h.alternates.join(", ")),
                line: h.line.clone(),
                suggestion: h.screen.clone(),
                alternates: h.alternates.clone(),
                answered: false,
                pick: String::new(),
            });
        }
    }
    let mut rows = group_rows(lib, &hits);
    for row in &mut rows {
        row.hidden = !filters.is_empty() && !row_visible(lib, row, filters);
    }
    let paste_names_by_size: Vec<String> = rows.iter().map(|r| r.use_name.clone()).collect();
    let mut paste_names_screen_specific: Vec<String> = hits.iter().map(|h| h.screen.clone()).collect();
    paste_names_screen_specific.sort();
    paste_names_screen_specific.dedup();
    SorterResult { rows, hits, unmatched, flags, paste_names_by_size, paste_names_screen_specific }
}

fn group_rows(lib: &Library, hits: &[SorterHit]) -> Vec<SorterRow> {
    let mut groups: Vec<((u32, u32, String), Vec<&SorterHit>)> = Vec::new();
    for h in hits {
        let key = group_key(h);
        if let Some((_, v)) = groups.iter_mut().find(|(k, _)| *k == key) {
            v.push(h);
        } else {
            groups.push((key, vec![h]));
        }
    }
    groups.sort_by_key(|a| std::cmp::Reverse(a.1.len()));
    groups
        .into_iter()
        .map(|((w, h, _), items)| {
            let library_names: Vec<String> = items.iter().map(|x| x.screen.clone()).collect();
            let pasted: Vec<String> = items.iter().map(|x| if x.line.is_empty() { x.screen.clone() } else { x.line.clone() }).collect();
            let use_name = cover_export_name((w, h), &pasted);
            let conf = items.iter().map(|x| (x.score * 100.0).round() as u32).max().unwrap_or(0);
            let reason = items.first().map(|x| x.reason.clone()).unwrap_or_default();
            let (prod_width, prod_height, covers_label) = production_covers(lib, w, h, &library_names, items.len());
            let library_hint = library_names.first().filter(|n| normalize(n) != normalize(&use_name)).cloned().unwrap_or_default();
            SorterRow {
                size: format_size(w, h),
                width: w,
                height: h,
                use_name,
                covers: pasted,
                count: items.len(),
                confidence: conf,
                reason,
                needs_review: conf < 68,
                hidden: false,
                prod_width,
                prod_height,
                covers_label,
                library_hint,
            }
        })
        .collect()
}

/// Face size vs combined deliverable, and the Covers pill text.
fn production_covers(lib: &Library, face_w: u32, face_h: u32, names: &[String], pasted_count: usize) -> (u32, u32, String) {
    let probe = names.first().map(String::as_str).unwrap_or("");
    let accepted = accepted_sizes_for(lib, probe);
    let combo = accepted.iter().filter(|a| a.width != face_w || a.height != face_h).max_by_key(|a| a.width.saturating_mul(a.height));
    if let Some(c) = combo {
        let label = if is_al_salam_sync(probe) {
            "Left + Right".into()
        } else if c.copies > 1 {
            format!("{} {}", c.copies, if c.copies == 1 { "screen" } else { "screens" })
        } else {
            screens_pill(pasted_count)
        };
        return (c.width, c.height, label);
    }
    (face_w, face_h, screens_pill(pasted_count))
}

/// `← 1 screen` / `← 3 screens`.
pub fn screens_pill(n: usize) -> String {
    format!("← {n} {}", if n == 1 { "screen" } else { "screens" })
}

/// Entry · size second line: `1920×1080` or `1536×576 → 3072×576`.
pub fn row_size_text(row: &SorterRow) -> String {
    if row.prod_width > 0 && (row.prod_width != row.width || row.prod_height != row.height) {
        format!("{} → {}", format_size(row.width, row.height), format_size(row.prod_width, row.prod_height))
    } else {
        row.size.clone()
    }
}

fn group_key(h: &SorterHit) -> (u32, u32, String) {
    if is_al_salam_sync(&h.screen) {
        return (h.width, h.height, "al-salam-sync".into());
    }
    if is_thuraya(&h.screen) {
        return (h.width, h.height, format!("thuraya-{}", normalize(&h.screen)));
    }
    if is_avenues_entrance(&h.screen) {
        return (h.width, h.height, "avenues-entrance".into());
    }
    if is_yaal_slayel(&h.screen) {
        return (h.width, h.height, format!("yaal-{}", normalize(&h.screen)));
    }
    if is_palm_trees(&h.screen) {
        return (h.width, h.height, "palm-trees".into());
    }
    (h.width, h.height, String::new())
}

fn match_line(inv: &[&Screen], line: &str, mode: MatchMode) -> Vec<SorterHit> {
    if let Some(hits) = associated_group(inv, line) {
        return hits;
    }
    if let Some(hit) = exact_alias(inv, line) {
        return vec![hit];
    }
    if let Some((w, h)) = parse_pixel_size(line) {
        if let Some(s) = inv.iter().find(|s| s.width == w && s.height == h) {
            return vec![hit_of(s, 1.0, "Exact pixel size")];
        }
        if mode == MatchMode::Strict {
            return Vec::new();
        }
        if let Some(s) = inv.iter().min_by_key(|s| s.width.abs_diff(w).saturating_add(s.height.abs_diff(h)))
            && s.width.abs_diff(w) <= 10
            && s.height.abs_diff(h) <= 10
        {
            return vec![hit_of(s, 0.9, "Pixel size within 10 px")];
        }
    }
    let q = normalize(line);
    let compact = compact_size(line);
    if compact == "17hd"
        && let Some(s) = inv.iter().find(|s| s.width == 1920 && s.height == 1080)
    {
        return vec![hit_of(s, 1.0, "Format alias 1.7HD")];
    }
    if compact == "26"
        && let Some(s) = inv.iter().find(|s| s.width == 1536 && s.height == 576 && !is_al_salam_sync(&s.name) && !is_thuraya(&s.name))
    {
        return vec![hit_of(s, 1.0, "Format alias 2.6")];
    }
    if let Some(s) = inv.iter().find(|s| normalize(&s.name) == q || s.aliases.iter().any(|a| normalize(a) == q)) {
        return vec![hit_of(s, 1.0, "Exact screen name")];
    }
    if mode == MatchMode::Strict {
        return Vec::new();
    }
    let mut scored: Vec<(&Screen, f32)> = inv
        .iter()
        .map(|s| {
            let name = token_score(line, &s.name);
            let loc = token_score(line, &s.location);
            (*s, name.max(loc))
        })
        .filter(|(_, sc)| *sc >= FLEXIBLE_MIN)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let Some((best, score)) = scored.first().copied() else {
        return Vec::new();
    };
    let alternates: Vec<String> = scored.iter().skip(1).filter(|(_, sc)| (score - *sc).abs() <= 0.005).map(|(s, _)| s.name.clone()).take(5).collect();
    let mut h = hit_of(best, score, "Best fuzzy match");
    h.alternates = alternates;
    vec![h]
}

fn hit_of(s: &Screen, score: f32, reason: &str) -> SorterHit {
    SorterHit { line: String::new(), screen: s.name.clone(), width: s.width, height: s.height, score, reason: reason.into(), alternates: Vec::new() }
}

fn exact_alias(inv: &[&Screen], line: &str) -> Option<SorterHit> {
    let q = normalize(line);
    let target = match q.as_str() {
        "mak" | "mak gate" | "mak g" | "makg" => "mubarak al kabeer gate",
        "top gear" => "top gear",
        "quartz" | "avenues quartz" | "the avenues quartz" => "the avenues quartz",
        "diamond" | "avenues diamond" | "the avenues diamond" => "the avenues diamond",
        "marina palms full" | "marina palm trees" => "marina palm trees",
        "piccadilly" => "piccadilly",
        "grand avenues" | "grand avenues entrance" | "the avenues - grand avenues entrance" => "the avenues - grand avenues entrance",
        "eye of kuwait" => "eye of kuwait",
        "1st ring road" | "first ring road" => "1st ring road",
        "al nassar tower" => "al nassar tower",
        "al nassar tower vertical" => "al nassar tower vertical",
        "al salam sync" | "salam sync" => "al salam sync",
        "jahra prime" => "jahra prime",
        "salmiya express" => "salmiya express",
        "jahra rotunda" => "jahra rotunda",
        "baitak" => "baitak",
        _ => return size_mode_alias(line).and_then(|n| inv.iter().find(|s| normalize(&s.name) == normalize(n)).map(|s| hit_of(s, 1.0, "Size-mode alias"))),
    };
    inv.iter()
        .find(|s| {
            let n = normalize(&s.name);
            n == target
                || (target.contains("palm") && is_palm_trees(&s.name))
                || (target.contains("eye of kuwait") && n.contains("eye of kuwait"))
                || (target.contains("grand avenues") && is_avenues_entrance(&s.name) && n.contains("grand avenues"))
        })
        .map(|s| hit_of(s, 1.0, "Sorter exact alias"))
}

fn associated_group(inv: &[&Screen], line: &str) -> Option<Vec<SorterHit>> {
    let q = normalize(line);
    let groups: &[(&str, &[&str])] = &[
        ("thuraya", &["thuraya"]),
        ("boursa", &["boursa"]),
        ("al salam sync", &["al salam sync"]),
        ("platinum", &["platinum square"]),
        ("capital hub", &["capital hub"]),
        ("marina crescent", &["crescent sync"]),
        ("palm", &["palm tree"]),
    ];
    for (alias, needles) in groups {
        if q == *alias || q.contains(alias) {
            let hits: Vec<SorterHit> =
                inv.iter().filter(|s| needles.iter().any(|n| normalize(&s.name).contains(n))).map(|s| hit_of(s, 1.0, "Associated group")).collect();
            if !hits.is_empty() {
                return Some(hits);
            }
        }
    }
    let _ = similarity;
    None
}

pub fn apply_flag_answers(result: &mut SorterResult, answers: &[(String, String, String)]) {
    for (id, action, pick) in answers {
        if let Some(f) = result.flags.iter_mut().find(|f| f.id == *id) {
            f.answered = true;
            f.pick = pick.clone();
            if action == "pick" && !pick.is_empty() {
                f.suggestion = pick.clone();
            }
        }
    }
}

pub fn unresolved_flags(result: &SorterResult) -> usize {
    result.flags.iter().filter(|f| !f.answered).count()
}

fn row_visible(lib: &Library, row: &SorterRow, filters: &SorterFilters) -> bool {
    if filters.is_empty() {
        return true;
    }
    row.covers.iter().any(|n| lib.screen_by_name(n).is_some_and(|s| filters.allows(s)))
}

/// Names actually sent (filters never drop these).
pub fn send_names(result: &SorterResult, screen_specific: bool) -> Vec<String> {
    if screen_specific { result.paste_names_screen_specific.clone() } else { result.paste_names_by_size.clone() }
}

pub fn hidden_row_count(result: &SorterResult) -> usize {
    result.rows.iter().filter(|r| r.hidden).count()
}
