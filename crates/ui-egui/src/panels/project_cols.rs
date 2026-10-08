//! Project panel list columns: stored widths, leftover space on Name, auto-fit, and
//! middle-ellipsis so the end of a long file name (screen size) stays visible.

use std::collections::BTreeMap;

/// Default Name column width when the user has not resized it.
pub const NAME_DEFAULT: f32 = 160.0;
/// Name never shrinks below this, even in a narrow panel.
pub const NAME_MIN: f32 = 80.0;
pub const COL_MIN: f32 = 36.0;
pub const COL_MAX: f32 = 720.0;
/// Hit strip on a column-header divider, in points.
pub const DIVIDER_HIT: f32 = 6.0;
/// Padding added around measured text when auto-fitting a column.
pub const AUTO_FIT_PAD: f32 = 16.0;

/// Width stored for `key`, or `fallback` (the column's built-in default).
pub fn stored_width(widths: &BTreeMap<String, f32>, key: &str, fallback: f32) -> f32 {
    let min = if key == "name" { NAME_MIN } else { COL_MIN };
    widths.get(key).copied().unwrap_or(fallback).clamp(min, COL_MAX)
}

/// Remember a dragged or auto-fitted width.
pub fn set_width(widths: &mut BTreeMap<String, f32>, key: &str, w: f32) {
    let min = if key == "name" { NAME_MIN } else { COL_MIN };
    widths.insert(key.to_string(), w.clamp(min, COL_MAX));
}

/// Drag a divider: `current` plus `delta`, clamped.
pub fn resize_width(current: f32, delta: f32, min: f32, max: f32) -> f32 {
    (current + delta).clamp(min, max)
}

/// Extra space after the Label chrome (not the optional Type/Size columns) goes to Name.
/// Widening the panel therefore lengthens the names; optional columns keep their stored
/// widths and scroll when they no longer fit.
pub fn name_width(panel_w: f32, stored_name: f32, chrome: f32) -> f32 {
    stored_name.max(panel_w - chrome).max(NAME_MIN)
}

/// How far the optional columns overflow the panel to the right of `opt_x0`.
pub fn optional_overflow(opt_end: f32, panel_max_x: f32) -> f32 {
    (opt_end - panel_max_x + 4.0).max(0.0)
}

/// Auto-fit a column to its header or the longest cell, plus padding.
pub fn auto_fit_width<'a>(header: &str, cells: impl IntoIterator<Item = &'a str>, measure: impl Fn(&str) -> f32, min: f32, max: f32) -> f32 {
    let mut w = measure(header);
    for c in cells {
        w = w.max(measure(c));
    }
    (w + AUTO_FIT_PAD).clamp(min, max)
}

/// Combining marks, Arabic tashkeel, variation selectors and ZWJ/ZWNJ — must stay with the
/// preceding letter so truncation never splits a shaped cluster.
pub fn is_combiner(c: char) -> bool {
    let u = c as u32;
    matches!(
        u,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x05BF
            | 0x05C1
            | 0x05C2
            | 0x05C4
            | 0x05C5
            | 0x05C7
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06DC
            | 0x06DF..=0x06E4
            | 0x06E7
            | 0x06E8
            | 0x06EA..=0x06ED
            | 0x08D3..=0x08E1
            | 0x08E3..=0x08FF
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0x200C
            | 0x200D
            | 0xE0100..=0xE01EF
    )
}

/// Grapheme-like clusters: a base character plus any following combiners. Never splits a
/// codepoint or an Arabic letter from its harakat.
pub fn clusters(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut pending = false;
    for (i, c) in s.char_indices() {
        if pending && !is_combiner(c) {
            if let Some(piece) = s.get(start..i) {
                out.push(piece);
            }
            start = i;
        }
        pending = true;
    }
    if pending && let Some(piece) = s.get(start..) {
        out.push(piece);
    }
    out
}

fn join_ellipsis(parts: &[&str], left: usize, right: usize) -> String {
    let n = parts.len();
    let left = left.min(n);
    let right = right.min(n.saturating_sub(left));
    let mut out = String::new();
    for p in parts.iter().take(left) {
        out.push_str(p);
    }
    out.push('…');
    if right > 0 {
        let start = n.saturating_sub(right);
        for p in parts.iter().skip(start) {
            out.push_str(p);
        }
    }
    out
}

