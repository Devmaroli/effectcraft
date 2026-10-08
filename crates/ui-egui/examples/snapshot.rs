//! Headless UI snapshot: runs the real [`EffectcraftApp`] without a window (egui_kittest) and
//! writes PNGs. Works regardless of window focus, Spaces or display sleep, so agents can look
//! at the UI at any time. Default is wgpu; `--software` tessellates on the CPU (no GPU adapter
//! required — use it on cloud VMs with no `/dev/dri`).
//!
//! ```text
//! cargo run -p effectcraft-ui-egui --example snapshot -- [--out ui.png] [--size 1680x1020]
//!     [--scale 2] [--settle 1.5] [--empty] [--software]
//!     [--script steps.jsonl | --step '<json>']...
//! ```
//!
//! Each script step is a control-channel request (`{"method":"engine.execute","params":{...}}`,
//! see `docs/control-protocol.md`), run in order with the app settling in between. The extra
//! method `{"method":"snap","params":{"path":"x.png"}}` writes an intermediate snapshot.
//! Replies are printed to stdout as JSON lines.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use effectcraft_ui_egui::{ControlRequest, EffectcraftApp};
use egui::epaint::{Color32, ImageData, Primitive, TextureId};
use egui_kittest::{Harness, TestRenderer};
use image::RgbaImage;
use serde_json::{Value, json};

struct Args {
    out: String,
    size: (f32, f32),
    scale: f32,
    settle: f64,
    demo: bool,
    software: bool,
    steps: Vec<Value>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { out: "ui.png".into(), size: (1680.0, 1020.0), scale: 2.0, settle: 1.5, demo: true, software: false, steps: Vec::new() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => a.out = val()?,
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--scale" => a.scale = val()?.parse().map_err(|_| "bad --scale")?,
            "--settle" => a.settle = val()?.parse().map_err(|_| "bad --settle")?,
            "--empty" => a.demo = false,
            "--software" => a.software = true,
            "--step" => a.steps.push(serde_json::from_str(&val()?).map_err(|e| format!("--step: {e}"))?),
            "--script" => {
                let path = val()?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")) {
                    a.steps.push(serde_json::from_str(line).map_err(|e| format!("{path}: {e}: {line}"))?);
                }
            }
            "-h" | "--help" => {
                return Err(
                    "usage: snapshot [--out ui.png] [--size WxH] [--scale 2] [--settle secs] [--empty] [--software] [--script f.jsonl | --step json]...".into(),
                );
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(a)
}

/// One frame, first queueing the app's synthetic input (kittest doesn't call
/// `raw_input_hook`); the harness runs one frame per queued event.
fn step_frame(h: &mut Harness<'_, EffectcraftApp>) {
    for e in h.state_mut().take_synthetic_input() {
        h.event(e);
    }
    h.step();
}

/// Step the app for `secs` of wall time so background frame renders land in the viewer.
fn settle(h: &mut Harness<'_, EffectcraftApp>, secs: f64) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end {
        step_frame(h);
        std::thread::sleep(Duration::from_millis(30));
    }
    step_frame(h);
}

