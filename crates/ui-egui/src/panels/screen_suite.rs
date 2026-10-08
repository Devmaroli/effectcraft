//! Window ▸ Screen Suite — Option 1 tool rail (v2): Size Sorter → Screen Manager → EncodeCraft.

use effectcraft_engine::commands::screen::apply_source;
use effectcraft_screens::sorter::{hidden_row_count, row_size_text};
use effectcraft_screens::{AlertLevel, CompNameFrom, JobMode, MatchMode, PanelAlert, collect_alerts, default_send_preset, tab_alert_count};
use egui::{Align, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2, pos2};
use serde_json::{Value, json};

use crate::EffectcraftApp;
use crate::dock::PanelKind;
use crate::icons::{self, Icon};
use crate::panels::forms::Field;

const TOOLS: [(&str, &str, &str, &str, Icon); 8] = [
    ("booking", "Size Sorter", "screenSuite.tab.booking", "Alt+1", Icon::Grid),
    ("build", "Screen Manager", "screenSuite.tab.build", "Alt+2", Icon::Comp),
    ("adapter", "Screen Adapter", "screenSuite.tab.adapter", "Alt+3", Icon::Link),
    ("sizeMaster", "SizeMaster", "screenSuite.tab.sizeMaster", "Alt+4", Icon::Rectangle),
    ("freeze", "Freeze", "screenSuite.tab.freeze", "Alt+5", Icon::Pause),
    ("screenshot", "Screenshot", "screenSuite.tab.screenshot", "Alt+6", Icon::Snapshot),
    ("renamer", "Renamer", "screenSuite.tab.renamer", "Alt+7", Icon::Type),
    ("qc", "Size Matcher", "screenSuite.tab.qc", "Alt+8", Icon::Checker),
];

const BLUE: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xd6);
const MUTED: Color32 = Color32::from_gray(140);
const ROW_HOVER: Color32 = Color32::from_rgb(0x24, 0x2c, 0x38);
const ROW_SEL: Color32 = Color32::from_rgb(0x1a, 0x2e, 0x4a);

fn exec(app: &mut EffectcraftApp, id: &str, p: Value) {
    match app.session.execute(id, p) {
        Ok(_) => {}
        Err(e) => app.ui.status = e.to_string(),
    }
}

fn auto_btn(app: &mut EffectcraftApp, ui: &mut egui::Ui, id: &str, label: &str) -> bool {
    let r = ui.button(label);
    app.auto.add(id, r.rect, label);
    r.clicked()
}

/// Cover a visible vertical scrollbar so the agent audit can address it.
fn register_vscroll(app: &mut EffectcraftApp, ui: &egui::Ui, id: &str, inner: Rect, content_h: f32) {
    if content_h <= inner.height() + 1.0 {
        return;
    }
    let w = ui.spacing().scroll.bar_width.max(10.0) + 4.0;
    let bar = Rect::from_min_size(egui::pos2(inner.max.x - w, inner.min.y), Vec2::new(w * 2.0, inner.height().max(2.0)));
    app.auto.add(id, bar, "scroll");
}

fn fill_for(level: AlertLevel) -> Color32 {
    match level {
        AlertLevel::Error => Color32::from_rgb(0x2b, 0x15, 0x13),
        AlertLevel::Warning => Color32::from_rgb(0x33, 0x24, 0x0c),
    }
}

fn stroke_for(app: &EffectcraftApp, level: AlertLevel) -> Stroke {
    match level {
        AlertLevel::Error => Stroke::new(1.0, app.tokens.danger),
        AlertLevel::Warning => Stroke::new(1.0, app.tokens.warning),
    }
}

fn fg_for(level: AlertLevel) -> Color32 {
    match level {
        AlertLevel::Error => Color32::from_rgb(0xff, 0xb4, 0xad),
        AlertLevel::Warning => Color32::from_rgb(0xff, 0xd7, 0x9a),
    }
}

/// Compact single-line pill (~20px). Painted at an exact size so wrapping layouts
/// cannot squeeze the text onto two lines and balloon `CornerRadius(99)` into an oval.
fn pill(ui: &mut egui::Ui, text: &str, fill: Color32, fg: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(10.5), fg);
    let pad_x = 7.0;
    let size = Vec2::new((galley.size().x + pad_x * 2.0).ceil().max(16.0), 20.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(99), fill);
    let pos = pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(pos, galley, fg);
}

/// Wrapping chip rows must not inherit the ScrollArea's leftover height (that leaves a
/// huge gap and pushes the Send card off screen). Pills themselves never wrap. Two 20px
/// rows (44px) is enough at a narrow dock; extra space on a wide panel is only ~12px.
fn wrap_chips(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width().max(1.0);
    let h = 44.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Min).with_main_wrap(true)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
    add(&mut child);
}

fn table_cell(ui: &mut egui::Ui, row: Rect, x: f32, w: f32, layout: Layout, add: impl FnOnce(&mut egui::Ui)) {
    let cell = Rect::from_min_size(pos2(row.min.x + x, row.min.y), Vec2::new(w, row.height()));
    let inner = cell.shrink2(Vec2::new(4.0, 1.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
    child.set_clip_rect(cell.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = Vec2::new(4.0, 1.0);
    add(&mut child);
}

fn primary_btn(app: &mut EffectcraftApp, ui: &mut egui::Ui, id: &str, label: &str) -> bool {
    let r = ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE).strong()).fill(BLUE).min_size(Vec2::new(168.0, 28.0)));
    app.auto.add(id, r.rect, label);
    r.clicked()
}

fn secondary_btn(app: &mut EffectcraftApp, ui: &mut egui::Ui, id: &str, label: &str) -> bool {
    let r = ui.add(
        egui::Button::new(RichText::new(label).color(Color32::from_rgb(0xbc, 0xd9, 0xff)))
            .stroke(Stroke::new(1.0, BLUE))
            .fill(Color32::from_rgb(0x14, 0x1c, 0x28)),
    );
    app.auto.add(id, r.rect, label);
    r.clicked()
}

fn paint_banner(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    if alerts.is_empty() {
        return;
    }
    let errors = alerts.iter().any(|a| a.level == AlertLevel::Error);
    let level = if errors { AlertLevel::Error } else { AlertLevel::Warning };
    let n = alerts.len();
    let inner = egui::Frame::new().fill(fill_for(level)).stroke(stroke_for(app, level)).inner_margin(8.0).corner_radius(6.0).show(ui, |ui| {
        ui.colored_label(fg_for(level), RichText::new(format!("Look out — {n} issue{} in this job", if n == 1 { "" } else { "s" })).strong());
        for a in alerts.iter().take(6) {
            ui.colored_label(fg_for(a.level), format!("• {} ({})", a.title, a.row));
        }
    });
    app.auto.add("screenSuite.alert.banner", inner.response.rect, "Look out");
    app.auto.add("screenSuite.alert.count", inner.response.rect, &n.to_string());
    ui.add_space(4.0);
}

fn paint_issue_card(app: &mut EffectcraftApp, ui: &mut egui::Ui, alert: &PanelAlert, fix: Option<(&str, &str)>) {
    let inner = egui::Frame::new()
        .fill(fill_for(alert.level))
        .stroke(stroke_for(app, alert.level))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(8.0)
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(520.0));
            ui.colored_label(fg_for(alert.level), RichText::new(&alert.title).strong().size(13.0));
            ui.colored_label(fg_for(alert.level), RichText::new(plain_alert_message(&alert.message)).size(12.0));
            if let Some((id, label)) = fix
                && auto_btn(app, ui, id, label)
            {
                exec(app, "screen.suite.tab", json!({"tab": "build"}));
            }
        });
    app.auto.add(&format!("screenSuite.alert.row.{}", alert.id), inner.response.rect, &alert.row);
}

fn paint_alert_row(app: &mut EffectcraftApp, ui: &mut egui::Ui, alert: &PanelAlert, extra: &str) {
    let _ = extra;
    paint_issue_card(app, ui, alert, None);
}

fn alert_for<'a>(alerts: &'a [PanelAlert], tab: &str, name: &str) -> Option<&'a PanelAlert> {
    let n = name.to_ascii_lowercase();
    alerts
        .iter()
        .find(|a| a.tab == tab && (a.row.eq_ignore_ascii_case(name) || a.row.to_ascii_lowercase().contains(&n) || n.contains(&a.row.to_ascii_lowercase())))
}

