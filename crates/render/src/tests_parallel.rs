//! Parallel 2D layer baking and static plates match the serial compositor.

use effectcraft_color::{BlendMode, Label};
use effectcraft_keyframe::Value;
use effectcraft_project::build::{self, Ids};
use effectcraft_project::{Comp, ItemId, ItemKind, LayerSource, Project, Solid};
use effectcraft_time::{FrameRate, Tick};

use crate::{LayerCache, NoFootage, RenderOpts, Renderer};

fn setup(w: u32, h: u32) -> (Project, ItemId, Comp) {
    let mut p = Project::default();
    p.settings.bit_depth = effectcraft_project::BitDepth::Bpc32;
    let comp = Comp::new(w, h, FrameRate::FPS_25, Tick::from_seconds_f64(2.0));
    let cid = p.add_item("Comp", Label::Sandstone, None, ItemKind::Comp(comp.clone().into()));
    (p, cid, comp)
}

fn solid(p: &mut Project, comp: &Comp, name: &str, color: [f32; 3], w: u32, h: u32) -> effectcraft_project::Layer {
    let sid = p.add_item(name, Label::Red, None, ItemKind::Solid(Solid { color, width: w, height: h, pixel_aspect: 1.0 }));
    build::layer(p, comp, name, LayerSource::Solid { item: sid }, (w, h), None)
}

fn frame(p: &Project, cid: ItemId, t: Tick, parallel: bool, cache: Option<&LayerCache>) -> crate::Image {
    let mut r = Renderer::new(p, &NoFootage, RenderOpts { parallel, motion_blur: false, ..Default::default() });
    r.cache = cache;
    r.comp_frame_cpu(cid, t)
}

fn max_diff(a: &crate::Image, b: &crate::Image) -> f32 {
    a.data.iter().zip(&b.data).map(|(p, q)| (0..4).map(|i| (p[i] - q[i]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max)
}

#[test]
fn parallel_2d_layers_match_serial() {
    let (mut p, cid, comp) = setup(240, 120);
    let mut bottom = solid(&mut p, &comp, "Plate", [0.2, 0.1, 0.05], 240, 120);
    bottom.blend_mode = BlendMode::Normal;
    let mut mid = solid(&mut p, &comp, "Graphic", [0.8, 0.2, 0.1], 80, 80);
    mid.blend_mode = BlendMode::Screen;
    if let Some(pr) = mid.props.prop_mut("transform/position") {
        pr.value = Value::Vec3([80.0, 40.0, 0.0]);
    }
    let mut top = solid(&mut p, &comp, "Title bar", [0.1, 0.4, 0.9], 240, 24);
    top.blend_mode = BlendMode::Multiply;
    if let Some(pr) = top.props.prop_mut("transform/position") {
        pr.value = Value::Vec3([120.0, 12.0, 0.0]);
    }
    let c = p.comp_mut(cid).unwrap();
    c.layers.push(top);
    c.layers.push(mid);
    c.layers.push(bottom);
    let serial = frame(&p, cid, Tick::ZERO, false, None);
    let parallel = frame(&p, cid, Tick::ZERO, true, None);
    assert_eq!((serial.width, serial.height), (parallel.width, parallel.height));
    assert!(max_diff(&serial, &parallel) < 1e-6, "parallel vs serial max {}", max_diff(&serial, &parallel));
}

#[test]
fn static_plate_reuses_bottom_layers() {
    let (mut p, cid, comp) = setup(160, 90);
    let plate = solid(&mut p, &comp, "Still", [0.3, 0.3, 0.35], 160, 90);
    let mut moving = solid(&mut p, &comp, "Bug", [1.0, 0.0, 0.0], 20, 20);
    if let Some(pr) = moving.props.prop_mut("transform/position") {
        pr.keys = vec![
            effectcraft_keyframe::Keyframe::new(Tick::ZERO, Value::Vec3([20.0, 45.0, 0.0])),
            effectcraft_keyframe::Keyframe::new(Tick::from_seconds_f64(1.0), Value::Vec3([140.0, 45.0, 0.0])),
        ];
    }
    let c = p.comp_mut(cid).unwrap();
    c.layers.push(moving);
    c.layers.push(plate);
    let cache = LayerCache::default();
    let a = frame(&p, cid, Tick::ZERO, true, Some(&cache));
    let b = frame(&p, cid, Tick::from_seconds_f64(0.5), true, Some(&cache));
    let serial_b = frame(&p, cid, Tick::from_seconds_f64(0.5), false, None);
    assert!(max_diff(&b, &serial_b) < 1e-6);
    assert!(a.get(20, 45)[0] > 0.9);
    assert!(b.get(80, 45)[0] > 0.9);
    assert!(cache.stats().hits >= 1, "the static plate or the still's layer buffer was reused");
}

#[test]
fn gaussian_blur_on_one_layer_stays_local() {
    let (mut p, cid, comp) = setup(80, 80);
    let mut l = solid(&mut p, &comp, "Blur me", [1.0, 1.0, 1.0], 40, 40);
    let spec = effectcraft_effects::find("ec.blur.gaussian").or_else(|| effectcraft_effects::find("Gaussian Blur")).expect("gaussian blur");
    let mut next = p.next_id;
    let g = effectcraft_effects::instantiate(spec, &mut Ids(&mut next), spec.name, [40.0, 40.0]);
    p.next_id = next;
    l.props.sub_mut("effects").unwrap().children.push(g.into());
    p.comp_mut(cid).unwrap().layers.push(l);
    let serial = frame(&p, cid, Tick::ZERO, false, None);
    let parallel = frame(&p, cid, Tick::ZERO, true, None);
    assert!(max_diff(&serial, &parallel) < 1e-5);
}
