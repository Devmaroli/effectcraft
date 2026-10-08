//! Comp naming for Screen Manager Apply: prefix+suffix, or the material file's stem.
//! Original footage items are never renamed.

use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CompNameFrom {
    #[default]
    PrefixScreen,
    Material,
}

/// Build a composition name. Empty prefix/suffix are omitted (no extra underscores).
pub fn compose_comp_name(from: CompNameFrom, screen: &str, material_stem: Option<&str>, prefix: &str, suffix: &str) -> String {
    match from {
        CompNameFrom::Material => {
            let stem = material_stem.filter(|s| !s.is_empty()).unwrap_or(screen);
            join_name(prefix, stem, suffix)
        }
        CompNameFrom::PrefixScreen => join_name(prefix, screen, suffix),
    }
}

pub fn join_name(prefix: &str, screen: &str, suffix: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let p = prefix.trim();
    let s = screen.trim();
    let x = suffix.trim();
    if !p.is_empty() {
        parts.push(p);
    }
    if !s.is_empty() {
        parts.push(s);
    }
    if !x.is_empty() {
        parts.push(x);
    }
    if parts.is_empty() { "Comp".into() } else { parts.join("_") }
}

/// Footage path or Project-panel name → stem, never touching the original file.
pub fn material_stem(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    match base.rfind('.') {
        Some(i) if i > 0 => base[..i].to_string(),
        _ => base.to_string(),
    }
}

/// If `wanted` already exists in `taken` (case-insensitive), append `_2`, `_3`, …
/// and report whether a clash was resolved.
pub fn unique_comp_name(wanted: &str, taken: &[String]) -> (String, bool) {
    let exists = |n: &str| taken.iter().any(|t| normalize(t) == normalize(n));
    if !exists(wanted) {
        return (wanted.to_string(), false);
    }
    for n in 2..=99 {
        let candidate = format!("{wanted}_{n}");
        if !exists(&candidate) {
            return (candidate, true);
        }
    }
    (format!("{wanted}_x"), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_stem_strips_extension_not_folders() {
        assert_eq!(material_stem("TopGear_final_v2.mov"), "TopGear_final_v2");
        assert_eq!(material_stem(r"C:\job\Jahra Prime EN.mp4"), "Jahra Prime EN");
        assert_eq!(material_stem("noext"), "noext");
    }

    #[test]
    fn prefix_suffix_preview() {
        assert_eq!(compose_comp_name(CompNameFrom::PrefixScreen, "1.7HD", None, "SpringSale", "EN"), "SpringSale_1.7HD_EN");
        assert_eq!(compose_comp_name(CompNameFrom::PrefixScreen, "Piccadilly", None, "", ""), "Piccadilly");
        assert_eq!(compose_comp_name(CompNameFrom::Material, "Top Gear", Some("TopGear_final_v2"), "SpringSale", "EN"), "SpringSale_TopGear_final_v2_EN");
    }

    #[test]
    fn clash_gets_numeric_suffix() {
        let taken = vec!["TopGear_final_v2".into()];
        let (n, clash) = unique_comp_name("TopGear_final_v2", &taken);
        assert_eq!(n, "TopGear_final_v2_2");
        assert!(clash);
    }
}
