//! Footage GPU/CPU fast path and layer-cache keys for footage / static precomps.

use std::sync::Arc;

use effectcraft_color::Label;
use effectcraft_keyframe::Value;
use effectcraft_project::build;
use effectcraft_project::{AlphaMode, BitDepth, Comp, Footage, FootageKind, ItemId, ItemKind, LayerSource, Project};
use effectcraft_time::{FrameRate, Tick};

use crate::cache;
use crate::{FootageSource, Image, RenderOpts, Renderer};

struct Flat;

impl FootageSource for Flat {
    fn frame(&self, _: ItemId, f: &Footage, _: Tick) -> Option<Arc<Image>> {
        Some(Arc::new(Image::filled(f.width, f.height, [0.2, 0.4, 0.8, 1.0])))
    }
    fn frame_scaled(&self, item: ItemId, footage: &Footage, t: Tick, scale: f64) -> Option<Arc<Image>> {
        let img = self.frame(item, footage, t)?;
        if scale >= 0.99 {
            return Some(img);
        }
        let (w, h) = (((footage.width as f64 * scale).round() as u32).max(1), ((footage.height as f64 * scale).round() as u32).max(1));
        Some(Arc::new(effectcraft_raster::resample(&img, w, h)))
    }
}

fn footage_comp(size: (u32, u32)) -> (Project, ItemId) {
    let mut p = Project::default();
    p.settings.bit_depth = BitDepth::Bpc32;
    let comp = Comp::new(size.0, size.1, FrameRate::FPS_30, Tick::from_seconds_f64(2.0));
    let cid = p.add_item("Comp", Label::Sandstone, None, ItemKind::Comp(comp.clone().into()));
    let f = Footage {
        path: "clip.mov".into(),
        kind: FootageKind::Video,
        width: size.0,
        height: size.1,
        pixel_aspect: 1.0,
        frame_rate: FrameRate::FPS_30,
        native_rate: None,
        duration: Tick::from_seconds_f64(2.0),
        has_video: true,
        has_audio: false,
        alpha: AlphaMode::Ignore,
        premul_color: [0.0; 3],
        loop_count: 1,
        codec: "ProRes 422 HQ".into(),
        missing: false,
        sequence: vec![],
        color_profile: None,
        ..Default::default()
    };
    let fid = p.add_item("clip", Label::Aqua, None, ItemKind::Footage(f));
    let l = build::layer(&mut p, &comp, "clip", LayerSource::Footage { item: fid }, size, None);
    p.comp_mut(cid).unwrap().layers.push(l);
    (p, cid)
}

#[test]
fn untransformed_footage_uses_the_fast_path() {
    let (p, cid) = footage_comp((64, 32));
    let r = Renderer::new(&p, &Flat, RenderOpts::default());
    let ctx = r.eval_ctx(cid, Tick::ZERO).unwrap();
    let fast = r.simple_footage_canvas(&ctx, 64, 32).expect("identity footage fills the canvas");
    assert_eq!((fast.width, fast.height), (64, 32));
    let px = fast.get(8, 8);
    assert!((px[0] - 0.2).abs() < 1e-5 && (px[2] - 0.8).abs() < 1e-5, "{px:?}");
    let full = r.comp_frame(cid, Tick::ZERO);
    let fp = full.get(8, 8);
    assert!((fp[0] - px[0]).abs() < 1e-5 && (fp[2] - px[2]).abs() < 1e-5, "fast {px:?} vs composite {fp:?}");
}

#[test]
fn half_res_fast_path_decodes_scaled() {
    let (p, cid) = footage_comp((80, 40));
    let r = Renderer::new(&p, &Flat, RenderOpts { scale: 0.5, ..Default::default() });
    let ctx = r.eval_ctx(cid, Tick::ZERO).unwrap();
    let fast = r.simple_footage_canvas(&ctx, 40, 20).expect("half-res identity");
    assert_eq!((fast.width, fast.height), (40, 20));
}

#[test]
fn rotation_drops_the_fast_path() {
    let (mut p, cid) = footage_comp((48, 48));
    p.comp_mut(cid).unwrap().layers[0].props.prop_mut("transform/rotation").unwrap().value = Value::Scalar(15.0);
    let r = Renderer::new(&p, &Flat, RenderOpts::default());
    let ctx = r.eval_ctx(cid, Tick::ZERO).unwrap();
    assert!(r.simple_footage_canvas(&ctx, 48, 48).is_none());
}

#[test]
fn raw_footage_is_not_duplicated_in_the_layer_cache() {
    let (p, cid) = footage_comp((16, 8));
    let r = Renderer::new(&p, &Flat, RenderOpts::default());
    let ctx = r.eval_ctx(cid, Tick::ZERO).unwrap();
    let layer = &ctx.comp.layers[0];
    assert!(cache::layer_key(&ctx, layer, 1.0, false, false).is_none());
}

#[test]
fn static_precomp_key_is_stable_across_time() {
    let mut p = Project::default();
    p.settings.bit_depth = BitDepth::Bpc32;
    let mut inner = Comp::new(20, 10, FrameRate::FPS_30, Tick::from_seconds_f64(2.0));
    let sid =
        p.add_item("S", Label::Red, None, ItemKind::Solid(effectcraft_project::Solid { color: [1.0, 0.0, 0.0], width: 20, height: 10, pixel_aspect: 1.0 }));
    let sl = build::layer(&mut p, &inner, "S", LayerSource::Solid { item: sid }, (20, 10), None);
    inner.layers.push(sl);
    let iid = p.add_item("Inner", Label::Sandstone, None, ItemKind::Comp(inner.into()));
    let outer = Comp::new(20, 10, FrameRate::FPS_30, Tick::from_seconds_f64(2.0));
    let cid = p.add_item("Outer", Label::Sandstone, None, ItemKind::Comp(outer.clone().into()));
    let pre = build::layer(&mut p, &outer, "Inner", LayerSource::Comp { item: iid }, (20, 10), None);
    p.comp_mut(cid).unwrap().layers.push(pre);
    let r = Renderer::new(&p, &Flat, RenderOpts::default());
    let a = cache::layer_key(&r.eval_ctx(cid, Tick::ZERO).unwrap(), &p.comp(cid).unwrap().layers[0], 1.0, false, false);
    let b = cache::layer_key(&r.eval_ctx(cid, FrameRate::FPS_30.tick_of(12)).unwrap(), &p.comp(cid).unwrap().layers[0], 1.0, false, false);
    assert!(a.is_some() && a == b, "static precomp {a:?} vs {b:?}");
}
