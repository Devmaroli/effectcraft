//! Screen Suite: Size Sorter, Screen Manager, combiners, Size Matcher and related studio tools.
//!
//! Commands are `screen.*`. Comp fps is always 25. Combiner extra-stack warnings live on
//! [`effectcraft_screens::DupWarning`].

use effectcraft_project::{ItemId, ItemKind};
use effectcraft_screens::combiner::{CombinerLayout, unique_source_slots};
use effectcraft_screens::inventory::{Combiner, CombinerColumn, duration_for};
use effectcraft_screens::library_edit::{self, ImportPreview};
use effectcraft_screens::live::{BookingDiff, OrphanedComp, orphaned_comps};
use effectcraft_screens::manager::JobMode;
use effectcraft_screens::naming::{CompNameFrom, compose_comp_name, material_stem, unique_comp_name};
use effectcraft_screens::normalize::normalize;
use effectcraft_screens::send::{SendPreset, default_send_preset};
use effectcraft_screens::sorter::{MatchMode, SorterFilters, apply_flag_answers, send_names};
use effectcraft_screens::{
    CompProbe, Library, ManagerSelection, MatchReport, SorterResult, accepted_sizes_for, active_combiners, check_comps, collect_alerts, parse_tag_text,
    select_pasted, sort_lines,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CommandSpec, always, b_p, bad, str_p};
use crate::{EngineError, Event, Result, Session, cmd, query};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSuiteState {
    #[serde(default = "booking_tab")]
    pub tab: String,
    #[serde(default)]
    pub paste: String,
    #[serde(default = "true_bool")]
    pub cleanup: bool,
    #[serde(default)]
    pub send_all: bool,
    #[serde(default)]
    pub match_mode: MatchMode,
    #[serde(default)]
    pub filters: SorterFilters,
    #[serde(default)]
    pub job_mode: JobMode,
    #[serde(default)]
    pub sorter: SorterResult,
    #[serde(default)]
    pub manager: ManagerSelection,
    #[serde(default)]
    pub matcher: MatchReport,
    #[serde(default)]
    pub adapter_tag: String,
    #[serde(default)]
    pub screenshot_folder: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default = "spot_duration")]
    pub duration_s: f64,
    #[serde(default)]
    pub name_from: CompNameFrom,
    #[serde(default)]
    pub job_name: String,
    #[serde(default)]
    pub diff: BookingDiff,
    #[serde(default)]
    pub orphans: Vec<OrphanedComp>,
    #[serde(default)]
    pub undo: Vec<ScreenUndo>,
    #[serde(default)]
    pub updated_tabs: Vec<String>,
    #[serde(default)]
    pub filters_open: bool,
    #[serde(default)]
    pub send_anyway_open: bool,
    #[serde(default)]
    pub send_dialog_open: bool,
    #[serde(default)]
    pub library_section: String,
    #[serde(skip)]
    pub library: Option<Library>,
    #[serde(skip)]
    pub library_undo: Vec<Library>,
    #[serde(skip)]
    pub library_redo: Vec<Library>,
    #[serde(skip)]
    pub import_preview: Option<ImportPreview>,
    #[serde(skip)]
    pub import_incoming: Option<Library>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenUndo {
    pub paste: String,
    pub sorter: SorterResult,
    pub manager: ManagerSelection,
    pub matcher: MatchReport,
    pub orphans: Vec<OrphanedComp>,
    pub diff: BookingDiff,
}

fn booking_tab() -> String {
    "booking".into()
}

fn true_bool() -> bool {
    true
}

fn spot_duration() -> f64 {
    10.0
}

impl Default for ScreenSuiteState {
    fn default() -> Self {
        Self {
            tab: booking_tab(),
            paste: String::new(),
            cleanup: true,
            send_all: false,
            match_mode: MatchMode::Flexible,
            filters: SorterFilters::default(),
            job_mode: JobMode::BySize,
            sorter: SorterResult::default(),
            manager: ManagerSelection::default(),
            matcher: MatchReport::default(),
            adapter_tag: String::new(),
            screenshot_folder: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            duration_s: spot_duration(),
            name_from: CompNameFrom::PrefixScreen,
            job_name: String::new(),
            diff: BookingDiff::default(),
            orphans: Vec::new(),
            undo: Vec::new(),
            updated_tabs: Vec::new(),
            filters_open: false,
            send_anyway_open: false,
            send_dialog_open: false,
            library_section: "screens".into(),
            library: None,
            library_undo: Vec::new(),
            library_redo: Vec::new(),
            import_preview: None,
            import_incoming: None,
        }
    }
}

fn session_lib(s: &Session) -> Library {
    s.state.screen.library.clone().unwrap_or_else(Library::load)
}

fn canonical_tab(tab: &str) -> Option<String> {
    Some(match tab.to_ascii_lowercase().as_str() {
        "sorter" | "booking" => "booking".into(),
        "manager" | "build" => "build".into(),
        "adapter" => "adapter".into(),
        "sizemaster" | "size-master" => "sizeMaster".into(),
        "freeze" | "deliver" => "freeze".into(),
        "screenshot" => "screenshot".into(),
        "renamer" | "rename" => "renamer".into(),
        "matcher" | "qc" => "qc".into(),
        _ => return None,
    })
}

fn push_undo(s: &mut Session) {
    let snap = ScreenUndo {
        paste: s.state.screen.paste.clone(),
        sorter: s.state.screen.sorter.clone(),
        manager: s.state.screen.manager.clone(),
        matcher: s.state.screen.matcher.clone(),
        orphans: s.state.screen.orphans.clone(),
        diff: s.state.screen.diff.clone(),
    };
    s.state.screen.undo.push(snap);
    if s.state.screen.undo.len() > 16 {
        s.state.screen.undo.remove(0);
    }
}

fn suffix_variants(suffix: &str) -> Vec<String> {
    let parts: Vec<String> = suffix.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() { vec![String::new()] } else { parts }
}

fn set_tab(s: &mut Session, tab: &str) {
    s.state.screen.tab = tab.to_string();
    s.events.push(Event::Frontend { command: "window.panel".into(), params: json!({"panel": "screenSuite"}) });
}

/// In-app only: a toast plus opening Screen Suite. Nothing is emailed or sent outside EffectCraft.
fn notice_in_app(s: &mut Session, message: String) {
    s.events.push(Event::Toast { message, error: true });
    s.events.push(Event::Frontend { command: "window.panel".into(), params: json!({"panel": "screenSuite"}) });
}

fn current_alerts(s: &Session) -> Vec<effectcraft_screens::PanelAlert> {
    collect_alerts(&s.state.screen.sorter, &s.state.screen.manager, &s.state.screen.matcher)
}

