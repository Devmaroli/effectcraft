//! Window ▸ Screen Suite: Size Sorter, Screen Manager, Adapter, deliverables and Size Matcher.
//!
//! Every flag (extra-stack size, missing/mismatch, Size Matcher) is painted **in this panel**:
//! a persistent red/orange banner, a count badge on the tab, and a highlighted row. Nothing is
//! emailed or sent outside EffectCraft.

use effectcraft_screens::{AlertLevel, JobMode, MatchMode, PanelAlert, collect_alerts, tab_alert_count};
use egui::{Color32, Rect, RichText, Stroke, Vec2};
use serde_json::{Value, json};

use crate::EffectcraftApp;

const TABS: [(&str, &str, &str); 5] = [
    ("booking", "Booking", "screenSuite.tab.booking"),
    ("build", "Build", "screenSuite.tab.build"),
    ("adapter", "Adapter", "screenSuite.tab.adapter"),
    ("deliver", "Deliver", "screenSuite.tab.deliver"),
    ("qc", "QC", "screenSuite.tab.qc"),
];

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

fn fill_for(level: AlertLevel) -> Color32 {
    match level {
        AlertLevel::Error => Color32::from_rgb(0x6a, 0x18, 0x10),
        AlertLevel::Warning => Color32::from_rgb(0x5a, 0x3a, 0x08),
    }
}

fn stroke_for(app: &EffectcraftApp, level: AlertLevel) -> Stroke {
    match level {
        AlertLevel::Error => Stroke::new(2.0, app.tokens.danger),
        AlertLevel::Warning => Stroke::new(2.0, app.tokens.warning),
    }
}

fn fg_for(level: AlertLevel) -> Color32 {
    match level {
        AlertLevel::Error => Color32::from_rgb(0xff, 0xe0, 0x80),
        AlertLevel::Warning => Color32::from_rgb(0xff, 0xc8, 0x6a),
    }
}

fn paint_banner(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    if alerts.is_empty() {
        return;
    }
    let errors = alerts.iter().any(|a| a.level == AlertLevel::Error);
    let level = if errors { AlertLevel::Error } else { AlertLevel::Warning };
    let n = alerts.len();
    let inner = egui::Frame::new().fill(fill_for(level)).stroke(stroke_for(app, level)).inner_margin(10.0).show(ui, |ui| {
        ui.colored_label(fg_for(level), RichText::new(format!("Look out — {n} issue{} in this job", if n == 1 { "" } else { "s" })).strong().size(16.0));
        ui.colored_label(fg_for(level), RichText::new("Shown here in Screen Suite. Nothing is sent outside the app.").small());
        for a in alerts {
            ui.colored_label(fg_for(a.level), format!("{} — {}: {}", a.title, a.row, a.message));
        }
    });
    app.auto.add("screenSuite.alert.banner", inner.response.rect, "Look out");
    app.auto.add("screenSuite.alert.count", inner.response.rect, &n.to_string());
    ui.add_space(6.0);
}

fn paint_alert_row(app: &mut EffectcraftApp, ui: &mut egui::Ui, alert: &PanelAlert, extra: &str) {
    let inner = egui::Frame::new().fill(fill_for(alert.level)).stroke(stroke_for(app, alert.level)).inner_margin(6.0).show(ui, |ui| {
        ui.colored_label(fg_for(alert.level), RichText::new(format!("{}  [{}]", extra, alert.kind)).strong());
        if extra != alert.message {
            ui.colored_label(fg_for(alert.level), &alert.message);
        }
    });
    app.auto.add(&format!("screenSuite.alert.row.{}", alert.id), inner.response.rect, &alert.row);
}

fn alert_for<'a>(alerts: &'a [PanelAlert], tab: &str, name: &str) -> Option<&'a PanelAlert> {
    let n = name.to_ascii_lowercase();
    alerts
        .iter()
        .find(|a| a.tab == tab && (a.row.eq_ignore_ascii_case(name) || a.row.to_ascii_lowercase().contains(&n) || n.contains(&a.row.to_ascii_lowercase())))
}

