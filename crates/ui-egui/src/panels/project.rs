//! The Project panel: item preview header (rendered thumbnail of the comp / footage frame),
//! search, item list with columns (Name, Label, then the visible optional columns: Type, Size,
//! Media Duration, Frame Rate, File Path, Comment — shown or hidden from the header's context
//! menu), folders (drag items into and out of them), renaming (Enter, or double-click the name),
//! the label colour picker, and the bottom bar (interpret, new folder, new comp, bit depth,
//! delete). Edits are `project.*` engine commands, so they are undoable.

use std::sync::{Arc, Mutex};

use effectcraft_engine::project::{Item, ItemId, ItemKind};
use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::panels::DragPayload;
use crate::panels::project_cols;
use crate::panels::project_select::{
    self, SCROLL_BAND, SELECT_BLUE, SelectOp, auto_scroll_delta, click_select, content_pos, count_kinds, marquee_fill, marquee_rect, select_op, touches,
};
use crate::state::ProjectView;
use crate::theme::Tokens;
use crate::{EffectcraftApp, widgets};

/// Item row height (After Effects' Project panel rows are compact, about 19 pt).
const ROW_H: f32 = 19.0;

fn item_icon(it: &Item) -> Icon {
    match &it.kind {
        ItemKind::Folder => Icon::Folder,
        ItemKind::Comp(_) => Icon::Comp,
        ItemKind::Solid(_) => Icon::Solid,
        ItemKind::Footage(f) => match f.kind {
            effectcraft_engine::project::FootageKind::Still | effectcraft_engine::project::FootageKind::Sequence => Icon::Image,
            effectcraft_engine::project::FootageKind::Audio => Icon::Audio,
            effectcraft_engine::project::FootageKind::Model => Icon::Cube,
            _ => Icon::Footage,
        },
    }
}

/// Duration as AE prints it in the Project panel header: `0:00:10:00` (h:mm:ss:ff).
pub fn fmt_dur(secs: f64, fps: f64) -> String {
    let fpsi = fps.round().max(1.0) as i64;
    let f = (secs * fps).round() as i64;
    let s = f / fpsi;
    format!("{}:{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60, f % fpsi)
}

/// File size the way AE's Size column shows it (`512 KB`, `12.3 MB`).
pub fn fmt_size(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K * K {
        format!("{} KB", (b / K).ceil() as u64)
    } else if b < K * K * K {
        format!("{:.1} MB", b / (K * K))
    } else {
        format!("{:.2} GB", b / (K * K * K))
    }
}

/// Footage file size on disk (comps, solids and folders have none).
fn file_size(it: &Item) -> Option<u64> {
    match &it.kind {
        ItemKind::Footage(f) => std::fs::metadata(&f.path).ok().map(|m| m.len()),
        _ => None,
    }
}

fn file_path(it: &Item) -> &str {
    match &it.kind {
        ItemKind::Footage(f) => &f.path,
        _ => "",
    }
}

/// Optional columns: (key, header, width).
pub const COLUMNS: [(&str, &str, f32); 6] = [
    ("type", "Type", 84.0),
    ("size", "Size", 56.0),
    ("duration", "Media Duration", 100.0),
    ("fps", "Frame Rate", 72.0),
    ("path", "File Path", 220.0),
    ("comment", "Comment", 150.0),
];

/// Sort siblings by the Project panel's sort column; ties fall back to the name.
pub fn sort_items(items: &mut [&Item], col: &str, desc: bool) {
    let name = |i: &Item| i.name.to_lowercase();
    items.sort_by(|a, b| {
        let o = match col {
            "type" => a.type_name().cmp(b.type_name()),
            "size" => file_size(a).cmp(&file_size(b)),
            "fps" => a.frame_rate().map(|r| r.as_f64()).partial_cmp(&b.frame_rate().map(|r| r.as_f64())).unwrap_or(std::cmp::Ordering::Equal),
            "duration" => a.duration().cmp(&b.duration()),
            "path" => file_path(a).cmp(file_path(b)),
            "comment" => a.comment.cmp(&b.comment),
            "label" => (a.label as u8).cmp(&(b.label as u8)),
            _ => std::cmp::Ordering::Equal,
        }
        .then_with(|| name(a).cmp(&name(b)));
        if desc { o.reverse() } else { o }
    });
}

/// Where a drop at a row lands: into a folder row, else into the row's folder; `None` row = root.
pub fn drop_folder(project: &effectcraft_engine::project::Project, row: Option<ItemId>) -> Option<ItemId> {
    let it = project.item(row?)?;
    if it.is_folder() { Some(it.id) } else { it.parent }
}

/// A thumbnail being rendered off the UI thread: (revision, result slot).
type PendingThumb = (u64, Arc<Mutex<Option<Option<(u32, u32, Vec<u8>)>>>>);

/// Thumbnail of a comp (its current time) or footage (first frame), cached per item/revision.
/// Rendered on a background thread (inline on wasm32): until the new one lands the previous
/// thumbnail stays up, so selecting a big comp or editing it never stalls the panel.
fn thumbnail(app: &EffectcraftApp, ctx: &egui::Context, it: &Item) -> Option<egui::TextureHandle> {
    let key = egui::Id::new(("proj-thumb", it.id.0));
    let pkey = egui::Id::new(("proj-thumb-pending", it.id.0));
    let rev = app.session.revision;
    let cached: Option<(u64, egui::TextureHandle)> = ctx.data(|d| d.get_temp(key));
    let mut tex = cached.as_ref().map(|(_, t)| t.clone());
    let have = cached.as_ref().map(|(r, _)| *r);
    // A finished render: upload it.
    let pending: Option<PendingThumb> = ctx.data(|d| d.get_temp(pkey));
    if let Some((prev, slot)) = &pending {
        let done = slot.lock().ok().and_then(|mut s| s.take());
        match done {
            Some(Some((w, h, px))) => {
                let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                let t = ctx.load_texture(format!("proj-thumb-{}", it.id.0), ci, egui::TextureOptions::LINEAR);
                ctx.data_mut(|d| {
                    d.insert_temp(key, (*prev, t.clone()));
                    d.remove::<PendingThumb>(pkey);
                });
                tex = Some(t);
                if *prev == rev {
                    return tex;
                }
            }
            Some(None) => {
                ctx.data_mut(|d| d.remove::<PendingThumb>(pkey));
                return tex;
            }
            None => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                return tex;
            }
        }
    }
    if have == Some(rev) {
        return tex;
    }
    let t = app.session.state.times.get(&it.id).copied().unwrap_or_default();
    let job = app.session.thumbnail_job(it.id, t, 192)?;
    let slot: Arc<Mutex<Option<Option<(u32, u32, Vec<u8>)>>>> = Arc::default();
    if cfg!(target_arch = "wasm32") {
        *slot.lock().ok()? = Some(job());
    } else {
        let s = slot.clone();
        let spawned = std::thread::Builder::new().name("proj-thumb".into()).spawn(move || {
            let r = job();
            if let Ok(mut s) = s.lock() {
                *s = Some(r);
            }
        });
        if spawned.is_err() {
            return tex;
        }
    }
    ctx.data_mut(|d| d.insert_temp::<PendingThumb>(pkey, (rev, slot)));
    ctx.request_repaint();
    tex
}

fn fit(r: Rect, w: f32, h: f32) -> Rect {
    let s = (r.width() / w.max(1.0)).min(r.height() / h.max(1.0));
    Rect::from_center_size(r.center(), vec2(w * s, h * s))
}

/// An inline text edit in progress: (item, field `name`|`comment`, text).
type Editing = (u64, String, String);
fn edit_id() -> egui::Id {
    egui::Id::new("proj-inline-edit")
}

#[derive(Clone)]
struct MarqueeDrag {
    start_content: egui::Pos2,
    previous: Vec<ItemId>,
    op: SelectOp,
}

fn marquee_id() -> egui::Id {
    egui::Id::new("proj-marquee")
}

fn selection_anchor_id() -> egui::Id {
    egui::Id::new("project-selection-anchor")
}

/// Start renaming the selected item (Enter in the Project panel).
pub fn begin_rename(app: &EffectcraftApp, ctx: &egui::Context) {
    if let Some(id) = app.session.state.project_selection.first()
        && let Some(it) = app.session.project.item(*id)
    {
        ctx.data_mut(|d| d.insert_temp::<Editing>(edit_id(), (id.0, "name".into(), it.name.clone())));
    }
}

