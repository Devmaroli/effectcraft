//! Live booking updates: added/removed diffs and comps built for a screen that left the list.

use serde::{Deserialize, Serialize};

use crate::combiner::names_related;
use crate::normalize::normalize;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingDiff {
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
}

impl BookingDiff {
    pub fn from_names(prev: &[String], next: &[String]) -> Self {
        let added: Vec<String> = next.iter().filter(|n| !contains_name(prev, n)).cloned().collect();
        let removed: Vec<String> = prev.iter().filter(|n| !contains_name(next, n)).cloned().collect();
        Self { added, removed }
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }

    pub fn note(&self) -> String {
        format!("Updated · {} added, {} removed", self.added.len(), self.removed.len())
    }
}

fn contains_name(list: &[String], name: &str) -> bool {
    list.iter().any(|n| normalize(n) == normalize(name))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedComp {
    pub name: String,
    pub screen: String,
    /// `None` = still asking Keep / Remove. `Some(true)` = keep (leave out of checks).
    #[serde(default)]
    pub keep: Option<bool>,
}

/// Project comps whose names still refer to a screen that left the booking.
pub fn orphaned_comps(comp_names: &[String], removed: &[String], already: &[OrphanedComp]) -> Vec<OrphanedComp> {
    let mut out = Vec::new();
    for comp in comp_names {
        for screen in removed {
            if names_related(comp, screen) || normalize(comp).contains(&normalize(screen)) {
                if already.iter().any(|o| normalize(&o.name) == normalize(comp)) {
                    continue;
                }
                if out.iter().any(|o: &OrphanedComp| normalize(&o.name) == normalize(comp)) {
                    continue;
                }
                out.push(OrphanedComp { name: comp.clone(), screen: screen.clone(), keep: None });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_added_and_removed() {
        let prev = vec!["1.7HD".into(), "Top Gear".into(), "Piccadilly".into()];
        let next = vec!["1.7HD".into(), "Piccadilly".into(), "Eye of Kuwait".into()];
        let d = BookingDiff::from_names(&prev, &next);
        assert_eq!(d.added, vec!["Eye of Kuwait"]);
        assert_eq!(d.removed, vec!["Top Gear"]);
        assert!(d.note().contains("1 added") && d.note().contains("1 removed"));
    }
}