fn plain_alert_message(msg: &str) -> String {
    let t = msg.trim();
    t.strip_prefix("Look out — ").unwrap_or(t).trim().to_string()
}

fn unique_matcher_problems(alerts: &[PanelAlert]) -> Vec<&PanelAlert> {
    let mut out = Vec::new();
    for a in alerts {
        if a.tab != "qc" && a.kind != "extraStack" {
            continue;
        }
        let same = out.iter().any(|b: &&PanelAlert| {
            b.row.eq_ignore_ascii_case(&a.row)
                && (b.kind == a.kind || matches!((b.kind.as_str(), a.kind.as_str()), ("extraStack", "sizeMismatch") | ("sizeMismatch", "extraStack")))
        });
        if !same {
            out.push(a);
        }
    }
    out
}

fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(15.0));
        if !sub.is_empty() {
            ui.label(RichText::new(sub).small().color(Color32::from_gray(140)));
        }
    });
}

fn sorted_ready(app: &EffectcraftApp) -> bool {
    !app.session.state.screen.sorter.hits.is_empty() || !app.session.state.screen.sorter.unmatched.is_empty()
}

pub fn show(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Min)));
    ui.set_min_size(rect.size());
    let alerts = collect_alerts(&app.session.state.screen.sorter, &app.session.state.screen.manager, &app.session.state.screen.matcher);
    let rail_w = 84.0;
    let rail = Rect::from_min_size(rect.min, Vec2::new(rail_w, rect.height()));
    let body = Rect::from_min_max(egui::pos2(rect.min.x + rail_w, rect.min.y), rect.max);
    paint_rail(app, &mut ui, rail, &alerts);
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    body_ui.set_min_size(Vec2::new((body.width() - 16.0).max(1.0), 0.0));
    body_ui.horizontal(|ui| {
        ui.label(RichText::new("Screen Suite").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let job = if app.session.state.screen.job_name.is_empty() { "Job" } else { &app.session.state.screen.job_name };
            ui.label(RichText::new(format!("Job: {job}")).small().color(Color32::from_gray(150)));
        });
    });
    paint_banner(app, &mut body_ui, &alerts);
    if !app.session.state.screen.diff.is_empty() {
        let note = app.session.state.screen.diff.note();
        body_ui.horizontal(|ui| {
            egui::Frame::new().fill(Color32::from_rgb(0x13, 0x25, 0x38)).stroke(Stroke::new(1.0, Color32::from_rgb(0x2f, 0x80, 0xed))).inner_margin(6.0).show(
                ui,
                |ui| {
                    ui.label(RichText::new(&note).color(Color32::from_rgb(0xbc, 0xd9, 0xff)));
                },
            );
            if auto_btn(app, ui, "screenSuite.updateUndo", "Undo") {
                exec(app, "screen.suite.undo", json!({}));
            }
        });
    }
    let tab = app.session.state.screen.tab.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut body_ui, |ui| match tab.as_str() {
        "build" => manager_tab(app, ui, &alerts),
        "adapter" => adapter_tab(app, ui),
        "sizeMaster" => size_master_tab(app, ui),
        "freeze" => freeze_tab(app, ui),
        "screenshot" => screenshot_tab(app, ui),
        "renamer" => renamer_tab(app, ui),
        "qc" => matcher_tab(app, ui, &alerts),
        _ => sorter_tab(app, ui, &alerts),
    });
}

fn paint_rail(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect, alerts: &[PanelAlert]) {
    ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0x1f, 0x1f, 0x1f));
    ui.painter().line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(0x19, 0x19, 0x19)));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 6.0))).layout(egui::Layout::top_down(egui::Align::Center)));
    let cur = app.session.state.screen.tab.clone();
    let ready = sorted_ready(app);
    let locked = !ready;
    for (id, label, auto, shortcut, icon) in TOOLS {
        let on = cur == id;
        let n = tab_alert_count(alerts, id);
        let updated = app.session.state.screen.updated_tabs.iter().any(|t| t == id);
        let tool_locked = locked && matches!(id, "build" | "adapter" | "sizeMaster" | "qc");
        let fill = if on { Color32::from_rgb(0x2f, 0x6f, 0xd6) } else { Color32::TRANSPARENT };
        let fg = if on { Color32::WHITE } else { Color32::from_gray(189) };
        let r = egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(2, 6)).show(&mut ui, |ui| {
            ui.set_min_width(72.0);
            ui.vertical_centered(|ui| {
                let ir = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover()).0;
                icons::paint(ui.painter(), ir, icon, fg);
                ui.label(RichText::new(label).size(10.0).color(fg));
                ui.label(RichText::new(if tool_locked { "locked" } else { shortcut }).size(9.0).color(fg.gamma_multiply(0.7)));
            });
        });
        app.auto.add(auto, r.response.rect, label);
        if n > 0 {
            app.auto.add(&format!("{auto}.badge"), r.response.rect, &n.to_string());
            let badge = if alerts.iter().any(|a| a.tab == id && a.level == AlertLevel::Error) {
                Color32::from_rgb(0xe5, 0x48, 0x4d)
            } else {
                Color32::from_rgb(0xf5, 0x9e, 0x0b)
            };
            let br = Rect::from_center_size(r.response.rect.right_top() + Vec2::new(-7.0, 7.0), Vec2::splat(12.0));
            ui.painter().circle_filled(br.center(), 6.0, badge);
            ui.painter().text(br.center(), egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(8.5), Color32::WHITE);
        } else if updated {
            let br = Rect::from_center_size(r.response.rect.right_top() + Vec2::new(-7.0, 7.0), Vec2::splat(8.0));
            ui.painter().circle_filled(br.center(), 4.0, Color32::from_rgb(0x2f, 0x80, 0xed));
        }
        if r.response.clicked() && !tool_locked {
            exec(app, "screen.suite.tab", json!({"tab": id}));
        }
        ui.add_space(2.0);
    }
}

