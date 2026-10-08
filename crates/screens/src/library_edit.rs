//! Screen Library editor: merged list, inline checks, combiner-link Fix, import preview.

use serde::{Deserialize, Serialize};

use crate::fuzzy::{similarity, token_score};
use crate::inventory::{Combiner, Library, Preset, Screen, is_al_salam_sync};
use crate::normalize::{format_size, normalize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedScreenRow {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub group: String,
    pub in_manager: bool,
    pub in_sizemaster: bool,
    pub in_adapter: bool,
    #[serde(default)]
    pub animated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIssue {
    pub id: String,
    pub section: String,
    pub level: String,
    pub row: String,
    pub message: String,
    #[serde(default)]
    pub fix: String,
    #[serde(default)]
    pub fix_label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub mode: String,
    pub screens_added: Vec<String>,
    pub screens_changed: Vec<String>,
    pub screens_removed: Vec<String>,
    pub combiners_changed: Vec<String>,
    pub summary: String,
}

pub fn merged_screens(lib: &Library) -> Vec<MergedScreenRow> {
    let mut rows: Vec<MergedScreenRow> = Vec::new();
    let push = |rows: &mut Vec<MergedScreenRow>, name: &str, width: u32, height: u32, group: &str, animated: bool| {
        if rows.iter().any(|r| normalize(&r.name) == normalize(name)) {
            return;
        }
        let n = normalize(name);
        rows.push(MergedScreenRow {
            name: name.to_string(),
            width,
            height,
            group: group.to_string(),
            in_manager: lib.presets.iter().any(|p| normalize(&p.name) == n),
            in_sizemaster: lib.sizemaster.iter().any(|p| normalize(&p.name) == n),
            in_adapter: lib.adapter.iter().any(|p| normalize(&p.name) == n),
            animated,
        });
    };
    // Inventory sizes win (Al Nassar Tower is 1536×576, not the old 2688×1152 SM preset).
    for s in &lib.screens {
        push(&mut rows, &s.name, s.width, s.height, &s.group, s.animated);
    }
    for p in &lib.presets {
        let animated = lib.screen_lookup(&p.name).is_some_and(|s| s.animated);
        push(&mut rows, &p.name, p.width, p.height, &p.group, animated);
    }
    for p in lib.sizemaster.iter().chain(lib.adapter.iter()) {
        let animated = lib.screen_lookup(&p.name).is_some_and(|s| s.animated);
        push(&mut rows, &p.name, p.width, p.height, &p.group, animated);
    }
    rows.sort_by_key(|a| a.name.to_ascii_lowercase());
    rows
}

pub fn library_issues(lib: &Library) -> Vec<LibraryIssue> {
    let mut out = Vec::new();
    let rows = merged_screens(lib);
    for (i, r) in rows.iter().enumerate() {
        if r.width % 2 == 1 || r.height % 2 == 1 {
            let intended = (normalize(&r.name).contains("piccadilly") && r.width == 2027) || (normalize(&r.name).contains("kuwait gate") && r.width == 4459);
            if !intended {
                out.push(LibraryIssue {
                    id: format!("odd-{i}"),
                    section: "screens".into(),
                    level: "warn".into(),
                    row: r.name.clone(),
                    message: format!("Odd size · {}×{}. Double-check it.", r.width, r.height),
                    fix: "markOk".into(),
                    fix_label: "Mark OK".into(),
                });
            }
        }
    }
    let mut seen: Vec<(String, u32, u32, usize)> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let key = (normalize(&r.name), r.width, r.height);
        if let Some((_, _, _, prev)) = seen.iter().find(|(n, w, h, _)| *n == key.0 && *w == key.1 && *h == key.2) {
            out.push(LibraryIssue {
                id: format!("dup-{i}"),
                section: "screens".into(),
                level: "warn".into(),
                row: r.name.clone(),
                message: format!("Duplicate · same name and size as row {}", prev + 1),
                fix: "removeDup".into(),
                fix_label: "Remove duplicate".into(),
            });
        } else {
            seen.push((key.0, key.1, key.2, i));
        }
    }
    for (ci, c) in lib.combiners.iter().enumerate() {
        for (pi, col) in c.columns.iter().enumerate() {
            if col.screen_name.is_empty() {
                continue;
            }
            if lib.preset_by_name(&col.screen_name).is_some() || lib.screen_by_name(&col.screen_name).is_some() {
                continue;
            }
            let suggestion = closest_screen(lib, &col.screen_name);
            let msg = match &suggestion {
                Some(s) => format!("No screen preset is called “{}”. Did you mean {s}?", col.screen_name),
                None => format!("No screen preset is called “{}”.", col.screen_name),
            };
            out.push(LibraryIssue {
                id: format!("link-{ci}-{pi}"),
                section: "combiners".into(),
                level: "bad".into(),
                row: c.name.clone(),
                message: msg,
                fix: suggestion.unwrap_or_default(),
                fix_label: "Fix".into(),
            });
        }
    }
    for s in &lib.screens {
        if let Some(p) = lib.preset_by_name(&s.name)
            && (p.width != s.width || p.height != s.height)
        {
            out.push(LibraryIssue {
                id: format!("mismatch-{}", normalize(&s.name).replace(' ', "-")),
                section: "screens".into(),
                level: "bad".into(),
                row: s.name.clone(),
                message: format!("Size mismatch · planner {}×{} vs Screen Manager {}×{}.", s.width, s.height, p.width, p.height),
                fix: format!("{}x{}", s.width, s.height),
                fix_label: format!("Fix → {}×{}", s.width, s.height),
            });
        }
    }
    for r in &rows {
        if is_al_salam_sync(&r.name) && r.width == 3072 {
            out.push(LibraryIssue {
                id: "salam-conflict".into(),
                section: "screens".into(),
                level: "info".into(),
                row: r.name.clone(),
                message: "Al Salam Sync is one 1536×576 screen plus the 3072×576 h-dup ×2 combiner, not a size conflict.".into(),
                fix: String::new(),
                fix_label: String::new(),
            });
        }
    }
    out
}

fn closest_screen(lib: &Library, asked: &str) -> Option<String> {
    let q = asked.replace("(6 screens)", "").replace("(6 screen)", "");
    let mut best: Option<(f32, String)> = None;
    for s in lib.screens.iter().map(|s| s.name.as_str()).chain(lib.presets.iter().map(|p| p.name.as_str())) {
        let sc = token_score(&q, s).max(similarity(&q, s));
        if sc >= 0.55 && best.as_ref().is_none_or(|(b, _)| sc > *b) {
            best = Some((sc, s.to_string()));
        }
    }
    best.map(|(_, n)| n)
}

/// Apply a Fix action. `fix` is either a command (`markOk`, `removeDup`) or a replacement name.
pub fn apply_fix(lib: &mut Library, issue_id: &str, fix: &str) -> bool {
    if issue_id.starts_with("link-") {
        let rest = issue_id.trim_start_matches("link-");
        let mut parts = rest.split('-');
        let Some(ci) = parts.next().and_then(|s| s.parse::<usize>().ok()) else { return false };
        let Some(pi) = parts.next().and_then(|s| s.parse::<usize>().ok()) else { return false };
        if let Some(col) = lib.combiners.get_mut(ci).and_then(|c| c.columns.get_mut(pi))
            && !fix.is_empty()
            && fix != "Fix"
        {
            col.screen_name = fix.to_string();
            return true;
        }
        return false;
    }
    if fix == "removeDup" {
        let issues = library_issues(lib);
        if let Some(iss) = issues.iter().find(|i| i.id == issue_id) {
            let n = normalize(&iss.row);
            if let Some(idx) = lib.presets.iter().enumerate().rev().position(|(_, p)| normalize(&p.name) == n) {
                let idx = lib.presets.len() - 1 - idx;
                if lib.presets.iter().filter(|p| normalize(&p.name) == n).count() > 1 {
                    lib.presets.remove(idx);
                    return true;
                }
            }
        }
        return false;
    }
    if issue_id.starts_with("odd-") && fix == "markOk" {
        return true;
    }
    if issue_id.starts_with("mismatch-")
        && let Some((w, h)) = crate::normalize::parse_pixel_size(fix)
    {
        let issues = library_issues(lib);
        if let Some(iss) = issues.iter().find(|i| i.id == issue_id) {
            set_screen_size(lib, &iss.row, w, h);
            return true;
        }
    }
    false
}

pub fn add_screen(lib: &mut Library, name: &str, width: u32, height: u32, group: &str) {
    if name.trim().is_empty() || width == 0 || height == 0 {
        return;
    }
    if lib.preset_by_name(name).is_some() {
        return;
    }
    lib.presets.push(Preset { name: name.to_string(), width, height, group: group.to_string(), duration_s: 10.0 });
    if lib.screen_by_name(name).is_none() {
        let id = lib.screens.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1);
        lib.screens.push(Screen {
            id,
            name: name.to_string(),
            group: group.to_string(),
            width,
            height,
            kind: "DOOH".into(),
            location: String::new(),
            governorate: String::new(),
            category: String::new(),
            physical: String::new(),
            custom: true,
            aliases: Vec::new(),
            not_in_planner: true,
            animated: false,
        });
    }
}

