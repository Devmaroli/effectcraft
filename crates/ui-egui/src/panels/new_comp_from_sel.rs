//! New Comp From Selection dialog: Single or Multiple cards, size/rate/naming, folder
//! placement, a WILL CREATE preview, and one undo step via `file.newCompFromSelection`.

use effectcraft_engine::project::{Item, ItemId};
use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use super::project_select::{SELECT_BLUE, file_stem, is_audio_only, job_folder_name, new_comp_items, sizes_differ};
use crate::theme::Tokens;
use crate::{Dialog, EffectcraftApp};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeMode {
    #[default]
    EachOwn,
    AllSame,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FolderMode {
    Existing,
    #[default]
    New,
}

#[derive(Clone, Debug)]
pub struct NewCompFromSelDraft {
    pub items: Vec<ItemId>,
    pub single: bool,
    pub size_mode: SizeMode,
    pub from: usize,
    pub custom_w: u32,
    pub custom_h: u32,
    pub fps: f64,
    pub keep_video_rate: bool,
    pub stacked: bool,
    pub overlap: bool,
    pub overlap_secs: f64,
    pub still_secs: f64,
    pub name: String,
    pub folder_mode: FolderMode,
    pub folder: Option<u64>,
    pub new_folder: String,
    pub open: bool,
}

impl Default for NewCompFromSelDraft {
    fn default() -> Self {
        Self {
            items: vec![],
            single: false,
            size_mode: SizeMode::EachOwn,
            from: 0,
            custom_w: 1920,
            custom_h: 1080,
            fps: 25.0,
            keep_video_rate: false,
            stacked: true,
            overlap: true,
            overlap_secs: 12.0 / 25.0,
            still_secs: 3.0,
            name: String::new(),
            folder_mode: FolderMode::New,
            folder: None,
            new_folder: "New Comps".into(),
            open: true,
        }
    }
}

impl NewCompFromSelDraft {
    pub fn from_session(s: &effectcraft_engine::Session) -> Self {
        let items: Vec<ItemId> = s.state.project_selection.clone();
        let listed: Vec<&Item> = items.iter().filter_map(|id| s.project.item(*id)).collect();
        let visual: Vec<(u32, u32)> = listed.iter().filter_map(|it| it.dimensions()).collect();
        let differ = sizes_differ(&visual);
        let eligible = new_comp_items(listed.iter().copied(), !differ);
        let lead = eligible.first().and_then(|id| s.project.item(*id));
        let (custom_w, custom_h) = lead.and_then(|it| it.dimensions()).unwrap_or((1920, 1080));
        let names: Vec<String> = listed.iter().filter(|it| !it.is_folder()).map(|it| it.name.clone()).collect();
        let name = lead.map(|it| file_stem(&it.name)).unwrap_or_else(|| "Comp".into());
        Self {
            items,
            single: !differ,
            size_mode: SizeMode::EachOwn,
            from: 0,
            custom_w,
            custom_h,
            fps: 25.0,
            keep_video_rate: false,
            stacked: true,
            overlap: true,
            overlap_secs: 12.0 / 25.0,
            still_secs: 3.0,
            name,
            folder_mode: FolderMode::New,
            folder: None,
            new_folder: job_folder_name(&names),
            open: true,
        }
    }

    fn eligible<'a>(&self, s: &'a effectcraft_engine::Session) -> Vec<&'a Item> {
        let listed: Vec<&Item> = self.items.iter().filter_map(|id| s.project.item(*id)).collect();
        let ids = new_comp_items(listed.iter().copied(), self.single);
        ids.into_iter().filter_map(|id| s.project.item(id)).collect()
    }

    pub fn will_create(&self, s: &effectcraft_engine::Session) -> usize {
        let n = self.eligible(s).len();
        if self.single { n.min(1) } else { n }
    }

    pub fn params(&self, s: &effectcraft_engine::Session) -> Value {
        let eligible = self.eligible(s);
        let mut p = json!({
            "single": self.single && eligible.len() > 1,
            "duration": self.still_secs,
            "frameRate": self.fps,
            "keepVideoRate": self.keep_video_rate,
            "open": self.open,
            "dimensionsFrom": self.from.min(eligible.len().saturating_sub(1)),
        });
        if let Some(m) = p.as_object_mut() {
            if self.single {
                m.insert("name".into(), json!(self.name));
                m.insert("sequence".into(), json!(!self.stacked));
                m.insert("overlap".into(), json!(!self.stacked && self.overlap));
                m.insert("overlapDuration".into(), json!(self.overlap_secs));
                if self.size_mode == SizeMode::Custom {
                    m.insert("sizeMode".into(), json!("custom"));
                    m.insert("width".into(), json!(self.custom_w));
                    m.insert("height".into(), json!(self.custom_h));
                }
            } else {
                m.insert(
                    "sizeMode".into(),
                    json!(match self.size_mode {
                        SizeMode::EachOwn => "each",
                        SizeMode::AllSame => "same",
                        SizeMode::Custom => "custom",
                    }),
                );
                if self.size_mode == SizeMode::Custom {
                    m.insert("width".into(), json!(self.custom_w));
                    m.insert("height".into(), json!(self.custom_h));
                }
            }
            match self.folder_mode {
                FolderMode::New if !self.new_folder.trim().is_empty() => {
                    m.insert("newFolder".into(), json!(self.new_folder.trim()));
                }
                FolderMode::Existing => {
                    if let Some(f) = self.folder {
                        m.insert("folder".into(), json!(f));
                    }
                }
                _ => {}
            }
        }
        p
    }
}

