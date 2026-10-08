//! Combiner layouts: h-dup / v-dup / single columns.
//!
//! Configured duplication (Al Salam 1536×576 × 2 → 3072×576, Palm Trees 240×960 × 4 → 960×960)
//! is the normal deliverable and does **not** warn. A warning is raised only when the combiner
//! receives more unique source comps than its configured slots; extra pieces are stacked
//! vertically and the result is larger than the configured size.

use serde::{Deserialize, Serialize};

use crate::inventory::{Combiner, CombinerColumn, Library};
use crate::normalize::{format_size, normalize};

/// Unique source comps a combiner is configured to take (one per column; h-dup/v-dup still
/// consume a single original that is copied `count` times).
pub fn unique_source_slots(c: &Combiner) -> u32 {
    (c.columns.len() as u32).max(1)
}

/// Attention flag: extra source pieces were stacked and the combined size grew past the norm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DupWarning {
    pub screen: String,
    pub expected_width: u32,
    pub expected_height: u32,
    pub actual_width: u32,
    pub actual_height: u32,
    pub extra_pieces: u32,
}

impl DupWarning {
    pub fn message(&self) -> String {
        let piece = if self.extra_pieces == 1 { "piece" } else { "pieces" };
        format!(
            "This combined composition grew beyond the normal size for this screen. Expected {ew}×{eh}, actual {aw}×{ah} ({n} extra {piece} stacked vertically).",
            ew = self.expected_width,
            eh = self.expected_height,
            aw = self.actual_width,
            ah = self.actual_height,
            n = self.extra_pieces,
        )
    }
}

