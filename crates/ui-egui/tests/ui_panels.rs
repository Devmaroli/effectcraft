//! Headless checks for the M13.6 panels: each opens from the Window menu as a real panel and
//! registers its automation ids; the Footage panel's buttons edit into the comp; the Progress
//! panel lists and cancels a job; Lumetri Scopes switch scope through their dropdown.

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use effectcraft_ui_egui::dock::{DockNode, PanelKind, SplitSize};
use egui::{Event, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, EffectcraftApp> {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Panels", "width": 320, "height": 180, "duration": 4})).unwrap();
    s.execute("layer.newSolid", json!({"name": "Plate", "color": "#406080"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    h
}

fn open(h: &mut Harness<'_, EffectcraftApp>, panel: &str) {
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.panel", json!({"panel": panel})).unwrap();
    h.run_steps(3);
}

fn click(h: &mut Harness<'_, EffectcraftApp>, id: &str) {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}")).clone();
    let p: Pos2 = pos2(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

#[test]
fn window_menu_opens_the_new_panels() {
    let mut h = harness();
    for (name, kind, auto) in [
        ("lumetriScopes", PanelKind::LumetriScopes, "scopes.kind"),
        ("footage", PanelKind::Footage, "footage.empty"),
        ("mediaBrowser", PanelKind::MediaBrowser, "mediaBrowser.path"),
        ("metadata", PanelKind::Metadata, "metadata.projectComment"),
        ("progress", PanelKind::Progress, ""),
        ("contentAwareFill", PanelKind::ContentAwareFill, "contentFill.method"),
        ("screenSuite", PanelKind::ScreenSuite, "screenSuite.tab.booking"),
    ] {
        open(&mut h, name);
        assert!(h.state().ui.dock.contains(kind) || h.state().ui.floating.iter().any(|f| f.panels.contains(&kind)), "{name} shown");
        if !auto.is_empty() {
            assert!(h.state().auto.find(auto).is_some(), "{name}: {auto}");
        }
    }
}

#[test]
fn scopes_switch_kind() {
    let mut h = harness();
    open(&mut h, "lumetriScopes");
    assert!(h.state().auto.find("scopes.plot").is_some());
    h.state_mut().ui.scopes.scope = "vectorscopeYuv".into();
    h.run_steps(2);
    assert_eq!(h.state().auto.find("scopes.plot").unwrap().label, "Vectorscope YUV");
    h.state_mut().ui.scopes.scope = "histogram".into();
    h.run_steps(2);
    assert_eq!(h.state().auto.find("scopes.kind").unwrap().label, "Histogram");
}

#[test]
fn footage_panel_buttons_edit_into_the_comp() {
    let mut h = harness();
    let item = h.state().session.project.items.values().find(|i| matches!(i.kind, effectcraft_engine::project::ItemKind::Solid(_))).unwrap().id.0;
    h.state_mut().session.execute("footage.open", json!({"item": item})).unwrap();
    h.run_steps(4);
    assert!(h.state().ui.dock.contains(PanelKind::Footage) || h.state().ui.floating.iter().any(|f| f.panels.contains(&PanelKind::Footage)));
    assert!(h.state().auto.find("footage.overlayEdit").is_some());
    let before = h.state().session.active_comp().unwrap().layers.len();
    click(&mut h, "footage.overlayEdit");
    assert_eq!(h.state().session.active_comp().unwrap().layers.len(), before + 1);
    click(&mut h, "footage.rippleInsertEdit");
    assert!(h.state().session.active_comp().unwrap().layers.len() >= before + 2);
}

#[test]
fn progress_panel_cancels_a_job() {
    let mut h = harness();
    h.state_mut()
        .session
        .spawn_task("test", "Long analysis", false, |ctl| {
            while ctl.progress(1, 2) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err("cancelled".into())
        })
        .unwrap();
    open(&mut h, "progress");
    let id = h.state().session.jobs()[0].id.clone();
    assert!(h.state().auto.find(&format!("progress.job.{id}")).is_some());
    click(&mut h, &format!("progress.cancel.{id}"));
    h.state_mut().session.wait_jobs();
    h.run_steps(2);
    assert!(h.state().session.jobs().is_empty());
    assert_eq!(h.state().session.job_log.last().unwrap().status, "cancelled");
}

fn click_at(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

fn hover(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.run_steps(3);
}

/// Issue #191: a workspace saved with Save as New Workspace is listed in Window ▸ Workspace
/// (below the built-ins, where the submenu used to be cut off) and choosing it there brings its
/// layout back.
#[test]
fn saved_workspace_is_listed_in_the_workspace_menu() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.workspace", json!({"name": "Minimal"})).unwrap();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.saveWorkspaceAs", json!({"name": "My Layout"})).unwrap();
    h.run_steps(3);
    let saved = h.state().ui.dock.clone();
    assert_eq!(h.state().saved_workspace_names(), ["My Layout"]);
    let cx = effectcraft_engine::menus::DynCtx { workspace: Some("My Layout"), saved_workspaces: &["My Layout".to_string()] };
    let (entries, _) = effectcraft_engine::menus::dynamic(&h.state().session, "savedWorkspaces", &cx);
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].label.as_str(), entries[0].command.as_str(), &entries[0].params), ("My Layout", "window.workspace", &json!({"name": "My Layout"})));
    // Leave it, then pick it from the in-window menu bar.
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.workspace", json!({"name": "Default"})).unwrap();
    h.run_steps(3);
    assert_ne!(h.state().ui.dock, saved);
    click(&mut h, "menu.Window");
    let ws = h.query_by_label(" Workspace ⏵").expect("Window ▸ Workspace").rect();
    hover(&mut h, ws.center());
    let entry = h.state().auto.find("menu.savedWorkspaces.0").expect("the saved workspace is listed").clone();
    assert_eq!(entry.label, "My Layout");
    let at = h.query_by_label(" My Layout").expect("the saved workspace entry").rect().center();
    // Into the submenu, then down it (as a pointer moves).
    hover(&mut h, pos2(at.x, ws.center().y));
    for k in 1..=10 {
        hover(&mut h, pos2(at.x, ws.center().y + (at.y - ws.center().y) * k as f32 / 10.0));
    }
    click_at(&mut h, at);
    assert_eq!(h.state().ui.workspace, "My Layout");
    assert_eq!(h.state().ui.dock, saved, "its layout came back");
}

/// Headless look at the panels (wgpu offscreen; needs a GPU adapter). Run with
/// `PANELS_SNAPSHOT=/abs/dir cargo test -p effectcraft-ui-egui --test ui_panels -- --ignored`.
#[test]
#[ignore]
fn panels_snapshot() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let solid = s.project.items.values().find(|i| matches!(i.kind, effectcraft_engine::project::ItemKind::Solid(_))).map(|i| i.id.0);
    let mut app = Some(EffectcraftApp::new(s));
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_eframe(|_| app.take().expect("app"));
    h.run_steps(3);
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.workspace", json!({"name": "All Panels"})).unwrap();
    h.run_steps(3);
    let dir = std::env::var("PANELS_SNAPSHOT").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-out").into());
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(id) = solid {
        h.state_mut().session.execute("footage.open", json!({"item": id})).unwrap();
    }
    for (panel, scope) in [
        ("lumetriScopes", "waveformRgb"),
        ("lumetriScopes", "vectorscopeYuv"),
        ("lumetriScopes", "histogram"),
        ("lumetriScopes", "paradeRgb"),
        ("contentAwareFill", ""),
        ("metadata", ""),
        ("progress", ""),
        ("mediaBrowser", ""),
        ("footage", ""),
    ] {
        if !scope.is_empty() {
            h.state_mut().ui.scopes.scope = scope.into();
        }
        open(&mut h, panel);
        h.run_steps(3);
        let img = h.render().expect("render");
        img.save(format!("{dir}/{panel}{scope}.png")).unwrap();
    }
}

#[test]
fn screen_suite_booking_build_and_extra_stack_warning() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Plate", "width": 320, "height": 180, "duration": 4, "frameRate": 25.0})).unwrap();
    s.execute("screen.sorter.sort", json!({"paste": "1.7HD\nAl Salam Sync\nPiccadilly\nBaitak\nDiamond\nGhost Screen That Does Not Exist", "cleanup": true}))
        .unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1680.0, 1020.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    open(&mut h, "screenSuite");
    h.state_mut().ui.maximized = Some(PanelKind::ScreenSuite);
    h.run_steps(4);
    assert!(h.state().auto.find("screenSuite.tab.booking").is_some());
    assert!(h.state().auto.find("screenSuite.paste").is_some());
    assert!(h.state().auto.find("screenSuite.sort").is_some());
    assert!(h.state().auto.find("screenSuite.alert.banner").is_some(), "unmatched booking flags must show an in-app banner");
    assert!(h.state().auto.find("screenSuite.tab.booking.badge").is_some() || h.state().session.state.screen.sorter.flags.is_empty());

    h.state_mut().session.execute("screen.sorter.send", json!({"screenSpecific": false})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().session.state.screen.tab, "build");
    assert!(h.state().session.state.screen.manager.show_selected_only);
    assert!(h.state().auto.find("screenSuite.apply").is_some() || h.state().auto.find("screenSuite.tab.build").is_some());
    assert!(h.state().auto.find("screenSuite.prefix").is_some(), "Screen Manager must show Prefix in both modes");
    assert!(h.state().auto.find("screenSuite.suffix").is_some(), "Screen Manager must show Suffix in both modes");
    assert!(h.state().auto.find("screenSuite.editLibrary").is_some());

    h.state_mut().session.execute("comp.new", json!({"name": "Al Salam Sync A", "width": 1536, "height": 576, "frameRate": 25.0, "open": false})).unwrap();
    h.state_mut().session.execute("comp.new", json!({"name": "Al Salam Sync B", "width": 1536, "height": 576, "frameRate": 25.0, "open": false})).unwrap();
    h.state_mut().session.execute("screen.manager.select", json!({"names": ["Al Salam Sync"], "jobMode": "bySize"})).unwrap();
    h.state_mut().session.execute("screen.manager.combine", json!({"combiner": "Al_Salam_Sync"})).unwrap();
    h.state_mut().session.execute("screen.matcher.check", json!({"names": ["Al Salam Sync"]})).unwrap();
    h.state_mut().session.execute("screen.suite.tab", json!({"tab": "qc"})).unwrap();
    h.run_steps(4);
    assert!(!h.state().session.state.screen.manager.warnings.is_empty());
    assert!(!h.state().session.state.screen.matcher.oversized.is_empty());
    assert_eq!(h.state().session.state.screen.tab, "qc");
    assert!(h.state().auto.find("screenSuite.matcher.check").is_some());
    assert!(h.state().auto.find("screenSuite.sendAnyway").is_some(), "Send anyway… when Size Matcher has not passed");
    assert!(h.state().auto.find("screenSuite.alert.banner").is_some(), "extra-stack and Size Matcher flags must stay as an in-app banner");
    assert!(h.state().auto.find("screenSuite.tab.qc.badge").is_some() || h.state().auto.find("screenSuite.tab.build.badge").is_some());
    assert!(
        h.state().auto.previous.iter().chain(h.state().auto.elements.iter()).any(|e| e.id.starts_with("screenSuite.alert.row.")),
        "affected rows must be highlighted"
    );
}