pub fn open(app: &mut EffectcraftApp) -> Result<(), String> {
    let d = NewCompFromSelDraft::from_session(&app.session);
    if d.eligible(&app.session).is_empty() {
        return Err("select footage in the Project panel".into());
    }
    // One item of matching size: still offer the dialog (Multiple would make one comp).
    let listed: Vec<&Item> = d.items.iter().filter_map(|id| app.session.project.item(*id)).collect();
    let visual: Vec<(u32, u32)> = listed.iter().filter_map(|it| it.dimensions()).collect();
    let mut d = d;
    d.single = !sizes_differ(&visual);
    if listed.iter().filter(|it| !it.is_folder() && (d.single || !is_audio_only(it))).count() <= 1 {
        d.single = true;
    }
    if sizes_differ(&visual) {
        d.single = false;
        d.size_mode = SizeMode::EachOwn;
    }
    app.dialog_state.ncs = d;
    app.dialog = Some(Dialog::NewCompFromSelection);
    Ok(())
}

pub fn show(app: &mut EffectcraftApp, ctx: &egui::Context, t: &Tokens) {
    let mut close = false;
    let mut create = false;
    let create_n = app.dialog_state.ncs.will_create(&app.session);
    super::dialogs::modal(ctx, "New Comp From Selection", vec2(560.0, 640.0), t, |ui| {
        let d = &mut app.dialog_state.ncs;
        mode_cards(ui, d, t, &mut app.auto);
        ui.add_space(10.0);
        if d.single {
            single_body(ui, d, &app.session, t, &mut app.auto);
        } else {
            multiple_body(ui, d, &app.session, t, &mut app.auto);
        }
        ui.add_space(8.0);
        folder_row(ui, d, &app.session, t, &mut app.auto);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let hint = if create_n == 1 { "1 comp · one undo step".to_string() } else { format!("{create_n} comps · one undo step") };
            ui.label(egui::RichText::new(hint).color(t.text_dim).size(11.5));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let create_lbl = if d.single || create_n <= 1 { "Create Comp".into() } else { format!("Create {create_n} Comps") };
                let btn = egui::Button::new(egui::RichText::new(create_lbl).color(Color32::WHITE)).fill(SELECT_BLUE);
                let r = ui.add(btn);
                app.auto.add("dialog.ncs.ok", r.rect, "Create");
                if r.clicked() {
                    create = true;
                }
                let r = ui.add(egui::Button::new("Cancel"));
                app.auto.add("dialog.ncs.cancel", r.rect, "Cancel");
                if r.clicked() {
                    close = true;
                }
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        create = true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if create {
        let params = app.dialog_state.ncs.params(&app.session);
        app.dialog = None;
        if let Err(e) = crate::menus::invoke(app, ctx, "file.newCompFromSelection", params) {
            app.ui.status = e;
        }
    } else if close {
        app.dialog = None;
    }
}

fn mode_cards(ui: &mut egui::Ui, d: &mut NewCompFromSelDraft, t: &Tokens, auto: &mut crate::automation::Registry) {
    ui.horizontal(|ui| {
        let w = ((ui.available_width() - 8.0) / 2.0).max(160.0);
        for (single, title, sub) in [
            (true, "Single composition", "All items in one comp, stacked or one after another (with overlap)."),
            (false, "Multiple compositions", "One comp per item, each at its own size and length."),
        ] {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 72.0), Sense::click());
            let on = d.single == single;
            let p = ui.painter();
            p.rect_filled(rect, 6.0, if on { Color32::from_rgb(0x1a, 0x24, 0x48) } else { t.field_bg });
            p.rect_stroke(rect, 6.0, Stroke::new(if on { 1.6 } else { 1.0 }, if on { SELECT_BLUE } else { t.separator }), egui::StrokeKind::Inside);
            let radio = Rect::from_center_size(pos2(rect.min.x + 16.0, rect.min.y + 16.0), vec2(12.0, 12.0));
            p.circle_stroke(radio.center(), 6.0, Stroke::new(1.2, if on { SELECT_BLUE } else { t.text_dim }));
            if on {
                p.circle_filled(radio.center(), 3.2, SELECT_BLUE);
            }
            p.text(pos2(rect.min.x + 30.0, rect.min.y + 16.0), Align2::LEFT_CENTER, title, Tokens::semibold(12.5), t.text);
            let wrap = Rect::from_min_max(pos2(rect.min.x + 12.0, rect.min.y + 30.0), rect.max - vec2(8.0, 6.0));
            p.with_clip_rect(wrap).text(pos2(wrap.min.x, wrap.min.y + 8.0), Align2::LEFT_TOP, sub, Tokens::ui(11.0), t.text_dim);
            // Mini diagram.
            let dx = rect.max.x - 52.0;
            let dy = rect.min.y + 38.0;
            if single {
                p.rect_filled(Rect::from_min_size(pos2(dx, dy), vec2(36.0, 8.0)), 2.0, SELECT_BLUE.gamma_multiply(0.85));
                p.rect_filled(Rect::from_min_size(pos2(dx + 4.0, dy + 7.0), vec2(36.0, 8.0)), 2.0, Color32::from_rgb(0x7a, 0x8c, 0xff));
            } else {
                p.rect_filled(Rect::from_min_size(pos2(dx, dy), vec2(16.0, 10.0)), 2.0, SELECT_BLUE.gamma_multiply(0.85));
                p.rect_filled(Rect::from_min_size(pos2(dx + 20.0, dy + 4.0), vec2(12.0, 10.0)), 2.0, Color32::from_rgb(0x7a, 0x8c, 0xff));
                p.rect_filled(Rect::from_min_size(pos2(dx + 36.0, dy), vec2(10.0, 10.0)), 2.0, SELECT_BLUE.gamma_multiply(0.7));
            }
            auto.add(if single { "dialog.ncs.single" } else { "dialog.ncs.multiple" }, rect, title);
            if resp.clicked() {
                d.single = single;
            }
            ui.add_space(8.0);
        }
    });
}