/// Cut the middle of `s` so it fits in `max_w`. Keeps the start and (preferentially) the end,
/// so a screen size like `_3072x576.mp4` stays readable. Combiners are never split from the
/// letter they mark.
pub fn middle_ellipsis(s: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if measure(s) <= max_w {
        return s.to_string();
    }
    let parts = clusters(s);
    if parts.is_empty() {
        return s.to_string();
    }
    if measure("…") > max_w {
        return "…".into();
    }
    let n = parts.len();
    let mut left = 0usize;
    let mut right = 0usize;
    let mut best = "…".to_string();
    let try_grow = |left: usize, right: usize| join_ellipsis(&parts, left, right);
    // Keep about 60% of the width for the end (screen name and size), then fill the start,
    // then spend any leftover on whichever side still fits.
    let suffix_cap = (max_w * 0.62).max(measure("…") + 1.0);
    while left + right + 1 < n {
        let cand = try_grow(left, right + 1);
        let w = measure(&cand);
        if w > max_w || (w > suffix_cap && right > 0) {
            break;
        }
        right += 1;
        best = cand;
    }
    while left + right + 1 < n {
        let cand = try_grow(left + 1, right);
        if measure(&cand) > max_w {
            break;
        }
        left += 1;
        best = cand;
    }
    while left + right + 1 < n {
        let cand = try_grow(left, right + 1);
        if measure(&cand) > max_w {
            break;
        }
        right += 1;
        best = cand;
    }
    best
}

/// Round-trip the width map the way prefs.json stores it.
pub fn widths_from_json(v: &serde_json::Value) -> Option<BTreeMap<String, f32>> {
    let obj = v.as_object()?;
    let mut out = BTreeMap::new();
    for (k, val) in obj {
        let w = val.as_f64()? as f32;
        if w.is_finite() {
            set_width(&mut out, k, w);
        }
    }
    Some(out)
}

pub fn widths_to_json(widths: &BTreeMap<String, f32>) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (k, w) in widths {
        if w.is_finite() {
            m.insert(k.clone(), serde_json::json!(*w));
        }
    }
    serde_json::Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn name_column_grows_with_the_panel_first() {
        let chrome = 56.0;
        assert_eq!(name_width(500.0, 160.0, chrome), 444.0);
        assert_eq!(name_width(300.0, 160.0, chrome), 244.0);
        assert_eq!(name_width(200.0, 160.0, chrome), 160.0);
        assert!(name_width(100.0, 40.0, chrome) >= NAME_MIN);
        assert_eq!(optional_overflow(400.0, 300.0), 104.0);
        assert_eq!(optional_overflow(300.0, 300.0), 4.0);
        assert_eq!(optional_overflow(200.0, 300.0), 0.0);
    }

    #[test]
    fn resize_and_auto_fit_and_saved_widths() {
        assert_eq!(resize_width(160.0, 40.0, NAME_MIN, COL_MAX), 200.0);
        assert_eq!(resize_width(90.0, -40.0, NAME_MIN, COL_MAX), NAME_MIN);
        let cells = ["a.mp4", "Honor_400_AlSalam_3072x576.mp4"];
        let w = auto_fit_width("Name", cells, ch, NAME_MIN, COL_MAX);
        assert!(w >= ch("Honor_400_AlSalam_3072x576.mp4") + AUTO_FIT_PAD - 0.1);
        let mut stored = BTreeMap::new();
        set_width(&mut stored, "name", w);
        set_width(&mut stored, "type", 120.0);
        let json = widths_to_json(&stored);
        let back = widths_from_json(&json).expect("round-trip");
        assert!((stored_width(&back, "name", NAME_DEFAULT) - w).abs() < 0.01);
        assert!((stored_width(&back, "type", 84.0) - 120.0).abs() < 0.01);
        assert_eq!(stored_width(&BTreeMap::new(), "size", 56.0), 56.0);
    }

    #[test]
    fn middle_ellipsis_keeps_the_end_of_a_long_name() {
        let s = "Honor_400_AlSalam_3072x576.mp4";
        let out = middle_ellipsis(s, ch(s), ch);
        assert_eq!(out, s);
        let out = middle_ellipsis(s, 24.0, ch);
        assert!(out.contains('…'), "{out}");
        assert!(out.contains("3072x576") || out.ends_with("576.mp4"), "{out}");
        assert!(out.starts_with("Honor"), "{out}");
        assert!(ch(&out) <= 24.0 + 0.1, "{out}");
    }

    #[test]
    fn middle_ellipsis_keeps_arabic_clusters_and_the_size_suffix() {
        // Fatha / sukun / shadda stay on their letters.
        let s = "مَرْحَبّ_الشاشة_3072x576.mp4";
        for c in clusters(s) {
            let mut chars = c.chars();
            let first = chars.next().expect("cluster");
            assert!(!is_combiner(first) || c.chars().count() == 1, "combiner split: {c:?}");
        }
        let out = middle_ellipsis(s, 22.0, ch);
        assert!(out.contains('…'), "{out}");
        assert!(out.contains("3072x576.mp4"), "{out}");
        assert!(out.starts_with('م'), "{out}");
        for c in clusters(&out) {
            if c == "…" {
                continue;
            }
            let mut chars = c.chars();
            let first = chars.next().expect("cluster");
            assert!(!is_combiner(first), "truncated Arabic broke shaping: {out:?} cluster {c:?}");
        }
        // A name that is only Arabic still truncates on cluster boundaries.
        let ar = "تكريم_الصالة_الكويت";
        let out = middle_ellipsis(ar, 8.0, ch);
        assert!(out.contains('…'), "{out}");
        assert!(out.starts_with('ت'), "{out}");
    }
}
