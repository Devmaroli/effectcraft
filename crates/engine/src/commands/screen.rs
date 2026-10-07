//! Screen Suite: Size Sorter, Screen Manager, combiners, Size Matcher and related studio tools.
//!
//! Commands are `screen.*`. Comp fps is always 25. Combiner extra-stack warnings live on
//! [`effectcraft_screens::DupWarning`].

use std::sync::OnceLock;

use effectcraft_project::ItemId;
use effectcraft_screens::combiner::{CombinerLayout, unique_source_slots};
use effectcraft_screens::inventory::{Combiner, CombinerColumn, duration_for};
use effectcraft_screens::manager::JobMode;
use effectcraft_screens::normalize::normalize;
use effectcraft_screens::sorter::{MatchMode, SorterFilters, apply_flag_answers};
use effectcraft_screens::{
    CompProbe, Library, ManagerSelection, MatchReport, SorterResult, accepted_sizes_for, active_combiners, check_comps, parse_tag_text, select_pasted,
    sort_lines,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CommandSpec, always, b_p, bad, str_p};
use crate::{EngineError, Event, Result, Session, cmd, query};

fn library() -> &'static Library {
    static LIB: OnceLock<Library> = OnceLock::new();
    LIB.get_or_init(Library::load)
}

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
}

fn booking_tab() -> String {
    "booking".into()
}

fn true_bool() -> bool {
    true
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
        }
    }
}

fn set_tab(s: &mut Session, tab: &str) {
    s.state.screen.tab = tab.to_string();
    s.events.push(Event::Frontend { command: "window.panel".into(), params: json!({"panel": "screenSuite"}) });
}

