//! Project panel selection: marquee hit-testing, modifier keys, Esc restore, Ctrl+A and
//! auto-scroll. Pure functions so the rules can be tested without a window.

use effectcraft_engine::project::{FootageKind, Item, ItemId, ItemKind};
use egui::{Color32, Pos2, Rect, pos2};

/// Marquee, selected-tile outline, footer badge and the dialog's primary button (`#2448F5`).
pub const SELECT_BLUE: Color32 = Color32::from_rgb(0x24, 0x48, 0xF5);

/// Translucent fill for the live marquee rectangle.
pub fn marquee_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(0x24, 0x48, 0xF5, 0x55)
}

/// Edge band, in points, that auto-scrolls the list or grid while a marquee is dragged.
pub const SCROLL_BAND: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    /// Drag started on a name or icon: move the item (or the whole selection).
    Move,
    /// Drag started on empty space or a row's info columns: draw a selection box.
    Marquee,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectOp {
    Replace,
    Add,
    Toggle,
}

/// A drag that starts at `x` is a move when it is in the name column, otherwise a marquee.
/// `name_end` is the right edge of the Name column (info columns Type/Size/Duration start there).
pub fn drag_kind(x: f32, name_end: f32) -> DragKind {
    if x < name_end { DragKind::Move } else { DragKind::Marquee }
}

/// Shift adds; Ctrl/Cmd toggles; otherwise the box replaces the selection.
pub fn select_op(shift: bool, ctrl: bool) -> SelectOp {
    if ctrl {
        SelectOp::Toggle
    } else if shift {
        SelectOp::Add
    } else {
        SelectOp::Replace
    }
}

/// Axis-aligned marquee from the press point to the current pointer.
pub fn marquee_rect(start: Pos2, current: Pos2) -> Rect {
    Rect::from_two_pos(start, current)
}

/// A row or tile is hit when the marquee touches any part of it.
pub fn touches(item: Rect, marquee: Rect) -> bool {
    item.intersects(marquee)
}