fn item_choices(d: &NewCompFromSelDraft, s: &effectcraft_engine::Session) -> Vec<(String, usize)> {
    d.eligible(s)
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let dim = it.dimensions().map(|(w, h)| format!("{w}×{h}")).unwrap_or_else(|| "audio".into());
            (format!("{} · {dim}", it.name), i)
        })
        .collect()
}

fn single_body(ui: &mut egui::Ui, d: &mut NewCompFromSelDraft, s: &effectcraft_engine::Session, t: &Tokens, auto: &mut crate::automation::Registry) {
    ui.label(egui::RichText::new("COMP").color(t.text_dim).size(11.0));
    ui.horizontal(|ui| {
        ui.label("Name");
        let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(280.0));
        auto.add("dialog.ncs.name", r.rect, "Name");
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("SETTINGS").color(t.text_dim).size(11.0));
    let choices = item_choices(d, s);
    ui.horizontal(|ui| {
        ui.label("Settings from");
        let cur = choices.get(d.from).map(|(n, _)| n.as_str()).unwrap_or("Custom");
        let r = egui::ComboBox::from_id_salt("ncs-from").width(280.0).selected_text(if d.size_mode == SizeMode::Custom { "Custom" } else { cur }).show_ui(
            ui,
            |ui| {
                for (label, i) in &choices {
                    if ui.selectable_label(d.size_mode != SizeMode::Custom && d.from == *i, label).clicked() {
                        d.from = *i;
                        d.size_mode = SizeMode::EachOwn;
                        if let Some(it) = d.eligible(s).get(*i)
                            && let Some((w, h)) = it.dimensions()
                        {
                            d.custom_w = w;
                            d.custom_h = h;
                        }
                    }
                }
                if ui.selectable_label(d.size_mode == SizeMode::Custom, "Custom").clicked() {
                    d.size_mode = SizeMode::Custom;
                }
            },
        );
        auto.add("dialog.ncs.settingsFrom", r.response.rect, "Settings from");
    });
    if d.size_mode == SizeMode::Custom {
        ui.horizontal(|ui| {
            ui.label("W");
            let r = ui.add(egui::DragValue::new(&mut d.custom_w).range(4..=30000));
            auto.add("dialog.ncs.width", r.rect, "Width");
            ui.label("H");
            let r = ui.add(egui::DragValue::new(&mut d.custom_h).range(4..=30000));
            auto.add("dialog.ncs.height", r.rect, "Height");
            ui.label("fps");
            let r = ui.add(egui::DragValue::new(&mut d.fps).range(1.0..=120.0).max_decimals(3));
            auto.add("dialog.ncs.frameRate", r.rect, "Frame rate");
        });
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("LAYERS").color(t.text_dim).size(11.0));
    ui.horizontal(|ui| {
        if ui.selectable_label(d.stacked, "Stacked: all layers start together").clicked() {
            d.stacked = true;
        }
        if ui.selectable_label(!d.stacked, "Sequenced: one after another").clicked() {
            d.stacked = false;
        }
    });
    auto.add("dialog.ncs.stacked", ui.min_rect(), "Layer layout");
    if !d.stacked {
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.overlap, "Overlap");
            let r = ui.add(egui::DragValue::new(&mut d.overlap_secs).speed(0.04).range(0.0..=30.0).suffix(" s"));
            auto.add("dialog.ncs.overlap", r.rect, "Overlap");
        });
    }
    timeline_preview(ui, d, t);
    ui.horizontal(|ui| {
        ui.label("Still images last");
        let r = ui.add(egui::DragValue::new(&mut d.still_secs).speed(0.1).range(0.04..=600.0).suffix(" s"));
        auto.add("dialog.ncs.stillDuration", r.rect, "Still duration");
        ui.label(egui::RichText::new("PNG and JPG have no length of their own").color(t.text_dim).size(11.0));
    });
}

