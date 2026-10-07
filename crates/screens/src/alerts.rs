//! In-app Screen Suite alerts. These are shown only inside EffectCraft (banner + row highlight).
//! Nothing is emailed or sent outside the app.

use serde::{Deserialize, Serialize};

use crate::manager::ManagerSelection;
use crate::matcher::MatchReport;
use crate::sorter::SorterResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertLevel {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelAlert {
    pub id: String,
    /// booking | build | qc
    pub tab: String,
    pub level: AlertLevel,
    pub kind: String,
    pub row: String,
    pub title: String,
    pub message: String,
}

/// Every flag the panel must paint as a banner and a highlighted row.
pub fn collect_alerts(sorter: &SorterResult, manager: &ManagerSelection, matcher: &MatchReport) -> Vec<PanelAlert> {
    let mut out = Vec::new();
    for (i, f) in sorter.flags.iter().enumerate() {
        if f.answered {
            continue;
        }
        let warning = f.kind == "lowConfidence" || f.kind == "duplicate" || f.kind == "ambiguous";
        out.push(PanelAlert {
            id: if f.id.is_empty() { format!("sorter-{i}") } else { f.id.clone() },
            tab: "booking".into(),
            level: if warning { AlertLevel::Warning } else { AlertLevel::Error },
            kind: f.kind.clone(),
            row: if f.line.is_empty() { f.suggestion.clone() } else { f.line.clone() },
            title: sorter_title(&f.kind),
            message: f.message.clone(),
        });
    }
    for (i, m) in manager.matches.iter().enumerate() {
        if m.status == "ok" {
            continue;
        }
        out.push(PanelAlert {
            id: format!("manager-{i}"),
            tab: "build".into(),
            level: AlertLevel::Error,
            kind: m.status.clone(),
            row: m.asked.clone(),
            title: if m.status == "sizeMismatch" { "Size does not match".into() } else { "Screen not found".into() },
            message: m.detail.clone(),
        });
    }
    for (i, w) in manager.warnings.iter().enumerate() {
        out.push(PanelAlert {
            id: format!("stack-{i}"),
            tab: "build".into(),
            level: AlertLevel::Error,
            kind: "extraStack".into(),
            row: w.screen.clone(),
            title: "Combined size grew past the norm".into(),
            message: w.message(),
        });
    }
    for (i, r) in matcher.rows.iter().enumerate() {
        if r.status == "pass" {
            continue;
        }
        let kind = r.status.clone();
        let title = match r.status.as_str() {
            "sizeMismatch" => "Size mismatch (pre-render)",
            "missing" => "Missing composition (pre-render)",
            "flag" => "Needs a look (pre-render)",
            _ => "Size Matcher",
        };
        out.push(PanelAlert {
            id: format!("matcher-{i}"),
            tab: "qc".into(),
            level: if r.status == "flag" { AlertLevel::Warning } else { AlertLevel::Error },
            kind,
            row: r.screen.clone(),
            title: title.into(),
            message: r.detail.clone(),
        });
    }
    out
}

fn sorter_title(kind: &str) -> String {
    match kind {
        "unmatched" => "No screen matched this name".into(),
        "duplicate" => "Duplicate booking line".into(),
        "lowConfidence" => "Low-confidence match".into(),
        "ambiguous" => "More than one possible screen".into(),
        _ => "Booking flag".into(),
    }
}

pub fn tab_alert_count(alerts: &[PanelAlert], tab: &str) -> usize {
    alerts.iter().filter(|a| a.tab == tab).count()
}