fn mockup_dock() -> DockNode {
    mockup_dock_width(520.0)
}

fn mockup_dock_width(panel: f32) -> DockNode {
    DockNode::Split {
        vertical: false,
        size: SplitSize::FixedB(panel),
        a: Box::new(DockNode::Split {
            vertical: true,
            size: SplitSize::Ratio(0.58),
            a: Box::new(DockNode::Split {
                vertical: false,
                size: SplitSize::FixedA(260.0),
                a: Box::new(DockNode::Tabs { panels: vec![PanelKind::Project, PanelKind::EffectControls], active: 0 }),
                b: Box::new(DockNode::Tabs { panels: vec![PanelKind::Composition, PanelKind::Layer], active: 0 }),
            }),
            b: Box::new(DockNode::Tabs { panels: vec![PanelKind::Timeline, PanelKind::RenderQueue], active: 0 }),
        }),
        b: Box::new(DockNode::Tabs { panels: vec![PanelKind::ScreenSuite, PanelKind::Properties, PanelKind::Preview], active: 0 }),
    }
}

/// Comparison shots for the approved v2 mockups (steps 2, 2b, 4b, 6, library1).
/// `SCREEN_SUITE_V2_SNAP=/abs/dir cargo test -p effectcraft-ui-egui --test ui_panels -- --ignored screen_suite_v2`
#[test]
#[ignore]
fn screen_suite_v2_snapshots() {
    let dir = std::env::var("SCREEN_SUITE_V2_SNAP").unwrap_or_else(|_| "/opt/cursor/artifacts/screenshots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let paste = "Jahra Prime\nSalmiya Express\nJahra Rotonda\nAl Salam Sync\nPiccadilly\nTop Gear\nAvenues Quartz\nAl Nassar Tower\n1st Ring Road\nMarina Palm Trees\nGrand Avenues Entrance";
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "SpringSale_Master", "width": 1920, "height": 1080, "duration": 10, "frameRate": 25.0})).unwrap();
    s.execute("layer.newSolid", json!({"name": "LOGO", "color": "#ffffff", "width": 160, "height": 48})).unwrap();
    s.execute("layer.newText", json!({"name": "Headline SPRING SALE", "text": "SPRING SALE", "size": 72})).unwrap();
    s.execute("layer.newText", json!({"name": "CTA Shop now", "text": "Shop now", "size": 28})).unwrap();
    s.execute("layer.newSolid", json!({"name": "Product shot", "color": "#6b4cff", "width": 320, "height": 180})).unwrap();
    s.execute("screen.suite.naming", json!({"jobName": "Spring Sale", "prefix": "SpringSale", "suffix": "EN, AR"})).unwrap();
    s.execute("screen.sorter.sort", json!({"paste": paste, "cleanup": true, "kind": "Outdoor", "matchMode": "flexible"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1680.0, 1020.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    h.state_mut().ui.dock = mockup_dock();
    h.state_mut().ui.maximized = None;
    h.state_mut().ui.focused = PanelKind::ScreenSuite;
    h.run_steps(6);
    h.render().expect("render").save(format!("{dir}/v2-step2-size-sorter.png")).unwrap();

    h.state_mut().ui.dock = mockup_dock_width(340.0);
    h.state_mut().ui.maximized = None;
    h.run_steps(6);
    h.render().expect("render").save(format!("{dir}/v2-step2-size-sorter-narrow.png")).unwrap();

    h.state_mut().session.execute("screen.sorter.send", json!({"screenSpecific": false})).unwrap();
    h.state_mut().session.execute("screen.manager.apply", json!({"prefix": "SpringSale", "suffix": "EN"})).unwrap();
    h.state_mut()
        .session
        .execute(
            "screen.sorter.sort",
            json!({"paste": "Jahra Prime\nSalmiya Express\nJahra Rotonda\nAl Salam Sync\nPiccadilly\nEye of Kuwait\nAvenues Quartz\nAl Nassar Tower\n1st Ring Road\nMarina Palm Trees\nGrand Avenues Entrance", "cleanup": true}),
        )
        .unwrap();
    h.state_mut().session.execute("screen.suite.tab", json!({"tab": "booking"})).unwrap();
    h.state_mut().ui.dock = mockup_dock();
    h.state_mut().ui.maximized = None;
    h.run_steps(4);
    h.render().expect("render").save(format!("{dir}/v2-step2b-live-update.png")).unwrap();

    h.state_mut().session.execute("screen.sorter.send", json!({"screenSpecific": true})).unwrap();
    h.state_mut().session.execute("screen.suite.naming", json!({"nameFrom": "prefixScreen", "prefix": "SpringSale", "suffix": "EN, AR"})).unwrap();
    h.state_mut().session.execute("screen.suite.tab", json!({"tab": "build"})).unwrap();
    h.state_mut().ui.dock = mockup_dock();
    h.state_mut().ui.maximized = None;
    h.run_steps(4);
    h.render().expect("render").save(format!("{dir}/v2-step4b-naming.png")).unwrap();

    h.state_mut().session.execute("comp.new", json!({"name": "Al Salam Sync A", "width": 1536, "height": 576, "frameRate": 25.0, "open": false})).unwrap();
    h.state_mut().session.execute("comp.new", json!({"name": "Al Salam Sync B", "width": 1536, "height": 576, "frameRate": 25.0, "open": false})).unwrap();
    h.state_mut().session.execute("screen.manager.select", json!({"names": ["Al Salam Sync"], "jobMode": "bySize"})).unwrap();
    h.state_mut().session.execute("screen.manager.combine", json!({"combiner": "Al_Salam_Sync"})).unwrap();
    h.state_mut().session.execute("screen.matcher.check", json!({})).unwrap();
    h.state_mut().session.execute("screen.suite.tab", json!({"tab": "qc"})).unwrap();
    h.state_mut().ui.dock = mockup_dock();
    h.state_mut().ui.maximized = None;
    h.run_steps(4);
    h.render().expect("render").save(format!("{dir}/v2-step6-size-matcher.png")).unwrap();

    h.state_mut().session.execute("screen.library.open", json!({})).unwrap();
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.panel", json!({"panel": "screenLibrary", "float": true})).unwrap();
    h.state_mut().ui.dock = mockup_dock();
    h.state_mut().ui.maximized = None;
    h.run_steps(4);
    h.render().expect("render").save(format!("{dir}/v2-library1-editor.png")).unwrap();
}

#[test]
fn screen_manager_apply_places_source_through_the_gui() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Honor_400_EN.mp4", "width": 1920, "height": 1080, "duration": 8, "frameRate": 25.0})).unwrap();
    s.execute("layer.newSolid", json!({"name": "Artwork", "color": "#e23d28", "width": 1920, "height": 1080})).unwrap();
    let src = s.active_comp_id().expect("source");
    s.state.project_selection = vec![src];
    s.execute("screen.manager.select", json!({"names": ["Al Salam Sync", "Piccadilly"], "jobMode": "bySize"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1680.0, 1020.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    open(&mut h, "screenSuite");
    h.state_mut().ui.maximized = Some(PanelKind::ScreenSuite);
    h.state_mut().session.execute("screen.suite.tab", json!({"tab": "build"})).unwrap();
    h.run_steps(4);
    let src_el = h.state().auto.find("screenSuite.source").expect("Source: line in Screen Manager");
    assert!(src_el.label.contains("Honor_400_EN"), "{}", src_el.label);
    click(&mut h, "screenSuite.apply");
    h.run_steps(8);
    let src = h.state().session.active_comp_id().or_else(|| h.state().session.state.project_selection.first().copied());
    let src = src.expect("source still present");
    let created: Vec<_> =
        h.state().session.project.items.values().filter(|i| i.id != src && i.as_comp().is_some_and(|c| c.width != 1920 || c.height != 1080)).cloned().collect();
    assert!(created.len() >= 2, "expected screen comps, got {}", created.len());
    let mut faces = 0;
    for item in &created {
        let Some(c) = item.as_comp() else { continue };
        let face = matches!((c.width, c.height), (1536, 576) | (2027, 720));
        if !face {
            assert!(!c.layers.is_empty(), "combiner {} was created empty", item.name);
            continue;
        }
        faces += 1;
        assert!(!c.layers.is_empty(), "{} was created empty", item.name);
        let refs_src = c.layers.iter().any(|l| match l.source {
            effectcraft_project::LayerSource::Comp { item } | effectcraft_project::LayerSource::Footage { item } => item == src,
            _ => false,
        });
        assert!(refs_src, "{} has no layer referencing the source", item.name);
        assert!((c.frame_rate.as_f64() - 25.0).abs() < 0.01);
    }
    assert!(faces >= 2, "expected Al Salam + Piccadilly face comps, got {faces}");
}

#[test]
fn screen_manager_apply_disabled_without_source() {
    let mut s = Session::default();
    s.execute("screen.manager.select", json!({"names": ["Piccadilly"], "jobMode": "bySize"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    open(&mut h, "screenSuite");
    h.state_mut().ui.maximized = Some(PanelKind::ScreenSuite);
    h.state_mut().session.execute("screen.suite.tab", json!({"tab": "build"})).unwrap();
    h.run_steps(4);
    assert!(h.state().auto.find("screenSuite.source.missing").is_some());
    assert!(h.state().auto.find("screenSuite.apply").is_some());
    let n = h.state().session.project.items.len();
    click(&mut h, "screenSuite.apply");
    h.run_steps(4);
    assert_eq!(h.state().session.project.items.len(), n, "disabled Apply must not create empty comps");
}

#[test]
fn window_menu_fits_1080p_and_screen_suite_is_near_the_top() {
    let mut h = Harness::builder().with_size(egui::vec2(1920.0, 1080.0)).build_eframe(|_| {
        let mut s = Session::default();
        s.execute("comp.new", json!({"name": "Menu", "width": 320, "height": 180, "duration": 2})).unwrap();
        EffectcraftApp::new(s)
    });
    h.run_steps(4);
    click(&mut h, "menu.Window");
    h.run_steps(4);
    let suite = h.state().auto.find("menu.window.screenSuite").expect("Screen Suite in the open Window menu").clone();
    let lib = h.state().auto.find("menu.window.screenLibrary").expect("Screen Library in the open Window menu").clone();
    let popup = h.state().auto.find("menu.Window.popup").expect("Window menu popup").clone();
    let popup_bottom = popup.rect[1] + popup.rect[3];
    assert!(popup_bottom <= 1080.0 + 1.0, "Window menu popup clipped at y={popup_bottom}");
    assert!(popup.rect[3] > 500.0, "Window menu should use available 1080p height, got h={}", popup.rect[3]);
    let suite_bottom = suite.rect[1] + suite.rect[3];
    let lib_bottom = lib.rect[1] + lib.rect[3];
    assert!(suite_bottom <= 1080.0, "Screen Suite clipped at y={suite_bottom}");
    assert!(lib_bottom <= 1080.0, "Screen Library clipped at y={lib_bottom}");
    assert!(suite.rect[1] < 1080.0 * 0.20, "Screen Suite y={} is not in the top 20% of 1080p", suite.rect[1]);
    assert!(lib.rect[1] < 1080.0 * 0.20, "Screen Library y={} is not in the top 20% of 1080p", lib.rect[1]);
    let mut rows: Vec<_> = h
        .state()
        .auto
        .previous
        .iter()
        .chain(h.state().auto.elements.iter())
        .filter(|e| e.id.starts_with("menu.window.") || e.id.starts_with("menu.submenu."))
        .map(|e| (e.id.clone(), e.rect[1]))
        .collect();
    rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    rows.dedup_by(|a, b| a.0 == b.0);
    let n = rows.len().max(1);
    let suite_i = rows.iter().position(|(id, _)| id == "menu.window.screenSuite").expect("Screen Suite in row list");
    let lib_i = rows.iter().position(|(id, _)| id == "menu.window.screenLibrary").expect("Screen Library in row list");
    assert!(suite_i * 5 < n, "Screen Suite index {suite_i} of {n} is not < 20%: {rows:?}");
    assert!(lib_i * 5 < n, "Screen Library index {lib_i} of {n} is not < 20%: {rows:?}");
    assert!(h.state().auto.find("menu.window.renderQueue").is_some(), "Render Queue must still be in the Window menu");
}