/// Live selection from the items the marquee currently touches and the selection at drag start.
pub fn apply_marquee(visible: &[ItemId], hit: &[bool], previous: &[ItemId], op: SelectOp) -> Vec<ItemId> {
    let n = visible.len().min(hit.len());
    let prev: Vec<ItemId> = previous.iter().copied().filter(|id| visible.contains(id)).collect();
    match op {
        SelectOp::Replace => visible.iter().take(n).zip(hit.iter().take(n)).filter(|(_, h)| **h).map(|(id, _)| *id).collect(),
        SelectOp::Add => {
            let mut out = prev;
            for i in 0..n {
                if hit.get(i).copied().unwrap_or(false) {
                    let id = visible[i];
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
            out
        }
        SelectOp::Toggle => {
            let mut out = prev;
            for i in 0..n {
                if hit.get(i).copied().unwrap_or(false) {
                    let id = visible[i];
                    if let Some(k) = out.iter().position(|x| *x == id) {
                        out.remove(k);
                    } else {
                        out.push(id);
                    }
                }
            }
            out
        }
    }
}

/// Click: one item, Shift+click a range from `anchor`, Ctrl+click toggles.
pub fn click_select(visible: &[ItemId], clicked: ItemId, previous: &[ItemId], anchor: Option<ItemId>, op: SelectOp) -> (Vec<ItemId>, ItemId) {
    match op {
        SelectOp::Add => {
            let a = anchor.filter(|id| visible.contains(id)).or_else(|| previous.first().copied());
            let range = match (a.and_then(|a| visible.iter().position(|id| *id == a)), visible.iter().position(|id| *id == clicked)) {
                (Some(i), Some(j)) => {
                    let lo = i.min(j);
                    let hi = i.max(j);
                    visible.get(lo..=hi).unwrap_or_default().to_vec()
                }
                _ => vec![clicked],
            };
            (range, a.unwrap_or(clicked))
        }
        SelectOp::Toggle => {
            let mut out: Vec<ItemId> = previous.to_vec();
            if let Some(k) = out.iter().position(|id| *id == clicked) {
                out.remove(k);
            } else {
                out.push(clicked);
            }
            (out, clicked)
        }
        SelectOp::Replace => (vec![clicked], clicked),
    }
}

/// Ctrl+A: every visible row. A closed folder is one item. When `filter` is set, only items
/// whose name contains it (folders included only when they themselves match).
pub fn select_all_visible(visible: &[ItemId], names: &[String], filter: &str) -> Vec<ItemId> {
    let q = filter.trim().to_lowercase();
    if q.is_empty() {
        return visible.to_vec();
    }
    visible.iter().zip(names.iter()).filter(|(_, n)| n.to_lowercase().contains(&q)).map(|(id, _)| *id).collect()
}

/// Esc during a drag restores `previous`; otherwise it clears.
pub fn esc_selection(dragging: bool, previous: &[ItemId]) -> Vec<ItemId> {
    if dragging { previous.to_vec() } else { Vec::new() }
}

/// Auto-scroll delta while the pointer sits in the top or bottom edge band.
/// Negative = scroll up (content moves down). Magnitude grows deeper into the band.
pub fn auto_scroll_delta(pointer_y: f32, list: Rect, band: f32, max_step: f32) -> f32 {
    if band <= 0.0 || list.height() <= 0.0 {
        return 0.0;
    }
    let band = band.min(list.height() * 0.45);
    if pointer_y < list.min.y + band {
        let t = ((list.min.y + band - pointer_y) / band).clamp(0.0, 1.0);
        return -max_step * t;
    }
    if pointer_y > list.max.y - band {
        let t = ((pointer_y - (list.max.y - band)) / band).clamp(0.0, 1.0);
        return max_step * t;
    }
    0.0
}

/// Map a pointer in view space to content space so the marquee grows as the list scrolls.
pub fn content_pos(view: Pos2, scroll: f32) -> Pos2 {
    pos2(view.x, view.y + scroll)
}

pub fn file_stem(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.len() <= 5 => stem.to_string(),
        _ => name.to_string(),
    }
}

pub fn sizes_differ(dims: &[(u32, u32)]) -> bool {
    match dims.first() {
        None => false,
        Some(first) => dims.iter().any(|d| d != first),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KindCounts {
    pub videos: usize,
    pub images: usize,
    pub audio: usize,
    pub comps: usize,
    pub folders: usize,
    pub other: usize,
}

impl KindCounts {
    pub fn total(&self) -> usize {
        self.videos + self.images + self.audio + self.comps + self.folders + self.other
    }
    /// Header line: `3 videos · 2 images · 1 audio`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let push = |n: usize, one: &str, many: &str, parts: &mut Vec<String>| {
            if n == 1 {
                parts.push(format!("1 {one}"));
            } else if n > 1 {
                parts.push(format!("{n} {many}"));
            }
        };
        push(self.videos, "video", "videos", &mut parts);
        push(self.images, "image", "images", &mut parts);
        push(self.audio, "audio", "audio", &mut parts);
        push(self.comps, "comp", "comps", &mut parts);
        push(self.folders, "folder", "folders", &mut parts);
        push(self.other, "item", "items", &mut parts);
        parts.join(" · ")
    }
}

pub fn count_kinds(items: &[&Item]) -> KindCounts {
    let mut c = KindCounts::default();
    for it in items {
        match &it.kind {
            ItemKind::Folder => c.folders += 1,
            ItemKind::Comp(_) => c.comps += 1,
            ItemKind::Solid(_) => c.images += 1,
            ItemKind::Footage(f) => match f.kind {
                FootageKind::Video | FootageKind::Sequence => c.videos += 1,
                FootageKind::Still => c.images += 1,
                FootageKind::Audio => c.audio += 1,
                _ => c.other += 1,
            },
        }
    }
    c
}

pub fn is_audio_only(it: &Item) -> bool {
    match &it.kind {
        ItemKind::Footage(f) => f.kind == FootageKind::Audio || (f.has_audio && !f.has_video),
        _ => false,
    }
}

/// Items New Comp From Selection will use: folders skipped; in Multiple, audio is skipped too.
pub fn new_comp_items<'a>(items: impl IntoIterator<Item = &'a Item>, single: bool) -> Vec<ItemId> {
    items.into_iter().filter(|it| !it.is_folder()).filter(|it| single || !is_audio_only(it)).map(|it| it.id).collect()
}

