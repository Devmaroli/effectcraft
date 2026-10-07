//! Digital-out-of-home preview playback benchmark (`effectcraft-cli bench --dooh`).
//!
//! Builds 25 fps compositions at the four canvas sizes a DOOH motion designer actually uses,
//! with a video plate, stills, a title, common effects and blend modes. Reports achieved fps
//! against the 25 fps target for serial vs parallel compositing, cold vs warm layer cache, and
//! Full / Half / Quarter preview resolution.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use effectcraft_color::{BlendMode, Label};
use effectcraft_keyframe::{Justify, Keyframe, TextDoc, Value};
use effectcraft_project::build::{self, Ids};
use effectcraft_project::{AlphaMode, Comp, Footage, FootageKind, ItemId, ItemKind, Layer, LayerSource, Project, Solid};
use effectcraft_raster::Image;
use effectcraft_render::{FootageSource, LayerCache, RenderOpts, Renderer};
use effectcraft_time::{FrameRate, Tick};
use rayon::prelude::*;
use serde_json::{Value as Json, json};

/// (name, width, height) — 16:9 HD, a 5:1 LED strip, a wide LED ribbon, a square kiosk.
pub const SIZES: [(&str, u32, u32); 4] =
    [("hd-1920x1080", 1920, 1080), ("led-3072x576", 3072, 576), ("ribbon-6080x720", 6080, 720), ("kiosk-960x960", 960, 960)];

/// Target preview rate for the harness (PAL / European DOOH).
pub const TARGET_FPS: f64 = 25.0;

/// Procedural video + stills (no files on disk). Video pixels change every frame; stills do not.
pub struct DoohMedia {
    stills: Mutex<HashMap<u64, Arc<Image>>>,
}

impl Default for DoohMedia {
    fn default() -> Self {
        Self { stills: Mutex::new(HashMap::new()) }
    }
}

impl FootageSource for DoohMedia {
    fn frame(&self, item: ItemId, footage: &Footage, t: Tick) -> Option<Arc<Image>> {
        self.frame_scaled(item, footage, t, 1.0)
    }

    fn frame_scaled(&self, item: ItemId, footage: &Footage, t: Tick, scale: f64) -> Option<Arc<Image>> {
        let w = ((footage.width as f64 * scale.clamp(0.05, 1.0)).round() as u32).max(1);
        let h = ((footage.height as f64 * scale.clamp(0.05, 1.0)).round() as u32).max(1);
        if footage.kind == FootageKind::Video {
            let i = footage.frame_rate.frame_at(t).max(0);
            return Some(Arc::new(video_frame(w, h, i, item.0)));
        }
        let mut g = self.stills.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(img) = g.get(&item.0).filter(|im| im.width == w && im.height == h) {
            return Some(img.clone());
        }
        let img = Arc::new(still_frame(w, h, item.0));
        g.insert(item.0, img.clone());
        Some(img)
    }
}

fn video_frame(w: u32, h: u32, frame: i64, seed: u64) -> Image {
    let mut img = Image::new(w, h);
    let t = (frame as f32 * 0.12 + seed as f32 * 0.01).sin() * 0.5 + 0.5;
    let ww = w.max(1);
    img.data.par_iter_mut().enumerate().for_each(|(i, p)| {
        let x = (i as u32 % ww) as f32 / ww as f32;
        let y = (i as u32 / ww) as f32 / h.max(1) as f32;
        let bar = if (x - t).abs() < 0.07 { 0.45 } else { 0.0 };
        *p = [(x * 0.55 + bar).min(1.0), (y * 0.4 + 0.15).min(1.0), 0.2 + t * 0.25, 1.0];
    });
    img
}

fn still_frame(w: u32, h: u32, seed: u64) -> Image {
    let mut img = Image::new(w, h);
    let k = (seed as f32 * 0.17).fract();
    let ww = w.max(1);
    img.data.par_iter_mut().enumerate().for_each(|(i, p)| {
        let x = (i as u32 % ww) as f32 / ww as f32;
        let y = (i as u32 / ww) as f32 / h.max(1) as f32;
        *p = [0.12 + x * 0.25 + k * 0.1, 0.1 + y * 0.2, 0.18 + (1.0 - x) * 0.15, 1.0];
    });
    img
}

fn apply_fx(p: &mut Project, l: &mut Layer, id: &str, vals: &[(&str, Value)]) {
    let Some(spec) = effectcraft_effects::find(id) else { return };
    let mut next = p.next_id;
    let size = effectcraft_render::source_size(p, l);
    let mut g = effectcraft_effects::instantiate(spec, &mut Ids(&mut next), spec.name, [size.0 as f64, size.1 as f64]);
    p.next_id = next;
    for (k, v) in vals {
        if let Some(pr) = g.prop_mut(k) {
            pr.value = v.clone();
        }
    }
    if let Some(fx) = l.props.sub_mut("effects") {
        fx.children.push(g.into());
    }
}

