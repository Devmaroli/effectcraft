//! EncodeCraft send presets: Better Res vs Installation (rules), plus one-off Approval Res / Sultan.

use serde::{Deserialize, Serialize};

use crate::inventory::is_avenues_entrance;
use crate::normalize::normalize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SendPreset {
    #[default]
    Installation,
    BetterRes,
    ApprovalRes,
    Sultan,
}

impl SendPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Installation => "Installation",
            Self::BetterRes => "Better Res",
            Self::ApprovalRes => "Approval Res",
            Self::Sultan => "Sultan",
        }
    }

    pub fn bitrate(self) -> &'static str {
        match self {
            Self::Installation => "5–6 Mbps",
            Self::BetterRes => "7–8 Mbps",
            Self::ApprovalRes => "1 Mbps · with audio",
            Self::Sultan => "2–3 Mbps · no audio",
        }
    }

    /// EncodeCraft `preset_id`. One-off studio presets are never chosen by the rules.
    pub fn preset_id(self) -> &'static str {
        match self {
            Self::Installation => "studio.installation",
            Self::BetterRes => "studio.better-res",
            Self::ApprovalRes => "studio.approval-res",
            Self::Sultan => "studio.sultan",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match normalize(id).as_str() {
            "studio.installation" | "installation" => Some(Self::Installation),
            "studio.better-res" | "studio.betterres" | "better res" | "betterres" => Some(Self::BetterRes),
            "studio.approval-res" | "approval res" | "approvalres" => Some(Self::ApprovalRes),
            "studio.sultan" | "sultan" => Some(Self::Sultan),
            _ => None,
        }
    }

    pub fn is_one_off(self) -> bool {
        matches!(self, Self::ApprovalRes | Self::Sultan)
    }
}

/// Better Res when the screen is ultra-wide (>5:1) or on the always-Better-Res list.
pub fn default_send_preset(name: &str, width: u32, height: u32) -> SendPreset {
    if always_better_res(name, width, height) { SendPreset::BetterRes } else { SendPreset::Installation }
}

pub fn always_better_res(name: &str, width: u32, height: u32) -> bool {
    let n = normalize(name);
    if ultra_wide(width, height) {
        return true;
    }
    if n.contains("piccadilly") || n.contains("al salam sync") || n.contains("eye of kuwait") {
        return true;
    }
    if n.contains("1st ring") || n.contains("first ring") {
        return true;
    }
    if n.contains("al nassar tower vertical") || n.contains("nassar") && n.contains("vertical") {
        return true;
    }
    if is_avenues_entrance(name) || n.contains("grand avenues") || n.contains("grand plaza") || n.contains("the mall") {
        return true;
    }
    if n.contains("avenues gate") {
        return true;
    }
    false
}

pub fn ultra_wide(width: u32, height: u32) -> bool {
    height > 0 && (width as f32 / height as f32) > 5.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_sale_presets() {
        assert_eq!(default_send_preset("1.7HD", 1920, 1080), SendPreset::Installation);
        assert_eq!(default_send_preset("Piccadilly", 6080, 720), SendPreset::BetterRes);
        assert_eq!(default_send_preset("Al Salam Sync", 3072, 576), SendPreset::BetterRes);
        assert_eq!(default_send_preset("Eye of Kuwait", 7560, 2100), SendPreset::BetterRes);
        assert_eq!(default_send_preset("1st Ring Road", 1008, 5184), SendPreset::BetterRes);
        assert_eq!(default_send_preset("Grand Avenues Entrance", 1920, 1080), SendPreset::BetterRes);
        assert_eq!(default_send_preset("Top Gear", 2624, 608), SendPreset::Installation);
        assert_eq!(default_send_preset("Marina - Palm Trees", 960, 960), SendPreset::Installation);
        assert!(SendPreset::ApprovalRes.is_one_off());
        assert!(SendPreset::Sultan.is_one_off());
        assert!(!SendPreset::BetterRes.is_one_off());
    }
}