/// Shared prefix of stems used to name the destination folder (`Honor_400 comps`).
pub fn job_folder_name(names: &[String]) -> String {
    if names.is_empty() {
        return "New Comps".into();
    }
    let stems: Vec<String> = names.iter().map(|n| file_stem(n)).collect();
    let mut prefix = stems[0].clone();
    for s in stems.iter().skip(1) {
        let n = prefix.chars().zip(s.chars()).take_while(|(a, b)| a == b).count();
        prefix = prefix.chars().take(n).collect();
        while prefix.ends_with('_') || prefix.ends_with('-') || prefix.ends_with(' ') {
            prefix.pop();
        }
    }
    if prefix.len() < 3 { format!("{} comps", stems[0]) } else { format!("{prefix} comps") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use effectcraft_engine::color::Label;
    use effectcraft_engine::project::{Footage, Project};

    fn ids(n: u64) -> Vec<ItemId> {
        (1..=n).map(ItemId).collect()
    }

    #[test]
    fn name_column_moves_info_columns_marquees() {
        assert_eq!(drag_kind(40.0, 120.0), DragKind::Move);
        assert_eq!(drag_kind(119.9, 120.0), DragKind::Move);
        assert_eq!(drag_kind(120.0, 120.0), DragKind::Marquee);
        assert_eq!(drag_kind(400.0, 120.0), DragKind::Marquee);
    }

    #[test]
    fn marquee_hits_live_and_drops_out_when_dragged_back() {
        let vis = ids(4);
        let previous = vec![];
        let r = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(200.0, 19.0));
        let rows = [0, 1, 2, 3].map(|i| Rect::from_min_size(pos2(0.0, i as f32 * 19.0), r.size()));
        let box1 = marquee_rect(pos2(10.0, 5.0), pos2(80.0, 45.0));
        let hit: Vec<bool> = rows.iter().map(|row| touches(*row, box1)).collect();
        assert_eq!(hit, vec![true, true, true, false]);
        assert_eq!(apply_marquee(&vis, &hit, &previous, SelectOp::Replace), vec![ItemId(1), ItemId(2), ItemId(3)]);
        let box2 = marquee_rect(pos2(10.0, 5.0), pos2(80.0, 20.0));
        let hit2: Vec<bool> = rows.iter().map(|row| touches(*row, box2)).collect();
        assert_eq!(apply_marquee(&vis, &hit2, &previous, SelectOp::Replace), vec![ItemId(1), ItemId(2)]);
    }

    #[test]
    fn shift_adds_ctrl_toggles() {
        let vis = ids(4);
        let prev = vec![ItemId(1), ItemId(2)];
        let hit = vec![false, true, true, false];
        assert_eq!(apply_marquee(&vis, &hit, &prev, SelectOp::Add), vec![ItemId(1), ItemId(2), ItemId(3)]);
        assert_eq!(apply_marquee(&vis, &hit, &prev, SelectOp::Toggle), vec![ItemId(1), ItemId(3)]);
        assert_eq!(select_op(true, false), SelectOp::Add);
        assert_eq!(select_op(false, true), SelectOp::Toggle);
        assert_eq!(select_op(true, true), SelectOp::Toggle);
        assert_eq!(select_op(false, false), SelectOp::Replace);
    }

    #[test]
    fn click_range_and_toggle() {
        let vis = ids(5);
        let (sel, anchor) = click_select(&vis, ItemId(4), &[ItemId(2)], Some(ItemId(2)), SelectOp::Add);
        assert_eq!(sel, vec![ItemId(2), ItemId(3), ItemId(4)]);
        assert_eq!(anchor, ItemId(2));
        let (sel, _) = click_select(&vis, ItemId(2), &[ItemId(1), ItemId(2)], Some(ItemId(1)), SelectOp::Toggle);
        assert_eq!(sel, vec![ItemId(1)]);
        let (sel, a) = click_select(&vis, ItemId(3), &[], None, SelectOp::Replace);
        assert_eq!((sel, a), (vec![ItemId(3)], ItemId(3)));
    }

    #[test]
    fn esc_restores_while_dragging_else_clears() {
        let prev = vec![ItemId(1), ItemId(9)];
        assert_eq!(esc_selection(true, &prev), prev);
        assert!(esc_selection(false, &prev).is_empty());
    }

    #[test]
    fn ctrl_a_respects_the_search_filter_and_counts_a_closed_folder_as_one() {
        let vis = vec![ItemId(1), ItemId(2), ItemId(3)];
        let names = ["2_Footage".into(), "clip.mp4".into(), "logo.png".into()];
        assert_eq!(select_all_visible(&vis, &names, ""), vis);
        assert_eq!(select_all_visible(&vis, &names, "clip"), vec![ItemId(2)]);
        assert_eq!(select_all_visible(&vis, &names, "FOOT"), vec![ItemId(1)]);
    }

    #[test]
    fn auto_scroll_grows_in_the_edge_bands() {
        let list = Rect::from_min_max(pos2(0.0, 100.0), pos2(200.0, 300.0));
        assert_eq!(auto_scroll_delta(200.0, list, 20.0, 12.0), 0.0);
        assert!(auto_scroll_delta(105.0, list, 20.0, 12.0) < 0.0);
        assert!(auto_scroll_delta(100.0, list, 20.0, 12.0) <= auto_scroll_delta(110.0, list, 20.0, 12.0));
        assert!(auto_scroll_delta(295.0, list, 20.0, 12.0) > 0.0);
        let start = content_pos(pos2(10.0, 110.0), 0.0);
        let scrolled = content_pos(pos2(10.0, 290.0), 80.0);
        let m = marquee_rect(start, scrolled);
        assert!(m.height() > 200.0, "the box grows in content space as the list scrolls");
    }

    #[test]
    fn stems_and_job_folder_and_kind_counts() {
        assert_eq!(file_stem("Honor_400_EN.mp4"), "Honor_400_EN");
        assert_eq!(file_stem("stc_logo"), "stc_logo");
        assert_eq!(job_folder_name(&["Honor_400_EN.mp4".into(), "Honor_400_AR.mp4".into(), "Honor_400_Marina.mp4".into()]), "Honor_400 comps");
        let mut p = Project::default();
        let v = Footage { kind: FootageKind::Video, has_video: true, width: 1920, height: 1080, ..Default::default() };
        let img = Footage { kind: FootageKind::Still, has_video: true, width: 400, height: 400, ..Default::default() };
        let a = Footage { kind: FootageKind::Audio, has_audio: true, has_video: false, ..Default::default() };
        p.add_item("v.mp4", Label::None, None, ItemKind::Footage(v));
        p.add_item("pic.png", Label::None, None, ItemKind::Footage(img));
        p.add_item("vo.wav", Label::None, None, ItemKind::Footage(a));
        p.add_item("F", Label::None, None, ItemKind::Folder);
        let items: Vec<&Item> = p.items.values().collect();
        let c = count_kinds(&items);
        assert_eq!((c.videos, c.images, c.audio, c.folders), (1, 1, 1, 1));
        assert_eq!(c.summary(), "1 video · 1 image · 1 audio · 1 folder");
        let one: Vec<&Item> = p.items.values().collect();
        assert_eq!(new_comp_items(one.iter().copied(), false).len(), 2, "multiple skips folders and audio");
        assert_eq!(new_comp_items(one.iter().copied(), true).len(), 3, "single keeps audio as a layer");
        assert!(sizes_differ(&[(1920, 1080), (1080, 1920)]));
        assert!(!sizes_differ(&[(1920, 1080), (1920, 1080)]));
    }
}