pub(crate) fn selectable_ids(app: &EffectcraftApp) -> Vec<ItemId> {
    if app.ui.project_view == ProjectView::Grid { grid_items(app) } else { visible_rows(app).into_iter().map(|(id, _)| id).collect() }
}

pub(crate) fn visible_rows(app: &EffectcraftApp) -> Vec<(ItemId, usize)> {
    fn walk(app: &EffectcraftApp, folder: Option<ItemId>, depth: usize, q: &str, out: &mut Vec<(ItemId, usize)>) {
        let mut kids = app.session.project.children(folder);
        sort_items(&mut kids, &app.ui.project_sort, app.ui.project_sort_desc);
        for it in kids {
            if !q.is_empty() && !it.is_folder() && !it.name.to_lowercase().contains(q) {
                continue;
            }
            out.push((it.id, depth));
            if it.is_folder() && (app.ui.project_open_folders.contains(&it.id.0) || !q.is_empty()) {
                walk(app, Some(it.id), depth + 1, q, out);
            }
        }
    }
    let mut rows = vec![];
    walk(app, None, 0, &app.ui.project_search.to_lowercase(), &mut rows);
    rows
}

pub fn show(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    // Preview header.
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), 74.0));
    let selected_items: Vec<Item> = app.session.state.project_selection.iter().filter_map(|i| app.session.project.item(*i).cloned()).collect();
    if selected_items.len() > 1 {
        paint_multi_header(app, &p, &ctx, head, &selected_items, &t);
    } else if let Some(it) = selected_items.first() {
        let thumb = Rect::from_min_size(head.min + vec2(10.0, 10.0), vec2(96.0, 54.0));
        p.rect_filled(thumb, 3.0, Color32::from_rgb(0x15, 0x15, 0x15));
        paint_item_thumb(app, &p, &ctx, thumb, it, &t);
        app.auto.add("project.thumbnail", thumb, &it.name);
        // The details stay inside the panel (a margin on the right), cut short with "…".
        let tx = thumb.max.x + 10.0;
        let max_w = head.max.x - 10.0 - tx;
        let shown = middle_name(&p, &it.name, Tokens::semibold(12.0), max_w);
        let r = widgets::text_fit(&p, pos2(tx, thumb.min.y + 6.0), Align2::LEFT_CENTER, &shown, Tokens::semibold(12.0), max_w, t.text);
        app.auto.add("project.details.name", r, &it.name);
        let mut lines = vec![];
        if let Some((w, h)) = it.dimensions() {
            lines.push(format!("{w} x {h} (1.00)"));
        }
        if let Some(d) = it.duration() {
            let fps = it.frame_rate().map(|r| r.as_f64()).unwrap_or(30.0);
            lines.push(format!("Δ {}, {:.2} fps", fmt_dur(d.seconds(), fps), fps));
        }
        if let ItemKind::Footage(f) = &it.kind {
            lines.push(f.codec.clone());
        }
        for (i, l) in lines.iter().enumerate() {
            let r = widgets::text_fit(&p, pos2(tx, thumb.min.y + 22.0 + 13.0 * i as f32), Align2::LEFT_CENTER, l, Tokens::ui(11.0), max_w, t.text_dim);
            app.auto.add(&format!("project.details.{i}"), r, l);
        }
    } else {
        let hint = "Select an item to see its details";
        let r = widgets::text_fit(&p, head.center(), Align2::CENTER_CENTER, hint, Tokens::ui(11.0), head.width() - 20.0, t.text_faint);
        app.auto.add("project.details.hint", r, hint);
    }
    // Search.
    let sr = Rect::from_min_size(pos2(rect.min.x + 8.0, head.max.y + 2.0), vec2(rect.width() - 16.0, 22.0));
    let mut q = app.ui.project_search.clone();
    widgets::search_field(ui, sr, &mut q, "Search project", &t);
    app.ui.project_search = q;
    app.auto.add("project.search", sr, "Search");
    let is_grid = app.ui.project_view == ProjectView::Grid;
    // Column header: Name, Label, then the visible optional columns.
    let hdr = Rect::from_min_size(pos2(rect.min.x, sr.max.y + 6.0), vec2(rect.width(), 20.0));
    let mut opt_defs: Vec<(&'static str, &'static str, f32)> = vec![];
    let mut opt_total = 0.0;
    for key in app.ui.project_columns.clone() {
        if let Some((k, l, fallback)) = COLUMNS.iter().find(|c| c.0 == key) {
            let w = project_cols::stored_width(&app.ui.project_col_widths, k, *fallback);
            opt_defs.push((k, l, w));
            opt_total += w + 4.0;
        }
    }
    // Label + optional columns. Leftover panel width grows Name first; Type/Size/… keep
    // their stored widths and scroll when they no longer fit.
    let other = 14.0 + 18.0 + 16.0 + opt_total + 8.0;
    let stored_name = project_cols::stored_width(&app.ui.project_col_widths, "name", project_cols::NAME_DEFAULT);
    let name_w = project_cols::name_width(rect.width(), stored_name, other);
    let label_x = rect.min.x + name_w + 14.0;
    let mut cols: Vec<(&'static str, &'static str, f32, f32)> = vec![("name", "Name", rect.min.x + 26.0, name_w - 26.0), ("label", "", label_x - 8.0, 18.0)];
    // Optional columns wider than the panel scroll horizontally under the frozen Name and Label
    // columns (Shift+wheel / trackpad over the list, or the scroll bar under it).
    let opt_x0 = label_x + 16.0;
    let overflow = project_cols::optional_overflow(opt_x0 + opt_total, rect.max.x);
    app.ui.project_hscroll = app.ui.project_hscroll.clamp(0.0, overflow);
    let hscroll = app.ui.project_hscroll;
    let mut x = opt_x0 - hscroll;
    for (k, l, w) in &opt_defs {
        cols.push((k, l, x, *w));
        x += *w + 4.0;
    }
    let opt_clip = |r: Rect| Rect::from_min_max(pos2(r.min.x.max(opt_x0 - 4.0), r.min.y), r.max);
    if is_grid {
        paint_breadcrumb(app, ui, &p, hdr, &t);
    } else {
        p.line_segment([hdr.left_bottom(), hdr.right_bottom()], Stroke::new(1.0, t.separator));
        let hp = p.with_clip_rect(hdr);
        for (key, label, x, w) in &cols {
            let hr = Rect::from_min_size(pos2(x - 4.0, hdr.min.y), vec2(*w, hdr.height()));
            let hr = if matches!(*key, "name" | "label") { hr } else { opt_clip(hr) };
            if hr.width() <= 0.0 {
                continue;
            }
            let resp = ui.interact(hr.intersect(hdr), egui::Id::new(("proj-sort", *key)), Sense::click());
            hp.with_clip_rect(hr).text(
                pos2(*x, hdr.center().y),
                Align2::LEFT_CENTER,
                *label,
                Tokens::ui(11.0),
                if resp.hovered() { t.text } else { t.text_dim },
            );
            if *key == "label" {
                icons::paint(&hp, Rect::from_center_size(pos2(label_x, hdr.center().y), vec2(10.0, 10.0)), Icon::Keyframe, t.text_dim);
            }
            if app.ui.project_sort == *key {
                let c = pos2(hr.max.x - 10.0, hdr.center().y);
                let d = if app.ui.project_sort_desc { 1.0 } else { -1.0 };
                hp.add(egui::Shape::convex_polygon(
                    vec![pos2(c.x - 4.0, c.y - 2.0 * d), pos2(c.x + 4.0, c.y - 2.0 * d), pos2(c.x, c.y + 2.5 * d)],
                    t.text_dim,
                    Stroke::NONE,
                ));
            }
            app.auto.add(&format!("project.sort.{key}"), hr, if label.is_empty() { "Label" } else { label });
            if resp.clicked() {
                if app.ui.project_sort == *key {
                    app.ui.project_sort_desc = !app.ui.project_sort_desc;
                } else {
                    app.ui.project_sort = key.to_string();
                    app.ui.project_sort_desc = false;
                }
            }
            column_menu(app, &resp);
            if *key != "label" {
                let displayed = if *key == "name" { name_w } else { *w };
                column_divider(app, ui, &hp, hdr, key, hr.max.x, displayed, &t);
            }
        }
        let hresp = ui.interact(Rect::from_min_max(pos2(x, hdr.min.y), hdr.max), egui::Id::new("proj-hdr-rest"), Sense::click());
        column_menu(app, &hresp);
        app.auto.add("project.columns", hdr, "Columns (right-click to show or hide)");
    }
    // Rows.
    let footer_h = 28.0;
    let list = Rect::from_min_max(pos2(rect.min.x, hdr.max.y), pos2(rect.max.x, rect.max.y - footer_h));
    let lp = p.with_clip_rect(list);
    // The list's empty area (under the rows and the scroll bar, which take their own clicks):
    // a double-click imports, as in After Effects (File ▸ Import ▸ File...).
    let empty = ui.interact(list, egui::Id::new("proj-empty"), Sense::click_and_drag());
    let mut item_rects: Vec<(ItemId, Rect)> = vec![];
    let name_end = label_x - 8.0;
    app.auto.add("project.empty", list, "Double-click to import files");
    if overflow > 0.0 && ui.rect_contains_pointer(list) {
        let (dx, dy, shift) = ui.input(|i| (i.smooth_scroll_delta.x, i.smooth_scroll_delta.y, i.modifiers.shift));
        let d = if dx.abs() > 0.0 {
            dx
        } else if shift {
            dy
        } else {
            0.0
        };
        app.ui.project_hscroll = (app.ui.project_hscroll - d).clamp(0.0, overflow);
    }
    if overflow > 0.0 {
        let track = Rect::from_min_max(pos2(opt_x0, list.max.y - 5.0), pos2(rect.max.x - 4.0, list.max.y - 1.0));
        let tw = (track.width() * track.width() / (track.width() + overflow)).max(16.0);
        let thumb = Rect::from_min_size(pos2(track.min.x + (track.width() - tw) * (hscroll / overflow), track.min.y), vec2(tw, track.height()));
        let sresp = ui.interact(track.expand2(vec2(0.0, 2.0)), egui::Id::new("proj-hscroll"), Sense::drag());
        p.rect_filled(track, 2.0, t.field_bg);
        p.rect_filled(thumb, 2.0, if sresp.hovered() || sresp.dragged() { t.text_dim } else { t.text_faint });
        app.auto.add("project.hscroll", track, &format!("{hscroll}/{overflow}"));
        if sresp.dragged() {
            app.ui.project_hscroll = (hscroll + sresp.drag_delta().x * overflow / (track.width() - tw).max(1.0)).clamp(0.0, overflow);
        }
    }
    // Enter renames the selected item (Project panel focused, not typing).
    if app.ui.focused == crate::dock::PanelKind::Project
        && app.dialog.is_none()
        && !ctx.egui_wants_keyboard_input()
        && ctx.data(|d| d.get_temp::<Editing>(edit_id())).is_none()
        && ctx.input(|i| i.key_pressed(egui::Key::Enter))
    {
        begin_rename(app, &ctx);
    }
    let editing: Option<Editing> = ctx.data(|d| d.get_temp(edit_id()));
    let dragging = egui::DragAndDrop::payload::<DragPayload>(&ctx).and_then(|p| match *p {
        DragPayload::Item(i) => Some(i),
        _ => None,
    });
    let hover_y = ctx.pointer_hover_pos().filter(|p| list.contains(*p)).map(|p| p.y);
    let mut drop_row: Option<ItemId> = None;
    let mut actions: Vec<(String, serde_json::Value)> = vec![];
    if is_grid {
        show_grid(app, ui, list, &lp, &t, &ctx, &mut actions, &mut item_rects, &mut drop_row, editing.clone());
    } else {
        let rows = visible_rows(app);
        // Vertical scrolling: only the rows in view are drawn (and registered), so a project with
        // thousands of items costs what one screenful does.
        let content_h = rows.len() as f32 * ROW_H;
        let max_scroll = (content_h - list.height() + ROW_H).max(0.0);
        if ui.rect_contains_pointer(list) {
            let (dy, shift) = ui.input(|i| (i.smooth_scroll_delta.y, i.modifiers.shift));
            if !shift && dy != 0.0 {
                app.ui.project_scroll -= dy;
            }
        }
        // A selection made elsewhere (a command, an agent, the viewer) scrolls into view.
        let sel_key = egui::Id::new("proj-sel-seen");
        let sel_now = app.session.state.project_selection.last().map(|i| i.0);
        if ctx.data(|d| d.get_temp::<Option<u64>>(sel_key)) != Some(sel_now) {
            ctx.data_mut(|d| d.insert_temp(sel_key, sel_now));
            if let Some(row) = sel_now.and_then(|s| rows.iter().position(|(id, _)| id.0 == s)) {
                let top = row as f32 * ROW_H;
                if top < app.ui.project_scroll {
                    app.ui.project_scroll = top;
                } else if top + ROW_H > app.ui.project_scroll + list.height() {
                    app.ui.project_scroll = top + ROW_H - list.height();
                }
            }
        }
        app.ui.project_scroll = app.ui.project_scroll.clamp(0.0, max_scroll);
        let scroll = app.ui.project_scroll;
        if max_scroll > 0.0 {
            let track = Rect::from_min_max(pos2(rect.max.x - 6.0, list.min.y + 1.0), pos2(rect.max.x - 2.0, list.max.y - 7.0));
            let th = (track.height() * list.height() / content_h).clamp(16.0, track.height());
            let thumb = Rect::from_min_size(pos2(track.min.x, track.min.y + (track.height() - th) * (scroll / max_scroll)), vec2(track.width(), th));
            let vresp = ui.interact(track.expand2(vec2(2.0, 0.0)), egui::Id::new("proj-vscroll"), Sense::drag());
            p.rect_filled(thumb, 2.0, if vresp.hovered() || vresp.dragged() { t.text_dim } else { t.text_faint });
            app.auto.add("project.vscroll", track, &format!("{scroll}/{max_scroll}"));
            if vresp.dragged() {
                app.ui.project_scroll = (scroll + vresp.drag_delta().y * max_scroll / (track.height() - th).max(1.0)).clamp(0.0, max_scroll);
            }
        }
        let first = ((scroll / ROW_H).floor() as usize).min(rows.len());
        let mut y = list.min.y - (scroll - first as f32 * ROW_H);
        let project = app.session.project.clone();
        for (i, (id, depth)) in rows.iter().enumerate().skip(first) {
            let Some(it) = project.item(*id) else { continue };
            let r = Rect::from_min_size(pos2(list.min.x, y), vec2(list.width(), ROW_H));
            y += ROW_H;
            if r.min.y > list.max.y {
                break;
            }
            if hover_y.is_some_and(|hy| hy >= r.min.y && hy < r.max.y) {
                drop_row = Some(*id);
            }
            let selected = app.session.state.project_selection.contains(id);
            lp.rect_filled(
                r,
                0.0,
                // Flat rows like After Effects' Project panel (no stripes).
                if selected { t.row_selected } else { t.row },
            );
            item_rects.push((*id, r.intersect(list)));
            // Name/icon moves; Type/Size/Duration and the rest of the row start a marquee.
            let move_r = Rect::from_min_max(r.min, pos2(name_end, r.max.y)).intersect(list);
            let info_r = Rect::from_min_max(pos2(name_end, r.min.y), r.max).intersect(list);
            let mut resp = ui.interact(move_r, egui::Id::new(("pitem", id.0)), Sense::click_and_drag());
            let info_resp = ui.interact(info_r, egui::Id::new(("pinfo", id.0)), Sense::click_and_drag());
            app.auto.add(&format!("project.item.{}", id.0), r, &it.name);
            let _ = i;
            let x0 = r.min.x + 8.0 + 14.0 * *depth as f32;
            if it.is_folder() {
                let open = app.ui.project_open_folders.contains(&id.0);
                let tw = Rect::from_center_size(pos2(x0 + 4.0, r.center().y), vec2(12.0, 12.0));
                if widgets::twirl(ui, tw, open, egui::Id::new(("pf", id.0)), &t).clicked() {
                    if open {
                        app.ui.project_open_folders.remove(&id.0);
                    } else {
                        app.ui.project_open_folders.insert(id.0);
                    }
                }
                app.auto.add(&format!("project.item.{}.twirl", id.0), tw, &it.name);
            }
            icons::paint(&lp, Rect::from_center_size(pos2(x0 + 18.0, r.center().y), vec2(14.0, 14.0)), item_icon(it), t.text_dim);
            // Proxy indicator: filled = the proxy is used, hollow = set but off. Click to switch.
            let proxy_job = app.proxy_cache.as_ref().and_then(|c| {
                c.job_for_item(id.0).or_else(|| match &it.kind {
                    ItemKind::Footage(f) => c.job(&f.path),
                    _ => None,
                })
            });
            if let Some(px) = it.proxy.as_ref() {
                let pr = Rect::from_center_size(pos2(x0 + 4.0, r.center().y), vec2(9.0, 9.0));
                if px.enabled {
                    lp.rect_filled(pr, 1.0, t.text);
                } else {
                    lp.rect_stroke(pr, 1.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
                }
                let presp = ui.interact(pr.expand(2.0).intersect(list), egui::Id::new(("pproxy", id.0)), Sense::click()).on_hover_text(format!(
                    "Proxy: {} ({})",
                    px.footage.path,
                    if px.enabled { "in use" } else { "off" }
                ));
                app.auto.add(&format!("project.item.{}.proxy", id.0), pr, if px.enabled { "Proxy in use" } else { "Proxy off" });
                if presp.clicked() {
                    actions.push(("file.useProxy".into(), json!({"item": id.0})));
                }
            } else if let Some(job) =
                proxy_job.as_ref().filter(|j| matches!(j.status, effectcraft_media::ProxyStatus::Queued | effectcraft_media::ProxyStatus::Running))
            {
                let pr = Rect::from_center_size(pos2(x0 + 4.0, r.center().y), vec2(9.0, 9.0));
                lp.rect_stroke(pr, 1.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                let frac = (job.progress as f32 / 1000.0).clamp(0.0, 1.0);
                if frac > 0.0 {
                    lp.rect_filled(Rect::from_min_size(pr.min, vec2(pr.width() * frac, pr.height())), 1.0, t.accent);
                }
                app.auto.add(&format!("project.item.{}.proxy", id.0), pr, &format!("Proxy {}%", (frac * 100.0) as u32));
            }
            let name_clip = Rect::from_min_max(pos2(x0 + 30.0, r.min.y), pos2(r.min.x + name_w + 4.0, r.max.y)).intersect(list);
            let name_rect = Rect::from_min_max(pos2(x0 + 28.0, r.min.y + 2.0), pos2(r.min.x + name_w + 2.0, r.max.y - 2.0));
            // Inline edits (rename / comment).
            let cell_edit = |app: &mut EffectcraftApp, ui: &mut egui::Ui, actions: &mut Vec<(String, serde_json::Value)>, field: &str, cell: Rect| -> bool {
                let Some((eid, ef, mut buf)) = editing.clone().filter(|(e, f, _)| *e == id.0 && f == field) else { return false };
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell));
                let resp = child.add(egui::TextEdit::singleline(&mut buf).desired_width(cell.width()).font(Tokens::ui(12.0)));
                app.auto.add(&format!("project.item.{}.{field}Edit", id.0), cell, field);
                if resp.lost_focus() {
                    ctx.data_mut(|d| d.remove::<Editing>(edit_id()));
                    if !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        let (cmd, params) = if ef == "name" {
                            ("project.rename", json!({"item": eid, "name": buf}))
                        } else {
                            ("project.setComment", json!({"items": [eid], "comment": buf}))
                        };
                        actions.push((cmd.into(), params));
                    }
                } else {
                    widgets::keep_focus(&ctx, &resp, &buf);
                    ctx.data_mut(|d| d.insert_temp::<Editing>(edit_id(), (eid, ef, buf)));
                }
                true
            };
            if !cell_edit(app, ui, &mut actions, "name", name_rect) {
                let max_w = (name_clip.max.x - (x0 + 30.0)).max(8.0);
                let font = Tokens::ui(12.0);
                let color = if selected { Color32::from_gray(0x16) } else { t.text };
                let shown = middle_name(&lp, &it.name, font.clone(), max_w);
                // The selected item's name sits in a light cell with dark text.
                if selected {
                    let g = lp.layout_no_wrap(shown.clone(), font.clone(), Color32::BLACK);
                    let cell = Rect::from_min_max(pos2(x0 + 27.0, r.min.y + 1.0), pos2((x0 + 33.0 + g.size().x).min(name_clip.max.x), r.max.y - 1.0));
                    lp.with_clip_rect(name_clip.expand2(vec2(3.0, 0.0))).rect_filled(cell, 0.0, Color32::from_rgb(0xa6, 0xa6, 0xa6));
                }
                lp.with_clip_rect(name_clip).text(pos2(x0 + 30.0, r.center().y), Align2::LEFT_CENTER, &shown, font, color);
            }
            // Label swatch: click for the label colour menu.
            let sw = Rect::from_center_size(pos2(label_x, r.center().y), vec2(10.0, 10.0));
            lp.rect_filled(sw, 2.0, t.label(it.label));
            let sresp = ui.interact(sw.expand(3.0).intersect(list), egui::Id::new(("plabel", id.0)), Sense::click());
            app.auto.add(&format!("project.item.{}.label", id.0), sw, it.label.name());
            let pop = egui::Id::new(("plabel-pop", id.0));
            if sresp.clicked() {
                widgets::open_popup(ui, pop);
            }
            let names: Vec<String> = effectcraft_engine::color::Label::ALL.iter().map(|l| app.session.prefs.label_name(*l)).collect();
            let cur = effectcraft_engine::color::Label::ALL.iter().position(|l| *l == it.label);
            if let Some(li) = widgets::popup_menu(ui, pop, sw.left_bottom(), &names, cur) {
                let items: Vec<u64> = if selected { app.session.state.project_selection.iter().map(|i| i.0).collect() } else { vec![id.0] };
                actions.push(("project.setLabel".into(), json!({"items": items, "label": li})));
            }
            // Optional columns.
            for (key, _, cx, w) in cols.iter().skip(2) {
                let cell = opt_clip(Rect::from_min_size(pos2(*cx, r.min.y), vec2(*w, r.height())).intersect(list));
                if cell.width() <= 0.0 {
                    continue;
                }
                let cp = lp.with_clip_rect(cell);
                let txt = match *key {
                    "type" => it.type_name().to_string(),
                    "size" => file_size(it).map(fmt_size).unwrap_or_default(),
                    "duration" => it.duration().map(|d| fmt_dur(d.seconds(), it.frame_rate().map(|r| r.as_f64()).unwrap_or(30.0))).unwrap_or_default(),
                    "fps" => it.frame_rate().map(|f| format!("{:.2}", f.as_f64())).unwrap_or_default(),
                    "path" => file_path(it).to_string(),
                    "comment" => {
                        let cr = Rect::from_min_size(pos2(*cx - 2.0, r.min.y + 2.0), vec2(*w, r.height() - 4.0));
                        if cell_edit(app, ui, &mut actions, "comment", cr) {
                            continue;
                        }
                        let cresp = ui.interact(cell, egui::Id::new(("pcomment", id.0)), Sense::click());
                        app.auto.add(&format!("project.item.{}.comment", id.0), cell, &it.comment);
                        if cresp.double_clicked() {
                            ctx.data_mut(|d| d.insert_temp::<Editing>(edit_id(), (id.0, "comment".into(), it.comment.clone())));
                        } else {
                            // Other clicks select the row and open its context menu.
                            resp = resp.union(cresp);
                        }
                        it.comment.clone()
                    }
                    _ => String::new(),
                };
                let align = if *key == "fps" { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
                let tx = if *key == "fps" { cx + w - 6.0 } else { *cx };
                let shown = if matches!(*key, "path" | "comment") { middle_name(&cp, &txt, Tokens::ui(11.5), (*w - 4.0).max(8.0)) } else { txt };
                cp.text(pos2(tx, r.center().y), align, shown, Tokens::ui(11.5), t.text_dim);
            }
            if dragging.is_some_and(|d| d != id.0) && drop_row == Some(*id) && it.is_folder() {
                lp.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            let nresp = ui.interact(name_clip, egui::Id::new(("pname", id.0)), Sense::click_and_drag()).on_hover_text(&it.name);
            app.auto.add(&format!("project.item.{}.name", id.0), name_clip, &it.name);
            resp = resp.union(nresp);
            if (resp.clicked() || info_resp.clicked()) && !resp.dragged() && !info_resp.dragged() {
                apply_click_select(app, &ctx, &rows.iter().map(|(i, _)| *i).collect::<Vec<_>>(), *id, ui.input(|i| i.modifiers));
            }
            // Double-click opens the item, as in After Effects (a comp gets its own Timeline tab and
            // the viewer); Enter or the context menu renames.
            if resp.double_clicked() {
                match &it.kind {
                    ItemKind::Comp(_) => actions.push(("comp.open".into(), json!({"comp": id.0}))),
                    ItemKind::Folder if !app.ui.project_open_folders.remove(&id.0) => {
                        app.ui.project_open_folders.insert(id.0);
                    }
                    // Footage (not data) and solids open in the Footage panel.
                    ItemKind::Footage(f) if f.kind != effectcraft_engine::project::FootageKind::Data => {
                        actions.push(("footage.open".into(), json!({"item": id.0})))
                    }
                    ItemKind::Solid(_) => actions.push(("footage.open".into(), json!({"item": id.0}))),
                    _ => {}
                }
            }
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(&ctx, DragPayload::Item(id.0));
            }
            if info_resp.drag_started() {
                begin_marquee(app, &ctx, info_resp.interact_pointer_pos().unwrap_or(info_resp.rect.center()));
            }
            let sel_n = if selected { app.session.state.project_selection.len() } else { 1 };
            resp.union(info_resp).context_menu(|ui| project_item_menu(ui, app, &ctx, it, selected, sel_n, &mut actions));
        }
    }
    // Dropping a dragged item on the list: into the folder under the pointer (or the folder of
    // the item under it), or the project root below the rows.
    if let Some(iid) = dragging
        && hover_y.is_some_and(|y| y < rect.max.y - footer_h)
    {
        if drop_row.is_none() {
            lp.rect_stroke(list.shrink(1.0), 0.0, Stroke::new(1.0, t.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        }
        if ctx.input(|i| i.pointer.any_released()) && drop_row != Some(ItemId(iid)) {
            let folder = drop_folder(&app.session.project, drop_row);
            let cur = app.session.project.item(ItemId(iid)).and_then(|i| i.parent);
            if folder != cur {
                let moving: Vec<u64> = if app.session.state.project_selection.contains(&ItemId(iid)) {
                    app.session.state.project_selection.iter().map(|i| i.0).collect()
                } else {
                    vec![iid]
                };
                actions.push(("project.move".into(), json!({"items": moving, "folder": folder.map(|f| f.0)})));
                if let Some(f) = folder {
                    app.ui.project_open_folders.insert(f.0);
                }
            }
            egui::DragAndDrop::clear_payload(&ctx);
        }
    }
    // Files dragged from the Media Browser: import.
    if let Some(DragPayload::Files(paths)) = egui::DragAndDrop::payload::<DragPayload>(&ctx).as_deref()
        && hover_y.is_some()
    {
        lp.rect_stroke(list.shrink(1.0), 0.0, Stroke::new(1.0, t.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        if ctx.input(|i| i.pointer.any_released()) {
            actions.push(("mediaBrowser.import".into(), json!({"paths": paths})));
            egui::DragAndDrop::clear_payload(&ctx);
        }
    }
    // Drag ghost.
    if let Some(DragPayload::Item(iid)) = egui::DragAndDrop::payload::<DragPayload>(&ctx).as_deref()
        && let Some(pos) = ctx.pointer_hover_pos()
        && let Some(it) = app.session.project.item(ItemId(*iid))
    {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dnd-ghost")));
        let g = painter.layout_no_wrap(it.name.clone(), Tokens::ui(12.0), t.text);
        let r = Rect::from_min_size(pos + vec2(12.0, 8.0), g.size() + vec2(16.0, 8.0));
        painter.rect_filled(r, 4.0, Color32::from_rgba_premultiplied(40, 40, 40, 230));
        painter.galley(r.min + vec2(8.0, 4.0), g, t.text);
    }
    if empty.drag_started()
        && dragging.is_none()
        && let Some(pos) = empty.interact_pointer_pos()
    {
        let on_move = !is_grid && pos.x < name_end && item_rects.iter().any(|(_, r)| r.contains(pos));
        if !on_move {
            begin_marquee(app, &ctx, pos);
        }
    }
    tick_marquee(app, ui, &ctx, list, &item_rects, &lp);
    // Footer.
    let foot = Rect::from_min_max(pos2(rect.min.x, rect.max.y - footer_h), rect.max);
    p.line_segment([foot.left_top(), foot.right_top()], Stroke::new(1.0, t.separator));
    let mut x = foot.min.x + 8.0;
    for (icon, id, tip) in [
        (Icon::Gear, "interpret", "Interpret Footage"),
        (Icon::NewFolder, "newFolder", "Create a new Folder"),
        (Icon::NewComp, "newComp", "Create a new Composition"),
    ] {
        let r = Rect::from_min_size(pos2(x, foot.min.y + 3.0), vec2(22.0, 22.0));
        let over_drop = id == "newComp" && dragging.is_some() && ui.rect_contains_pointer(r);
        let resp = widgets::icon_button(ui, r, icon, over_drop, &t, egui::Id::new(("pfoot", id))).on_hover_text(tip);
        app.auto.add(&format!("project.{id}"), r, tip);
        if over_drop {
            p.rect_stroke(r.expand(1.0), 4.0, Stroke::new(1.5, SELECT_BLUE), egui::StrokeKind::Outside);
            if ctx.input(|i| i.pointer.any_released()) {
                actions.push(("file.newCompFromSelection".into(), json!({})));
                egui::DragAndDrop::clear_payload(&ctx);
            }
        }
        if resp.clicked() {
            match id {
                "newComp" => crate::panels::dialogs::open_new_comp(app),
                "newFolder" => actions.push(("project.newFolder".into(), json!({}))),
                _ => app.ui.status = "Interpret Footage: select a footage item".into(),
            }
        }
        x += 26.0;
    }
    let depth = app.session.project.settings.bit_depth.label();
    let dr = Rect::from_min_size(pos2(x + 4.0, foot.min.y + 5.0), vec2(48.0, 18.0));
    let dresp = ui.interact(dr, egui::Id::new("bpc"), Sense::click());
    p.text(dr.center(), Align2::CENTER_CENTER, depth, Tokens::ui(11.5), if dresp.hovered() { t.text } else { t.text_dim });
    app.auto.add("project.bitDepth", dr, depth);
    // Click cycles 8/16/32 bpc; Alt-click opens Project Settings (as in After Effects).
    if dresp.clicked() {
        if ui.input(|i| i.modifiers.alt) {
            actions.push(("file.projectSettings".into(), json!({})));
        } else {
            actions.push(("file.cycleBitDepth".into(), json!({})));
        }
    }
    dresp.on_hover_text("Project color depth: click to cycle 8/16/32 bpc, Alt-click for Project Settings");
    x = dr.max.x + 8.0;
    let nsel = app.session.state.project_selection.len();
    if nsel > 0 {
        let label = format!("{nsel} selected");
        let w = 70.0 + (nsel.ilog10() as f32) * 8.0;
        let badge = Rect::from_min_size(pos2(x, foot.min.y + 5.0), vec2(w, 18.0));
        p.rect_filled(badge, 9.0, SELECT_BLUE);
        p.text(pos2(badge.min.x + 8.0, badge.center().y), Align2::LEFT_CENTER, &label, Tokens::ui(11.0), Color32::WHITE);
        let xr = Rect::from_center_size(pos2(badge.max.x - 10.0, badge.center().y), vec2(10.0, 10.0));
        p.text(xr.center(), Align2::CENTER_CENTER, "×", Tokens::ui(12.0), Color32::WHITE);
        let bresp = ui.interact(badge, egui::Id::new("proj-sel-badge"), Sense::click());
        app.auto.add("project.selectedCount", badge, &label);
        if bresp.clicked() {
            actions.push(("project.select".into(), json!({"items": []})));
        }
        x = badge.max.x + 8.0;
    }
    // List / grid toggle, then a thumbnail size slider in grid view.
    let list_r = Rect::from_min_size(pos2(foot.max.x - 82.0, foot.min.y + 3.0), vec2(22.0, 22.0));
    let grid_r = Rect::from_min_size(pos2(foot.max.x - 56.0, foot.min.y + 3.0), vec2(22.0, 22.0));
    if widgets::icon_button(ui, list_r, Icon::ListRows, app.ui.project_view == ProjectView::List, &t, egui::Id::new("pview-list")).clicked() {
        app.ui.project_view = ProjectView::List;
    }
    app.auto.add("project.view.list", list_r, "List view");
    if widgets::icon_button(ui, grid_r, Icon::Grid, app.ui.project_view == ProjectView::Grid, &t, egui::Id::new("pview-grid")).clicked() {
        app.ui.project_view = ProjectView::Grid;
    }
    app.auto.add("project.view.grid", grid_r, "Grid view");
    if is_grid {
        let sr = Rect::from_min_size(pos2(x, foot.min.y + 6.0), vec2((list_r.min.x - x - 8.0).clamp(40.0, 90.0), 16.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sr));
        let r = child.add(egui::Slider::new(&mut app.ui.project_thumb_size, 64.0..=200.0).show_value(false));
        app.auto.add("project.thumbSize", r.rect, "Thumbnail size");
    }
    let tr = Rect::from_min_size(pos2(foot.max.x - 30.0, foot.min.y + 3.0), vec2(22.0, 22.0));
    if widgets::icon_button(ui, tr, Icon::Trash, false, &t, egui::Id::new("ptrash")).on_hover_text("Delete selected project items").clicked()
        && !app.session.state.project_selection.is_empty()
    {
        actions.push(("project.delete".into(), json!({})));
    }
    app.auto.add("project.delete", tr, "Delete");
    if empty.double_clicked() {
        actions.push(("file.import".into(), json!({})));
    }
    for (id, params) in actions {
        if let Err(e) = crate::menus::invoke(app, &ctx, &id, params) {
            app.ui.status = e;
        }
    }
}

fn paint_item_thumb(app: &EffectcraftApp, p: &egui::Painter, ctx: &egui::Context, thumb: Rect, it: &Item, t: &Tokens) {
    match &it.kind {
        ItemKind::Solid(s) => {
            p.rect_filled(thumb.shrink(2.0), 2.0, Color32::from_rgb((s.color[0] * 255.0) as u8, (s.color[1] * 255.0) as u8, (s.color[2] * 255.0) as u8));
        }
        _ => match thumbnail(app, ctx, it) {
            Some(tex) => {
                let [w, h] = tex.size();
                p.image(tex.id(), fit(thumb, w as f32, h as f32), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => icons::paint(p, Rect::from_center_size(thumb.center(), vec2(24.0, 24.0)), item_icon(it), t.text_dim),
        },
    }
}

fn paint_multi_header(app: &mut EffectcraftApp, p: &egui::Painter, ctx: &egui::Context, head: Rect, items: &[Item], t: &Tokens) {
    let n = items.len();
    let stack = Rect::from_min_size(head.min + vec2(10.0, 10.0), vec2(96.0, 54.0));
    p.rect_filled(stack, 3.0, Color32::from_rgb(0x15, 0x15, 0x15));
    for (i, it) in items.iter().take(3).rev().enumerate() {
        let off = vec2(i as f32 * 6.0, i as f32 * 4.0);
        let r = Rect::from_min_size(stack.min + off, stack.size() - vec2(12.0, 10.0));
        p.rect_filled(r, 2.0, Color32::from_rgb(0x20, 0x20, 0x20));
        paint_item_thumb(app, p, ctx, r.shrink(2.0), it, t);
    }
    app.auto.add("project.thumbnail", stack, &format!("{n} selected"));
    let tx = stack.max.x + 10.0;
    let max_w = (head.max.x - 10.0 - tx).max(20.0);
    let title = format!("{n} items selected");
    let r = widgets::text_fit(p, pos2(tx, stack.min.y + 8.0), Align2::LEFT_CENTER, &title, Tokens::semibold(12.0), max_w, t.text);
    app.auto.add("project.details.name", r, &title);
    let refs: Vec<&Item> = items.iter().collect();
    let summary = count_kinds(&refs).summary();
    let r = widgets::text_fit(p, pos2(tx, stack.min.y + 24.0), Align2::LEFT_CENTER, &summary, Tokens::ui(11.0), max_w, t.text_dim);
    app.auto.add("project.details.0", r, &summary);
    let hint = "Right-click › New Comp From Selection…";
    let r = widgets::text_fit(p, pos2(tx, stack.min.y + 40.0), Align2::LEFT_CENTER, hint, Tokens::ui(11.0), max_w, t.text_faint);
    app.auto.add("project.details.1", r, hint);
}

fn begin_marquee(app: &EffectcraftApp, ctx: &egui::Context, pos: egui::Pos2) {
    let m = ui_input_op(ctx);
    ctx.data_mut(|d| {
        d.insert_temp(
            marquee_id(),
            MarqueeDrag { start_content: content_pos(pos, app.ui.project_scroll), previous: app.session.state.project_selection.clone(), op: m },
        );
    });
}

fn ui_input_op(ctx: &egui::Context) -> SelectOp {
    ctx.input(|i| select_op(i.modifiers.shift, i.modifiers.command))
}

fn tick_marquee(app: &mut EffectcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, list: Rect, items: &[(ItemId, Rect)], lp: &egui::Painter) {
    let mut marquee: Option<MarqueeDrag> = ctx.data(|d| d.get_temp(marquee_id()));
    if app.ui.focused == crate::dock::PanelKind::Project
        && app.dialog.is_none()
        && !ctx.egui_wants_keyboard_input()
        && ctx.input(|i| i.key_pressed(egui::Key::Escape))
    {
        app.session.state.project_selection =
            project_select::esc_selection(marquee.is_some(), &marquee.as_ref().map(|m| m.previous.clone()).unwrap_or_default());
        marquee = None;
        ctx.data_mut(|d| d.remove::<MarqueeDrag>(marquee_id()));
        ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
    let pointer = ctx.pointer_hover_pos().or_else(|| ctx.pointer_interact_pos()).filter(|p| list.expand(24.0).contains(*p));
    if marquee.is_some()
        && let Some(pos) = pointer
    {
        let dy = auto_scroll_delta(pos.y, list, SCROLL_BAND, 16.0);
        if dy.abs() > 0.05 {
            app.ui.project_scroll = (app.ui.project_scroll + dy).max(0.0);
            ctx.request_repaint();
        }
    }
    if let (Some(m), Some(pos)) = (marquee.as_ref(), pointer) {
        let start_view = pos2(m.start_content.x, m.start_content.y - app.ui.project_scroll);
        let boxr = marquee_rect(start_view, pos).intersect(list.translate(vec2(0.0, 0.0)));
        let vis: Vec<ItemId> = items.iter().map(|(id, _)| *id).collect();
        let hit: Vec<bool> = items.iter().map(|(_, r)| touches(*r, boxr)).collect();
        app.session.state.project_selection = project_select::apply_marquee(&vis, &hit, &m.previous, m.op);
        lp.rect_filled(boxr, 2.0, marquee_fill());
        lp.rect_stroke(boxr, 2.0, Stroke::new(1.2, SELECT_BLUE), egui::StrokeKind::Inside);
    }
    if ctx.input(|i| i.pointer.any_released()) {
        ctx.data_mut(|d| d.remove::<MarqueeDrag>(marquee_id()));
    } else if let Some(m) = marquee {
        ctx.data_mut(|d| d.insert_temp(marquee_id(), m));
    }
    let _ = ui;
}

fn apply_click_select(app: &mut EffectcraftApp, ctx: &egui::Context, visible: &[ItemId], clicked: ItemId, mods: egui::Modifiers) {
    let op = select_op(mods.shift, mods.command);
    let anchor = ctx.data(|d| d.get_temp::<ItemId>(selection_anchor_id()));
    let (sel, new_anchor) = click_select(visible, clicked, &app.session.state.project_selection, anchor, op);
    app.session.state.project_selection = sel;
    ctx.data_mut(|d| d.insert_temp(selection_anchor_id(), new_anchor));
}

fn project_item_menu(
    ui: &mut egui::Ui,
    app: &EffectcraftApp,
    ctx: &egui::Context,
    it: &Item,
    selected: bool,
    sel_n: usize,
    actions: &mut Vec<(String, serde_json::Value)>,
) {
    let n_label = if sel_n == 1 { "1 item".into() } else { format!("{sel_n} items") };
    let ncs = egui::RichText::new(format!("New Comp From Selection…    {n_label}")).color(Color32::WHITE);
    if ui.add(egui::Button::new(ncs).fill(SELECT_BLUE)).clicked() {
        if !selected {
            actions.push(("project.select".into(), json!({"items": [it.id.0]})));
        }
        actions.push(("file.newCompFromSelection".into(), json!({})));
        ui.close();
    }
    if matches!(it.kind, ItemKind::Footage(_)) {
        if it.proxy.is_some() {
            if ui.button("Remove Proxy").clicked() {
                if let ItemKind::Footage(f) = &it.kind
                    && let Some(c) = &app.proxy_cache
                {
                    c.remove(&f.path);
                }
                actions.push(("file.setProxyNone".into(), json!({"item": it.id.0})));
                ui.close();
            }
        } else if ui.button("Create Proxy").clicked() {
            if let ItemKind::Footage(f) = &it.kind
                && let Some(c) = &app.proxy_cache
            {
                c.start(app.session.footage.clone(), f, it.id);
            }
            ui.close();
        }
    }
    if !it.is_folder() && ui.button("Add to Composition").clicked() {
        actions.push(("layer.addItem".into(), json!({"item": it.id.0})));
        ui.close();
    }
    if it.parent.is_some() && ui.button("Move to Project Root").clicked() {
        actions.push(("project.move".into(), json!({"items": [it.id.0], "folder": null})));
        ui.close();
    }
    if matches!(it.kind, ItemKind::Comp(_)) && ui.button("Open Composition").clicked() {
        actions.push(("comp.open".into(), json!({"comp": it.id.0})));
        ui.close();
    }
    if ui.button("Rename").clicked() {
        ctx.data_mut(|d| d.insert_temp::<Editing>(edit_id(), (it.id.0, "name".into(), it.name.clone())));
        ui.close();
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        let items: Vec<u64> = if selected {
            /* filled by caller via selection */
            vec![]
        } else {
            vec![it.id.0]
        };
        if items.is_empty() {
            actions.push(("project.delete".into(), json!({})));
        } else {
            actions.push(("project.delete".into(), json!({"items": items})));
        }
        ui.close();
    }
}

fn folder_path(project: &effectcraft_engine::project::Project, folder: Option<ItemId>) -> Vec<(Option<ItemId>, String)> {
    let mut chain = vec![];
    let mut cur = folder;
    let mut guard = 0;
    while let Some(id) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(it) = project.item(id) else { break };
        chain.push((Some(id), it.name.clone()));
        cur = it.parent;
    }
    chain.reverse();
    let mut out = vec![(None, "Project".into())];
    out.extend(chain);
    out
}

fn paint_breadcrumb(app: &mut EffectcraftApp, ui: &mut egui::Ui, p: &egui::Painter, hdr: Rect, t: &Tokens) {
    p.line_segment([hdr.left_bottom(), hdr.right_bottom()], Stroke::new(1.0, t.separator));
    let path = folder_path(&app.session.project, app.ui.project_grid_folder.map(ItemId));
    let mut x = hdr.min.x + 10.0;
    let mut crumb = String::new();
    for (i, (id, name)) in path.iter().enumerate() {
        if i > 0 {
            p.text(pos2(x, hdr.center().y), Align2::LEFT_CENTER, "›", Tokens::ui(11.0), t.text_faint);
            x += 12.0;
            crumb.push_str(" › ");
        }
        let w = name.len() as f32 * 6.5 + 8.0;
        let r = Rect::from_min_size(pos2(x, hdr.min.y), vec2(w.min(hdr.max.x - x), hdr.height()));
        let resp = ui.interact(r, egui::Id::new(("pcrumb", id.map(|i| i.0).unwrap_or(0))), Sense::click());
        p.text(r.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, name, Tokens::ui(11.5), if resp.hovered() { t.text } else { t.text_dim });
        if resp.clicked() {
            app.ui.project_grid_folder = id.map(|i| i.0);
            app.ui.project_scroll = 0.0;
        }
        crumb.push_str(name);
        x += w;
        if x > hdr.max.x - 8.0 {
            break;
        }
    }
    app.auto.add("project.breadcrumb", hdr, &crumb);
}

fn grid_items(app: &EffectcraftApp) -> Vec<ItemId> {
    let folder = app.ui.project_grid_folder.map(ItemId);
    let mut kids = app.session.project.children(folder);
    sort_items(&mut kids, &app.ui.project_sort, app.ui.project_sort_desc);
    let q = app.ui.project_search.to_lowercase();
    kids.into_iter().filter(|it| q.is_empty() || it.name.to_lowercase().contains(&q)).map(|it| it.id).collect()
}

fn show_grid(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    list: Rect,
    lp: &egui::Painter,
    t: &Tokens,
    ctx: &egui::Context,
    actions: &mut Vec<(String, serde_json::Value)>,
    item_rects: &mut Vec<(ItemId, Rect)>,
    drop_row: &mut Option<ItemId>,
    editing: Option<Editing>,
) {
    let items = grid_items(app);
    if ctx.input(|i| i.key_pressed(egui::Key::Backspace))
        && app.ui.focused == crate::dock::PanelKind::Project
        && editing.is_none()
        && app.dialog.is_none()
        && !ctx.egui_wants_keyboard_input()
        && let Some(cur) = app.ui.project_grid_folder.and_then(|id| app.session.project.item(ItemId(id)))
    {
        app.ui.project_grid_folder = cur.parent.map(|p| p.0);
        app.ui.project_scroll = 0.0;
    }
    let thumb = app.ui.project_thumb_size.clamp(64.0, 200.0);
    let tile_w = thumb + 16.0;
    let tile_h = thumb + 36.0;
    let cols = ((list.width() - 12.0) / tile_w).floor().max(1.0) as usize;
    let rows = items.len().div_ceil(cols);
    let content_h = rows as f32 * tile_h + 8.0;
    let max_scroll = (content_h - list.height()).max(0.0);
    if ui.rect_contains_pointer(list) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            app.ui.project_scroll -= dy;
        }
    }
    app.ui.project_scroll = app.ui.project_scroll.clamp(0.0, max_scroll);
    let scroll = app.ui.project_scroll;
    if max_scroll > 0.0 {
        let track = Rect::from_min_max(pos2(list.max.x - 6.0, list.min.y + 1.0), pos2(list.max.x - 2.0, list.max.y - 4.0));
        let th = (track.height() * list.height() / content_h).clamp(16.0, track.height());
        let thumb_r = Rect::from_min_size(pos2(track.min.x, track.min.y + (track.height() - th) * (scroll / max_scroll)), vec2(track.width(), th));
        lp.rect_filled(thumb_r, 2.0, t.text_faint);
    }
    let hover = ctx.pointer_hover_pos().filter(|p| list.contains(*p));
    let project = app.session.project.clone();
    for (i, id) in items.iter().enumerate() {
        let Some(it) = project.item(*id) else { continue };
        let col = i % cols;
        let row = i / cols;
        let x = list.min.x + 8.0 + col as f32 * tile_w;
        let y = list.min.y + 6.0 + row as f32 * tile_h - scroll;
        let tile = Rect::from_min_size(pos2(x, y), vec2(tile_w - 8.0, tile_h - 6.0));
        if tile.max.y < list.min.y || tile.min.y > list.max.y {
            continue;
        }
        let vis = tile.intersect(list);
        item_rects.push((*id, vis));
        if hover.is_some_and(|p| vis.contains(p)) {
            *drop_row = Some(*id);
        }
        let selected = app.session.state.project_selection.contains(id);
        let pad = Rect::from_min_size(tile.min + vec2(4.0, 2.0), vec2(tile.width() - 8.0, thumb - 4.0));
        lp.rect_filled(pad, 3.0, Color32::from_rgb(0x14, 0x14, 0x14));
        let (tw, th) = it.dimensions().unwrap_or((4, 3));
        let dest = fit(pad.shrink(3.0), tw as f32, th.max(1) as f32);
        paint_item_thumb(app, lp, ctx, dest, it, t);
        if selected {
            lp.rect_stroke(tile.shrink(1.0), 3.0, Stroke::new(1.8, SELECT_BLUE), egui::StrokeKind::Inside);
            let tick = Rect::from_min_size(pos2(tile.max.x - 18.0, tile.min.y + 4.0), vec2(14.0, 14.0));
            lp.circle_filled(tick.center(), 7.0, SELECT_BLUE);
            lp.text(tick.center(), Align2::CENTER_CENTER, "✓", Tokens::ui(10.0), Color32::WHITE);
        }
        let shown = middle_name(lp, &it.name, Tokens::ui(11.0), tile.width() - 8.0);
        lp.text(pos2(tile.min.x + 4.0, pad.max.y + 8.0), Align2::LEFT_CENTER, shown, Tokens::ui(11.0), t.text);
        if let Some((w, h)) = it.dimensions() {
            widgets::text_fit(
                lp,
                pos2(tile.min.x + 4.0, pad.max.y + 20.0),
                Align2::LEFT_CENTER,
                &format!("{w}×{h}"),
                Tokens::ui(10.0),
                tile.width() - 8.0,
                t.text_dim,
            );
        }
        let move_r = pad.intersect(list);
        let resp = ui.interact(move_r, egui::Id::new(("ptile", id.0)), Sense::click_and_drag());
        app.auto.add(&format!("project.item.{}", id.0), tile, &it.name);
        app.auto.add(&format!("project.item.{}.name", id.0), Rect::from_min_size(pos2(tile.min.x, pad.max.y), vec2(tile.width(), 22.0)), &it.name);
        if resp.clicked() {
            apply_click_select(app, ctx, &items, *id, ui.input(|i| i.modifiers));
        }
        if resp.double_clicked() {
            match &it.kind {
                ItemKind::Folder => {
                    app.ui.project_grid_folder = Some(id.0);
                    app.ui.project_scroll = 0.0;
                }
                ItemKind::Comp(_) => actions.push(("comp.open".into(), json!({"comp": id.0}))),
                ItemKind::Footage(f) if f.kind != effectcraft_engine::project::FootageKind::Data => {
                    actions.push(("footage.open".into(), json!({"item": id.0})))
                }
                ItemKind::Solid(_) => actions.push(("footage.open".into(), json!({"item": id.0}))),
                _ => {}
            }
        }
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ctx, DragPayload::Item(id.0));
        }
        let sel_n = if selected { app.session.state.project_selection.len() } else { 1 };
        resp.context_menu(|ui| project_item_menu(ui, app, ctx, it, selected, sel_n, actions));
    }
}

fn middle_name(p: &egui::Painter, text: &str, font: egui::FontId, max_w: f32) -> String {
    let measure = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x;
    project_cols::middle_ellipsis(text, max_w, measure)
}

fn persist_col_widths(app: &mut EffectcraftApp) {
    app.session.prefs.extra.insert("projectColumnWidths".into(), project_cols::widths_to_json(&app.ui.project_col_widths));
    app.session.save_prefs();
}

/// Drag the column-header divider to resize; double-click auto-fits to the longest cell.
fn column_divider(app: &mut EffectcraftApp, ui: &mut egui::Ui, p: &egui::Painter, hdr: Rect, key: &str, x: f32, displayed_w: f32, t: &Tokens) {
    let hit = Rect::from_center_size(pos2(x, hdr.center().y), vec2(project_cols::DIVIDER_HIT, hdr.height() + 4.0)).intersect(hdr.expand2(vec2(2.0, 0.0)));
    let resp = ui.interact(hit, egui::Id::new(("proj-col-div", key.to_string())), Sense::click_and_drag());
    app.auto.add(&format!("project.colResize.{key}"), hit, "Resize column");
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        let line = Rect::from_center_size(pos2(x, hdr.center().y), vec2(2.0, hdr.height()));
        p.rect_filled(line, 0.0, if resp.dragged() { SELECT_BLUE } else { t.focus });
    }
    if resp.dragged() {
        let min = if key == "name" { project_cols::NAME_MIN } else { project_cols::COL_MIN };
        let nw = project_cols::resize_width(displayed_w, resp.drag_delta().x, min, project_cols::COL_MAX);
        project_cols::set_width(&mut app.ui.project_col_widths, key, nw);
    }
    if resp.drag_stopped() {
        persist_col_widths(app);
    }
    if resp.double_clicked() {
        auto_fit_column(app, ui, key);
        persist_col_widths(app);
    }
}

