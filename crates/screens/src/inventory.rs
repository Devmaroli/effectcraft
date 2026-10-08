//! Inventory + studio overrides + Screen Manager / Adapter / SizeMaster presets.

use serde::{Deserialize, Serialize};

use crate::normalize::{format_size, normalize};

pub const STUDIO_FPS: f64 = 25.0;
pub const DEFAULT_SPOT_SEC: f64 = 10.0;
pub const MALL_15S: &[&str] = &["al kout", "360 mall", "khairan mall"];
/// SizeMaster also treats Warehouse Mall as 15 s except Outdoor.
pub const SIZEMASTER_15S: &[&str] = &["al kout", "360 mall", "khairan mall", "warehouse mall"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub id: u32,
    pub name: String,
    pub group: String,
    pub width: u32,
    pub height: u32,
    pub kind: String,
    pub location: String,
    pub governorate: String,
    pub category: String,
    #[serde(default)]
    pub physical: String,
    #[serde(default)]
    pub custom: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Not in the planner inventory (matcher: NOT CHECKABLE).
    #[serde(default)]
    pub not_in_planner: bool,
}

impl Screen {
    pub fn size_label(&self) -> String {
        format_size(self.width, self.height)
    }

    pub fn format_alias(&self) -> Option<&'static str> {
        match (self.width, self.height) {
            (1920, 1080) if !is_avenues_entrance(&self.name) && !is_yaal_slayel(&self.name) => Some("1.7HD"),
            (1536, 576) if !name_has(&self.name, "thuraya") && !name_has(&self.name, "al salam sync") => Some("2.6"),
            (2624, 608) if !name_has(&self.name, "thuraya") => Some("4.3"),
            (1200, 240) => Some("Marina Balcony"),
            (1440, 1800) => Some("Avenues QD"),
            _ => None,
        }
    }
}

fn name_has(name: &str, needle: &str) -> bool {
    normalize(name).contains(needle)
}

pub fn is_avenues_entrance(name: &str) -> bool {
    let n = normalize(name);
    n.contains("grand avenues entrance") || n.contains("the mall entrance") || n.contains("grand plaza entrance")
}

pub fn is_yaal_slayel(name: &str) -> bool {
    let n = normalize(name);
    n.contains("yaal mall digital") || n.contains("slayel al jahra")
}

pub fn is_al_salam_sync(name: &str) -> bool {
    normalize(name).contains("al salam sync")
}

pub fn is_thuraya(name: &str) -> bool {
    normalize(name).contains("thuraya")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub duration_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CombinerColumn {
    pub screen_name: String,
    #[serde(default)]
    pub layout: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default)]
    pub match_wh: Option<(u32, u32)>,
    #[serde(default)]
    pub force_scale: bool,
    #[serde(default)]
    pub comp_w: Option<u32>,
    #[serde(default)]
    pub v_align: String,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Combiner {
    pub name: String,
    #[serde(default)]
    pub columns: Vec<CombinerColumn>,
}

#[derive(Clone, Debug)]
pub struct Library {
    pub screens: Vec<Screen>,
    pub presets: Vec<Preset>,
    pub adapter: Vec<Preset>,
    pub sizemaster: Vec<Preset>,
    pub combiners: Vec<Combiner>,
}

impl Library {
    pub fn load() -> Self {
        let mut screens: Vec<Screen> = serde_json::from_str(include_str!("../data/screens.json")).unwrap_or_default();
        apply_screen_overrides(&mut screens);
        add_custom_screens(&mut screens);
        let mut presets = load_presets(include_str!("../data/sm-presets.json"));
        let mut adapter = load_presets(include_str!("../data/adapter-presets.json"));
        let mut sizemaster = load_presets(include_str!("../data/sizemaster-presets.json"));
        apply_preset_overrides(&mut presets);
        apply_preset_overrides(&mut adapter);
        apply_preset_overrides(&mut sizemaster);
        ensure_tawfeer(&mut presets);
        ensure_tawfeer(&mut adapter);
        ensure_tawfeer(&mut sizemaster);
        let mut combiners = load_combiners(include_str!("../data/combiners.json"));
        fix_palm_trees(&mut screens, &mut presets, &mut adapter, &mut sizemaster, &mut combiners);
        Self { screens, presets, adapter, sizemaster, combiners }
    }