fn snap(h: &mut Harness<'_, EffectcraftApp>, path: &str) -> Result<(), String> {
    let img = h.render()?;
    img.save(path).map_err(|e| format!("{path}: {e}"))?;
    println!("{}", json!({"snapshot": path, "width": img.width(), "height": img.height()}));
    Ok(())
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let mut session = effectcraft_host::session();
    if args.demo {
        let _ = session.execute("file.openDemoProject", json!({}));
    }
    let (tx, rx) = mpsc::channel::<ControlRequest>();
    let size = egui::vec2(args.size.0, args.size.1);
    let mut harness = if args.software {
        Harness::builder()
            .with_size(size)
            .with_pixels_per_point(args.scale)
            .renderer(CpuRenderer::default())
            .build_eframe(move |_cc| EffectcraftApp::new(session).with_control(rx))
    } else {
        Harness::builder().with_size(size).with_pixels_per_point(args.scale).wgpu().build_eframe(move |_cc| EffectcraftApp::new(session).with_control(rx))
    };
    settle(&mut harness, args.settle);
    for step in &args.steps {
        let method = step["method"].as_str().unwrap_or_default().to_string();
        let params = step.get("params").cloned().unwrap_or(json!({}));
        if method == "snap" {
            if let Err(e) = snap(&mut harness, params["path"].as_str().unwrap_or("snap.png")) {
                eprintln!("{e}");
            }
            continue;
        }
        // Pointer input straight into the harness (menus and popups need real input events):
        // {"method":"pointer","params":{"x":237,"y":12,"click":true}}
        if method == "pointer" {
            let pos = egui::pos2(params["x"].as_f64().unwrap_or(0.0) as f32, params["y"].as_f64().unwrap_or(0.0) as f32);
            harness.input_mut().events.push(egui::Event::PointerMoved(pos));
            if params["click"].as_bool() == Some(true) {
                for pressed in [true, false] {
                    harness.input_mut().events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
            }
            settle(&mut harness, 0.3);
            println!("{}", json!({"method": method, "reply": {"ok": true}}));
            continue;
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        let _ = tx.send(ControlRequest { method: method.clone(), params, reply: reply_tx });
        // Requests are answered on a later frame (some wait for input to be processed).
        let deadline = Instant::now() + Duration::from_secs(10);
        let reply = loop {
            step_frame(&mut harness);
            if let Ok(v) = reply_rx.try_recv() {
                break v;
            }
            if Instant::now() > deadline {
                break json!({"ok": false, "error": "no reply within 10 s"});
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        println!("{}", json!({"method": method, "reply": reply}));
        settle(&mut harness, 0.3);
    }
    settle(&mut harness, args.settle.min(1.0));
    if let Err(e) = snap(&mut harness, &args.out) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// CPU tessellate-and-fill so `--software` does not need a wgpu adapter.
#[derive(Default)]
struct CpuRenderer {
    textures: HashMap<TextureId, CpuTexture>,
}

struct CpuTexture {
    w: u32,
    h: u32,
    px: Vec<Color32>,
}

impl TestRenderer for CpuRenderer {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        for (id, images) in delta.set.drain() {
            for image in images {
                let ImageData::Color(src) = &image.image;
                let (sw, sh) = (src.size[0], src.size[1]);
                if let Some(pos) = image.pos {
                    let dest = self.textures.entry(id).or_insert_with(|| CpuTexture {
                        w: (pos[0].saturating_add(sw)) as u32,
                        h: (pos[1].saturating_add(sh)) as u32,
                        px: vec![Color32::TRANSPARENT; pos[0].saturating_add(sw) * pos[1].saturating_add(sh)],
                    });
                    let dw = dest.w as usize;
                    for y in 0..sh {
                        let dy = pos[1].saturating_add(y);
                        if dy >= dest.h as usize {
                            break;
                        }
                        for x in 0..sw {
                            let dx = pos[0].saturating_add(x);
                            if dx >= dw {
                                break;
                            }
                            if let (Some(p), Some(slot)) = (src.pixels.get(y * sw + x), dest.px.get_mut(dy * dw + dx)) {
                                *slot = *p;
                            }
                        }
                    }
                } else {
                    self.textures.insert(id, CpuTexture { w: sw as u32, h: sh as u32, px: src.pixels.clone() });
                }
            }
        }
        for id in delta.free.drain() {
            self.textures.remove(&id);
        }
    }

    fn render(&mut self, ctx: &egui::Context, output: &egui::FullOutput) -> Result<RgbaImage, String> {
        let ppp = ctx.pixels_per_point();
        let size = ctx.content_rect().size() * ppp;
        let w = size.x.round().max(1.0) as u32;
        let h = size.y.round().max(1.0) as u32;
        let mut buf = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
        let tessellated = ctx.tessellate(output.shapes.clone(), ppp);
        for clipped in tessellated {
            let Primitive::Mesh(mesh) = clipped.primitive else { continue };
            let clip = clipped.clip_rect * ppp;
            let tex = self.textures.get(&mesh.texture_id);
            let mut i = 0;
            while i + 2 < mesh.indices.len() {
                let Some(ia) = mesh.indices.get(i).copied() else { break };
                let Some(ib) = mesh.indices.get(i + 1).copied() else { break };
                let Some(ic) = mesh.indices.get(i + 2).copied() else { break };
                i += 3;
                let Some(va) = mesh.vertices.get(ia as usize) else { continue };
                let Some(vb) = mesh.vertices.get(ib as usize) else { continue };
                let Some(vc) = mesh.vertices.get(ic as usize) else { continue };
                fill_tri(&mut buf, w, h, ppp, clip, va, vb, vc, tex);
            }
        }
        unpremultiply(&mut buf);
        RgbaImage::from_raw(w, h, buf).ok_or_else(|| "CPU snapshot buffer size mismatch".into())
    }
}

fn fill_tri(
    buf: &mut [u8],
    w: u32,
    h: u32,
    ppp: f32,
    clip: egui::Rect,
    a: &egui::epaint::Vertex,
    b: &egui::epaint::Vertex,
    c: &egui::epaint::Vertex,
    tex: Option<&CpuTexture>,
) {
    let pa = a.pos.to_vec2() * ppp;
    let pb = b.pos.to_vec2() * ppp;
    let pc = c.pos.to_vec2() * ppp;
    let min_x = pa.x.min(pb.x).min(pc.x).max(clip.min.x).max(0.0).floor() as i32;
    let max_x = pa.x.max(pb.x).max(pc.x).min(clip.max.x).min(w as f32).ceil() as i32;
    let min_y = pa.y.min(pb.y).min(pc.y).max(clip.min.y).max(0.0).floor() as i32;
    let max_y = pa.y.max(pb.y).max(pc.y).min(clip.max.y).min(h as f32).ceil() as i32;
    if max_x <= min_x || max_y <= min_y {
        return;
    }
    let area = edge(pa, pb, pc);
    if area.abs() < 1e-4 {
        return;
    }
    for y in min_y..max_y {
        for x in min_x..max_x {
            let p = egui::vec2(x as f32 + 0.5, y as f32 + 0.5);
            let wa = edge(pb, pc, p) / area;
            let wb = edge(pc, pa, p) / area;
            let wc = 1.0 - wa - wb;
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let uv = egui::pos2(a.uv.x * wa + b.uv.x * wb + c.uv.x * wc, a.uv.y * wa + b.uv.y * wb + c.uv.y * wc);
            let vert = lerp_color(a.color, b.color, c.color, wa, wb, wc);
            let sampled = sample(tex, uv);
            let src = mul_color(vert, sampled);
            blend(buf, w, x as u32, y as u32, src);
        }
    }
}

fn edge(a: egui::Vec2, b: egui::Vec2, c: egui::Vec2) -> f32 {
    (c.x - a.x) * (b.y - a.y) - (c.y - a.y) * (b.x - a.x)
}

fn lerp_color(a: Color32, b: Color32, c: Color32, wa: f32, wb: f32, wc: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        (a.r() as f32 * wa + b.r() as f32 * wb + c.r() as f32 * wc).round() as u8,
        (a.g() as f32 * wa + b.g() as f32 * wb + c.g() as f32 * wc).round() as u8,
        (a.b() as f32 * wa + b.b() as f32 * wb + c.b() as f32 * wc).round() as u8,
        (a.a() as f32 * wa + b.a() as f32 * wb + c.a() as f32 * wc).round() as u8,
    )
}

fn sample(tex: Option<&CpuTexture>, uv: egui::Pos2) -> Color32 {
    let Some(t) = tex else { return Color32::WHITE };
    if t.w == 0 || t.h == 0 || t.px.is_empty() {
        return Color32::WHITE;
    }
    let x = uv.x.mul_add(t.w as f32, -0.5).clamp(0.0, t.w.saturating_sub(1) as f32);
    let y = uv.y.mul_add(t.h as f32, -0.5).clamp(0.0, t.h.saturating_sub(1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = x0.saturating_add(1).min(t.w.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(t.h.saturating_sub(1));
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let c00 = texel(t, x0, y0);
    let c10 = texel(t, x1, y0);
    let c01 = texel(t, x0, y1);
    let c11 = texel(t, x1, y1);
    lerp_color(lerp_color(c00, c10, c00, 1.0 - fx, fx, 0.0), lerp_color(c01, c11, c01, 1.0 - fx, fx, 0.0), c00, 1.0 - fy, fy, 0.0)
}

fn texel(t: &CpuTexture, x: u32, y: u32) -> Color32 {
    t.px.get((y as usize) * (t.w as usize) + (x as usize)).copied().unwrap_or(Color32::TRANSPARENT)
}

fn mul_color(a: Color32, b: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(
        ((u16::from(a.r()) * u16::from(b.r()) + 127) / 255) as u8,
        ((u16::from(a.g()) * u16::from(b.g()) + 127) / 255) as u8,
        ((u16::from(a.b()) * u16::from(b.b()) + 127) / 255) as u8,
        ((u16::from(a.a()) * u16::from(b.a()) + 127) / 255) as u8,
    )
}

fn blend(buf: &mut [u8], w: u32, x: u32, y: u32, src: Color32) {
    let i = ((y as usize) * (w as usize) + (x as usize)).saturating_mul(4);
    let Some(d) = buf.get_mut(i..i + 4) else { return };
    let ia = 255u16.saturating_sub(u16::from(src.a()));
    d[0] = (u16::from(src.r()) + (u16::from(d[0]) * ia + 127) / 255) as u8;
    d[1] = (u16::from(src.g()) + (u16::from(d[1]) * ia + 127) / 255) as u8;
    d[2] = (u16::from(src.b()) + (u16::from(d[2]) * ia + 127) / 255) as u8;
    d[3] = (u16::from(src.a()) + (u16::from(d[3]) * ia + 127) / 255) as u8;
}

fn unpremultiply(buf: &mut [u8]) {
    let mut i = 0;
    while i + 3 < buf.len() {
        let a = buf[i + 3];
        if a > 0 && a < 255 {
            buf[i] = ((u16::from(buf[i]) * 255) / u16::from(a)) as u8;
            buf[i + 1] = ((u16::from(buf[i + 1]) * 255) / u16::from(a)) as u8;
            buf[i + 2] = ((u16::from(buf[i + 2]) * 255) / u16::from(a)) as u8;
        }
        i += 4;
    }
}