fn auto_fit_column(app: &mut EffectcraftApp, ui: &egui::Ui, key: &str) {
    let font = Tokens::ui(if key == "name" { 12.0 } else { 11.5 });
    let measure = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x;
    let rows = visible_rows(app);
    let project = app.session.project.clone();
    let mut cells: Vec<String> = Vec::new();
    for (id, _) in &rows {
        let Some(it) = project.item(*id) else { continue };
        let txt = match key {
            "name" => it.name.clone(),
            "type" => it.type_name().to_string(),
            "size" => file_size(it).map(fmt_size).unwrap_or_default(),
            "duration" => it.duration().map(|d| fmt_dur(d.seconds(), it.frame_rate().map(|r| r.as_f64()).unwrap_or(30.0))).unwrap_or_default(),
            "fps" => it.frame_rate().map(|f| format!("{:.2}", f.as_f64())).unwrap_or_default(),
            "path" => file_path(it).to_string(),
            "comment" => it.comment.clone(),
            _ => String::new(),
        };
        cells.push(txt);
    }
    let header = COLUMNS.iter().find(|c| c.0 == key).map(|c| c.1).unwrap_or("Name");
    let min = if key == "name" { project_cols::NAME_MIN } else { project_cols::COL_MIN };
    let refs: Vec<&str> = cells.iter().map(String::as_str).collect();
    let w = project_cols::auto_fit_width(header, refs, measure, min, project_cols::COL_MAX);
    project_cols::set_width(&mut app.ui.project_col_widths, key, w);
}