pub fn set_screen_size(lib: &mut Library, name: &str, width: u32, height: u32) {
    for p in lib.presets.iter_mut().chain(lib.sizemaster.iter_mut()).chain(lib.adapter.iter_mut()) {
        if normalize(&p.name) == normalize(name) {
            p.width = width;
            p.height = height;
        }
    }
    for s in &mut lib.screens {
        if normalize(&s.name) == normalize(name) {
            s.width = width;
            s.height = height;
        }
    }
    for c in &mut lib.combiners {
        for col in &mut c.columns {
            if normalize(&col.screen_name) == normalize(name) {
                col.match_wh = Some((width, height));
            }
        }
    }
}

pub fn set_screen_animated(lib: &mut Library, name: &str, animated: bool) {
    for s in &mut lib.screens {
        if normalize(&s.name) == normalize(name) {
            s.animated = animated;
        }
    }
}

pub fn delete_screen(lib: &mut Library, name: &str) {
    let n = normalize(name);
    lib.presets.retain(|p| normalize(&p.name) != n);
    lib.sizemaster.retain(|p| normalize(&p.name) != n);
    lib.adapter.retain(|p| normalize(&p.name) != n);
    lib.screens.retain(|s| normalize(&s.name) != n);
}

pub fn duplicate_screen(lib: &mut Library, name: &str) -> Option<String> {
    let src = lib.preset_by_name(name)?.clone();
    let new_name = format!("{} copy", src.name);
    add_screen(lib, &new_name, src.width, src.height, &src.group);
    Some(new_name)
}