fn sorter_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    let hits = app.session.state.screen.sorter.hits.len();
    heading(ui, "Size Sorter", if hits == 0 { "Paste-to-size sorter" } else { "Result" });
    let paste_open = hits == 0 || ui.memory(|m| m.data.get_temp::<bool>(egui::Id::new("screenSuite.pasteOpen")).unwrap_or(false));
    if hits == 0 {
        ui.label(RichText::new("Paste screen names or pixel sizes, one per line, in any order.").small().color(Color32::from_gray(140)));
    } else {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} lines pasted", app.session.state.screen.paste.lines().filter(|l| !l.trim().is_empty()).count())).small().color(MUTED),
            );
            let r = ui.selectable_label(paste_open, if paste_open { "Hide paste" } else { "Edit paste" });
            app.auto.add("screenSuite.paste.toggle", r.rect, "edit paste");
            if r.clicked() {
                ui.memory_mut(|m| m.data.insert_temp(egui::Id::new("screenSuite.pasteOpen"), !paste_open));
            }
        });
    }
    if paste_open {
        let mut paste = app.session.state.screen.paste.clone();
        let paste_rows = if hits == 0 { 8 } else { 4 };
        let r =
            ui.add(egui::TextEdit::multiline(&mut paste).desired_width(f32::INFINITY).desired_rows(paste_rows).hint_text("1. Jahra Prime\n2. Al Salam Sync"));
        app.auto.add("screenSuite.paste", r.rect, "paste");
        if r.changed() {
            app.session.state.screen.paste = paste;
        }
        if r.lost_focus() && sorted_ready(app) {
            exec(app, "screen.sorter.sort", sort_body(app));
        }
    } else {
        app.auto.add("screenSuite.paste", ui.min_rect(), "paste");
    }
    ui.add_space(4.0);
    filters_block(app, ui);
    ui.horizontal(|ui| {
        if auto_btn(app, ui, "screenSuite.sort", "Sort sizes") {
            exec(app, "screen.sorter.sort", sort_body(app));
        }
        if auto_btn(app, ui, "screenSuite.clear", "Clear") {
            app.session.state.screen.paste.clear();
            app.session.state.screen.sorter = Default::default();
        }
        ui.label(RichText::new("Ctrl+Enter sorts").small().color(Color32::from_gray(140)));
    });
    if hits == 0 {
        return;
    }
    let sorter = app.session.state.screen.sorter.clone();
    let associated = sorter.hits.iter().filter(|h| h.reason.contains("Associated")).count();
    let dups = sorter.flags.iter().filter(|f| f.kind == "duplicate").count();
    let review = sorter.flags.iter().filter(|f| f.kind == "lowConfidence" || f.kind == "ambiguous" || f.kind == "unmatched").count();
    wrap_chips(ui, |ui| {
        pill(ui, &format!("Pasted {}", sorter.hits.len()), Color32::from_rgb(0x33, 0x33, 0x33), Color32::from_gray(210));
        pill(
            ui,
            &format!("Matched {}", sorter.hits.len().saturating_sub(sorter.unmatched.len())),
            Color32::from_rgb(0x16, 0x3d, 0x24),
            Color32::from_rgb(0x5f, 0xd5, 0x85),
        );
        pill(ui, &format!("Unique sizes {}", sorter.rows.len()), Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
        pill(ui, &format!("Associated {associated}"), Color32::from_rgb(0x2a, 0x21, 0x3a), Color32::from_rgb(0xc4, 0xb5, 0xfd));
        pill(ui, &format!("Duplicates {dups}"), Color32::from_rgb(0x3a, 0x27, 0x12), Color32::from_rgb(0xfd, 0xba, 0x74));
        if review > 0 {
            pill(ui, &format!("Need review {review}"), Color32::from_rgb(0x3a, 0x2a, 0x10), Color32::from_rgb(0xf5, 0xd0, 0x76));
        }
        if !sorter.unmatched.is_empty() {
            pill(ui, &format!("Unmatched {}", sorter.unmatched.len()), Color32::from_rgb(0x4a, 0x1b, 0x18), Color32::from_rgb(0xff, 0x7b, 0x72));
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("By size groups: screens that share a production size fold into one entry (1.7HD, 2.6). Al Salam Sync and Marina keep their combined deliverable. Screen specific keeps every screen separate.")
            .small()
            .color(MUTED),
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new("Flags").small().color(MUTED));
        let fuzzy = sorter.hits.iter().any(|h| h.reason.contains("fuzzy") || h.score < 0.99);
        let group = associated > 0;
        let alias = sorter.hits.iter().any(|h| h.reason.contains("alias") || h.reason.contains("Alias"));
        if fuzzy {
            pill(ui, "FUZZY", Color32::from_rgb(0x3a, 0x2a, 0x10), Color32::from_rgb(0xf5, 0xd0, 0x76));
        }
        if group {
            pill(ui, "GROUP", Color32::from_rgb(0x2a, 0x21, 0x3a), Color32::from_rgb(0xc4, 0xb5, 0xfd));
        }
        if alias {
            pill(ui, "ALIAS", Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
        }
        if !fuzzy && !group && !alias {
            ui.label(RichText::new("none").small().color(MUTED));
        }
    });
    ui.add_space(6.0);
    paint_sorter_table(app, ui, alerts, &sorter);
    let hidden = hidden_row_count(&sorter);
    if hidden > 0 {
        ui.label(
            RichText::new(format!("{hidden} row{} hidden by filters · still sent", if hidden == 1 { "" } else { "s" }))
                .color(Color32::from_rgb(0x9c, 0xc4, 0xf5)),
        );
    }
    ui.add_space(8.0);
    egui::Frame::new().fill(Color32::from_rgb(0x14, 0x1c, 0x28)).stroke(Stroke::new(1.0, BLUE)).inner_margin(10.0).corner_radius(8.0).show(ui, |ui| {
        ui.label(RichText::new("Send sorted names").strong().size(14.0));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Send as").small().color(MUTED));
            let by = app.session.state.screen.job_mode == JobMode::BySize;
            let n_size = sorter.paste_names_by_size.len();
            let n_spec = sorter.paste_names_screen_specific.len();
            egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x24, 0x32)).corner_radius(6.0).inner_margin(2.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let r = ui.selectable_label(by, format!("By size · {n_size}"));
                    app.auto.add("screenSuite.sendBySizeToggle", r.rect, "By size");
                    if r.clicked() {
                        app.session.state.screen.job_mode = JobMode::BySize;
                    }
                    let r = ui.selectable_label(!by, format!("Screen specific · {n_spec}"));
                    app.auto.add("screenSuite.sendScreenSpecific", r.rect, "Screen specific");
                    if r.clicked() {
                        app.session.state.screen.job_mode = JobMode::ScreenSpecific;
                    }
                });
            });
        });
        let specific = app.session.state.screen.job_mode == JobMode::ScreenSpecific;
        let n = if specific { sorter.paste_names_screen_specific.len() } else { sorter.paste_names_by_size.len() };
        ui.label(RichText::new(format!("Sends all {n} sizes, including {hidden} hidden by filters.")).color(Color32::from_rgb(0xbc, 0xd9, 0xff)));
        ui.horizontal(|ui| {
            if primary_btn(app, ui, "screenSuite.sendBySize", "Send to Screen Manager →") {
                exec(app, "screen.sorter.send", json!({"screenSpecific": specific, "to": "manager"}));
            }
            if secondary_btn(app, ui, "screenSuite.sendToMatcher", "Send to Size Matcher →") {
                exec(app, "screen.sorter.send", json!({"screenSpecific": specific, "to": "matcher"}));
            }
        });
    });
    for o in app.session.state.screen.orphans.clone() {
        if o.keep == Some(true) {
            continue;
        }
        egui::Frame::new().fill(Color32::from_rgb(0x33, 0x24, 0x0c)).stroke(Stroke::new(1.0, Color32::from_rgb(0xb8, 0x6b, 0x0c))).inner_margin(8.0).show(
            ui,
            |ui| {
                ui.label(RichText::new("COMP BUILT FOR A SCREEN NO LONGER IN THE BOOKING").strong().color(Color32::from_rgb(0xf5, 0xb8, 0x4a)));
                ui.label(format!("{}: {} left the booking.", o.name, o.screen));
                ui.horizontal(|ui| {
                    if auto_btn(app, ui, "screenSuite.orphanKeep", "Keep (leave them out of checks)") {
                        exec(app, "screen.suite.keepOrphans", json!({"name": o.name}));
                    }
                    if auto_btn(app, ui, "screenSuite.orphanRemove", "Remove comps") {
                        exec(app, "screen.suite.removeOrphans", json!({"name": o.name}));
                    }
                });
            },
        );
    }
}