/// One face in a combined master. `left`/`top` are the snapped edges (no gap, no overlap).
/// `position` is the EffectCraft layer Position (source centre).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacePlacement {
    pub screen: String,
    pub left: f64,
    pub top: f64,
    pub width: u32,
    pub height: u32,
    pub position: [f64; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinerLayout {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub faces: Vec<FacePlacement>,
    pub warnings: Vec<DupWarning>,
    /// Unique sources consumed (configured slots + extras).
    #[serde(default)]
    pub unique_sources: u32,
    #[serde(default)]
    pub extra_pieces: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedSize {
    pub width: u32,
    pub height: u32,
    pub duplicated: bool,
    pub copies: u32,
    pub label: String,
}

impl CombinerLayout {
    /// Configured layout: no extra sources, never a size-grown warning.
    pub fn from_combiner(c: &Combiner) -> Self {
        Self::from_unique_sources(c, unique_source_slots(c))
    }

    /// Layout for `unique_sources` matching source comps/footage items.
    /// Extra sources beyond [`unique_source_slots`] stack additional copies of the configured
    /// layout vertically and produce a [`DupWarning`].
    pub fn from_unique_sources(c: &Combiner, unique_sources: u32) -> Self {
        let (width, row_h, row_faces) = layout_row(c);
        let slots = unique_source_slots(c);
        let extra = unique_sources.saturating_sub(slots);
        let rows = extra.saturating_add(1);
        let mut faces = Vec::new();
        for r in 0..rows {
            let yoff = f64::from(row_h) * f64::from(r);
            for f in &row_faces {
                let mut g = f.clone();
                g.top += yoff;
                g.position[1] += yoff;
                faces.push(g);
            }
        }
        let height = row_h.saturating_mul(rows).max(1);
        let warnings = if extra > 0 {
            vec![DupWarning {
                screen: display_name(c),
                expected_width: width,
                expected_height: row_h.max(1),
                actual_width: width,
                actual_height: height,
                extra_pieces: extra,
            }]
        } else {
            Vec::new()
        };
        Self { name: c.name.clone(), width, height, faces, warnings, unique_sources: unique_sources.max(slots), extra_pieces: extra }
    }
}

fn display_name(c: &Combiner) -> String {
    c.columns.first().map(|col| col.screen_name.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| c.name.clone())
}

/// One configured row of columns (h-dup / v-dup / single) with no extra stacking.
fn layout_row(c: &Combiner) -> (u32, u32, Vec<FacePlacement>) {
    let mut faces = Vec::new();
    let mut x = 0.0f64;
    let mut max_h = 0u32;
    for col in &c.columns {
        let (col_w, col_h, col_faces) = column_faces(col, x);
        faces.extend(col_faces);
        x += f64::from(col_w);
        max_h = max_h.max(col_h);
    }
    let width = x.round().max(1.0) as u32;
    (width, max_h.max(1), faces)
}

fn column_faces(col: &CombinerColumn, x: f64) -> (u32, u32, Vec<FacePlacement>) {
    let (fw, fh) = col.match_wh.unwrap_or((1, 1));
    let count = col.count.max(1);
    let layout = col.layout.to_ascii_lowercase();
    let mut faces = Vec::new();
    if layout == "h-dup" && count > 1 {
        let forced = col.comp_w.filter(|w| *w > 0).unwrap_or_else(|| fw.saturating_mul(count));
        let step = if col.force_scale { f64::from(forced) / f64::from(count) } else { f64::from(fw) };
        for i in 0..count {
            let left = x + f64::from(i) * step;
            faces.push(place(&col.screen_name, left, 0.0, fw, fh));
        }
        (forced, fh, faces)
    } else if layout == "v-dup" && count > 1 {
        for i in 0..count {
            let top = f64::from(i) * f64::from(fh);
            faces.push(place(&col.screen_name, x, top, fw, fh));
        }
        (fw, fh.saturating_mul(count), faces)
    } else {
        faces.push(place(&col.screen_name, x, 0.0, fw, fh));
        (fw, fh, faces)
    }
}

fn place(screen: &str, left: f64, top: f64, w: u32, h: u32) -> FacePlacement {
    FacePlacement { screen: screen.to_string(), left, top, width: w, height: h, position: [left + f64::from(w) / 2.0, top + f64::from(h) / 2.0] }
}

/// Combiners whose every column is among `selected` names (normalized, aliases allowed).
pub fn active_combiners(lib: &Library, selected: &[String]) -> Vec<CombinerLayout> {
    let sel: Vec<String> = selected.iter().map(|s| normalize(s)).collect();
    lib.combiners.iter().filter(|c| c.columns.iter().all(|col| column_selected(&sel, &col.screen_name))).map(CombinerLayout::from_combiner).collect()
}

fn column_selected(sel: &[String], screen: &str) -> bool {
    let n = normalize(screen);
    sel.iter().any(|s| s == &n || s.contains(&n) || n.contains(s.as_str()) || (n.contains("palm") && s.contains("palm")))
}

/// Sizes the matcher should treat as a pass for this screen (face + configured combined size).
/// Extra-stacked sizes are **not** accepted.
pub fn accepted_sizes_for(lib: &Library, screen_name: &str) -> Vec<AcceptedSize> {
    let n = normalize(screen_name);
    let mut out = Vec::new();
    if let Some(s) = lib
        .screens
        .iter()
        .find(|s| normalize(&s.name) == n || s.aliases.iter().any(|a| normalize(a) == n) || (n.contains("palm") && crate::inventory::is_palm_trees(&s.name)))
    {
        out.push(AcceptedSize {
            width: s.width,
            height: s.height,
            duplicated: false,
            copies: 1,
            label: format!("{} (normal {})", s.name, format_size(s.width, s.height)),
        });
        if crate::inventory::is_al_salam_sync(&s.name) {
            push_unique(
                &mut out,
                AcceptedSize {
                    width: 3072,
                    height: 576,
                    duplicated: true,
                    copies: 2,
                    label: "Al Salam Sync duplicated deliverable (1536×576 × 2 side by side → 3072×576)".into(),
                },
            );
        }
    }
    for c in &lib.combiners {
        let lay = CombinerLayout::from_combiner(c);
        if lay.faces.iter().any(|f| names_related(&f.screen, screen_name) || names_related(&lay.name, screen_name)) {
            let copies = c.columns.iter().map(|col| col.count.max(1)).max().unwrap_or(1);
            push_unique(
                &mut out,
                AcceptedSize {
                    width: lay.width,
                    height: lay.height,
                    duplicated: copies > 1,
                    copies,
                    label: if copies > 1 {
                        format!("{} combined deliverable {}", lay.name, format_size(lay.width, lay.height))
                    } else {
                        format!("{} combined {}", lay.name, format_size(lay.width, lay.height))
                    },
                },
            );
        }
    }
    out
}

pub fn names_related(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a) || (a.contains("palm") && b.contains("palm")) || (a.contains("salam sync") && b.contains("salam sync"))
}

fn push_unique(out: &mut Vec<AcceptedSize>, s: AcceptedSize) {
    if !out.iter().any(|x| x.width == s.width && x.height == s.height) {
        out.push(s);
    }
}

pub fn size_matches_accepted(accepted: &[AcceptedSize], w: u32, h: u32) -> Option<&AcceptedSize> {
    accepted.iter().find(|a| a.width == w && a.height == h)
}

/// When `w×h` is a vertical multiple of a configured (usually combined) accepted size.
/// `extra_pieces` is how many extra stacked copies sit below the norm.
pub fn extra_stacked(accepted: &[AcceptedSize], w: u32, h: u32) -> Option<(u32, u32, u32)> {
    if size_matches_accepted(accepted, w, h).is_some() {
        return None;
    }
    let mut best: Option<&AcceptedSize> = None;
    for a in accepted {
        if a.width != w || a.height == 0 || h <= a.height || !h.is_multiple_of(a.height) {
            continue;
        }
        best = match best {
            None => Some(a),
            Some(cur) if a.duplicated && !cur.duplicated => Some(a),
            Some(cur) if a.duplicated == cur.duplicated && a.height > cur.height => Some(a),
            Some(cur) => Some(cur),
        };
    }
    let a = best?;
    let extra = h.saturating_div(a.height).saturating_sub(1);
    if extra == 0 {
        return None;
    }
    Some((a.width, a.height, extra))
}