fn library_json(s: &mut Session, _: &Value) -> Result<Value> {
    let lib = session_lib(s);
    let (groups, kinds, govs, cats) = lib.filter_values();
    let merged = library_edit::merged_screens(&lib);
    let issues = library_edit::library_issues(&lib);
    let combiner_list: Vec<Value> = lib
        .combiners
        .iter()
        .map(|c| {
            let lay = CombinerLayout::from_combiner(c);
            json!({
                "name": c.name,
                "width": lay.width,
                "height": lay.height,
                "faces": lay.faces.len(),
                "extraPieces": lay.extra_pieces,
                "warning": lay.warnings.first().map(|w| w.message()),
                "columns": c.columns.iter().map(|col| json!({
                    "screenName": col.screen_name,
                    "layout": col.layout,
                    "count": col.count,
                    "matchWh": col.match_wh,
                    "vAlign": col.v_align,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({
        "screens": lib.screens.len(),
        "presets": lib.presets.len(),
        "combiners": lib.combiners.len(),
        "combinerList": combiner_list,
        "groups": groups,
        "kinds": kinds,
        "governorates": govs,
        "categories": cats,
        "fps": effectcraft_screens::STUDIO_FPS,
        "merged": merged,
        "issues": issues,
        "section": s.state.screen.library_section,
        "state": s.state.screen,
    }))
}

fn filters_from(p: &Value, cur: &SorterFilters) -> SorterFilters {
    SorterFilters {
        group: str_p(p, "group").unwrap_or(&cur.group).to_string(),
        kind: str_p(p, "kind").unwrap_or(&cur.kind).to_string(),
        governorate: str_p(p, "governorate").unwrap_or(&cur.governorate).to_string(),
        category: str_p(p, "category").unwrap_or(&cur.category).to_string(),
        search: str_p(p, "search").unwrap_or(&cur.search).to_string(),
    }
}

fn match_mode_of(p: &Value, cur: MatchMode) -> MatchMode {
    match str_p(p, "matchMode").or(str_p(p, "mode")).unwrap_or("").to_ascii_lowercase().as_str() {
        "strict" => MatchMode::Strict,
        "flexible" => MatchMode::Flexible,
        _ => cur,
    }
}

fn job_mode_of(p: &Value, cur: JobMode) -> JobMode {
    match str_p(p, "jobMode").unwrap_or("").to_ascii_lowercase().as_str() {
        "screenspecific" | "screen-specific" | "specific" => JobMode::ScreenSpecific,
        "bysize" | "by-size" | "size" => JobMode::BySize,
        _ => cur,
    }
}

fn sorter_sort(s: &mut Session, p: &Value) -> Result<Value> {
    let had_result = !s.state.screen.sorter.paste_names_by_size.is_empty() || !s.state.screen.sorter.hits.is_empty();
    if had_result {
        push_undo(s);
    }
    let prev_names = s.state.screen.sorter.paste_names_by_size.clone();
    if let Some(paste) = str_p(p, "paste") {
        s.state.screen.paste = paste.to_string();
    }
    if let Some(v) = b_p(p, "cleanup") {
        s.state.screen.cleanup = v;
    }
    if let Some(v) = b_p(p, "sendAll") {
        s.state.screen.send_all = v;
    }
    s.state.screen.match_mode = match_mode_of(p, s.state.screen.match_mode);
    s.state.screen.filters = filters_from(p, &s.state.screen.filters);
    let lib = session_lib(s);
    let result = sort_lines(&lib, &s.state.screen.paste, s.state.screen.cleanup, s.state.screen.match_mode, &s.state.screen.filters, s.state.screen.send_all);
    if let Some(Value::Array(ans)) = p.get("answers") {
        let tuples: Vec<(String, String, String)> = ans
            .iter()
            .map(|a| {
                (
                    a.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                    a.get("action").and_then(Value::as_str).unwrap_or("").to_string(),
                    a.get("pick").and_then(Value::as_str).unwrap_or("").to_string(),
                )
            })
            .collect();
        let mut r = result;
        apply_flag_answers(&mut r, &tuples);
        s.state.screen.sorter = r;
    } else {
        s.state.screen.sorter = result;
    }
    let next_names = s.state.screen.sorter.paste_names_by_size.clone();
    if had_result {
        let diff = BookingDiff::from_names(&prev_names, &next_names);
        s.state.screen.diff = diff.clone();
        if !diff.is_empty() {
            s.state.screen.updated_tabs = vec!["build".into(), "qc".into()];
            if !s.state.screen.manager.names.is_empty() || !s.state.screen.manager.selected.is_empty() {
                let names = send_names(&s.state.screen.sorter, s.state.screen.job_mode == JobMode::ScreenSpecific);
                s.state.screen.manager = select_pasted(&lib, &names, s.state.screen.job_mode);
                s.state.screen.manager.show_selected_only = true;
            }
            if !s.state.screen.matcher.rows.is_empty() {
                let names = send_names(&s.state.screen.sorter, s.state.screen.job_mode == JobMode::ScreenSpecific);
                let comps = probe_project(s);
                s.state.screen.matcher = check_comps(&lib, &names, &comps, s.state.screen.job_mode);
            }
            let comps: Vec<String> = s.project.items.values().filter(|i| i.as_comp().is_some()).map(|i| i.name.clone()).collect();
            let extra = orphaned_comps(&comps, &diff.removed, &s.state.screen.orphans);
            s.state.screen.orphans.extend(extra);
            s.events.push(Event::Toast { message: diff.note(), error: false });
        }
    }
    let n = s.state.screen.sorter.flags.iter().filter(|f| !f.answered).count();
    if n > 0 {
        notice_in_app(s, format!("Look out — {n} booking flag(s). See Screen Suite ▸ Size Sorter."));
    }
    serde_json::to_value(&s.state.screen.sorter).map_err(|e| EngineError::Other(e.to_string()))
}

fn sorter_send(s: &mut Session, p: &Value) -> Result<Value> {
    if s.state.screen.sorter.hits.is_empty() && !s.state.screen.paste.is_empty() {
        sorter_sort(s, p)?;
    }
    let screen_specific = b_p(p, "screenSpecific").unwrap_or(false)
        || str_p(p, "jobMode").is_some_and(|m| m.eq_ignore_ascii_case("screenSpecific") || m.eq_ignore_ascii_case("specific"));
    let names = send_names(&s.state.screen.sorter, screen_specific);
    if names.is_empty() {
        return Err(bad("screen.sorter.send", "sort a booking list first (no names to send)"));
    }
    s.state.screen.job_mode = if screen_specific { JobMode::ScreenSpecific } else { JobMode::BySize };
    let to = str_p(p, "to").unwrap_or("manager").to_ascii_lowercase();
    let lib = session_lib(s);
    if to == "matcher" || to == "both" {
        let comps = probe_project(s);
        s.state.screen.matcher = check_comps(&lib, &names, &comps, s.state.screen.job_mode);
        set_tab(s, "qc");
        if to == "matcher" {
            return serde_json::to_value(&s.state.screen.matcher).map_err(|e| EngineError::Other(e.to_string()));
        }
    }
    s.state.screen.manager = select_pasted(&lib, &names, s.state.screen.job_mode);
    s.state.screen.manager.show_selected_only = true;
    set_tab(s, "build");
    let n = s.state.screen.manager.matches.iter().filter(|m| m.status != "ok").count();
    if n > 0 {
        notice_in_app(s, format!("Look out — {n} screen(s) did not match. See Screen Suite ▸ Screen Manager."));
    }
    serde_json::to_value(&s.state.screen.manager).map_err(|e| EngineError::Other(e.to_string()))
}

fn manager_select(s: &mut Session, p: &Value) -> Result<Value> {
    s.state.screen.job_mode = job_mode_of(p, s.state.screen.job_mode);
    let mut names = Vec::new();
    if let Some(Value::Array(a)) = p.get("names") {
        names.extend(a.iter().filter_map(Value::as_str).map(str::to_string));
    }
    if let Some(paste) = str_p(p, "paste") {
        names.push(paste.to_string());
    }
    if names.is_empty() {
        names = s.state.screen.manager.names.clone();
    }
    s.state.screen.manager = select_pasted(&session_lib(s), &names, s.state.screen.job_mode);
    if let Some(v) = b_p(p, "showSelectedOnly") {
        s.state.screen.manager.show_selected_only = v;
    } else {
        s.state.screen.manager.show_selected_only = true;
    }
    let n = s.state.screen.manager.matches.iter().filter(|m| m.status != "ok").count();
    if n > 0 {
        notice_in_app(s, format!("Look out — {n} screen(s) did not match. See Screen Suite ▸ Build."));
    }
    serde_json::to_value(&s.state.screen.manager).map_err(|e| EngineError::Other(e.to_string()))
}

fn ensure_preset_comp(s: &mut Session, name: &str, width: u32, height: u32, duration_s: f64) -> Result<ItemId> {
    if let Some(id) = s.project.items.values().find(|i| i.name == name && i.as_comp().is_some()).map(|i| i.id) {
        return Ok(id);
    }
    let r = s.execute(
        "comp.new",
        json!({
            "name": name,
            "width": width,
            "height": height,
            "frameRate": 25.0,
            "duration": duration_s,
            "open": false,
        }),
    )?;
    r.get("comp").and_then(Value::as_u64).map(ItemId).ok_or_else(|| EngineError::Other("comp.new did not return a composition id".into()))
}

fn manager_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let size_master = str_p(p, "tool").is_some_and(|t| t.eq_ignore_ascii_case("sizeMaster"));
    if let Some(v) = str_p(p, "prefix") {
        s.state.screen.prefix = v.to_string();
    }
    if let Some(v) = str_p(p, "suffix") {
        s.state.screen.suffix = v.to_string();
    }
    if let Some(v) = p.get("duration").and_then(Value::as_f64) {
        s.state.screen.duration_s = v;
    }
    if let Some(v) = str_p(p, "nameFrom") {
        s.state.screen.name_from = if v.eq_ignore_ascii_case("material") { CompNameFrom::Material } else { CompNameFrom::PrefixScreen };
    }
    let lib = session_lib(s);
    let mut created = Vec::new();
    let selected = s.state.screen.manager.selected.clone();
    if selected.is_empty() {
        return Err(bad("screen.manager.apply", "no screens selected — paste names or send them from Size Sorter first"));
    }
    if s.state.screen.job_mode == JobMode::ScreenSpecific && s.state.screen.manager.matches.iter().any(|m| m.status != "ok") {
        return Err(bad("screen.manager.apply", "Apply locked — resolve missing screens and size mismatches in the Screen library first"));
    }
    let footage = selected_footage(s);
    let mut taken: Vec<String> = s.project.items.values().map(|i| i.name.clone()).collect();
    let suffixes = suffix_variants(&s.state.screen.suffix);
    for name in &selected {
        let (w, h, group) = if let Some(pr) = lib.preset_by_name(name) {
            (pr.width, pr.height, pr.group.clone())
        } else if let Some(sc) = lib.screen_by_name(name).or_else(|| lib.screens.iter().find(|sc| normalize(&sc.name) == normalize(name))) {
            (sc.width, sc.height, sc.group.clone())
        } else if let Some(m) = s.state.screen.manager.matches.iter().find(|m| m.preset == *name) {
            (m.width, m.height, String::new())
        } else {
            continue;
        };
        let dur = if size_master {
            duration_for(name, &group, true)
        } else if s.state.screen.duration_s > 0.0 {
            s.state.screen.duration_s
        } else {
            duration_for(name, &group, false)
        };
        let foot = match_footage_for_screen(name, w, h, &footage);
        for suf in &suffixes {
            let base = compose_comp_name(s.state.screen.name_from, name, foot.as_ref().map(|f| f.stem.as_str()), &s.state.screen.prefix, suf);
            let (comp_name, clash) = unique_comp_name(&base, &taken);
            if clash {
                s.state.screen.sorter.flags.push(effectcraft_screens::sorter::SorterFlag {
                    id: format!("name-clash-{comp_name}"),
                    kind: "nameClash".into(),
                    message: format!("Comp name “{base}” was taken; created “{comp_name}”. The original footage was not renamed."),
                    line: base.clone(),
                    suggestion: comp_name.clone(),
                    alternates: Vec::new(),
                    answered: true,
                    pick: comp_name.clone(),
                });
            }
            let id = ensure_preset_comp(s, &comp_name, w, h, dur)?;
            taken.push(comp_name.clone());
            if let Some(f) = &foot {
                let _ = s.execute("layer.addItem", json!({"comp": id.0, "item": f.id.0, "time": 0.0}));
            }
            created
                .push(json!({"name": comp_name, "screen": name, "comp": id.0, "width": w, "height": h, "duration": dur, "fps": 25.0, "footageRenamed": false}));
        }
    }
    let combined = combine_active(s, p)?;
    set_tab(s, if size_master { "sizeMaster" } else { "build" });
    Ok(json!({"created": created, "combiners": combined}))
}

struct FootagePick {
    id: ItemId,
    stem: String,
    width: u32,
    height: u32,
    name: String,
}

fn selected_footage(s: &Session) -> Vec<FootagePick> {
    s.state
        .project_selection
        .iter()
        .filter_map(|id| {
            let item = s.project.item(*id)?;
            match &item.kind {
                ItemKind::Footage(f) => {
                    Some(FootagePick { id: *id, stem: material_stem(&item.name), width: f.width, height: f.height, name: item.name.clone() })
                }
                _ => None,
            }
        })
        .collect()
}

fn match_footage_for_screen<'a>(screen: &str, w: u32, h: u32, footage: &'a [FootagePick]) -> Option<&'a FootagePick> {
    let size_ok = |f: &FootagePick| f.width.abs_diff(w) <= 10 && f.height.abs_diff(h) <= 10;
    footage
        .iter()
        .filter(|f| size_ok(f) && (effectcraft_screens::names_match_90(screen, &f.name) || effectcraft_screens::names_match_90(screen, &f.stem)))
        .max_by(|a, b| {
            let sa = effectcraft_screens::token_score(screen, &a.stem);
            let sb = effectcraft_screens::token_score(screen, &b.stem);
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .or_else(|| footage.iter().find(|f| size_ok(f)))
}

fn source_ids_for_column(s: &Session, col: &CombinerColumn, combined_w: u32, combined_h: u32) -> Vec<ItemId> {
    let (fw, fh) = col.match_wh.unwrap_or((0, 0));
    let mut out = Vec::new();
    for (id, c) in s.project.comps() {
        if c.width == combined_w && c.height == combined_h {
            continue;
        }
        if c.width != fw || c.height != fh {
            continue;
        }
        let name = s.project.item(*id).map(|i| i.name.as_str()).unwrap_or("");
        if effectcraft_screens::combiner::names_related(name, &col.screen_name) {
            out.push(*id);
        }
    }
    out.sort_by_key(|id| id.0);
    out.dedup();
    out
}

fn combine_active(s: &mut Session, p: &Value) -> Result<Value> {
    let lib = session_lib(s);
    let extra_force = p.get("extraSources").and_then(Value::as_u64).map(|n| n as u32);
    let selected = s.state.screen.manager.selected.clone();
    let combiners: Vec<&Combiner> = if let Some(name) = str_p(p, "combiner") {
        lib.combiners.iter().filter(|c| normalize(&c.name) == normalize(name) || normalize(&c.name).contains(&normalize(name))).collect()
    } else {
        lib.combiners
            .iter()
            .filter(|c| c.columns.iter().all(|col| selected.iter().any(|n| effectcraft_screens::combiner::names_related(n, &col.screen_name))))
            .collect()
    };
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for c in combiners {
        let normal = CombinerLayout::from_combiner(c);
        let mut sources = Vec::new();
        for col in &c.columns {
            sources.extend(source_ids_for_column(s, col, normal.width, normal.height));
        }
        sources.sort_by_key(|id| id.0);
        sources.dedup();
        let unique = extra_force.unwrap_or_else(|| (sources.len() as u32).max(unique_source_slots(c)));
        let lay = CombinerLayout::from_unique_sources(c, unique);
        if sources.is_empty() {
            continue;
        }
        if let Some(existing) =
            s.project.items.values().find(|i| i.name == c.name && i.as_comp().is_some_and(|cc| cc.width == lay.width && cc.height == lay.height))
        {
            out.push(json!({"name": c.name, "comp": existing.id.0, "width": lay.width, "height": lay.height, "reused": true, "warnings": lay.warnings}));
            warnings.extend(lay.warnings.clone());
            continue;
        }
        let dur = sources.first().and_then(|id| s.project.comp(*id)).map(|c| c.duration.seconds()).unwrap_or(10.0);
        let created = s.execute(
            "comp.new",
            json!({
                "name": c.name,
                "width": lay.width,
                "height": lay.height,
                "frameRate": 25.0,
                "duration": dur,
                "open": false,
            }),
        )?;
        let cid = created.get("comp").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("combiner comp.new returned no id".into()))?;
        let faces_per_row = CombinerLayout::from_combiner(c).faces.len().max(1);
        for (i, face) in lay.faces.iter().enumerate() {
            let row = i / faces_per_row;
            let src = sources.get(row).or(sources.first()).copied().ok_or_else(|| EngineError::Other("combiner has no source composition".into()))?;
            s.execute(
                "layer.addItem",
                json!({
                    "comp": cid,
                    "item": src.0,
                    "time": 0.0,
                    "position": [face.position[0], face.position[1]],
                }),
            )?;
        }
        warnings.extend(lay.warnings.clone());
        out.push(json!({
            "name": c.name,
            "comp": cid,
            "width": lay.width,
            "height": lay.height,
            "faces": lay.faces.len(),
            "extraPieces": lay.extra_pieces,
            "warnings": lay.warnings,
        }));
    }
    s.state.screen.manager.combiners = active_combiners(&lib, &selected);
    s.state.screen.manager.warnings = warnings;
    if let Some(w) = s.state.screen.manager.warnings.first() {
        notice_in_app(s, w.message());
    }
    Ok(json!(out))
}

fn manager_combine(s: &mut Session, p: &Value) -> Result<Value> {
    combine_active(s, p)
}

fn probe_project(s: &Session) -> Vec<CompProbe> {
    s.project
        .comps()
        .filter_map(|(id, c)| {
            let name = s.project.item(*id)?.name.clone();
            Some(CompProbe { name, width: c.width, height: c.height, duration_s: c.duration.seconds(), fps: c.frame_rate.as_f64() })
        })
        .collect()
}

fn matcher_check(s: &mut Session, p: &Value) -> Result<Value> {
    let lib = session_lib(s);
    let mode = job_mode_of(p, s.state.screen.job_mode);
    let required: Vec<String> = if let Some(Value::Array(a)) = p.get("names") {
        a.iter().filter_map(Value::as_str).map(str::to_string).collect()
    } else if !s.state.screen.manager.selected.is_empty() {
        s.state.screen.manager.selected.clone()
    } else {
        s.state.screen.sorter.paste_names_by_size.clone()
    };
    if required.is_empty() {
        return Err(bad("screen.matcher.check", "no screens to check — send a booking list first"));
    }
    let mut comps = probe_project(s);
    if let Some(Value::Array(extra)) = p.get("comps") {
        for c in extra {
            let name = c.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let width = c.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            let height = c.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
            if name.is_empty() || width == 0 || height == 0 {
                continue;
            }
            comps.push(CompProbe {
                name,
                width,
                height,
                duration_s: c.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
                fps: c.get("fps").or(c.get("frameRate")).and_then(Value::as_f64).unwrap_or(0.0),
            });
        }
    }
    let report = check_comps(&lib, &required, &comps, mode);
    s.state.screen.matcher = report.clone();
    s.state.screen.tab = "qc".into();
    s.events.push(Event::Frontend { command: "window.panel".into(), params: json!({"panel": "screenSuite"}) });
    if !report.pass {
        notice_in_app(s, report.summary.clone());
    }
    serde_json::to_value(&s.state.screen.matcher).map_err(|e| EngineError::Other(e.to_string()))
}

fn suite_tab(s: &mut Session, p: &Value) -> Result<Value> {
    let tab = str_p(p, "tab")
        .ok_or_else(|| bad("screen.suite.tab", "missing `tab` (booking|sorter|build|manager|adapter|sizeMaster|freeze|screenshot|renamer|qc|matcher)"))?;
    let t = canonical_tab(tab).ok_or_else(|| bad("screen.suite.tab", "unknown Screen Suite tool"))?;
    if let Some(i) = s.state.screen.updated_tabs.iter().position(|x| x == &t) {
        s.state.screen.updated_tabs.remove(i);
    }
    set_tab(s, &t);
    Ok(json!({"tab": t}))
}

fn suite_state(s: &mut Session, _: &Value) -> Result<Value> {
    let mut v = serde_json::to_value(&s.state.screen).map_err(|e| EngineError::Other(e.to_string()))?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert("alerts".into(), serde_json::to_value(current_alerts(s)).map_err(|e| EngineError::Other(e.to_string()))?);
    }
    Ok(v)
}

fn suite_alerts(s: &mut Session, _: &Value) -> Result<Value> {
    serde_json::to_value(current_alerts(s)).map_err(|e| EngineError::Other(e.to_string()))
}

fn adapter_parse(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_p(p, "text").map(str::to_string).unwrap_or_else(|| s.state.screen.adapter_tag.clone());
    s.state.screen.adapter_tag = text.clone();
    match parse_tag_text(&text) {
        Some(tag) => Ok(json!({"role": tag.role, "adapt": tag.adapt, "tag": effectcraft_screens::format_tag(&tag)})),
        None => Ok(json!({"role": Value::Null, "adapt": Value::Null})),
    }
}

fn adapter_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let parsed = adapter_parse(s, p)?;
    let tag = parsed.get("tag").and_then(Value::as_str).ok_or_else(|| bad("screen.adapter.apply", "no @role tag to apply"))?;
    s.execute("layer.setComment", json!({"comment": tag}))?;
    Ok(parsed)
}

fn freeze(s: &mut Session, p: &Value) -> Result<Value> {
    s.execute("layer.freezeFrame", p.clone())
}

fn screenshot(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_p(p, "path").map(str::to_string).or_else(|| {
        let folder = str_p(p, "folder").unwrap_or(&s.state.screen.screenshot_folder);
        if folder.is_empty() {
            return None;
        }
        let name = s.active_comp_id().and_then(|id| s.project.item(id)).map(|i| i.name.clone()).unwrap_or_else(|| "frame".into());
        Some(format!("{folder}/{name}.png"))
    });
    let path = path.ok_or_else(|| bad("screen.screenshot", "missing `path` or `folder`"))?;
    if let Some(folder) = str_p(p, "folder") {
        s.state.screen.screenshot_folder = folder.to_string();
    }
    s.execute("comp.saveFrameAs", json!({"path": path}))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_p(p, "name").ok_or_else(|| bad("screen.rename", "missing `name`"))?;
    s.execute("layer.rename", json!({"name": name}))
}

fn accepted(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_p(p, "name").ok_or_else(|| bad("screen.matcher.accepted", "missing `name`"))?;
    let sizes = accepted_sizes_for(&session_lib(s), name);
    serde_json::to_value(sizes).map_err(|e| EngineError::Other(e.to_string()))
}

fn suite_undo(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(snap) = s.state.screen.undo.pop() else {
        return Err(bad("screen.suite.undo", "nothing to undo"));
    };
    s.state.screen.paste = snap.paste;
    s.state.screen.sorter = snap.sorter;
    s.state.screen.manager = snap.manager;
    s.state.screen.matcher = snap.matcher;
    s.state.screen.orphans = snap.orphans;
    s.state.screen.diff = snap.diff;
    s.state.screen.updated_tabs.clear();
    Ok(json!({"ok": true, "paste": s.state.screen.paste}))
}

fn orphans_keep(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_p(p, "name").map(str::to_string);
    for o in &mut s.state.screen.orphans {
        if name.as_ref().is_none_or(|n| normalize(n) == normalize(&o.name) || normalize(&o.screen) == normalize(n)) {
            o.keep = Some(true);
        }
    }
    Ok(json!({"orphans": s.state.screen.orphans}))
}

fn orphans_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_p(p, "name").map(str::to_string);
    let mut ids = Vec::new();
    let mut remaining = Vec::new();
    for o in s.state.screen.orphans.drain(..) {
        let hit = name.as_ref().is_none_or(|n| normalize(n) == normalize(&o.name) || normalize(&o.screen) == normalize(n));
        if hit && o.keep != Some(true) {
            if let Some(item) = s.project.items.values().find(|i| i.name == o.name && i.as_comp().is_some()) {
                ids.push(item.id.0);
            }
        } else {
            remaining.push(o);
        }
    }
    s.state.screen.orphans = remaining;
    if !ids.is_empty() {
        s.execute("project.delete", json!({"items": ids}))?;
    }
    Ok(json!({"removed": ids.len()}))
}

fn naming_set(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = str_p(p, "prefix") {
        s.state.screen.prefix = v.to_string();
    }
    if let Some(v) = str_p(p, "suffix") {
        s.state.screen.suffix = v.to_string();
    }
    if let Some(v) = str_p(p, "nameFrom") {
        s.state.screen.name_from = if v.eq_ignore_ascii_case("material") { CompNameFrom::Material } else { CompNameFrom::PrefixScreen };
    }
    if let Some(v) = str_p(p, "jobName") {
        s.state.screen.job_name = v.to_string();
    }
    let preview = compose_comp_name(
        s.state.screen.name_from,
        "1.7HD",
        Some("Jahra Prime EN"),
        &s.state.screen.prefix,
        suffix_variants(&s.state.screen.suffix).first().map(String::as_str).unwrap_or(""),
    );
    Ok(json!({"prefix": s.state.screen.prefix, "suffix": s.state.screen.suffix, "nameFrom": s.state.screen.name_from, "preview": preview}))
}

fn matcher_send(s: &mut Session, p: &Value) -> Result<Value> {
    let anyway = b_p(p, "anyway").unwrap_or(false);
    if s.state.screen.matcher.rows.is_empty() {
        matcher_check(s, p)?;
    }
    if !s.state.screen.matcher.pass && !anyway {
        s.state.screen.send_anyway_open = true;
        return Err(bad("screen.matcher.send", format!("Send locked until PASS. {} Use Send anyway… to continue.", s.state.screen.matcher.summary)));
    }
    s.state.screen.send_anyway_open = false;
    s.state.screen.send_dialog_open = true;
    let required = if !s.state.screen.manager.selected.is_empty() {
        s.state.screen.manager.selected.clone()
    } else {
        send_names(&s.state.screen.sorter, s.state.screen.job_mode == JobMode::ScreenSpecific)
    };
    let jobs: Vec<(u64, String, SendPreset)> = s
        .project
        .comps()
        .filter_map(|(id, c)| {
            let item = s.project.item(*id)?;
            if s.state.screen.orphans.iter().any(|o| o.keep == Some(true) && o.name == item.name) {
                return None;
            }
            let screen =
                required.iter().find(|n| effectcraft_screens::combiner::names_related(&item.name, n) || normalize(&item.name).contains(&normalize(n)))?;
            let mut preset = default_send_preset(screen, c.width, c.height);
            if let Some(id) = p.get("presets").and_then(Value::as_object).and_then(|m| m.get(&item.name)).and_then(Value::as_str)
                && let Some(over) = SendPreset::from_id(id)
            {
                preset = over;
            }
            Some((id.0, item.name.clone(), preset))
        })
        .collect();
    let mut queued = Vec::new();
    let mut skipped = Vec::new();
    for (comp_id, name, preset) in jobs {
        match s.execute("encodecraft.queue", json!({"comp": comp_id, "presetId": preset.preset_id()})) {
            Ok(r) => queued.push(json!({"comp": name, "preset": preset.label(), "presetId": preset.preset_id(), "result": r})),
            Err(e) => skipped.push(json!({"comp": name, "error": e.to_string()})),
        }
    }
    s.state.screen.send_dialog_open = false;
    Ok(json!({"ok": skipped.is_empty(), "queued": queued, "skipped": skipped, "anyway": anyway}))
}

fn lib_push_undo(s: &mut Session) {
    let lib = session_lib(s);
    s.state.screen.library_undo.push(lib);
    if s.state.screen.library_undo.len() > 32 {
        s.state.screen.library_undo.remove(0);
    }
    s.state.screen.library_redo.clear();
}

fn library_ensure(s: &mut Session) -> Library {
    s.state.screen.library.get_or_insert_with(Library::load).clone()
}

fn library_open(s: &mut Session, _: &Value) -> Result<Value> {
    let _ = library_ensure(s);
    s.events.push(Event::Frontend { command: "window.panel".into(), params: json!({"panel": "screenLibrary", "float": true}) });
    library_json(s, &Value::Null)
}

fn library_edit(s: &mut Session, p: &Value) -> Result<Value> {
    lib_push_undo(s);
    let mut lib = library_ensure(s);
    if let Some(name) = str_p(p, "add") {
        let w = p.get("width").and_then(Value::as_u64).unwrap_or(1920) as u32;
        let h = p.get("height").and_then(Value::as_u64).unwrap_or(1080) as u32;
        let group = str_p(p, "group").unwrap_or("DOOH");
        library_edit::add_screen(&mut lib, name, w, h, group);
    }
    if let Some(name) = str_p(p, "delete") {
        library_edit::delete_screen(&mut lib, name);
    }
    if let Some(name) = str_p(p, "duplicate") {
        library_edit::duplicate_screen(&mut lib, name);
    }
    if let Some(name) = str_p(p, "name")
        && let (Some(w), Some(h)) = (p.get("width").and_then(Value::as_u64), p.get("height").and_then(Value::as_u64))
    {
        library_edit::set_screen_size(&mut lib, name, w as u32, h as u32);
    }
    if let Some(id) = str_p(p, "fix") {
        let fix = str_p(p, "value").unwrap_or("");
        library_edit::apply_fix(&mut lib, id, fix);
    }
    if let Some(sec) = str_p(p, "section") {
        s.state.screen.library_section = sec.to_string();
    }
    s.state.screen.library = Some(lib);
    library_json(s, p)
}

fn library_undo(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(prev) = s.state.screen.library_undo.pop() else {
        return Err(bad("screen.library.undo", "nothing to undo"));
    };
    if let Some(cur) = s.state.screen.library.take() {
        s.state.screen.library_redo.push(cur);
    }
    s.state.screen.library = Some(prev);
    library_json(s, &Value::Null)
}

fn library_redo(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(next) = s.state.screen.library_redo.pop() else {
        return Err(bad("screen.library.redo", "nothing to redo"));
    };
    lib_push_undo(s);
    s.state.screen.library = Some(next);
    library_json(s, &Value::Null)
}

fn library_import_preview(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_p(p, "json").ok_or_else(|| bad("screen.library.importPreview", "missing `json`"))?;
    let mode = str_p(p, "mode").unwrap_or("merge");
    let incoming = library_edit::parse_library_json(text).map_err(|e| bad("screen.library.importPreview", &e))?;
    let preview = library_edit::preview_import(&session_lib(s), &incoming, mode);
    s.state.screen.import_preview = Some(preview.clone());
    s.state.screen.import_incoming = Some(incoming);
    serde_json::to_value(preview).map_err(|e| EngineError::Other(e.to_string()))
}

fn library_import_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let incoming = s.state.screen.import_incoming.take().ok_or_else(|| bad("screen.library.importApply", "preview an import first"))?;
    let mode = str_p(p, "mode").unwrap_or("merge");
    lib_push_undo(s);
    if mode.eq_ignore_ascii_case("replace") {
        s.state.screen.library = Some(incoming);
    } else {
        let mut lib = library_ensure(s);
        library_edit::apply_merge(&mut lib, incoming);
        s.state.screen.library = Some(lib);
    }
    s.state.screen.import_preview = None;
    library_json(s, p)
}