fn paint_sorter_table(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert], sorter: &effectcraft_screens::SorterResult) {
    let avail = ui.available_width().max(280.0);
    let w_no = 28.0;
    let w_entry = 132.0;
    let w_covers = 88.0;
    let w_pasted = (avail - w_no - w_entry - w_covers).max(64.0);
    let xs = [0.0, w_no, w_no + w_entry, w_no + w_entry + w_pasted];
    let ws = [w_no, w_entry, w_pasted, w_covers];
    let (hdr, _) = ui.allocate_exact_size(Vec2::new(avail, 18.0), Sense::hover());
    for (i, title) in ["No.", "Entry · size", "Pasted screens", "Covers"].iter().enumerate() {
        table_cell(ui, hdr, xs[i], ws[i], Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(*title).small().strong().color(MUTED));
        });
    }
    ui.add(egui::Separator::default().spacing(2.0));
    let diff = app.session.state.screen.diff.clone();
    let sel_id = egui::Id::new("screenSuite.sorter.sel");
    let mut selected = ui.memory(|m| m.data.get_temp::<usize>(sel_id)).unwrap_or(0);
    let mut vis_i = 0usize;
    for row in &sorter.rows {
        if row.hidden {
            continue;
        }
        vis_i = vis_i.saturating_add(1);
        let added = diff.added.iter().any(|n| n == &row.use_name);
        let alert = alert_for(alerts, "booking", &row.use_name);
        let row_h = 36.0;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, row_h), Sense::click());
        if resp.clicked() {
            selected = vis_i;
        }
        let fill = if let Some(a) = alert {
            fill_for(a.level)
        } else if selected == vis_i {
            ROW_SEL
        } else if resp.hovered() {
            ROW_HOVER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4.0, fill);
        table_cell(ui, rect, xs[0], ws[0], Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(format!("{vis_i}")).small().color(MUTED));
        });
        table_cell(ui, rect, xs[1], ws[1], Layout::top_down(Align::Min), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&row.use_name).strong().color(if added {
                    Color32::from_rgb(0x5f, 0xd5, 0x85)
                } else {
                    Color32::from_gray(230)
                }))
                .truncate()
                .selectable(false),
            );
            ui.add(egui::Label::new(RichText::new(row_size_text(row)).small().color(MUTED)).truncate().selectable(false));
        });
        table_cell(ui, rect, xs[2], ws[2], Layout::top_down(Align::Min), |ui| {
            let extra = row.covers.len().saturating_sub(1);
            let shown = if extra == 0 {
                row.covers.first().cloned().unwrap_or_default()
            } else {
                format!("{} +{extra} more", row.covers.first().cloned().unwrap_or_default())
            };
            let tip = row.covers.join("\n");
            ui.add(egui::Label::new(RichText::new(shown).small().color(Color32::from_gray(200))).truncate().selectable(false)).on_hover_text(&tip);
            if !row.library_hint.is_empty() {
                ui.add(egui::Label::new(RichText::new(&row.library_hint).small().color(MUTED)).truncate().selectable(false));
            }
        });
        table_cell(ui, rect, xs[3], ws[3], Layout::left_to_right(Align::Center), |ui| {
            let label = if row.covers_label.is_empty() { effectcraft_screens::screens_pill(row.count) } else { row.covers_label.clone() };
            pill(ui, &label, Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x9c, 0xc4, 0xf5));
            if added {
                pill(ui, "new", Color32::from_rgb(0x16, 0x3d, 0x24), Color32::from_rgb(0x5f, 0xd5, 0x85));
            }
        });
        app.auto.add(&format!("screenSuite.sorter.row.{vis_i}"), rect, &row.use_name);
        if let Some(a) = alert {
            app.auto.add(&format!("screenSuite.alert.row.{}", a.id), rect, &a.row);
        }
    }
    ui.memory_mut(|m| m.data.insert_temp(sel_id, selected));
    for name in &diff.removed {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).strikethrough().color(Color32::from_rgb(0xff, 0x7b, 0x72)));
            pill(ui, "deleted", Color32::from_rgb(0x4a, 0x1b, 0x18), Color32::from_rgb(0xff, 0x7b, 0x72));
        });
    }
}

fn sort_body(app: &EffectcraftApp) -> Value {
    let st = &app.session.state.screen;
    json!({
        "paste": st.paste.clone(),
        "cleanup": st.cleanup,
        "sendAll": st.send_all,
        "matchMode": if st.match_mode == MatchMode::Strict { "strict" } else { "flexible" },
        "group": st.filters.group.clone(),
        "kind": st.filters.kind.clone(),
        "governorate": st.filters.governorate.clone(),
        "category": st.filters.category.clone(),
        "search": st.filters.search.clone(),
        "animated": st.filters.animated,
    })
}

fn filters_block(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    let mut open = app.session.state.screen.filters_open;
    let flexible = app.session.state.screen.match_mode == MatchMode::Flexible;
    let cleanup = app.session.state.screen.cleanup;
    let kind = app.session.state.screen.filters.kind.clone();
    let group = app.session.state.screen.filters.group.clone();
    let gov = app.session.state.screen.filters.governorate.clone();
    let cat = app.session.state.screen.filters.category.clone();
    let animated = app.session.state.screen.filters.animated;
    let active = usize::from(!kind.is_empty())
        + usize::from(!group.is_empty())
        + usize::from(!gov.is_empty())
        + usize::from(!cat.is_empty())
        + usize::from(animated)
        + usize::from(cleanup)
        + usize::from(flexible);
    wrap_chips(ui, |ui| {
        let r = ui.selectable_label(open, if open { "▾ Filters" } else { "▸ Filters" });
        app.auto.add("screenSuite.filters.toggle", r.rect, "Filters");
        if r.clicked() {
            open = !open;
            app.session.state.screen.filters_open = open;
        }
        pill(ui, &format!("{active} active"), Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
        if flexible {
            pill(ui, "Flexible", Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
        } else {
            pill(ui, "Strict", Color32::from_rgb(0x3a, 0x2a, 0x10), Color32::from_rgb(0xf5, 0xd0, 0x76));
        }
        if cleanup {
            pill(ui, "clean bullets", Color32::from_rgb(0x24, 0x24, 0x24), Color32::from_gray(200));
        }
        if !kind.is_empty() {
            pill(ui, &format!("Type: {}", kind.to_ascii_lowercase()), Color32::from_rgb(0x16, 0x3d, 0x24), Color32::from_rgb(0x5f, 0xd5, 0x85));
        }
        if !group.is_empty() {
            pill(ui, &format!("Group: {group}"), Color32::from_rgb(0x2a, 0x21, 0x3a), Color32::from_rgb(0xc4, 0xb5, 0xfd));
        }
        if !gov.is_empty() {
            pill(ui, &format!("Governorate: {gov}"), Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
        }
        if !cat.is_empty() {
            pill(ui, &format!("Category: {cat}"), Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
        }
        if animated {
            pill(ui, "Animated", Color32::from_rgb(0x3a, 0x2a, 0x10), Color32::from_rgb(0xf5, 0xd0, 0x76));
        }
    });
    if !open && !sorted_ready(app) {
        // Keep the first-run filters visible so the panel matches step 1.
        open = true;
    }
    if !open {
        return;
    }
    egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x1c, 0x1c)).stroke(Stroke::new(1.0, Color32::from_rgb(0x33, 0x33, 0x33))).inner_margin(8.0).show(
        ui,
        |ui| {
            ui.label(RichText::new("Matching mode").small().color(Color32::from_gray(140)));
            ui.horizontal(|ui| {
                let flexible = app.session.state.screen.match_mode == MatchMode::Flexible;
                let r = ui.selectable_label(flexible, "Flexible");
                app.auto.add("screenSuite.mode.flexible", r.rect, "Flexible");
                if r.clicked() {
                    app.session.state.screen.match_mode = MatchMode::Flexible;
                }
                let r = ui.selectable_label(!flexible, "Strict");
                app.auto.add("screenSuite.mode.strict", r.rect, "Strict");
                if r.clicked() {
                    app.session.state.screen.match_mode = MatchMode::Strict;
                }
            });
            let mut cleanup = app.session.state.screen.cleanup;
            let r = ui.checkbox(&mut cleanup, "Clean bullets / numbers from pasted lines");
            app.auto.add("screenSuite.cleanup", r.rect, "cleanup");
            if r.changed() {
                app.session.state.screen.cleanup = cleanup;
            }
            ui.label(RichText::new("Show results for (view only · hidden rows are still sent)").small().color(Color32::from_gray(140)));
            ui.horizontal(|ui| {
                ui.label("Group");
                let mut g = app.session.state.screen.filters.group.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut g).desired_width(90.0).hint_text("All groups"));
                app.auto.add("screenSuite.filter.group", r.rect, "group");
                if r.changed() {
                    app.session.state.screen.filters.group = g;
                }
                ui.label("Type");
                let mut k = app.session.state.screen.filters.kind.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut k).desired_width(80.0).hint_text("All types"));
                app.auto.add("screenSuite.filter.kind", r.rect, "kind");
                if r.changed() {
                    app.session.state.screen.filters.kind = k;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Governorate");
                let mut g = app.session.state.screen.filters.governorate.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut g).desired_width(90.0));
                app.auto.add("screenSuite.filter.governorate", r.rect, "governorate");
                if r.changed() {
                    app.session.state.screen.filters.governorate = g;
                }
                ui.label("Category");
                let mut c = app.session.state.screen.filters.category.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut c).desired_width(90.0));
                app.auto.add("screenSuite.filter.category", r.rect, "category");
                if r.changed() {
                    app.session.state.screen.filters.category = c;
                }
                let mut animated = app.session.state.screen.filters.animated;
                let r = ui.checkbox(&mut animated, "Animated");
                app.auto.add("screenSuite.filter.animated", r.rect, "Animated");
                if r.changed() {
                    app.session.state.screen.filters.animated = animated;
                }
            });
        },
    );
}