pub fn show(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    ui.set_min_size(Vec2::new((rect.width() - 16.0).max(1.0), 0.0));
    let t = app.tokens;
    ui.label(RichText::new("Screen Suite").strong().color(t.text));
    ui.label(RichText::new("Always 25 fps. Studio sizes win (Piccadilly 2027×720, Al Salam 1536 / 3072×576).").small().color(t.text_dim));
    ui.add_space(6.0);

    let alerts = collect_alerts(&app.session.state.screen.sorter, &app.session.state.screen.manager, &app.session.state.screen.matcher);
    paint_banner(app, &mut ui, &alerts);

    ui.horizontal(|ui| {
        let cur = app.session.state.screen.tab.clone();
        for (id, label, auto) in TABS {
            let n = tab_alert_count(&alerts, id);
            let on = cur == id;
            let text = if n == 0 { label.to_string() } else { format!("{label} ({n})") };
            let rich = if n == 0 { RichText::new(text) } else { RichText::new(text).color(app.tokens.danger).strong() };
            let r = ui.selectable_label(on, rich);
            app.auto.add(auto, r.rect, label);
            if n > 0 {
                app.auto.add(&format!("{auto}.badge"), r.rect, &n.to_string());
            }
            if r.clicked() {
                exec(app, "screen.suite.tab", json!({"tab": id}));
            }
        }
    });
    ui.separator();

    let tab = app.session.state.screen.tab.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut ui, |ui| match tab.as_str() {
        "build" => build_tab(app, ui, &alerts),
        "adapter" => adapter_tab(app, ui),
        "deliver" => deliver_tab(app, ui),
        "qc" => qc_tab(app, ui, &alerts),
        _ => booking_tab(app, ui, &alerts),
    });
}

fn booking_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    ui.label("Paste booked screen names");
    let mut paste = app.session.state.screen.paste.clone();
    let r = ui.add(egui::TextEdit::multiline(&mut paste).desired_width(f32::INFINITY).desired_rows(6).hint_text("Jahra Prime, Al Salam Sync, …"));
    app.auto.add("screenSuite.paste", r.rect, "paste");
    if r.changed() {
        app.session.state.screen.paste = paste;
    }
    ui.horizontal(|ui| {
        let mut cleanup = app.session.state.screen.cleanup;
        let r = ui.checkbox(&mut cleanup, "Clean list markers");
        app.auto.add("screenSuite.cleanup", r.rect, "cleanup");
        if r.changed() {
            app.session.state.screen.cleanup = cleanup;
        }
        let mut send_all = app.session.state.screen.send_all;
        let r = ui.checkbox(&mut send_all, "Send all (don't group by size)");
        app.auto.add("screenSuite.sendAll", r.rect, "sendAll");
        if r.changed() {
            app.session.state.screen.send_all = send_all;
        }
    });
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
    ui.horizontal(|ui| {
        ui.label("Group");
        let mut g = app.session.state.screen.filters.group.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut g).desired_width(90.0).hint_text("all"));
        app.auto.add("screenSuite.filter.group", r.rect, "group");
        if r.changed() {
            app.session.state.screen.filters.group = g;
        }
        ui.label("Type");
        let mut k = app.session.state.screen.filters.kind.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut k).desired_width(80.0));
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
    });
    ui.add_space(4.0);
    if auto_btn(app, ui, "screenSuite.sort", "Sort names") {
        let body = {
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
            })
        };
        exec(app, "screen.sorter.sort", body);
    }
    ui.horizontal(|ui| {
        if auto_btn(app, ui, "screenSuite.sendBySize", "Send by size → Screen Manager") {
            exec(app, "screen.sorter.send", json!({"screenSpecific": false}));
        }
        if auto_btn(app, ui, "screenSuite.sendScreenSpecific", "Send screen-specific names") {
            exec(app, "screen.sorter.send", json!({"screenSpecific": true}));
        }
    });
    ui.separator();
    let sorter = app.session.state.screen.sorter.clone();
    ui.label(RichText::new(format!("{} size groups · {} flags", sorter.rows.len(), sorter.flags.len())).small());
    for row in &sorter.rows {
        let line = format!("{}  {}  ({} screens)", row.use_name, row.size, row.count);
        if let Some(a) = alert_for(alerts, "booking", &row.use_name) {
            paint_alert_row(app, ui, a, &line);
        } else if row.needs_review {
            let fake = PanelAlert {
                id: format!("review-{}", row.use_name),
                tab: "booking".into(),
                level: AlertLevel::Warning,
                kind: "needsReview".into(),
                row: row.use_name.clone(),
                title: "Needs a look".into(),
                message: row.reason.clone(),
            };
            paint_alert_row(app, ui, &fake, &line);
        } else {
            ui.label(line);
        }
    }
    for a in alerts.iter().filter(|a| a.tab == "booking") {
        if sorter.rows.iter().any(|r| alert_for(std::slice::from_ref(a), "booking", &r.use_name).is_some()) {
            continue;
        }
        paint_alert_row(app, ui, a, &format!("{}: {}", a.title, a.row));
    }
}

