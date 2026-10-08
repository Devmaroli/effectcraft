//! Kuwait DOOH screen inventory, Size Sorter, Screen Manager matching, combiners and Size Matcher.
//!
//! L0: serde only, wasm-safe. Engine commands live in `effectcraft-engine`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod adapter;
pub mod alerts;
pub mod combiner;
pub mod fuzzy;
pub mod inventory;
pub mod library_edit;
pub mod live;
pub mod manager;
pub mod matcher;
pub mod naming;
pub mod normalize;
pub mod send;
pub mod sorter;

pub use adapter::{LayerTag, format_tag, parse_tag_text};
pub use alerts::{AlertLevel, PanelAlert, collect_alerts, tab_alert_count};
pub use combiner::{AcceptedSize, CombinerLayout, DupWarning, FacePlacement, accepted_sizes_for, active_combiners, extra_stacked, unique_source_slots};
pub use fuzzy::{SCREEN_SPECIFIC_NAME_MIN, names_match_90, similarity, token_score};
pub use inventory::{Library, STUDIO_FPS, Screen, duration_for, is_palm_trees, size_mode_alias};
pub use library_edit::{ImportPreview, LibraryIssue, MergedScreenRow, library_issues, merged_screens};
pub use live::{BookingDiff, OrphanedComp};
pub use manager::{JobMode, ManagerSelection, select_pasted};
pub use matcher::{CompProbe, MatchReport, check_comps};
pub use naming::{CompNameFrom, compose_comp_name, material_stem, unique_comp_name};
pub use normalize::{normalize, parse_pixel_size, split_paste, strip_leading_list_marker};
pub use send::{SendPreset, default_send_preset};
pub use sorter::{MatchMode, SorterFilters, SorterResult, hidden_row_count, row_size_text, screens_pill, send_names, sort_lines};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combiner::CombinerLayout;
    use crate::inventory::{is_al_salam_sync, is_avenues_entrance};
    use crate::manager::JobMode;
    use crate::sorter::unresolved_flags;

    fn lib() -> Library {
        Library::load()
    }

    #[test]
    fn studio_overrides_piccadilly_nassar_tawfeer() {
        let lib = lib();
        let p = lib.screen_by_name("Piccadilly").expect("piccadilly");
        assert_eq!((p.width, p.height), (2027, 720));
        let n = lib.screen_by_name("Al Nassar Tower").expect("nassar");
        assert_eq!((n.width, n.height), (1536, 576));
        let k = lib.screen_by_name("Khalijiya").expect("khalijiya");
        assert_eq!((k.width, k.height), (1536, 576));
        let t = lib.screen_by_name("Tawfeer").expect("tawfeer");
        assert_eq!((t.width, t.height), (1200, 960));
        assert!(t.custom);
        let sm = lib.preset_by_name("Piccadilly").expect("sm piccadilly");
        assert_eq!((sm.width, sm.height), (2027, 720));
        assert!((duration_for("AL Kout LED", "AL Kout", false) - 15.0).abs() < f64::EPSILON);
        assert!((duration_for("Jahra Prime", "Group 1", false) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn palm_trees_is_four_faces_not_six() {
        let lib = lib();
        let s = lib.screens.iter().find(|s| is_palm_trees(&s.name)).expect("palm trees screen");
        assert_eq!(s.name, "Marina - Palm Trees");
        assert_eq!((s.width, s.height), (240, 960));
        assert!(s.aliases.iter().any(|a| normalize(a) == "marina palms full"));
        assert!(!s.name.contains("6 screens"));
        let c = lib.combiners.iter().find(|c| normalize(&c.name).contains("palms")).expect("palms combiner");
        assert_eq!(c.columns.len(), 1);
        let col = &c.columns[0];
        assert_eq!(col.screen_name, "Marina - Palm Trees");
        assert_eq!(col.count, 4);
        assert_eq!(col.match_wh, Some((240, 960)));
        let lay = CombinerLayout::from_combiner(c);
        assert_eq!((lay.width, lay.height), (960, 960));
        assert!(lay.warnings.is_empty(), "configured 4-up must not warn: {:?}", lay.warnings);
        assert_eq!(lay.faces.len(), 4);
        let lefts: Vec<u32> = lay.faces.iter().map(|f| f.left.round() as u32).collect();
        assert_eq!(lefts, vec![0, 240, 480, 720]);
        let pos: Vec<[u32; 2]> = lay.faces.iter().map(|f| [f.position[0].round() as u32, f.position[1].round() as u32]).collect();
        assert_eq!(pos, vec![[120, 480], [360, 480], [600, 480], [840, 480]]);
        let sel = select_pasted(&lib, &["Marina Palms Full".into()], JobMode::BySize);
        assert!(sel.selected.iter().any(|n| is_palm_trees(n)), "{sel:?}");
    }

    #[test]
    fn al_salam_normal_duplication_does_not_warn() {
        let lib = lib();
        let c = lib.combiners.iter().find(|c| normalize(&c.name).contains("salam")).expect("al salam combiner");
        let lay = CombinerLayout::from_combiner(c);
        assert_eq!((lay.width, lay.height), (3072, 576));
        assert!(lay.warnings.is_empty(), "normal 1536×576 × 2 → 3072×576 must not warn: {:?}", lay.warnings);
        assert_eq!(lay.faces.len(), 2);
        assert_eq!(lay.faces[0].left.round() as u32, 0);
        assert_eq!(lay.faces[1].left.round() as u32, 1536);
        let accepted = accepted_sizes_for(&lib, "Al Salam Sync");
        assert!(accepted.iter().any(|a| a.width == 1536 && a.height == 576));
        assert!(accepted.iter().any(|a| a.width == 3072 && a.height == 576 && a.duplicated));
        assert!(!accepted.iter().any(|a| a.width == 3072 && a.height == 1152));
        let report = check_comps(
            &lib,
            &["Al Salam Sync".into()],
            &[CompProbe { name: "Al Salam Sync".into(), width: 3072, height: 576, duration_s: 10.0, fps: 25.0 }],
            JobMode::BySize,
        );
        assert!(report.pass, "{report:?}");
        assert!(report.mismatches.is_empty());
        assert!(report.oversized.is_empty());
        assert!(!report.duplicated_ok.is_empty());
    }

    #[test]
    fn extra_stacked_sources_warn_and_matcher_flags() {
        let lib = lib();
        let c = lib.combiners.iter().find(|c| normalize(&c.name).contains("salam")).expect("al salam combiner");
        let lay = CombinerLayout::from_unique_sources(c, 2);
        assert_eq!((lay.width, lay.height), (3072, 1152));
        assert_eq!(lay.extra_pieces, 1);
        assert_eq!(lay.faces.len(), 4);
        assert_eq!(lay.warnings.len(), 1);
        let w = &lay.warnings[0];
        assert_eq!((w.expected_width, w.expected_height), (3072, 576));
        assert_eq!((w.actual_width, w.actual_height), (3072, 1152));
        assert_eq!(w.extra_pieces, 1);
        let msg = w.message();
        assert!(msg.contains("3072×576"), "{msg}");
        assert!(msg.contains("3072×1152"), "{msg}");
        assert!(msg.contains("1 extra piece"), "{msg}");
        let report = check_comps(
            &lib,
            &["Al Salam Sync".into()],
            &[CompProbe { name: "Al Salam Sync".into(), width: 3072, height: 1152, duration_s: 10.0, fps: 25.0 }],
            JobMode::BySize,
        );
        assert!(!report.pass, "{report:?}");
        assert!(!report.oversized.is_empty(), "{report:?}");
        assert!(report.rows.iter().any(|r| r.status == "sizeMismatch"));
        assert!(report.oversized.iter().any(|m| m.contains("3072×576") && m.contains("3072×1152") && m.contains("extra")));
        let alerts = collect_alerts(&SorterResult::default(), &ManagerSelection { warnings: lay.warnings.clone(), ..ManagerSelection::default() }, &report);
        assert!(alerts.iter().any(|a| a.kind == "extraStack" && a.level == crate::alerts::AlertLevel::Error));
        assert!(alerts.iter().any(|a| a.tab == "qc" && a.kind == "sizeMismatch"));
        for c in &lib.combiners {
            let normal = CombinerLayout::from_combiner(c);
            assert!(normal.warnings.is_empty(), "{} configured layout must not warn", c.name);
            let extra = CombinerLayout::from_unique_sources(c, unique_source_slots(c) + 1);
            assert!(!extra.warnings.is_empty(), "{} extra source must warn", c.name);
            assert_eq!(extra.height, normal.height.saturating_mul(2));
            assert_eq!(extra.width, normal.width);
        }
    }

    #[test]
    fn screen_specific_is_90_percent_and_no_aliases() {
        assert!(names_match_90("Baitak", "Baitak"));
        assert!(names_match_90("Al Salam Sync", "Al Salam Syn"));
        assert!(!names_match_90("Baitak", "Top Gear"));
        assert!(!names_match_90("Diamond", "Quartz"));
        assert_eq!(size_mode_alias("Top Gear"), Some("Baitak"));
        assert_eq!(size_mode_alias("The Avenues Quartz"), Some("Diamond"));
        let lib = lib();
        let size = select_pasted(&lib, &["Top Gear".into()], JobMode::BySize);
        assert!(size.matches.iter().any(|m| m.status == "ok"), "{size:?}");
        assert!(size.selected.iter().any(|n| normalize(n).contains("top gear")), "by-size keeps the pasted name: {size:?}");
        assert!(!size.selected.iter().any(|n| normalize(n) == "baitak"), "Top Gear must not be renamed to Baitak when only Top Gear was pasted: {size:?}");
        let spec = select_pasted(&lib, &["Top Gear".into()], JobMode::ScreenSpecific);
        assert!(spec.matches.iter().all(|m| !normalize(&m.preset).contains("baitak") || m.status != "ok"), "{spec:?}");
        let diamond = select_pasted(&lib, &["Quartz".into()], JobMode::ScreenSpecific);
        assert!(diamond.matches.iter().all(|m| m.status != "ok" || !normalize(&m.preset).contains("diamond")), "{diamond:?}");
        let baitak = select_pasted(&lib, &["Baitak".into()], JobMode::ScreenSpecific);
        assert!(baitak.matches.iter().any(|m| m.status == "ok" && names_match_90(&m.preset, "Baitak")), "{baitak:?}");
    }

    #[test]
    fn sorter_cleanup_and_filters() {
        let lib = lib();
        let paste = "1. Jahra Prime\n2) Salmiya Express\n- Piccadilly\n4-Al Salam Sync\n";
        let r = sort_lines(&lib, paste, true, MatchMode::Flexible, &SorterFilters::default(), false);
        assert!(r.hits.iter().any(|h| normalize(&h.screen).contains("jahra prime")), "{r:?}");
        assert!(r.hits.iter().any(|h| is_al_salam_sync(&h.screen)), "{r:?}");
        assert!(r.paste_names_by_size.iter().any(|n| n == "1.7HD" || n.contains("Jahra") || n.contains("Salmiya")));
        let hd = sort_lines(&lib, "1.7HD", true, MatchMode::Flexible, &SorterFilters::default(), false);
        assert!(hd.hits.iter().any(|h| h.width == 1920 && h.height == 1080), "{hd:?}");
        assert!(hd.paste_names_by_size.iter().any(|n| n == "1.7HD"), "{hd:?}");
        assert!(r.paste_names_screen_specific.iter().any(|n| normalize(n).contains("piccadilly")));
        let filtered =
            sort_lines(&lib, "Piccadilly", true, MatchMode::Strict, &SorterFilters { group: "no-such-group".into(), ..SorterFilters::default() }, false);
        assert!(filtered.hits.iter().any(|h| normalize(&h.screen).contains("piccadilly")), "filters must not drop matches: {filtered:?}");
        assert!(filtered.rows.iter().any(|r| r.hidden), "filters only hide rows: {filtered:?}");
        assert!(filtered.paste_names_by_size.iter().any(|n| normalize(n).contains("piccadilly")));
        let _ = unresolved_flags(&r);
        let kept = strip_leading_list_marker("1.7HD");
        assert_eq!(kept, "1.7HD");
        assert_eq!(strip_leading_list_marker("1. Jahra Prime"), "Jahra Prime");
        assert_eq!(split_paste("A, B; C\nD", true).len(), 4);
    }

    #[test]
    fn matcher_flags_missing_and_wrong_size_not_silent() {
        let lib = lib();
        let required = vec!["1.7HD".into(), "Al Salam Sync".into(), "Missing Screen XYZ".into()];
        let comps = vec![
            CompProbe { name: "1.7HD".into(), width: 1920, height: 1080, duration_s: 10.0, fps: 25.0 },
            CompProbe { name: "Al Salam Sync".into(), width: 1920, height: 1080, duration_s: 10.0, fps: 25.0 },
        ];
        let report = check_comps(&lib, &required, &comps, JobMode::BySize);
        assert!(!report.pass);
        assert!(report.rows.iter().any(|r| r.screen.contains("Al Salam") && r.status == "sizeMismatch"));
        assert!(report.rows.iter().any(|r| r.status == "missing"));
        assert!(report.summary.contains("Not ready"));
        let alerts = collect_alerts(&SorterResult::default(), &select_pasted(&lib, &["Top Gear".into()], JobMode::ScreenSpecific), &report);
        assert!(alerts.iter().any(|a| a.kind == "missing" || a.kind == "sizeMismatch"));
        assert!(alerts.iter().any(|a| a.tab == "qc"));
    }

    #[test]
    fn by_size_groups_spring_sale_and_filters_still_send() {
        let lib = lib();
        let paste = "Jahra Prime\nSalmiya Express\nJahra Rotonda\nAl Salam Sync\nPiccadilly\nTop Gear\nAvenues Quartz\nAl Nassar Tower\n1st Ring Road\nMarina Palm Trees\nGrand Avenues Entrance";
        let r = sort_lines(&lib, paste, true, MatchMode::Flexible, &SorterFilters::default(), false);
        let names: Vec<&str> = r.paste_names_by_size.iter().map(String::as_str).collect();
        assert!(names.contains(&"1.7HD"), "{names:?}");
        assert!(names.contains(&"2.6"), "{names:?}");
        assert!(names.contains(&"Al Salam Sync"), "{names:?}");
        assert!(names.contains(&"Top Gear"), "{names:?}");
        assert!(!names.contains(&"Baitak"), "only Top Gear was pasted: {names:?}");
        assert!(names.contains(&"Avenues Quartz"), "by-size Entry uses the pasted name, not the library title: {names:?}");
        let quartz = r.rows.iter().find(|row| row.use_name.contains("Quartz")).expect("quartz");
        assert_eq!(quartz.use_name, "Avenues Quartz");
        assert!(!quartz.library_hint.is_empty(), "library name stays on the muted Pasted line: {quartz:?}");
        let salam = r.rows.iter().find(|row| row.use_name.contains("Salam")).expect("al salam");
        assert_eq!((salam.prod_width, salam.prod_height), (3072, 576), "{salam:?}");
        assert_eq!(salam.covers_label, "Left + Right", "{salam:?}");
        let marina = r.rows.iter().find(|row| row.use_name.contains("Marina") || row.covers.iter().any(|c| c.contains("Marina"))).expect("marina");
        assert_eq!((marina.prod_width, marina.prod_height), (960, 960), "{marina:?}");
        assert_eq!(marina.covers_label, "4 screens", "{marina:?}");
        assert!(names.contains(&"Grand Avenues"), "{names:?}");
        assert!(!names.iter().any(|n| *n == "1.7HD" && r.rows.iter().any(|row| row.use_name == "1.7HD" && row.covers.iter().any(|c| is_avenues_entrance(c)))));
        let hd = r.rows.iter().find(|row| row.use_name == "1.7HD").expect("1.7HD");
        assert!(hd.covers_label.contains("screen"), "{hd:?}");
        assert!(!hd.covers_label.contains("1 screens"), "{hd:?}");
        assert!(hd.count >= 2, "Jahra Prime + Salmiya (+ Rotunda) fold into 1.7HD: {hd:?}");
        assert!(!hd.covers.iter().any(|c| is_avenues_entrance(c)), "{hd:?}");
        let both = sort_lines(&lib, "Baitak\nTop Gear", true, MatchMode::Flexible, &SorterFilters::default(), false);
        assert!(both.paste_names_by_size.iter().any(|n| n.as_str() == "Baitak"), "{:?}", both.paste_names_by_size);
        assert_eq!(both.paste_names_by_size.iter().filter(|n| *n == "Baitak" || *n == "Top Gear").count(), 1);
        let outdoor = sort_lines(&lib, paste, true, MatchMode::Flexible, &SorterFilters { kind: "Outdoor".into(), ..SorterFilters::default() }, false);
        let hidden = hidden_row_count(&outdoor);
        assert!(hidden >= 1, "Grand Avenues (indoor) hidden: {:?}", outdoor.rows);
        assert!(outdoor.rows.iter().any(|row| !row.hidden && row.use_name.contains("Salam")), "Al Salam stays visible under Outdoor: {:?}", outdoor.rows);
        assert!(
            outdoor.rows.iter().any(|row| !row.hidden && (row.use_name.contains("Marina") || row.covers.iter().any(|c| c.contains("Marina")))),
            "Marina stays visible under Outdoor: {:?}",
            outdoor.rows
        );
        let visible = outdoor.rows.iter().filter(|r| !r.hidden).count();
        assert!(visible >= 6, "Outdoor still shows the production sizes: visible={visible} {:?}", outdoor.rows);
        let sent = send_names(&outdoor, false);
        assert!(sent.iter().any(|n| n == "Grand Avenues" || n.contains("Grand Avenues")), "hidden still sent: {sent:?}");
        assert_eq!(sent.len(), outdoor.paste_names_by_size.len());
    }

    #[test]
    fn fuzzy_token_and_similarity() {
        assert!(similarity("piccadilly", "piccadilly") > 0.99);
        assert!(token_score("jahra prime", "Jahra Prime LED") >= 0.9);
        assert!(names_match_90("Marina - Palm Trees", "Marina - Palm Trees"));
    }

    #[test]
    fn sheet_sync_kuwait_gate_animated_and_kept_groups() {
        let lib = lib();
        let kg = lib.screen_by_name("Kuwait Gate").expect("kuwait gate");
        assert_eq!((kg.width, kg.height), (4459, 378));
        let sm = lib.preset_by_name("Kuwait Gate").expect("sm kuwait");
        assert_eq!((sm.width, sm.height), (4459, 378));
        assert!(lib.screen_by_name("Piccadilly").is_some_and(|s| s.animated));
        assert!(lib.screen_by_name("The Gate Mall - Wall Screen").is_some_and(|s| s.animated));
        assert!(lib.screen_by_name("The Avenues Mall - Digital Screens").is_some(), "Avenues Digital kept");
        assert!(lib.screen_by_name("Yaal Mall - Digital Screens").is_none());
        assert!(lib.screen_by_name("Firdous").is_none());
        assert!(lib.screen_by_name("C.D.C").is_none());
        assert!(lib.screen_by_name("Aqarat Square (2 screens)").is_none());
        assert!(lib.presets.iter().any(|p| normalize(&p.name).contains("360") && normalize(&p.group).contains("360 mall")));
        assert!(lib.presets.iter().any(|p| normalize(&p.group).contains("al kout")));
        assert!(lib.presets.iter().any(|p| normalize(&p.group) == "dayarti"));
        assert!(lib.preset_by_name("Sultan Column").is_none());
        assert!(lib.sizemaster.iter().all(|p| normalize(&p.group) != "cinemas"));
        assert!(lib.screen_by_name("Assima Mono").is_none(), "sheet-only screens were not added");
        assert!(lib.screen_by_name("Al Nugra - Daaity").is_none());
        assert!(lib.presets.iter().any(|p| normalize(&p.group) == "print"));
        assert!(lib.presets.iter().any(|p| normalize(&p.group).contains("khairan")));
        assert!(lib.presets.iter().any(|p| normalize(&p.group).contains("warehouse")));
        let rows = merged_screens(&lib);
        let nassar = rows.iter().find(|r| normalize(&r.name) == "al nassar tower").expect("nassar");
        assert_eq!((nassar.width, nassar.height), (1536, 576));
        assert_eq!(lib.canonical_wh("Al Nassar Tower"), Some((1536, 576, nassar.group.clone())));
        let sz = lib.sizemaster.iter().find(|p| normalize(&p.name) == "al salam sync").expect("sz al salam");
        assert_eq!((sz.width, sz.height), (3072, 576), "SizeMaster keeps the combined Al Salam size");
        let thuraya_side = lib.screen_by_name("Thuraya Screen 2 (Side)").expect("thuraya side combiner member");
        assert_eq!((thuraya_side.width, thuraya_side.height), (960, 540));
    }

    #[test]
    fn animated_filter_hides_rows_but_still_sends() {
        let lib = lib();
        let paste = "Piccadilly\nJahra Prime\nAl Salam Sync";
        let shown = sort_lines(&lib, paste, true, MatchMode::Flexible, &SorterFilters { animated: true, ..SorterFilters::default() }, false);
        assert!(shown.hits.iter().any(|h| normalize(&h.screen).contains("piccadilly")), "matches must not be dropped: {shown:?}");
        assert!(shown.rows.iter().any(|r| r.hidden && !r.use_name.contains("Piccadilly")), "non-animated rows hide: {:?}", shown.rows);
        assert!(shown.rows.iter().any(|r| !r.hidden && r.use_name.contains("Piccadilly")));
        let sent = send_names(&shown, false);
        assert!(sent.iter().any(|n| n.contains("Jahra") || n == "1.7HD"), "hidden still sent: {sent:?}");
        assert_eq!(sent.len(), shown.paste_names_by_size.len());
    }
}