fn manager_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    ui.horizontal(|ui| {
        heading(ui, "Screen Manager", "Screens & Presets · Combiners · Log");
        if auto_btn(app, ui, "screenSuite.editLibrary", "Edit screen library…") {
            exec(app, "screen.library.open", json!({}));
            app.show_panel(PanelKind::ScreenLibrary);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Match");
        let by = app.session.state.screen.job_mode == JobMode::BySize;
        if ui.selectable_label(by, "By size").clicked() {
            app.session.state.screen.job_mode = JobMode::BySize;
            exec(app, "screen.manager.select", json!({"jobMode": "bySize"}));
        }
        if ui.selectable_label(!by, "Screen specific").clicked() {
            app.session.state.screen.job_mode = JobMode::ScreenSpecific;
            exec(app, "screen.manager.select", json!({"jobMode": "screenSpecific"}));
        }
        let mut only = app.session.state.screen.manager.show_selected_only;
        let r = ui.checkbox(&mut only, "Show selected only");
        app.auto.add("screenSuite.showSelectedOnly", r.rect, "show selected only");
        if r.changed() {
            app.session.state.screen.manager.show_selected_only = only;
        }
        if auto_btn(app, ui, "screenSuite.selectPasted", "Select pasted names") {
            exec(app, "screen.manager.select", json!({}));
        }
    });
    if app.session.state.screen.job_mode == JobMode::ScreenSpecific {
        ui.label(
            RichText::new("Every screen gets its own comp. A match needs the same pixel size and a name match of 90% or more. No aliases: Baitak ≠ Top Gear.")
                .small()
                .color(Color32::from_rgb(0xbc, 0xd6, 0xf5)),
        );
    }
    let m = app.session.state.screen.manager.clone();
    let avail = ui.available_width().max(280.0);
    let w_asked = 150.0;
    let w_size = 88.0;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(w_asked, 16.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("Material / screen").small().strong().color(MUTED));
        });
        ui.allocate_ui_with_layout(Vec2::new(w_size, 16.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("Size").small().strong().color(MUTED));
        });
        ui.label(RichText::new("New name").small().strong().color(MUTED));
    });
    ui.add(egui::Separator::default().spacing(4.0));
    let pfx = app.session.state.screen.prefix.clone();
    let sfx = app.session.state.screen.suffix.split(',').next().unwrap_or("").trim().to_string();
    let name_from = app.session.state.screen.name_from;
    for row in &m.matches {
        if m.show_selected_only && row.status != "ok" && row.status != "sizeMismatch" && row.status != "missing" {
            // still show problems
        }
        let preview = effectcraft_screens::compose_comp_name(name_from, &row.asked, Some(&row.asked), &pfx, &sfx);
        let alert = alert_for(alerts, "build", &row.asked);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(avail, 24.0), Sense::hover());
        if let Some(a) = alert {
            ui.painter().rect_filled(rect, 4.0, fill_for(a.level));
        }
        let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 1.0))).layout(Layout::left_to_right(Align::Center)));
        row_ui.allocate_ui_with_layout(Vec2::new(w_asked, 22.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(&row.asked).strong().size(12.0));
        });
        row_ui.allocate_ui_with_layout(Vec2::new(w_size, 22.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(format!("{}×{}", row.width, row.height)).small().color(MUTED));
        });
        row_ui.label(RichText::new(preview).small().color(Color32::from_gray(210)));
        if let Some(a) = alert {
            app.auto.add(&format!("screenSuite.alert.row.{}", a.id), rect, &a.row);
        }
    }
    let missing = m.matches.iter().filter(|r| r.status != "ok").count();
    if missing > 0 {
        egui::Frame::new().fill(Color32::from_rgb(0x2a, 0x21, 0x12)).stroke(Stroke::new(1.0, Color32::from_rgb(0x6b, 0x4a, 0x12))).inner_margin(8.0).show(
            ui,
            |ui| {
                ui.label(format!("{missing} screen(s) need a library match. Nothing is added to the library for you."));
                if auto_btn(app, ui, "screenSuite.openLibraryFlag", "Open Screen library…") {
                    exec(app, "screen.library.open", json!({}));
                    app.show_panel(PanelKind::ScreenLibrary);
                }
            },
        );
    }
    naming_block(app, ui);
    ui.add_space(4.0);
    let source = apply_source(&app.session);
    match &source {
        Some(src) => {
            let line = format!("Source: {}", src.name);
            let r = ui.label(RichText::new(&line).small().color(Color32::from_rgb(0xcd, 0xee, 0xd6)));
            app.auto.add("screenSuite.source", r.rect, &src.name);
        }
        None => {
            let r = ui.label(RichText::new("Select a comp or footage in the Project panel first").small().color(Color32::from_rgb(0xff, 0xd7, 0x9a)));
            app.auto.add("screenSuite.source.missing", r.rect, "no source");
        }
    }
    let issues = m.matches.iter().filter(|r| r.status != "ok").count();
    let locked = app.session.state.screen.job_mode == JobMode::ScreenSpecific && issues > 0;
    ui.add_space(6.0);
    if locked {
        ui.add_enabled(false, egui::Button::new(format!("Apply locked · {issues} issues to resolve")));
    } else if source.is_none() {
        let r = ui.add_enabled(false, egui::Button::new("Apply to Footage"));
        app.auto.add("screenSuite.apply", r.rect, "Apply to Footage");
    } else if auto_btn(app, ui, "screenSuite.apply", "Apply to Footage") {
        exec(app, "screen.manager.apply", naming_params(app));
    }
    for c in &m.combiners {
        let line = format!("Combiner {}  {}×{}  ({} faces)", c.name, c.width, c.height, c.faces.len());
        if let Some(a) = alerts.iter().find(|a| a.kind == "extraStack" && (a.row.contains(&c.name) || c.name.contains(&a.row) || a.message.contains(&c.name))) {
            paint_alert_row(app, ui, a, &line);
        } else {
            ui.label(line);
        }
    }
}

fn naming_block(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(Color32::from_rgb(0x1c, 0x1c, 0x1c))
        .stroke(Stroke::new(1.0, Color32::from_rgb(0x33, 0x33, 0x33)))
        .inner_margin(10.0)
        .corner_radius(8.0)
        .show(ui, |ui| {
            ui.label(RichText::new("Comp names from").strong());
            egui::Grid::new("screenSuite.naming.grid").num_columns(2).spacing([12.0, 8.0]).min_col_width(96.0).show(ui, |ui| {
                ui.label(RichText::new("Source").small().color(MUTED));
                ui.horizontal(|ui| {
                    let mat = app.session.state.screen.name_from == CompNameFrom::Material;
                    let r = ui.selectable_label(mat, "Material file");
                    if r.clicked() {
                        exec(app, "screen.suite.naming", json!({"nameFrom": "material"}));
                    }
                    let r = ui.selectable_label(!mat, "Prefix + screen");
                    if r.clicked() {
                        exec(app, "screen.suite.naming", json!({"nameFrom": "prefixScreen"}));
                    }
                });
                ui.end_row();
                ui.label(RichText::new("Prefix").small().color(MUTED));
                ui.horizontal(|ui| {
                    let mut p = app.session.state.screen.prefix.clone();
                    let r = ui.add(egui::TextEdit::singleline(&mut p).desired_width(140.0).hint_text("SpringSale"));
                    app.auto.add("screenSuite.prefix", r.rect, "prefix");
                    if r.changed() {
                        exec(app, "screen.suite.naming", json!({"prefix": p}));
                    }
                    ui.label(RichText::new("Suffix").small().color(MUTED));
                    let mut s = app.session.state.screen.suffix.clone();
                    let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(90.0).hint_text("EN, AR"));
                    app.auto.add("screenSuite.suffix", r.rect, "suffix");
                    if r.changed() {
                        exec(app, "screen.suite.naming", json!({"suffix": s}));
                    }
                });
                ui.end_row();
                ui.label(RichText::new("Duration").small().color(MUTED));
                ui.horizontal(|ui| {
                    let mut d = app.session.state.screen.duration_s;
                    let r = ui.add(egui::DragValue::new(&mut d).speed(0.5).suffix(" s"));
                    if r.changed() {
                        app.session.state.screen.duration_s = d;
                    }
                });
                ui.end_row();
            });
            let preview = {
                let pfx = app.session.state.screen.prefix.clone();
                let sfx = app.session.state.screen.suffix.split(',').next().unwrap_or("").trim().to_string();
                let core = if app.session.state.screen.name_from == CompNameFrom::Material {
                    app.session
                        .state
                        .project_selection
                        .iter()
                        .find_map(|id| app.session.project.item(*id).map(|i| i.name.clone()))
                        .map(|n| effectcraft_screens::material_stem(&n))
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| "TopGear_final_v2".into())
                } else {
                    app.session.state.screen.manager.selected.first().cloned().unwrap_or_else(|| "1.7HD".into())
                };
                effectcraft_screens::compose_comp_name(app.session.state.screen.name_from, &core, Some(&core), &pfx, &sfx)
            };
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Preview: {preview}")).small().color(Color32::from_rgb(0xf0, 0xd9, 0xa8)));
            ui.label(
                RichText::new("Original files are never renamed, not on disk and not in the Project panel.").small().color(Color32::from_rgb(0xcd, 0xee, 0xd6)),
            );
            app.auto.add("screenSuite.naming.lock", ui.min_rect(), "never rename footage");
        });
}