fn build_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    let m = app.session.state.screen.manager.clone();
    ui.label(if m.mode == JobMode::ScreenSpecific {
        "Matching by name (90%, no aliases — Baitak is not Top Gear)"
    } else {
        "Matching by size (Top Gear → Baitak, Quartz → Diamond)"
    });
    ui.horizontal(|ui| {
        if auto_btn(app, ui, "screenSuite.selectPasted", "Select pasted names") {
            exec(app, "screen.manager.select", json!({}));
        }
        let mut only = app.session.state.screen.manager.show_selected_only;
        let r = ui.checkbox(&mut only, "Show selected only");
        app.auto.add("screenSuite.showSelectedOnly", r.rect, "show selected only");
        if r.changed() {
            app.session.state.screen.manager.show_selected_only = only;
        }
    });
    for row in &m.matches {
        let line = format!("{} → {}  {}  [{}]", row.asked, row.preset, row.detail, row.status);
        if let Some(a) = alert_for(alerts, "build", &row.asked) {
            paint_alert_row(app, ui, a, &line);
        } else {
            ui.label(line);
        }
    }
    ui.add_space(6.0);
    if auto_btn(app, ui, "screenSuite.apply", "Apply presets (25 fps)") {
        exec(app, "screen.manager.apply", json!({}));
    }
    if auto_btn(app, ui, "screenSuite.sizeMaster", "Apply SizeMaster") {
        exec(app, "screen.sizeMaster.apply", json!({"tool": "sizeMaster"}));
    }
    for c in &m.combiners {
        let line = format!("Combiner {}  {}×{}  ({} faces)", c.name, c.width, c.height, c.faces.len());
        if let Some(a) = alerts.iter().find(|a| a.kind == "extraStack" && (a.row.contains(&c.name) || c.name.contains(&a.row) || a.message.contains(&c.name))) {
            paint_alert_row(app, ui, a, &line);
        } else {
            ui.label(line);
        }
    }
    for a in alerts.iter().filter(|a| a.kind == "extraStack") {
        if m.combiners.iter().any(|c| a.row.contains(&c.name) || c.name.contains(&a.row) || a.message.contains(&c.name)) {
            continue;
        }
        paint_alert_row(app, ui, a, &a.message);
    }
}

fn adapter_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
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

fn deliver_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    ui.label("Freeze, screenshot, rename, then EncodeCraft.");
    if auto_btn(app, ui, "screenSuite.freeze", "Freeze frame") {
        exec(app, "screen.freeze", json!({}));
    }
    let mut folder = app.session.state.screen.screenshot_folder.clone();
    let r = ui.add(egui::TextEdit::singleline(&mut folder).desired_width(f32::INFINITY).hint_text("screenshot folder"));
    app.auto.add("screenSuite.screenshotFolder", r.rect, "folder");
    if r.changed() {
        app.session.state.screen.screenshot_folder = folder;
    }
    if auto_btn(app, ui, "screenSuite.screenshot", "Screenshot PNG") {
        exec(app, "screen.screenshot", json!({"folder": app.session.state.screen.screenshot_folder.clone()}));
    }
    if auto_btn(app, ui, "screenSuite.encodecraft", "Add to EncodeCraft Queue") {
        exec(app, "encodecraft.queue", json!({}));
    }
}

fn qc_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui, alerts: &[PanelAlert]) {
    ui.label("Pre-render size check. Normal duplicated deliverables (Al Salam 3072×576, Palm Trees 960×960) pass. Extra-stacked sizes fail.");
    if auto_btn(app, ui, "screenSuite.matcher.check", "Check sizes") {
        exec(app, "screen.matcher.check", json!({}));
    }
    let report = app.session.state.screen.matcher.clone();
    let color = if report.pass { app.tokens.cache_green } else { app.tokens.danger };
    ui.colored_label(color, RichText::new(&report.summary).strong());
    for row in &report.rows {
        let line = format!("{}  {}  {}", row.screen, row.status, row.detail);
        if let Some(a) = alert_for(alerts, "qc", &row.screen) {
            paint_alert_row(app, ui, a, &line);
        } else {
            ui.label(line);
        }
    }
}