fn set_pos(l: &mut Layer, x: f64, y: f64) {
    if let Some(p) = l.props.prop_mut("transform/position") {
        p.value = Value::Vec3([x, y, 0.0]);
    }
}

fn text_layer(p: &mut Project, comp: &Comp, name: &str, text: &str, size: f64, pos: [f64; 2]) -> Layer {
    let mut l = build::layer(p, comp, name, LayerSource::Text, (comp.width, comp.height), None);
    if let Some(pr) = l.props.prop_mut("text/sourceText") {
        pr.value = Value::Text(Box::new(TextDoc { text: text.into(), size, style: "SemiBold".into(), justify: Justify::Center, ..Default::default() }));
    }
    set_pos(&mut l, pos[0], pos[1]);
    l
}

/// One DOOH-shaped composition: video plate, still graphic, title, effects and blend modes.
pub fn add_comp(p: &mut Project, name: &str, w: u32, h: u32) -> ItemId {
    let mut c = Comp::new(w, h, FrameRate::FPS_25, Tick::from_seconds_f64(2.0));
    c.work_area = (Tick::ZERO, c.duration);
    let video = Footage {
        path: format!("{name}-clip.mp4"),
        kind: FootageKind::Video,
        width: w,
        height: h,
        pixel_aspect: 1.0,
        frame_rate: FrameRate::FPS_25,
        native_rate: None,
        duration: Tick::from_seconds_f64(2.0),
        has_video: true,
        has_audio: false,
        alpha: AlphaMode::Straight,
        codec: "H.264".into(),
        ..Default::default()
    };
    let still = Footage {
        path: format!("{name}-still.png"),
        kind: FootageKind::Still,
        width: w / 3,
        height: h / 3,
        pixel_aspect: 1.0,
        has_video: true,
        alpha: AlphaMode::Straight,
        codec: "PNG".into(),
        ..Default::default()
    };
    let vid_name = format!("{name} footage");
    let still_name = format!("{name} still");
    let vid_id = p.add_item(&vid_name, Label::Aqua, None, ItemKind::Footage(video));
    let still_id = p.add_item(&still_name, Label::Lavender, None, ItemKind::Footage(still));
    let mut plate = build::layer(p, &c, "Video plate", LayerSource::Footage { item: vid_id }, (w, h), None);
    plate.blend_mode = BlendMode::Normal;
    if let Some(pr) = plate.props.prop_mut("transform/position") {
        pr.keys = vec![
            Keyframe::new(Tick::ZERO, Value::Vec3([w as f64 / 2.0, h as f64 / 2.0, 0.0])),
            Keyframe::new(Tick::from_seconds_f64(2.0), Value::Vec3([w as f64 / 2.0 + 12.0, h as f64 / 2.0, 0.0])),
        ];
    }
    apply_fx(p, &mut plate, "ec.color.levels", &[("inBlack", Value::Scalar(0.05)), ("inWhite", Value::Scalar(0.95))]);
    let mut graphic = build::layer(p, &c, "Still graphic", LayerSource::Footage { item: still_id }, (w / 3, h / 3), None);
    graphic.blend_mode = BlendMode::Screen;
    set_pos(&mut graphic, w as f64 * 0.22, h as f64 * 0.28);
    apply_fx(p, &mut graphic, "ec.blur.gaussian", &[("blurriness", Value::Scalar(4.0))]);
    let bar_w = (w * 3 / 4).max(8);
    let bar_h = (h / 8).max(8);
    let bar_name = format!("{name} bar");
    let sid = p.add_item(&bar_name, Label::Red, None, ItemKind::Solid(Solid { color: [0.08, 0.12, 0.22], width: bar_w, height: bar_h, pixel_aspect: 1.0 }));
    let mut bar = build::layer(p, &c, "Lower third", LayerSource::Solid { item: sid }, (bar_w, bar_h), None);
    bar.blend_mode = BlendMode::Multiply;
    set_pos(&mut bar, w as f64 * 0.5, h as f64 * 0.82);
    apply_fx(p, &mut bar, "ec.stylize.glow", &[("threshold", Value::Scalar(50.0)), ("radius", Value::Scalar(8.0))]);
    let title = text_layer(p, &c, "Title", "TONIGHT  ·  CITY CENTRE", (h as f64 * 0.07).clamp(18.0, 72.0), [w as f64 * 0.5, h as f64 * 0.84]);
    c.layers = vec![title, bar, graphic, plate];
    p.add_item(name, Label::Sandstone, None, ItemKind::Comp(c.into()))
}