fn naming_params(app: &EffectcraftApp) -> Value {
    let st = &app.session.state.screen;
    json!({
        "prefix": st.prefix,
        "suffix": st.suffix,
        "duration": st.duration_s,
        "nameFrom": if st.name_from == CompNameFrom::Material { "material" } else { "prefixScreen" },
    })
}

fn size_master_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    heading(ui, "SizeMaster", "Same merged screen list as Screen Manager · 15 s mall durations");
    if auto_btn(app, ui, "screenSuite.sizeMaster", "Apply SizeMaster") {
        exec(app, "screen.sizeMaster.apply", json!({"tool": "sizeMaster"}));
    }
    manager_tab(app, ui, &[]);
}

fn adapter_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    heading(ui, "Screen Adapter", "Tag selected layers");
    ui.label("Tag selected layers: @logo @headline @cta @product @bg @panel");
    let mut tag = app.session.state.screen.adapter_tag.clone();
    let r = ui.add(egui::TextEdit::singleline(&mut tag).desired_width(f32::INFINITY).hint_text("@logo .keep"));
    app.auto.add("screenSuite.adapter.tag", r.rect, "tag");
    if r.changed() {
        app.session.state.screen.adapter_tag = tag.clone();
    }
    if auto_btn(app, ui, "screenSuite.adapter.parse", "Parse tag") {
        exec(app, "screen.adapter.parse", json!({"text": tag}));
    }
    if auto_btn(app, ui, "screenSuite.adapter.apply", "Apply tag to selected layers") {
        exec(app, "screen.adapter.apply", json!({"text": app.session.state.screen.adapter_tag.clone()}));
    }
}

fn freeze_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    heading(ui, "Freeze", "Freeze the current frame on selected layers");
    if auto_btn(app, ui, "screenSuite.freeze", "Freeze frame") {
        exec(app, "screen.freeze", json!({}));
    }
}

fn screenshot_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    heading(ui, "Screenshot", "Save a PNG of the active composition");
    let mut folder = app.session.state.screen.screenshot_folder.clone();
    let r = ui.add(egui::TextEdit::singleline(&mut folder).desired_width(f32::INFINITY).hint_text("screenshot folder"));
    app.auto.add("screenSuite.screenshotFolder", r.rect, "folder");
    if r.changed() {
        app.session.state.screen.screenshot_folder = folder;
    }
    if auto_btn(app, ui, "screenSuite.screenshot", "Screenshot PNG") {
        exec(app, "screen.screenshot", json!({"folder": app.session.state.screen.screenshot_folder.clone()}));
    }
}

fn renamer_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    heading(ui, "Renamer", "Rename the selected layer — never the original footage file");
    let mut name = String::new();
    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY).hint_text("new layer name"));
    app.auto.add("screenSuite.renameField", r.rect, "name");
    if auto_btn(app, ui, "screenSuite.rename", "Rename layer") && !name.is_empty() {
        exec(app, "screen.rename", json!({"name": name}));
    }
}

fn matcher_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    heading(ui, "Size Matcher", "Pre-render check");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Needs").small().color(MUTED));
        let by = app.session.state.screen.job_mode == JobMode::BySize;
        let _ = ui.selectable_label(by, format!("By size · {} sizes", app.session.state.screen.sorter.paste_names_by_size.len()));
        let _ = ui.selectable_label(!by, format!("Screen specific · {} screens", app.session.state.screen.sorter.paste_names_screen_specific.len()));
        if auto_btn(app, ui, "screenSuite.matcher.check", "Check sizes") {
            exec(app, "screen.matcher.check", json!({}));
        }
    });
    let report = app.session.state.screen.matcher.clone();
    let color = if report.pass { app.tokens.cache_green } else { app.tokens.danger };
    ui.colored_label(color, RichText::new(&report.summary).strong());
    if !report.rows.is_empty() {
        let avail = ui.available_width().max(280.0);
        let w_name = 140.0;
        let w_size = 110.0;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(w_name, 16.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new("Needed").small().strong().color(MUTED));
            });
            ui.allocate_ui_with_layout(Vec2::new(w_size, 16.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new("Size").small().strong().color(MUTED));
            });
            ui.label(RichText::new("Check").small().strong().color(MUTED));
        });
        ui.add(egui::Separator::default().spacing(4.0));
        for row in &report.rows {
            let ok = row.status == "pass";
            let (rect, _) = ui.allocate_exact_size(Vec2::new(avail, 22.0), Sense::hover());
            if !ok {
                ui.painter().rect_filled(rect, 4.0, Color32::from_rgb(0x24, 0x18, 0x14));
            }
            let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 1.0))).layout(Layout::left_to_right(Align::Center)));
            row_ui.allocate_ui_with_layout(Vec2::new(w_name, 20.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(&row.screen).strong().size(12.0));
            });
            row_ui.allocate_ui_with_layout(Vec2::new(w_size, 20.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(&row.needs).small().color(MUTED));
            });
            if ok {
                row_ui.colored_label(Color32::from_rgb(0x5f, 0xd5, 0x85), "✓");
            } else {
                row_ui.colored_label(Color32::from_rgb(0xff, 0x7b, 0x72), "✗");
            }
        }
    }
    let problems = unique_matcher_problems(alerts);
    if !problems.is_empty() {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(Color32::from_rgb(0x22, 0x16, 0x14))
            .stroke(Stroke::new(1.0, Color32::from_rgb(0x8a, 0x3a, 0x32)))
            .inner_margin(10.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Not ready — {} problem{} to fix before rendering", problems.len(), if problems.len() == 1 { "" } else { "s" }))
                        .strong()
                        .color(Color32::from_rgb(0xff, 0xb4, 0xad)),
                );
                for a in problems {
                    let fix =
                        if a.kind == "missing" { Some(("screenSuite.matcher.fix.select", "Select it")) } else { Some(("screenSuite.matcher.fix", "Fix")) };
                    paint_issue_card(app, ui, a, fix);
                }
            });
    }
    if !report.rows.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("Send window presets").strong());
        ui.label(RichText::new("Better Res for ultra-wide (>5:1) plus Avenues Gate, Eye of Kuwait, Al Salam Sync, Piccadilly, 1st Ring Road, Al Nassar Tower Vertical and the Avenues entrances. Everything else is Installation. Approval Res and Sultan are one-off and never stick.").small().color(Color32::from_gray(150)));
        for row in &report.rows {
            let preset = default_send_preset(&row.screen, 0, 0);
            ui.label(format!("{}  →  {}  ({})", row.screen, preset.label(), preset.bitrate()));
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if report.pass {
            if auto_btn(app, ui, "screenSuite.encodecraft", "Send comps to EncodeCraft…") {
                exec(app, "screen.matcher.send", json!({}));
            }
        } else {
            ui.add_enabled(false, egui::Button::new("Send to EncodeCraft · unlocks at PASS"));
            if auto_btn(app, ui, "screenSuite.sendAnyway", "Send anyway…") {
                app.session.state.screen.send_anyway_open = true;
            }
        }
    });
    if app.session.state.screen.send_anyway_open && !report.pass {
        egui::Frame::new().fill(Color32::from_rgb(0x2b, 0x15, 0x13)).stroke(Stroke::new(1.0, app.tokens.danger)).inner_margin(8.0).corner_radius(8.0).show(
            ui,
            |ui| {
                ui.label(RichText::new("Send anyway? These checks are still failing:").strong());
                for row in report.rows.iter().filter(|r| r.status != "pass") {
                    ui.label(format!("• {} — {}", row.screen, plain_alert_message(&row.detail)));
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        app.session.state.screen.send_anyway_open = false;
                    }
                    if auto_btn(app, ui, "screenSuite.sendAnywayConfirm", "Send anyway") {
                        exec(app, "screen.matcher.send", json!({"anyway": true}));
                        app.session.state.screen.send_anyway_open = false;
                    }
                });
            },
        );
    }
}