    pub fn screen_by_name(&self, name: &str) -> Option<&Screen> {
        let n = normalize(name);
        self.screens.iter().find(|s| normalize(&s.name) == n)
    }

    pub fn preset_by_name(&self, name: &str) -> Option<&Preset> {
        let n = normalize(name);
        self.presets.iter().find(|p| normalize(&p.name) == n)
    }

    pub fn filter_values(&self) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        fn uniq(iter: impl Iterator<Item = String>) -> Vec<String> {
            let mut v: Vec<String> = iter.filter(|s| !s.is_empty()).collect();
            v.sort();
            v.dedup();
            v
        }
        (
            uniq(self.screens.iter().map(|s| s.group.clone())),
            uniq(self.screens.iter().map(|s| s.kind.clone())),
            uniq(self.screens.iter().map(|s| s.governorate.clone())),
            uniq(self.screens.iter().map(|s| s.category.clone())),
        )
    }
}

#[derive(Deserialize)]
struct RawPreset {
    name: String,
    w: u32,
    h: u32,
    #[serde(default)]
    group: String,
}

fn load_presets(json: &str) -> Vec<Preset> {
    let raw: Vec<RawPreset> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter().map(|p| Preset { name: p.name, width: p.w, height: p.h, group: p.group, duration_s: DEFAULT_SPOT_SEC }).collect()
}

#[derive(Deserialize)]
struct RawCol {
    #[serde(rename = "screenName", default)]
    screen_name: String,
    #[serde(default)]
    layout: String,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default, rename = "match")]
    match_wh: Option<Vec<u32>>,
    #[serde(default, rename = "forceScale")]
    force_scale: bool,
    #[serde(default, rename = "compW")]
    comp_w: Option<u32>,
    #[serde(default, rename = "vAlign")]
    v_align: String,
}

#[derive(Deserialize)]
struct RawCombiner {
    name: String,
    #[serde(default)]
    columns: Vec<RawCol>,
}

fn load_combiners(json: &str) -> Vec<Combiner> {
    let raw: Vec<RawCombiner> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter()
        .map(|c| Combiner {
            name: c.name,
            columns: c
                .columns
                .into_iter()
                .map(|col| CombinerColumn {
                    screen_name: col.screen_name,
                    layout: col.layout,
                    count: col.count.unwrap_or(1).max(1),
                    match_wh: col.match_wh.as_ref().and_then(|m| Some((*m.first()?, *m.get(1)?))),
                    force_scale: col.force_scale,
                    comp_w: col.comp_w,
                    v_align: col.v_align,
                })
                .collect(),
        })
        .collect()
}

fn apply_screen_overrides(screens: &mut [Screen]) {
    for s in screens.iter_mut() {
        let n = normalize(&s.name);
        if n == "piccadilly" {
            s.width = 2027;
            s.height = 720;
        }
        if n == "al nassar tower" || n == "khalijiya" {
            s.width = 1536;
            s.height = 576;
        }
    }
}

/// The inventory labelled this a 6-sided column; it is 4 faces → 960×960.
fn fix_palm_trees(screens: &mut [Screen], presets: &mut [Preset], adapter: &mut [Preset], sizemaster: &mut [Preset], combiners: &mut [Combiner]) {
    for s in screens.iter_mut() {
        if is_palm_trees(&s.name) {
            if !s.aliases.iter().any(|a| a == &s.name) {
                s.aliases.push(s.name.clone());
            }
            s.name = "Marina - Palm Trees".into();
            s.width = 240;
            s.height = 960;
            if !s.aliases.iter().any(|a| normalize(a) == "marina palms full") {
                s.aliases.push("Marina Palms Full".into());
            }
        }
    }
    for list in [presets, adapter, sizemaster] {
        for p in list.iter_mut() {
            if is_palm_trees(&p.name) {
                p.name = "Marina - Palm Trees".into();
                p.width = 240;
                p.height = 960;
            }
        }
    }
    for c in combiners.iter_mut() {
        if normalize(&c.name).contains("palms") || c.columns.iter().any(|col| is_palm_trees(&col.screen_name)) {
            for col in &mut c.columns {
                col.screen_name = "Marina - Palm Trees".into();
                col.layout = "h-dup".into();
                col.count = 4;
                col.match_wh = Some((240, 960));
            }
        }
    }
}