fn library_json(s: &mut Session, _: &Value) -> Result<Value> {
    let lib = library();
    let (groups, kinds, govs, cats) = lib.filter_values();
    Ok(json!({
        "screens": lib.screens.len(),
        "presets": lib.presets.len(),
        "combiners": lib.combiners.len(),
        "groups": groups,
        "kinds": kinds,
        "governorates": govs,
        "categories": cats,
        "fps": effectcraft_screens::STUDIO_FPS,
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
    let result =
        sort_lines(library(), &s.state.screen.paste, s.state.screen.cleanup, s.state.screen.match_mode, &s.state.screen.filters, s.state.screen.send_all);
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
    serde_json::to_value(&s.state.screen.sorter).map_err(|e| EngineError::Other(e.to_string()))
}

fn sorter_send(s: &mut Session, p: &Value) -> Result<Value> {
    if s.state.screen.sorter.hits.is_empty() && !s.state.screen.paste.is_empty() {
        sorter_sort(s, p)?;
    }
    let screen_specific = b_p(p, "screenSpecific").unwrap_or(false)
        || str_p(p, "jobMode").is_some_and(|m| m.eq_ignore_ascii_case("screenSpecific") || m.eq_ignore_ascii_case("specific"));
    let names = if screen_specific { s.state.screen.sorter.paste_names_screen_specific.clone() } else { s.state.screen.sorter.paste_names_by_size.clone() };
    if names.is_empty() {
        return Err(bad("screen.sorter.send", "sort a booking list first (no names to send)"));
    }
    s.state.screen.job_mode = if screen_specific { JobMode::ScreenSpecific } else { JobMode::BySize };
    s.state.screen.manager = select_pasted(library(), &names, s.state.screen.job_mode);
    s.state.screen.manager.show_selected_only = true;
    set_tab(s, "build");
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
    s.state.screen.manager = select_pasted(library(), &names, s.state.screen.job_mode);
    if let Some(v) = b_p(p, "showSelectedOnly") {
        s.state.screen.manager.show_selected_only = v;
    } else {
        s.state.screen.manager.show_selected_only = true;
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
    let lib = library();
    let mut created = Vec::new();
    let selected = s.state.screen.manager.selected.clone();
    if selected.is_empty() {
        return Err(bad("screen.manager.apply", "no screens selected — paste names or send them from Size Sorter first"));
    }
    for name in &selected {
        let (w, h, group) = if let Some(pr) = lib.preset_by_name(name) {
            (pr.width, pr.height, pr.group.as_str())
        } else if let Some(sc) = lib.screen_by_name(name).or_else(|| lib.screens.iter().find(|sc| normalize(&sc.name) == normalize(name))) {
            (sc.width, sc.height, sc.group.as_str())
        } else if let Some(m) = s.state.screen.manager.matches.iter().find(|m| m.preset == *name) {
            (m.width, m.height, "")
        } else {
            continue;
        };
        let dur = if size_master { duration_for(name, group, true) } else { duration_for(name, group, false) };
        let id = ensure_preset_comp(s, name, w, h, dur)?;
        created.push(json!({"name": name, "comp": id.0, "width": w, "height": h, "duration": dur, "fps": 25.0}));
    }
    let combined = combine_active(s, p)?;
    set_tab(s, "build");
    Ok(json!({"created": created, "combiners": combined}))
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
    let lib = library();
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
        for w in &lay.warnings {
            s.events.push(Event::Toast { message: w.message(), error: true });
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
    s.state.screen.manager.combiners = active_combiners(lib, &selected);
    s.state.screen.manager.warnings = warnings;
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
    let lib = library();
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
    let report = check_comps(lib, &required, &comps, mode);
    s.state.screen.matcher = report.clone();
    s.state.screen.tab = "qc".into();
    if !report.oversized.is_empty() {
        for m in &report.oversized {
            s.events.push(Event::Toast { message: m.clone(), error: true });
        }
    }
    serde_json::to_value(&s.state.screen.matcher).map_err(|e| EngineError::Other(e.to_string()))
}

fn suite_tab(s: &mut Session, p: &Value) -> Result<Value> {
    let tab = str_p(p, "tab").ok_or_else(|| bad("screen.suite.tab", "missing `tab` (booking|build|adapter|deliver|qc)"))?;
    let t = tab.to_ascii_lowercase();
    if !matches!(t.as_str(), "booking" | "build" | "adapter" | "deliver" | "qc") {
        return Err(bad("screen.suite.tab", "tab: booking|build|adapter|deliver|qc"));
    }
    set_tab(s, &t);
    Ok(json!({"tab": t}))
}

fn suite_state(s: &mut Session, _: &Value) -> Result<Value> {
    serde_json::to_value(&s.state.screen).map_err(|e| EngineError::Other(e.to_string()))
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
    let sizes = accepted_sizes_for(library(), name);
    let _s = s;
    serde_json::to_value(sizes).map_err(|e| EngineError::Other(e.to_string()))
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
            "{screenSpecific?: bool, jobMode?: bySize|screenSpecific}",
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
            "{tool?: manager|sizeMaster, combiner?, extraSources?}",
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
        cmd!("screen.suite.tab", "Screen Suite Tab", [], None, "{tab: booking|build|adapter|deliver|qc}", always, suite_tab),
        query!("screen.suite.state", "Screen Suite state", "{}", suite_state),
        cmd!("screen.adapter.parse", "Parse Adapter Tag", ["Composition", "Screen Suite"], None, "{text?}", always, adapter_parse),
        cmd!("screen.adapter.apply", "Apply Adapter Tag", ["Composition", "Screen Suite"], None, "{text?}", always, adapter_apply),
        cmd!("screen.freeze", "Freeze Frame (Screen Suite)", ["Composition", "Screen Suite"], None, "{layers?}", always, freeze),
        cmd!("screen.screenshot", "Save Screenshot", ["Composition", "Screen Suite"], None, "{path?, folder?}", always, screenshot),
        cmd!("screen.rename", "Rename Layer (Screen Suite)", ["Composition", "Screen Suite"], None, "{name}", always, rename),
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
}