pub fn show_library(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    ui.set_min_size(Vec2::new((rect.width() - 16.0).max(1.0), 0.0));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Screen Library").strong().size(16.0));
        if auto_btn(app, ui, "screenLibrary.undo", "Undo") {
            exec(app, "screen.library.undo", json!({}));
        }
        if auto_btn(app, ui, "screenLibrary.redo", "Redo") {
            exec(app, "screen.library.redo", json!({}));
        }
        if auto_btn(app, ui, "screenLibrary.import", "Import JSON…") {
            crate::panels::dialogs::form(
                app,
                "Import Screen Library",
                "screen.library.import",
                json!({}),
                vec![
                    Field::path("path", "JSON file", "", &["json"]),
                    Field::text("json", "Or paste JSON", ""),
                    Field::choice("mode", "Mode", &[("Merge", json!("merge")), ("Replace", json!("replace"))], 0),
                ],
            );
        }
        if auto_btn(app, ui, "screenLibrary.export", "Export JSON…") {
            crate::panels::dialogs::form(
                app,
                "Export Screen Library",
                "screen.library.save",
                json!({}),
                vec![Field::save_path("path", "Save as", "screen-library.json")],
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if auto_btn(app, ui, "screenLibrary.save", "Save · updates every tool") {
                exec(app, "screen.library.save", json!({}));
            }
            if auto_btn(app, ui, "screenLibrary.dock", "Dock as panel") {
                let _ = app.edit_layout(|l| l.dock(PanelKind::ScreenLibrary, PanelKind::ScreenSuite, crate::dock::Zone::Center));
            }
        });
    });
    let Ok(lib) = app.session.execute("screen.library", json!({})) else {
        ui.label("Could not load the screen library.");
        return;
    };
    let merged = lib.get("merged").and_then(Value::as_array).cloned().unwrap_or_default();
    let issues = lib.get("issues").and_then(Value::as_array).cloned().unwrap_or_default();
    let combiners_n = lib.get("combiners").and_then(Value::as_u64).unwrap_or(0);
    let section = app.session.state.screen.library_section.clone();
    ui.horizontal(|ui| {
        ui.label(format!("{} screens · one list for Screen Manager, SizeMaster and Screen Adapter", merged.len()));
        pill(ui, &format!("{} issues", issues.len()), Color32::from_rgb(0x4a, 0x1b, 0x18), Color32::from_rgb(0xff, 0x7b, 0x72));
    });
    let groups_n = lib.get("groups").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let body = ui.available_rect_before_wrap();
    let side_w = 176.0;
    let side = Rect::from_min_size(body.min, Vec2::new(side_w, body.height()));
    let main = Rect::from_min_max(egui::pos2(body.min.x + side_w + 8.0, body.min.y), body.max);
    let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side).layout(egui::Layout::top_down(egui::Align::Min)));
    side_ui.label(RichText::new("LIBRARY").small().color(Color32::from_gray(130)));
    for (id, label, count) in
        [("screens", "Screens · all tools", merged.len() as u64), ("combiners", "Combiners", combiners_n), ("groups", "Groups · durations", groups_n as u64)]
    {
        let on = section == id;
        let r = side_ui.selectable_label(on, format!("{label}  {count}"));
        if r.clicked() {
            exec(app, "screen.library.edit", json!({"section": id}));
        }
        app.auto.add(&format!("screenLibrary.nav.{id}"), r.rect, label);
    }
    side_ui.add_space(8.0);
    side_ui.label(RichText::new("CHECKS").small().color(Color32::from_gray(130)));
    let r = side_ui.selectable_label(section == "issues", format!("All issues  {}", issues.len()));
    if r.clicked() {
        exec(app, "screen.library.edit", json!({"section": "issues"}));
    }
    app.auto.add("screenLibrary.nav.issues", r.rect, "All issues");
    side_ui.label(RichText::new(format!("{} flagged as you type", issues.len())).small().color(MUTED));
    let mut main_ui = ui.new_child(egui::UiBuilder::new().max_rect(main).layout(egui::Layout::top_down(egui::Align::Min)));
    if section == "combiners" {
        combiners_editor(app, &mut main_ui, &lib);
        return;
    }
    if section == "groups" {
        main_ui.label(RichText::new("Groups · durations").strong());
        if let Some(groups) = lib.get("groups").and_then(Value::as_array) {
            for g in groups {
                if let Some(name) = g.as_str() {
                    main_ui.label(name);
                }
            }
        }
        return;
    }
    main_ui.horizontal(|ui| {
        if auto_btn(app, ui, "screenLibrary.add", "Add screen") {
            exec(app, "screen.library.edit", json!({"add": "New screen", "width": 1920, "height": 1080, "group": "DOOH"}));
        }
        if auto_btn(app, ui, "screenLibrary.duplicate", "Duplicate")
            && let Some(name) = merged.first().and_then(|r| r.get("name")).and_then(Value::as_str)
        {
            exec(app, "screen.library.edit", json!({"duplicate": name}));
        }
        if auto_btn(app, ui, "screenLibrary.delete", "Delete")
            && let Some(name) = merged.first().and_then(|r| r.get("name")).and_then(Value::as_str)
        {
            exec(app, "screen.library.edit", json!({"delete": name}));
        }
        let mut q = ui.memory(|m| m.data.get_temp::<String>(egui::Id::new("screenLibrary.search")).unwrap_or_default());
        let r = ui.add(egui::TextEdit::singleline(&mut q).desired_width(140.0).hint_text("Search"));
        app.auto.add("screenLibrary.search", r.rect, "search");
        ui.memory_mut(|m| m.data.insert_temp(egui::Id::new("screenLibrary.search"), q));
        let mut animated = app.session.state.screen.filters.animated;
        let r = ui.checkbox(&mut animated, "Animated");
        app.auto.add("screenLibrary.filter.animated", r.rect, "Animated");
        if r.changed() {
            app.session.state.screen.filters.animated = animated;
        }
        ui.label(RichText::new("Nothing is added to the library unless you type it here. Hidden rows are still sent.").small().color(Color32::from_gray(140)));
    });
    let search = main_ui.memory(|m| m.data.get_temp::<String>(egui::Id::new("screenLibrary.search")).unwrap_or_default());
    let search_n = effectcraft_screens::normalize(&search);
    let anim_only = app.session.state.screen.filters.animated;
    paint_library_table(app, &mut main_ui, &merged, &issues, section == "issues", &search_n, anim_only);
}

