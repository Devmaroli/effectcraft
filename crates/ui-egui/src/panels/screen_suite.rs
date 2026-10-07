//! Window ▸ Screen Suite: Size Sorter, Screen Manager, Adapter, deliverables and Size Matcher.

use effectcraft_screens::{JobMode, MatchMode};
use egui::{Color32, Rect, RichText, Vec2};
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

pub fn show(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    ui.set_min_size(Vec2::new((rect.width() - 16.0).max(1.0), 0.0));
    let t = app.tokens;
    ui.label(RichText::new("Screen Suite").strong().color(t.text));
    ui.label(RichText::new("Always 25 fps. Studio sizes win (Piccadilly 2027×720, Al Salam 1536 / 3072×576).").small().color(t.text_dim));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        let cur = app.session.state.screen.tab.clone();
        for (id, label, auto) in TABS {
            let on = cur == id;
            let r = ui.selectable_label(on, label);
            app.auto.add(auto, r.rect, label);
            if r.clicked() {
                exec(app, "screen.suite.tab", json!({"tab": id}));
            }
        }
    });
    ui.separator();

    let warnings: Vec<String> =
        app.session.state.screen.manager.warnings.iter().map(|w| w.message()).chain(app.session.state.screen.matcher.oversized.iter().cloned()).collect();
    if !warnings.is_empty() {
        egui::Frame::new().fill(Color32::from_rgb(0x6a, 0x18, 0x10)).stroke(egui::Stroke::new(2.0, t.warning)).inner_margin(8.0).show(&mut ui, |ui| {
            ui.colored_label(t.warning, RichText::new("Look out — combined size grew past the norm").strong());
            for w in &warnings {
                ui.colored_label(Color32::from_rgb(0xff, 0xe0, 0x80), w);
            }
        });
        ui.add_space(6.0);
    }

    let tab = app.session.state.screen.tab.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut ui, |ui| match tab.as_str() {
        "build" => build_tab(app, ui),
        "adapter" => adapter_tab(app, ui),
        "deliver" => deliver_tab(app, ui),
        "qc" => qc_tab(app, ui),
        _ => booking_tab(app, ui),
    });
}

fn booking_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
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
        ui.label(format!("{}  {}  ({} screens)", row.use_name, row.size, row.count));
    }
    for f in &sorter.flags {
        ui.colored_label(app.tokens.warning, format!("{}: {}", f.kind, f.message));
    }
}

fn build_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
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
        let color = if row.status == "ok" { app.tokens.text } else { app.tokens.danger };
        ui.colored_label(color, format!("{} → {}  {}  [{}]", row.asked, row.preset, row.detail, row.status));
    }
    ui.add_space(6.0);
    if auto_btn(app, ui, "screenSuite.apply", "Apply presets (25 fps)") {
        exec(app, "screen.manager.apply", json!({}));
    }
    if auto_btn(app, ui, "screenSuite.sizeMaster", "Apply SizeMaster") {
        exec(app, "screen.sizeMaster.apply", json!({"tool": "sizeMaster"}));
    }
    for c in &m.combiners {
        ui.label(format!("Combiner {}  {}×{}  ({} faces)", c.name, c.width, c.height, c.faces.len()));
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

fn qc_tab(app: &mut EffectcraftApp, ui: &mut egui::Ui) {
    ui.label("Pre-render size check. Normal duplicated deliverables (Al Salam 3072×576, Palm Trees 960×960) pass. Extra-stacked sizes fail.");
    if auto_btn(app, ui, "screenSuite.matcher.check", "Check sizes") {
        exec(app, "screen.matcher.check", json!({}));
    }
    let report = app.session.state.screen.matcher.clone();
    let color = if report.pass { app.tokens.cache_green } else { app.tokens.danger };
    ui.colored_label(color, RichText::new(&report.summary).strong());
    for row in &report.rows {
        let c = match row.status.as_str() {
            "pass" => app.tokens.text,
            "flag" => app.tokens.warning,
            _ => app.tokens.danger,
        };
        ui.colored_label(c, format!("{}  {}  {}", row.screen, row.status, row.detail));
    }
}