/// Merge incoming library into `base`. Replace uses incoming as-is (caller swaps).
pub fn preview_import(base: &Library, incoming: &Library, mode: &str) -> ImportPreview {
    let replace = mode.eq_ignore_ascii_case("replace");
    let mut screens_added = Vec::new();
    let mut screens_changed = Vec::new();
    let mut screens_removed = Vec::new();
    for p in &incoming.presets {
        match base.preset_by_name(&p.name) {
            None => screens_added.push(format!("{} {}", p.name, format_size(p.width, p.height))),
            Some(cur) if cur.width != p.width || cur.height != p.height => {
                screens_changed.push(format!("{} {}×{} → {}×{}", p.name, cur.width, cur.height, p.width, p.height));
            }
            Some(_) => {}
        }
    }
    if replace {
        for p in &base.presets {
            if incoming.preset_by_name(&p.name).is_none() {
                screens_removed.push(format!("{} {}", p.name, format_size(p.width, p.height)));
            }
        }
    }
    let mut combiners_changed = Vec::new();
    for c in &incoming.combiners {
        if let Some(cur) = base.combiners.iter().find(|x| normalize(&x.name) == normalize(&c.name)) {
            if cur.columns != c.columns {
                combiners_changed.push(c.name.clone());
            }
        } else {
            combiners_changed.push(format!("+ {}", c.name));
        }
    }
    let summary = format!(
        "{} screens added, {} changed, {} removed · {} combiners",
        screens_added.len(),
        screens_changed.len(),
        screens_removed.len(),
        combiners_changed.len()
    );
    ImportPreview { mode: if replace { "replace".into() } else { "merge".into() }, screens_added, screens_changed, screens_removed, combiners_changed, summary }
}

