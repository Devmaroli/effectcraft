//! Screen Adapter tag grammar (`@logo @headline @cta @product @bg @panel`).

use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerTag {
    pub role: String,
    pub adapt: String,
}

pub fn parse_tag_text(text: &str) -> Option<LayerTag> {
    let mut role = String::new();
    let mut adapt = String::new();
    for tok in text.split_whitespace() {
        let t = tok.trim().trim_start_matches('@').trim_start_matches('.');
        if t.is_empty() {
            continue;
        }
        let n = normalize(t);
        let mapped = match n.as_str() {
            "background" | "bg" => "bg",
            "brand" | "logo" => "logo",
            "text" | "copy" | "headline" => "headline",
            "cta" => "cta",
            "hero" | "fit" | "safe" | "image" | "visual" | "product" => "product",
            "block" | "panel" => "panel",
            "keep" | "stretch" | "cover" | "contain" => {
                adapt = n;
                continue;
            }
            _ => continue,
        };
        role = mapped.into();
    }
    if role.is_empty() {
        return None;
    }
    if adapt.is_empty() {
        adapt = default_adapt(&role).into();
    }
    Some(LayerTag { role, adapt })
}

pub fn default_adapt(role: &str) -> &'static str {
    match role {
        "bg" => "cover",
        "panel" => "stretch",
        _ => "keep",
    }
}

pub fn format_tag(tag: &LayerTag) -> String {
    format!("@{} .{}", tag.role, tag.adapt)
}