pub fn project() -> (Project, Vec<(String, ItemId, u32, u32)>) {
    let mut p = Project::default();
    p.settings.bit_depth = effectcraft_project::BitDepth::Bpc8;
    p.settings.gpu_acceleration = true;
    let mut comps = Vec::new();
    for (name, w, h) in SIZES {
        let id = add_comp(&mut p, name, w, h);
        comps.push((name.to_string(), id, w, h));
    }
    (p, comps)
}

/// One playback pass: `n` consecutive frames from time zero.
#[derive(Clone, Debug)]
pub struct PlayRun {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub parallel: bool,
    pub cache: bool,
    pub gpu: bool,
    pub frames: usize,
    pub mean_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub fps: f64,
}

impl PlayRun {
    pub fn realtime(&self) -> bool {
        self.fps + 0.05 >= TARGET_FPS
    }

    pub fn json(&self) -> Json {
        json!({
            "comp": self.label,
            "width": self.width,
            "height": self.height,
            "scale": self.scale,
            "parallel": self.parallel,
            "layerCache": self.cache,
            "gpu": self.gpu,
            "frames": self.frames,
            "meanMs": (self.mean_ms * 100.0).round() / 100.0,
            "minMs": (self.min_ms * 100.0).round() / 100.0,
            "maxMs": (self.max_ms * 100.0).round() / 100.0,
            "fps": (self.fps * 100.0).round() / 100.0,
            "realtime": self.realtime(),
            "targetFps": TARGET_FPS,
        })
    }
}

pub fn play(
    p: &Project,
    media: &DoohMedia,
    cid: ItemId,
    label: &str,
    w: u32,
    h: u32,
    scale: f64,
    parallel: bool,
    cache: bool,
    n: usize,
    accel: Option<&dyn effectcraft_render::Accelerator>,
    backend: effectcraft_render::Backend,
) -> PlayRun {
    let Some(comp) = p.comp(cid) else {
        return PlayRun {
            label: label.into(),
            width: w,
            height: h,
            scale,
            parallel,
            cache,
            gpu: accel.is_some(),
            frames: 0,
            mean_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            fps: 0.0,
        };
    };
    let layer_cache = LayerCache::default();
    let n = n.max(1);
    let mut times = Vec::with_capacity(n);
    for i in 0..n as i64 {
        let t = comp.frame_rate.tick_of(i);
        let mut r = Renderer::new(p, media, RenderOpts { scale, parallel, backend, motion_blur: false, ..Default::default() });
        if cache {
            r.cache = Some(&layer_cache);
        }
        r.accel = accel;
        let t0 = web_time::Instant::now();
        let img = r.comp_frame(cid, t);
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&img);
    }
    let sum: f64 = times.iter().sum();
    let mean = sum / n as f64;
    let min = times.iter().copied().fold(f64::INFINITY, f64::min);
    let max = times.iter().copied().fold(0.0f64, f64::max);
    PlayRun {
        label: label.into(),
        width: w,
        height: h,
        scale,
        parallel,
        cache,
        gpu: accel.is_some(),
        frames: n,
        mean_ms: mean,
        min_ms: min,
        max_ms: max,
        fps: if mean > 1e-9 { 1000.0 / mean } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dooh_comp_renders_and_parallel_matches_serial() {
        let mut p = Project::default();
        p.settings.bit_depth = effectcraft_project::BitDepth::Bpc32;
        let cid = add_comp(&mut p, "kiosk", 96, 96);
        let media = DoohMedia::default();
        let a = play(&p, &media, cid, "kiosk", 96, 96, 1.0, false, false, 2, None, effectcraft_render::Backend::Cpu);
        let b = play(&p, &media, cid, "kiosk", 96, 96, 1.0, true, true, 2, None, effectcraft_render::Backend::Cpu);
        assert!(a.mean_ms > 0.0 && b.mean_ms > 0.0);
        let ra = Renderer::new(&p, &media, RenderOpts { parallel: false, motion_blur: false, ..Default::default() });
        let rb = Renderer::new(&p, &media, RenderOpts { parallel: true, motion_blur: false, ..Default::default() });
        let ia = ra.comp_frame_cpu(cid, Tick::ZERO);
        let ib = rb.comp_frame_cpu(cid, Tick::ZERO);
        let max = ia.data.iter().zip(&ib.data).map(|(p, q)| (0..4).map(|i| (p[i] - q[i]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
        assert!(max < 1e-5, "serial vs parallel {max}");
    }
}