pub fn apply_merge(base: &mut Library, incoming: Library) {
    for p in incoming.presets {
        if let Some(cur) = base.presets.iter_mut().find(|x| normalize(&x.name) == normalize(&p.name)) {
            *cur = p;
        } else {
            base.presets.push(p);
        }
    }
    for s in incoming.screens {
        if let Some(cur) = base.screens.iter_mut().find(|x| normalize(&x.name) == normalize(&s.name)) {
            *cur = s;
        } else {
            base.screens.push(s);
        }
    }
    for c in incoming.combiners {
        if let Some(cur) = base.combiners.iter_mut().find(|x| normalize(&x.name) == normalize(&c.name)) {
            *cur = c;
        } else {
            base.combiners.push(c);
        }
    }
}

pub fn parse_library_json(text: &str) -> Result<Library, String> {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(default)]
        screens: Vec<Screen>,
        #[serde(default)]
        presets: Vec<Preset>,
        #[serde(default)]
        combiners: Vec<Combiner>,
        #[serde(default)]
        sizemaster: Vec<Preset>,
        #[serde(default)]
        adapter: Vec<Preset>,
    }
    if let Ok(env) = serde_json::from_str::<Envelope>(text) {
        let mut lib = Library::load();
        if !env.screens.is_empty() {
            lib.screens = env.screens;
        }
        if !env.presets.is_empty() {
            lib.presets = env.presets;
        }
        if !env.combiners.is_empty() {
            lib.combiners = env.combiners;
        }
        if !env.sizemaster.is_empty() {
            lib.sizemaster = env.sizemaster;
        }
        if !env.adapter.is_empty() {
            lib.adapter = env.adapter;
        }
        return Ok(lib);
    }
    if let Ok(presets) = serde_json::from_str::<Vec<Preset>>(text) {
        let mut lib = Library::load();
        lib.presets = presets;
        return Ok(lib);
    }
    Err("could not parse Screen Library JSON".into())
}

pub fn export_library_json(lib: &Library) -> Result<String, String> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        screens: &'a [Screen],
        presets: &'a [Preset],
        combiners: &'a [Combiner],
        sizemaster: &'a [Preset],
        adapter: &'a [Preset],
    }
    serde_json::to_string_pretty(&Envelope {
        screens: &lib.screens,
        presets: &lib.presets,
        combiners: &lib.combiners,
        sizemaster: &lib.sizemaster,
        adapter: &lib.adapter,
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::is_palm_trees;

    #[test]
    fn one_merged_list_al_salam_is_not_a_conflict() {
        let lib = Library::load();
        let rows = merged_screens(&lib);
        assert!(rows.iter().any(|r| is_al_salam_sync(&r.name) && r.width == 1536 && r.height == 576));
        assert!(!rows.iter().any(|r| is_al_salam_sync(&r.name) && r.width == 3072));
        let nassar = rows.iter().find(|r| normalize(&r.name) == "al nassar tower").expect("nassar");
        assert_eq!((nassar.width, nassar.height), (1536, 576));
        assert!(rows.iter().any(|r| is_palm_trees(&r.name) && r.width == 240));
    }

    #[test]
    fn broken_combiner_link_fix_points_at_palm_trees() {
        let mut lib = Library::load();
        let c = lib.combiners.iter_mut().find(|c| normalize(&c.name).contains("palms")).expect("palms");
        if let Some(col) = c.columns.first_mut() {
            col.screen_name = "Marina - Palm Trees (6 screens)".into();
        }
        let issues = library_issues(&lib);
        let link = issues.iter().find(|i| i.section == "combiners" && i.row.to_ascii_lowercase().contains("palm")).expect("link");
        assert!(link.message.contains("Marina - Palm Trees"), "{}", link.message);
        assert!(apply_fix(&mut lib, &link.id, &link.fix));
        let col = &lib.combiners.iter().find(|c| normalize(&c.name).contains("palms")).unwrap().columns[0];
        assert_eq!(col.screen_name, "Marina - Palm Trees");
        assert!(library_issues(&lib).iter().all(|i| i.id != link.id || i.section != "combiners"));
    }
}