fn library_save(s: &mut Session, p: &Value) -> Result<Value> {
    let lib = session_lib(s);
    let json = library_edit::export_library_json(&lib).map_err(EngineError::Other)?;
    let path = str_p(p, "path").map(str::to_string);
    if let Some(path) = path {
        if path.contains("..") {
            return Err(bad("screen.library.save", "path must not contain '..'"));
        }
        let backup = {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            format!("{path}.backup-{stamp}")
        };
        if std::path::Path::new(&path).exists() {
            let _ = std::fs::copy(&path, &backup);
        }
        std::fs::write(&path, json.as_bytes()).map_err(|e| EngineError::Other(e.to_string()))?;
        return Ok(json!({"ok": true, "path": path, "backup": backup}));
    }
    Ok(json!({"ok": true, "json": json, "inMemory": true}))
}

fn send_plan(s: &mut Session, _: &Value) -> Result<Value> {
    let required = if !s.state.screen.manager.selected.is_empty() {
        s.state.screen.manager.selected.clone()
    } else {
        send_names(&s.state.screen.sorter, s.state.screen.job_mode == JobMode::ScreenSpecific)
    };
    let mut rows = Vec::new();
    for (id, c) in s.project.comps() {
        let Some(item) = s.project.item(*id) else { continue };
        let Some(screen) =
            required.iter().find(|n| effectcraft_screens::combiner::names_related(&item.name, n) || normalize(&item.name).contains(&normalize(n)))
        else {
            continue;
        };
        let preset = default_send_preset(screen, c.width, c.height);
        rows.push(json!({
            "comp": item.name,
            "screen": screen,
            "width": c.width,
            "height": c.height,
            "preset": preset.label(),
            "presetId": preset.preset_id(),
            "why": preset.bitrate(),
            "oneOff": false,
        }));
    }
    Ok(json!({"rows": rows, "pass": s.state.screen.matcher.pass, "problems": s.state.screen.matcher.summary}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query!("screen.library", "Screen library", "{}", library_json),
        cmd!(
            "screen.sorter.sort",
            "Sort Booking Names",
            ["Composition", "Screen Suite"],
            None,
            "{paste?, cleanup?, sendAll?, matchMode?: flexible|strict, group?, kind?, governorate?, category?, search?, answers?: [{id, action, pick}]}",
            always,
            sorter_sort
        ),
        cmd!(
            "screen.sorter.send",
            "Send Sorted Names to Screen Manager",
            ["Composition", "Screen Suite"],
            None,
            "{screenSpecific?: bool, jobMode?: bySize|screenSpecific, to?: manager|matcher|both}",
            always,
            sorter_send
        ),
        cmd!(
            "screen.manager.select",
            "Select Pasted Screen Names",
            ["Composition", "Screen Suite"],
            None,
            "{paste?, names?: [string], jobMode?: bySize|screenSpecific, showSelectedOnly?}",
            always,
            manager_select
        ),
        cmd!(
            "screen.manager.apply",
            "Apply Screen Manager Presets",
            ["Composition", "Screen Suite"],
            None,
            "{tool?: manager|sizeMaster, combiner?, extraSources?, prefix?, suffix?, duration?, nameFrom?: prefixScreen|material}",
            always,
            manager_apply
        ),
        cmd!(
            "screen.manager.combine",
            "Build Combiner Compositions",
            ["Composition", "Screen Suite"],
            None,
            "{combiner?, extraSources?}",
            always,
            manager_combine
        ),
        cmd!(
            "screen.sizeMaster.apply",
            "Apply SizeMaster Presets",
            ["Composition", "Screen Suite"],
            None,
            "{tool?: sizeMaster, combiner?, extraSources?}",
            always,
            manager_apply
        ),
        cmd!(
            "screen.matcher.check",
            "Check Composition Sizes",
            ["Composition", "Screen Suite"],
            None,
            "{names?: [string], jobMode?: bySize|screenSpecific, comps?: [{name, width, height, duration?, fps?}]}",
            always,
            matcher_check
        ),
        query!("screen.matcher.accepted", "Accepted sizes for a screen", "{name}", accepted),
        cmd!(
            "screen.suite.tab",
            "Screen Suite Tab",
            [],
            None,
            "{tab: booking|sorter|build|manager|adapter|sizeMaster|freeze|screenshot|renamer|qc|matcher}",
            always,
            suite_tab
        ),
        query!("screen.suite.state", "Screen Suite state", "{}", suite_state),
        query!("screen.suite.alerts", "In-app Screen Suite alerts (banner + highlighted rows)", "{}", suite_alerts),
        cmd!("screen.suite.undo", "Undo Screen Suite booking update", [], None, "{}", always, suite_undo),
        cmd!(
            "screen.suite.keepOrphans",
            "Keep comps built for a screen that left the booking",
            [],
            None,
            "{name?}",
            always,
            orphans_keep
        ),
        cmd!(
            "screen.suite.removeOrphans",
            "Remove comps built for a screen that left the booking",
            [],
            None,
            "{name?}",
            always,
            orphans_remove
        ),
        cmd!(
            "screen.suite.naming",
            "Screen Manager prefix, suffix and name-from",
            [],
            None,
            "{prefix?, suffix?, nameFrom?, jobName?}",
            always,
            naming_set
        ),
        cmd!(
            "screen.matcher.send",
            "Send checked comps to EncodeCraft",
            [],
            None,
            "{anyway?: bool, presets?: {comp: presetId}}",
            always,
            matcher_send
        ),
        query!("screen.matcher.plan", "EncodeCraft preset plan for this booking", "{}", send_plan),
        cmd!("screen.library.open", "Edit Screen Library", [], None, "{}", always, library_open),
        cmd!(
            "screen.library.edit",
            "Edit a Screen Library row",
            [],
            None,
            "{add?, delete?, duplicate?, name?, width?, height?, group?, fix?, value?, section?}",
            always,
            library_edit
        ),
        cmd!("screen.library.undo", "Undo Screen Library edit", [], None, "{}", always, library_undo),
        cmd!("screen.library.redo", "Redo Screen Library edit", [], None, "{}", always, library_redo),
        cmd!(
            "screen.library.importPreview",
            "Preview a Screen Library JSON import",
            [],
            None,
            "{json, mode?: merge|replace}",
            always,
            library_import_preview
        ),
        cmd!(
            "screen.library.importApply",
            "Apply a previewed Screen Library import",
            [],
            None,
            "{mode?: merge|replace}",
            always,
            library_import_apply
        ),
        cmd!("screen.library.save", "Save Screen Library (updates every tool live)", [], None, "{path?}", always, library_save),
        cmd!("screen.adapter.parse", "Parse Adapter Tag", [], None, "{text?}", always, adapter_parse),
        cmd!("screen.adapter.apply", "Apply Adapter Tag", [], None, "{text?}", always, adapter_apply),
        cmd!("screen.freeze", "Freeze Frame (Screen Suite)", [], None, "{layers?}", always, freeze),
        cmd!("screen.screenshot", "Save Screenshot", [], None, "{path?, folder?}", always, screenshot),
        cmd!("screen.rename", "Rename Layer (Screen Suite)", [], None, "{name}", always, rename),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn session() -> Session {
        Session::new()
    }

    #[test]
    fn spring_sale_sorter_sends_to_manager_and_matcher() {
        let mut s = session();
        let paste = "1.7HD\nAl Salam Sync\nPiccadilly\nBaitak\nDiamond\nGhost Screen That Does Not Exist";
        s.execute("screen.sorter.sort", json!({"paste": paste, "cleanup": true, "matchMode": "flexible"})).unwrap();
        assert!(s.state.screen.sorter.hits.iter().any(|h| h.screen.contains("Al Salam")));
        assert!(!s.state.screen.sorter.unmatched.is_empty() || s.state.screen.sorter.flags.iter().any(|f| f.kind == "unmatched"));
        let sent = s.execute("screen.sorter.send", json!({"screenSpecific": false})).unwrap();
        assert_eq!(s.state.screen.tab, "build");
        assert!(s.state.screen.manager.show_selected_only);
        let selected: Vec<&str> = sent["selected"].as_array().unwrap().iter().filter_map(|n| n.as_str()).collect();
        assert!(selected.iter().any(|n| n.contains("1.7") || *n == "1.7HD"), "selected={selected:?} names={:?}", sent["names"]);
        s.execute("screen.manager.apply", json!({})).unwrap();
        let check = s.execute("screen.matcher.check", json!({})).unwrap();
        assert!(check["pass"].as_bool().unwrap() || check["rows"].as_array().unwrap().iter().any(|r| r["status"] == "pass"));
        let missing = s.execute("screen.matcher.check", json!({"names": ["Ghost Screen That Does Not Exist"]})).unwrap();
        assert!(!missing["pass"].as_bool().unwrap(), "{missing}");
        assert!(missing["rows"].as_array().unwrap().iter().any(|r| r["status"] == "missing"));
        s.execute("comp.new", json!({"name": "Baitak Wrong", "width": 100, "height": 50, "frameRate": 25.0, "open": false})).unwrap();
        let mismatch = s.execute("screen.matcher.check", json!({"names": ["Baitak"]})).unwrap();
        let baitak_rows: Vec<_> = mismatch["rows"].as_array().unwrap().iter().filter(|r| r["screen"].as_str().unwrap().contains("Baitak")).collect();
        assert!(baitak_rows.iter().any(|r| r["status"] == "pass" || r["status"] == "sizeMismatch"), "{mismatch}");
    }

    #[test]
    fn screen_specific_send_uses_individual_names() {
        let mut s = session();
        s.execute("screen.sorter.sort", json!({"paste": "Jahra Prime\nSalmiya Express", "cleanup": true})).unwrap();
        s.execute("screen.sorter.send", json!({"screenSpecific": true})).unwrap();
        assert_eq!(s.state.screen.job_mode, JobMode::ScreenSpecific);
        assert!(s.state.screen.manager.names.iter().any(|n| n.contains("Jahra") || n.contains("Salmiya")));
        assert!(!s.state.screen.manager.names.iter().any(|n| n == "1.7HD"));
    }

    #[test]
    fn palm_trees_combiner_four_faces_edge_to_edge() {
        let mut s = session();
        s.execute("screen.manager.select", json!({"names": ["Marina - Palm Trees"], "jobMode": "bySize"})).unwrap();
        s.execute("screen.manager.apply", json!({"combiner": "Marina_Palms_Full"})).unwrap();
        let item = s.project.items.values().find(|i| i.name.contains("Palms") && i.as_comp().is_some()).expect("combined palms");
        let c = item.as_comp().unwrap();
        assert_eq!((c.width, c.height), (960, 960));
        assert_eq!(c.layers.len(), 4);
        let mut xs: Vec<u32> = c
            .layers
            .iter()
            .map(|l| {
                let v = l.props.prop("transform/position").unwrap().value.components();
                v[0].round() as u32
            })
            .collect();
        xs.sort();
        assert_eq!(xs, vec![120, 360, 600, 840]);
        assert!(s.state.screen.manager.warnings.is_empty(), "{:?}", s.state.screen.manager.warnings);
    }

    #[test]
    fn al_salam_normal_combine_no_warning_extra_warns() {
        let mut s = session();
        s.execute("screen.manager.select", json!({"names": ["Al Salam Sync"], "jobMode": "bySize"})).unwrap();
        s.execute("screen.manager.apply", json!({"combiner": "Al_Salam_Sync"})).unwrap();
        let combined = s.project.items.values().find(|i| i.name.contains("Al_Salam") && i.as_comp().is_some_and(|c| c.width == 3072)).expect("combined");
        assert_eq!(combined.as_comp().unwrap().height, 576);
        assert!(s.state.screen.manager.warnings.is_empty());
        let pass = s.execute("screen.matcher.check", json!({"names": ["Al Salam Sync"]})).unwrap();
        assert!(pass["pass"].as_bool().unwrap(), "{pass}");
        assert!(pass["oversized"].as_array().unwrap().is_empty());

        let mut s2 = session();
        s2.execute("comp.new", json!({"name": "Al Salam Sync A", "width": 1536, "height": 576, "frameRate": 25.0, "open": false})).unwrap();
        s2.execute("comp.new", json!({"name": "Al Salam Sync B", "width": 1536, "height": 576, "frameRate": 25.0, "open": false})).unwrap();
        s2.execute("screen.manager.select", json!({"names": ["Al Salam Sync"]})).unwrap();
        let r = s2.execute("screen.manager.combine", json!({"combiner": "Al_Salam_Sync"})).unwrap();
        let first = r.as_array().unwrap().first().expect("combiner result");
        assert_eq!(first["width"], 3072);
        assert_eq!(first["height"], 1152);
        assert_eq!(first["extraPieces"], 1);
        assert!(!s2.state.screen.manager.warnings.is_empty());
        let msg = &s2.state.screen.manager.warnings[0].message();
        assert!(msg.contains("3072×576") && msg.contains("3072×1152") && msg.contains("1 extra piece"), "{msg}");
        let check = s2.execute("screen.matcher.check", json!({"names": ["Al Salam Sync"]})).unwrap();
        assert!(!check["pass"].as_bool().unwrap(), "{check}");
        assert!(!check["oversized"].as_array().unwrap().is_empty());
        let alerts = s2.execute("screen.suite.alerts", json!({})).unwrap();
        assert!(alerts.as_array().unwrap().iter().any(|a| a["kind"] == "extraStack" || a["kind"] == "sizeMismatch"), "{alerts}");
        let st = s2.execute("screen.suite.state", json!({})).unwrap();
        assert!(st["alerts"].as_array().unwrap().iter().any(|a| a["tab"] == "qc" || a["tab"] == "build"), "{st}");
        assert!(s2.drain_events().iter().any(|e| matches!(e, Event::Toast { error: true, .. })));
    }

    #[test]
    fn no_alias_in_screen_specific_manager() {
        let mut s = session();
        let r = s.execute("screen.manager.select", json!({"names": ["Top Gear", "Baitak"], "jobMode": "screenSpecific"})).unwrap();
        let matches = r["matches"].as_array().unwrap();
        let top = matches.iter().find(|m| m["asked"] == "Top Gear").unwrap();
        assert_ne!(top["preset"].as_str().unwrap_or("").to_ascii_lowercase().find("baitak"), Some(0));
        assert!(top["status"] != "ok" || !top["preset"].as_str().unwrap_or("").to_ascii_lowercase().contains("baitak"));
    }

    #[test]
    fn live_update_diffs_and_orphans_never_auto_delete() {
        let mut s = session();
        s.execute("screen.sorter.sort", json!({"paste": "1.7HD\nTop Gear\nPiccadilly", "cleanup": true})).unwrap();
        s.execute("screen.sorter.send", json!({"screenSpecific": false})).unwrap();
        s.execute("screen.manager.apply", json!({"prefix": "SpringSale", "suffix": "EN"})).unwrap();
        assert!(s.project.items.values().any(|i| i.name.contains("Top Gear") && i.as_comp().is_some()));
        s.execute("screen.sorter.sort", json!({"paste": "1.7HD\nPiccadilly\nEye of Kuwait", "cleanup": true})).unwrap();
        assert!(s.state.screen.diff.removed.iter().any(|n| n.contains("Top Gear") || n == "Top Gear"), "{:?}", s.state.screen.diff);
        assert!(s.state.screen.diff.added.iter().any(|n| n.contains("Eye") || n.contains("Kuwait")), "{:?}", s.state.screen.diff);
        assert!(!s.state.screen.updated_tabs.is_empty());
        assert!(s.project.items.values().any(|i| i.name.contains("Top Gear") && i.as_comp().is_some()), "must not auto-delete");
        assert!(!s.state.screen.orphans.is_empty(), "{:?}", s.state.screen.orphans);
        s.execute("screen.suite.keepOrphans", json!({})).unwrap();
        assert!(s.state.screen.orphans.iter().all(|o| o.keep == Some(true)));
        s.execute("screen.suite.undo", json!({})).unwrap();
        assert!(s.state.screen.sorter.paste_names_by_size.iter().any(|n| n.contains("Top Gear") || n == "Top Gear"));
    }

    #[test]
    fn prefix_suffix_and_material_names_never_rename_footage() {
        let mut s = session();
        s.execute("screen.manager.select", json!({"names": ["Piccadilly"], "jobMode": "bySize"})).unwrap();
        s.execute("screen.suite.naming", json!({"prefix": "SpringSale", "suffix": "EN, AR"})).unwrap();
        let r = s.execute("screen.manager.apply", json!({"prefix": "SpringSale", "suffix": "EN, AR"})).unwrap();
        let created = r["created"].as_array().unwrap();
        assert_eq!(created.len(), 2, "{r}");
        assert!(created.iter().any(|c| c["name"] == "SpringSale_Piccadilly_EN"));
        assert!(created.iter().any(|c| c["name"] == "SpringSale_Piccadilly_AR"));
        assert!(created.iter().all(|c| c["footageRenamed"] == false));
        let preview = s.execute("screen.suite.naming", json!({"nameFrom": "material", "prefix": "X", "suffix": "Y"})).unwrap();
        assert_eq!(preview["preview"], "X_Jahra Prime EN_Y");
    }

    #[test]
    fn matcher_send_locked_until_pass() {
        let mut s = session();
        s.execute("screen.sorter.sort", json!({"paste": "Al Salam Sync", "cleanup": true})).unwrap();
        s.execute("screen.sorter.send", json!({"to": "matcher"})).unwrap();
        assert_eq!(s.state.screen.tab, "qc");
        let err = s.execute("screen.matcher.send", json!({})).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("locked") || msg.contains("PASS") || msg.contains("Not ready") || msg.contains("Send"), "{msg}");
        s.execute("screen.manager.select", json!({"names": ["Al Salam Sync"]})).unwrap();
        s.execute("screen.manager.apply", json!({"combiner": "Al_Salam_Sync"})).unwrap();
        let check = s.execute("screen.matcher.check", json!({"names": ["Al Salam Sync"]})).unwrap();
        assert!(check["pass"].as_bool().unwrap(), "{check}");
        let plan = s.execute("screen.matcher.plan", json!({})).unwrap();
        assert!(plan["rows"].as_array().unwrap().iter().any(|r| r["preset"] == "Better Res"), "{plan}");
    }

    #[test]
    fn library_editor_fix_and_no_auto_add() {
        let mut s = session();
        let lib = s.execute("screen.library", json!({})).unwrap();
        assert!(lib["issues"].is_array());
        assert!(lib["merged"].as_array().unwrap().iter().any(|r| r["name"].as_str().unwrap().contains("Al Salam")));
        s.execute("screen.library.edit", json!({"add": "Top Gear", "width": 2624, "height": 608, "group": "DOOH"})).unwrap();
        s.execute("screen.library.undo", json!({})).unwrap();
        let open = s.execute("screen.library.open", json!({})).unwrap();
        assert!(open["merged"].as_array().is_some());
    }
}