pub fn is_palm_trees(name: &str) -> bool {
    let n = normalize(name);
    n.contains("palm tree") || n.contains("palms full") || n == "marina palms"
}

fn add_custom_screens(screens: &mut Vec<Screen>) {
    if screens.iter().any(|s| normalize(&s.name) == "tawfeer") {
        return;
    }
    let next_id = screens.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1);
    screens.push(Screen {
        id: next_id,
        name: "Tawfeer".into(),
        group: "Custom".into(),
        width: 1200,
        height: 960,
        kind: "Indoor".into(),
        location: "Tawfeer".into(),
        governorate: String::new(),
        category: "Custom".into(),
        physical: String::new(),
        custom: true,
        aliases: vec!["Tawfeer (1200x960)".into(), "Tawfeer 1200x960".into()],
        not_in_planner: false,
    });
}

fn apply_preset_overrides(presets: &mut [Preset]) {
    for p in presets.iter_mut() {
        let n = normalize(&p.name);
        if n == "piccadilly" {
            p.width = 2027;
            p.height = 720;
        }
        if n == "al nassar tower" || n == "khalijiya" {
            p.width = 1536;
            p.height = 576;
        }
        p.duration_s = duration_for(&p.name, &p.group, false);
    }
}

fn ensure_tawfeer(presets: &mut Vec<Preset>) {
    if presets.iter().any(|p| normalize(&p.name) == "tawfeer") {
        return;
    }
    presets.push(Preset { name: "Tawfeer".into(), width: 1200, height: 960, group: "Custom".into(), duration_s: DEFAULT_SPOT_SEC });
}

/// Screen Manager duration: 15 s for AL Kout / 360 Mall / Khairan Mall groups; else 10 s.
/// SizeMaster: also Warehouse Mall 15 s unless the name contains Outdoor; other screens keep duration.
pub fn duration_for(name: &str, group: &str, sizemaster: bool) -> f64 {
    let g = normalize(group);
    let n = normalize(name);
    let table = if sizemaster { SIZEMASTER_15S } else { MALL_15S };
    let hit = table.iter().any(|t| g.contains(t) || n.contains(t));
    if sizemaster && n.contains("outdoor") {
        return DEFAULT_SPOT_SEC;
    }
    if hit { 15.0 } else { DEFAULT_SPOT_SEC }
}

/// Size-mode aliases (paste name-map). **Not** applied in screen-specific mode.
pub fn size_mode_alias(name: &str) -> Option<&'static str> {
    let n = normalize(name);
    if n.contains("top gear") {
        return Some("Baitak");
    }
    if n.contains("avenues quartz") || n == "quartz" {
        return Some("Diamond");
    }
    None
}

pub fn is_size_alias_pair(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    let pair = |x: &str, y: &str| (a.contains(x) && b.contains(y)) || (a.contains(y) && b.contains(x));
    pair("top gear", "baitak") || pair("quartz", "diamond")
}

/// Format / pool cover names used when sending **by size**.
pub fn cover_export_name(size: (u32, u32), matched_names: &[String]) -> String {
    let blobs: Vec<String> = matched_names.iter().map(|n| normalize(n)).collect();
    let has = |needles: &[&str]| blobs.iter().any(|b| needles.iter().any(|n| b.contains(n)));
    match size {
        (1920, 1080) if !blobs.iter().any(|b| is_avenues_entrance(b) || is_yaal_slayel(b)) => "1.7HD".into(),
        (1536, 576) if blobs.iter().any(|b| b.contains("al salam sync")) => "Al Salam Sync".into(),
        (1536, 576) if !blobs.iter().any(|b| b.contains("thuraya")) => "2.6".into(),
        (2624, 608) if !blobs.iter().any(|b| b.contains("thuraya")) => {
            if has(&["baitak"]) {
                "Baitak".into()
            } else if has(&["top gear"]) {
                "Top Gear".into()
            } else {
                "Baitak".into()
            }
        }
        (1440, 1800) => {
            if has(&["diamond"]) {
                "Diamond".into()
            } else if has(&["quartz"]) {
                "The Avenues Quartz".into()
            } else {
                "Diamond".into()
            }
        }
        (1200, 240) => "Marina Balcony".into(),
        _ => matched_names.first().cloned().unwrap_or_else(|| format_size(size.0, size.1)),
    }
}