fn multiple_body(ui: &mut egui::Ui, d: &mut NewCompFromSelDraft, s: &effectcraft_engine::Session, t: &Tokens, auto: &mut crate::automation::Registry) {
    ui.label(egui::RichText::new("SIZE").color(t.text_dim).size(11.0));
    if ui.radio_value(&mut d.size_mode, SizeMode::EachOwn, "Each item's own size    Recommended").changed() {
        d.size_mode = SizeMode::EachOwn;
    }
    auto.add("dialog.ncs.sizeEach", ui.min_rect(), "Each item's own size");
    ui.horizontal(|ui| {
        ui.radio_value(&mut d.size_mode, SizeMode::AllSame, "All the same as");
        let choices = item_choices(d, s);
        let cur = choices.get(d.from).map(|(n, _)| n.clone()).unwrap_or_default();
        let r = egui::ComboBox::from_id_salt("ncs-same").width(240.0).selected_text(cur).show_ui(ui, |ui| {
            for (label, i) in &choices {
                if ui.selectable_label(d.from == *i, label).clicked() {
                    d.from = *i;
                    d.size_mode = SizeMode::AllSame;
                }
            }
        });
        auto.add("dialog.ncs.sizeSame", r.response.rect, "All the same as");
    });
    ui.horizontal(|ui| {
        ui.radio_value(&mut d.size_mode, SizeMode::Custom, "Custom");
        let r = ui.add(egui::DragValue::new(&mut d.custom_w).range(4..=30000));
        auto.add("dialog.ncs.width", r.rect, "Width");
        ui.label("×");
        let r = ui.add(egui::DragValue::new(&mut d.custom_h).range(4..=30000));
        auto.add("dialog.ncs.height", r.rect, "Height");
        ui.label("px");
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("TIMING").color(t.text_dim).size(11.0));
    ui.horizontal(|ui| {
        ui.label("Frame rate");
        let r = ui.add(egui::DragValue::new(&mut d.fps).range(1.0..=120.0).max_decimals(3).suffix(" fps"));
        auto.add("dialog.ncs.frameRate", r.rect, "Frame rate");
        let r = ui.checkbox(&mut d.keep_video_rate, "Keep each video's own frame rate");
        auto.add("dialog.ncs.keepVideoRate", r.rect, "Keep each video's own frame rate");
    });
    ui.horizontal(|ui| {
        ui.label("Length");
        ui.label(egui::RichText::new("Videos keep their own length · stills last").color(t.text_dim).size(11.5));
        let r = ui.add(egui::DragValue::new(&mut d.still_secs).speed(0.1).range(0.04..=600.0).suffix(" s"));
        auto.add("dialog.ncs.stillDuration", r.rect, "Still duration");
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("NAMES AND FOLDER").color(t.text_dim).size(11.0));
    ui.horizontal(|ui| {
        ui.label("Name each comp");
        ui.label(egui::RichText::new("Same as the item, without .mp4").color(t.text_dim).size(11.5));
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("WILL CREATE {} COMPS", d.eligible(s).len())).color(t.text_dim).size(11.0));
    let preview_h = (d.eligible(s).len() as f32 * 18.0 + 8.0).clamp(24.0, 90.0);
    egui::ScrollArea::vertical().max_height(preview_h).show(ui, |ui| {
        for it in d.eligible(s) {
            let (w, h) = it.dimensions().unwrap_or((0, 0));
            let line = if w > 0 { format!("  {}    {w}×{h}    {:.0} fps", file_stem(&it.name), d.fps) } else { format!("  {}", file_stem(&it.name)) };
            ui.label(egui::RichText::new(line).size(11.5).color(t.text));
        }
    });
    auto.add("dialog.ncs.preview", ui.min_rect(), "Will create");
}

fn folder_row(ui: &mut egui::Ui, d: &mut NewCompFromSelDraft, s: &effectcraft_engine::Session, t: &Tokens, auto: &mut crate::automation::Registry) {
    let _ = t;
    ui.horizontal(|ui| {
        ui.label("Put new comps in");
        let folders: Vec<(String, u64)> = s.project.items.values().filter(|i| i.is_folder()).map(|i| (i.name.clone(), i.id.0)).collect();
        let cur = match d.folder_mode {
            FolderMode::New => format!("New folder: \"{}\"", d.new_folder),
            FolderMode::Existing => folders.iter().find(|(_, id)| Some(*id) == d.folder).map(|(n, _)| n.clone()).unwrap_or_else(|| "Project root".into()),
        };
        let r = egui::ComboBox::from_id_salt("ncs-folder").width(280.0).selected_text(cur).show_ui(ui, |ui| {
            if ui.selectable_label(d.folder_mode == FolderMode::New, format!("New folder: \"{}\"", d.new_folder)).clicked() {
                d.folder_mode = FolderMode::New;
            }
            if ui.selectable_label(d.folder_mode == FolderMode::Existing && d.folder.is_none(), "Project root").clicked() {
                d.folder_mode = FolderMode::Existing;
                d.folder = None;
            }
            for (name, id) in &folders {
                if ui.selectable_label(d.folder == Some(*id), name).clicked() {
                    d.folder_mode = FolderMode::Existing;
                    d.folder = Some(*id);
                }
            }
        });
        auto.add("dialog.ncs.folder", r.response.rect, "Folder");
    });
    if d.folder_mode == FolderMode::New {
        ui.horizontal(|ui| {
            ui.label("Folder name");
            let r = ui.add(egui::TextEdit::singleline(&mut d.new_folder).desired_width(240.0));
            auto.add("dialog.ncs.newFolder", r.rect, "New folder name");
        });
    }
    let r = ui.checkbox(&mut d.open, "Open them when done");
    auto.add("dialog.ncs.open", r.rect, "Open them when done");
}

fn timeline_preview(ui: &mut egui::Ui, d: &NewCompFromSelDraft, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, t.field_bg);
    let n = d.items.len().clamp(1, 4) as f32;
    let gap = if d.stacked { 4.0 } else { 0.0 };
    let bar_h = ((rect.height() - 8.0 - gap * (n - 1.0)) / n).max(4.0);
    for i in 0..n as usize {
        let y = rect.min.y + 4.0 + i as f32 * (bar_h + gap);
        let (x0, w) = if d.stacked {
            (rect.min.x + 8.0, rect.width() - 16.0)
        } else {
            let step = (rect.width() - 16.0) / (n + 0.4);
            let overlap = if d.overlap { step * 0.25 } else { 0.0 };
            (rect.min.x + 8.0 + i as f32 * (step - overlap), step)
        };
        let col = if i % 2 == 0 { SELECT_BLUE.gamma_multiply(0.9) } else { Color32::from_rgb(0x6a, 0xb0, 0x88) };
        p.rect_filled(Rect::from_min_size(pos2(x0, y), vec2(w.max(12.0), bar_h)), 2.0, col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use effectcraft_engine::Session;
    use effectcraft_engine::color::Label;
    use effectcraft_engine::project::{Footage, FootageKind, ItemKind};

    fn session_with_mixed() -> Session {
        let mut s = Session::default();
        let add = |s: &mut Session, name: &str, kind: FootageKind, w: u32, h: u32, audio: bool| {
            let f = Footage { kind, width: w, height: h, has_video: !audio, has_audio: audio, ..Default::default() };
            std::sync::Arc::make_mut(&mut s.project).add_item(name, Label::None, None, ItemKind::Footage(f))
        };
        let a = add(&mut s, "Honor_400_EN.mp4", FootageKind::Video, 1920, 1080, false);
        let b = add(&mut s, "Honor_400_Marina.mp4", FootageKind::Video, 1080, 1920, false);
        let c = add(&mut s, "stc_logo.png", FootageKind::Still, 400, 400, false);
        let d = add(&mut s, "VO.wav", FootageKind::Audio, 0, 0, true);
        let f = std::sync::Arc::make_mut(&mut s.project).add_item("2_Footage", Label::None, None, ItemKind::Folder);
        s.state.project_selection = vec![a, b, c, d, f];
        s
    }

    #[test]
    fn defaults_to_multiple_when_sizes_differ_and_skips_audio_and_folders() {
        let s = session_with_mixed();
        let mut d = NewCompFromSelDraft::from_session(&s);
        d.single = false;
        assert!(sizes_differ(&[(1920, 1080), (1080, 1920), (400, 400)]));
        assert_eq!(d.eligible(&s).len(), 3);
        let p = d.params(&s);
        assert_eq!(p["single"], json!(false));
        assert_eq!(p["sizeMode"], json!("each"));
        assert_eq!(p["frameRate"], json!(25.0));
        assert_eq!(p["keepVideoRate"], json!(false));
        assert_eq!(p["duration"], json!(3.0));
        assert_eq!(p["newFolder"], json!(d.new_folder));
        assert!(d.new_folder.ends_with(" comps"));
        assert_eq!(p["open"], json!(true));
        d.single = true;
        d.name = "Honor_400_EN+AR loop".into();
        d.stacked = false;
        let p = d.params(&s);
        assert_eq!(p["single"], json!(true));
        assert_eq!(p["name"], json!("Honor_400_EN+AR loop"));
        assert_eq!(p["sequence"], json!(true));
        assert_eq!(d.eligible(&s).len(), 4, "single keeps audio");
    }

    #[test]
    fn folder_placement_and_custom_size_params() {
        let s = session_with_mixed();
        let mut d = NewCompFromSelDraft::from_session(&s);
        d.single = false;
        d.folder_mode = FolderMode::Existing;
        d.folder = s.project.items.values().find(|i| i.is_folder()).map(|i| i.id.0);
        let p = d.params(&s);
        assert!(p.get("newFolder").is_none());
        assert_eq!(p["folder"], json!(d.folder.unwrap()));
        d.size_mode = SizeMode::Custom;
        d.custom_w = 640;
        d.custom_h = 360;
        let p = d.params(&s);
        assert_eq!(p["sizeMode"], json!("custom"));
        assert_eq!(p["width"], json!(640));
        assert_eq!(p["height"], json!(360));
    }
}