fn paint_library_table(app: &mut EffectcraftApp, ui: &mut egui::Ui, merged: &[Value], issues: &[Value], issues_only: bool, search: &str, anim_only: bool) {
    let avail = ui.available_width().max(520.0);
    let w_name = 176.0;
    let w_w = 64.0;
    let w_h = 64.0;
    let w_group = 88.0;
    let w_anim = 78.0;
    let w_also = 110.0;
    let w_check = (avail - w_name - w_w - w_h - w_group - w_anim - w_also).max(140.0);
    let xs = [
        0.0,
        w_name,
        w_name + w_w,
        w_name + w_w + w_h,
        w_name + w_w + w_h + w_group,
        w_name + w_w + w_h + w_group + w_anim,
        w_name + w_w + w_h + w_group + w_anim + w_also,
    ];
    let ws = [w_name, w_w, w_h, w_group, w_anim, w_also, w_check];
    let (hdr, _) = ui.allocate_exact_size(Vec2::new(avail, 22.0), Sense::hover());
    ui.painter().rect_filled(hdr, 0.0, Color32::from_rgb(0x18, 0x18, 0x18));
    for (i, title) in ["Name", "Width", "Height", "Group", "Animated", "Also in", "Check"].iter().enumerate() {
        let align = if i == 1 || i == 2 { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
        table_cell(ui, hdr, xs[i], ws[i], align, |ui| {
            ui.label(RichText::new(*title).small().strong().color(MUTED));
        });
    }
    ui.add(egui::Separator::default().spacing(2.0));
    let visible: Vec<(usize, &Value)> = merged
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            let name = row.get("name").and_then(Value::as_str).unwrap_or("");
            if !search.is_empty() && !effectcraft_screens::normalize(name).contains(search) {
                return false;
            }
            if anim_only && !row.get("animated").and_then(Value::as_bool).unwrap_or(false) {
                return false;
            }
            if issues_only && issues.iter().all(|iss| iss.get("row").and_then(Value::as_str) != Some(name)) {
                return false;
            }
            true
        })
        .collect();
    let table = egui::ScrollArea::vertical().id_salt("screenLibrary.table").auto_shrink([false, false]).show(ui, |ui| {
        for (vis, (i, row)) in visible.into_iter().enumerate() {
            let name = row.get("name").and_then(Value::as_str).unwrap_or("");
            let issue = issues.iter().find(|iss| iss.get("row").and_then(Value::as_str) == Some(name));
            let w = row.get("width").and_then(Value::as_u64).unwrap_or(0);
            let h = row.get("height").and_then(Value::as_u64).unwrap_or(0);
            let group = row.get("group").and_then(Value::as_str).unwrap_or("");
            let sm = row.get("inManager").and_then(Value::as_bool).unwrap_or(false);
            let sz = row.get("inSizemaster").and_then(Value::as_bool).unwrap_or(false);
            let sa = row.get("inAdapter").and_then(Value::as_bool).unwrap_or(false);
            let mut animated = row.get("animated").and_then(Value::as_bool).unwrap_or(false);
            let row_h = 28.0;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, row_h), Sense::hover());
            let fill = if issue.is_some() {
                Color32::from_rgb(0x2b, 0x18, 0x14)
            } else if resp.hovered() {
                ROW_HOVER
            } else if vis % 2 == 1 {
                Color32::from_rgb(0x1c, 0x1c, 0x1c)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 0.0, fill);
            table_cell(ui, rect, xs[0], ws[0], Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(name).color(if issue.is_some() { Color32::from_rgb(0xff, 0x9b, 0x94) } else { Color32::from_gray(230) }));
            });
            table_cell(ui, rect, xs[1], ws[1], Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{w}")).small().color(Color32::from_gray(210)));
            });
            table_cell(ui, rect, xs[2], ws[2], Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{h}")).small().color(Color32::from_gray(210)));
            });
            table_cell(ui, rect, xs[3], ws[3], Layout::left_to_right(Align::Center), |ui| {
                if !group.is_empty() {
                    pill(ui, group, Color32::from_rgb(0x28, 0x28, 0x28), MUTED);
                }
            });
            table_cell(ui, rect, xs[4], ws[4], Layout::left_to_right(Align::Center), |ui| {
                let r = ui.checkbox(&mut animated, "");
                app.auto.add(&format!("screenLibrary.animated.{i}"), r.rect, "animated");
                if r.changed() {
                    exec(app, "screen.library.edit", json!({"name": name, "animated": animated}));
                }
            });
            table_cell(ui, rect, xs[5], ws[5], Layout::left_to_right(Align::Center), |ui| {
                if sm {
                    pill(ui, "SM", Color32::from_rgb(0x16, 0x30, 0x4f), Color32::from_rgb(0x79, 0xb4, 0xff));
                }
                if sz {
                    pill(ui, "SzM", Color32::from_rgb(0x16, 0x3d, 0x24), Color32::from_rgb(0x5f, 0xd5, 0x85));
                }
                if sa {
                    pill(ui, "SA", Color32::from_rgb(0x2a, 0x21, 0x3a), Color32::from_rgb(0xc4, 0xb5, 0xfd));
                }
            });
            table_cell(ui, rect, xs[6], ws[6], Layout::left_to_right(Align::Center), |ui| {
                if let Some(iss) = issue {
                    let msg = iss.get("message").and_then(Value::as_str).unwrap_or("Issue");
                    let short = msg.split('·').next().unwrap_or(msg).trim();
                    pill(ui, short, Color32::from_rgb(0x3a, 0x27, 0x12), Color32::from_rgb(0xfd, 0xba, 0x74));
                    if let Some(fix) = iss.get("fix").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        let label = iss.get("fixLabel").and_then(Value::as_str).unwrap_or("Fix");
                        let fid = iss.get("id").and_then(Value::as_str).unwrap_or("");
                        if auto_btn(app, ui, &format!("screenLibrary.fix.{fid}"), label) {
                            exec(app, "screen.library.edit", json!({"fix": fid, "value": fix}));
                        }
                    }
                } else {
                    ui.colored_label(Color32::from_rgb(0x5f, 0xd5, 0x85), "✓");
                }
            });
            app.auto.add(&format!("screenLibrary.row.{i}"), rect, name);
        }
    });
    register_vscroll(app, ui, "screenLibrary.table.scroll", table.inner_rect, table.content_size.y);
}

fn combiners_editor(app: &mut EffectcraftApp, ui: &mut egui::Ui, lib: &Value) {
    ui.label(RichText::new("Combiners").strong());
    ui.label("Each combiner is a list of pieces: which screen, single / h-dup / v-dup, how many copies, and the alignment. The live preview redraws as you edit. Marina Palm Trees is 4 × 240×960 edge to edge (960×960).");
    let list = lib.get("combinerList").and_then(Value::as_array).cloned().unwrap_or_default();
    app.auto.add("screenLibrary.combiners", ui.min_rect(), "combiners");
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for c in &list {
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            let w = c.get("width").and_then(Value::as_u64).unwrap_or(0);
            let h = c.get("height").and_then(Value::as_u64).unwrap_or(0);
            let faces = c.get("faces").and_then(Value::as_u64).unwrap_or(0);
            let extra = c.get("extraPieces").and_then(Value::as_u64).unwrap_or(0);
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{name}  ·  live preview {w}×{h}  ·  {faces} faces")).strong());
            if extra > 0 {
                if let Some(msg) = c.get("warning").and_then(Value::as_str) {
                    ui.colored_label(Color32::from_rgb(0xf5, 0xb8, 0x4a), msg);
                }
            } else {
                ui.colored_label(Color32::from_rgb(0x5f, 0xd5, 0x85), "Extra-stack check: no extra pieces.");
            }
            if let Some(cols) = c.get("columns").and_then(Value::as_array) {
                for col in cols {
                    let screen = col.get("screenName").and_then(Value::as_str).unwrap_or("");
                    let layout = col.get("layout").and_then(Value::as_str).unwrap_or("single");
                    let count = col.get("count").and_then(Value::as_u64).unwrap_or(1);
                    ui.label(format!("  {screen}  {layout} ×{count}"));
                }
            }
            let issues = lib.get("issues").and_then(Value::as_array).cloned().unwrap_or_default();
            for iss in issues.iter().filter(|i| i.get("row").and_then(Value::as_str) == Some(name)) {
                let msg = iss.get("message").and_then(Value::as_str).unwrap_or("");
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(0xff, 0x9b, 0x94), msg);
                    if let Some(fix) = iss.get("fix").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        let label = iss.get("fixLabel").and_then(Value::as_str).unwrap_or("Fix");
                        let fid = iss.get("id").and_then(Value::as_str).unwrap_or("");
                        if auto_btn(app, ui, &format!("screenLibrary.fix.{fid}"), label) {
                            exec(app, "screen.library.edit", json!({"fix": fid, "value": fix}));
                        }
                    }
                });
            }
        }
    });
}