/// The column header's context menu: show or hide the optional columns.
fn column_menu(app: &mut EffectcraftApp, resp: &egui::Response) {
    resp.context_menu(|ui| {
        ui.label(egui::RichText::new("Columns").weak());
        for (key, label, _) in COLUMNS {
            let mut on = app.ui.project_columns.iter().any(|c| c == key);
            if ui.checkbox(&mut on, label).changed() {
                set_column(&mut app.ui.project_columns, key, on);
            }
        }
    });
}

/// Show or hide a column, keeping the canonical column order.
pub fn set_column(cols: &mut Vec<String>, key: &str, on: bool) {
    cols.retain(|c| c != key);
    if on {
        cols.push(key.to_string());
        cols.sort_by_key(|c| COLUMNS.iter().position(|x| x.0 == c).unwrap_or(usize::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ae_duration_and_size_formats() {
        assert_eq!(fmt_dur(10.0, 29.97), "0:00:10:00");
        assert_eq!(fmt_dur(3725.48, 25.0), "1:02:05:12");
        assert_eq!(fmt_size(1000), "1 KB");
        assert_eq!(fmt_size(5 * 1024 * 1024 + 300_000), "5.3 MB");
    }

    #[test]
    fn folders_sort_with_items_by_name() {
        let mut p = effectcraft_engine::project::Project::default();
        let l = effectcraft_engine::color::Label::Yellow;
        p.add_item("Solids", l, None, ItemKind::Folder);
        p.add_item("Precomps", l, None, ItemKind::Folder);
        p.add_item("EffectCraft Intro", l, None, ItemKind::Folder);
        let mut kids = p.children(None);
        sort_items(&mut kids, "name", false);
        let names: Vec<&str> = kids.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["EffectCraft Intro", "Precomps", "Solids"]);
        sort_items(&mut kids, "name", true);
        assert_eq!(kids[0].name, "Solids");
    }

    #[test]
    fn drop_targets_and_column_toggles() {
        let mut p = effectcraft_engine::project::Project::default();
        let l = effectcraft_engine::color::Label::Yellow;
        let f = p.add_item("F", l, None, ItemKind::Folder);
        let inner = p.add_item("I", l, Some(f), ItemKind::Folder);
        assert_eq!(drop_folder(&p, Some(f)), Some(f));
        assert_eq!(drop_folder(&p, Some(inner)), Some(inner));
        assert_eq!(drop_folder(&p, None), None);
        let mut cols = crate::state::default_project_columns();
        set_column(&mut cols, "comment", true);
        set_column(&mut cols, "path", true);
        set_column(&mut cols, "size", false);
        assert_eq!(cols, ["type", "duration", "fps", "path", "comment"]);
    }
}
