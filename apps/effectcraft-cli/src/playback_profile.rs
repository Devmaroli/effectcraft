//! Headless playback-pipeline profile (`effectcraft-cli bench --playback-profile`).
//!
//! Times decode, compositing, viewer upload (CPU `ColorImage` conversion + memcpy; GPU
//! `write_texture` when an adapter exists) and end-to-end frame time for:
//! (a) 6880×1032 ProRes 422 HQ, (b) 1920×1080 H.264, (c) a 4-layer 25 fps comp.
//! Clips are generated with an external `ffmpeg` (oracle / fixture only; never linked).
//! This command does not change playback behaviour.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use effectcraft_engine::color::{BlendMode, Label};
use effectcraft_engine::keyframe::{Justify, Keyframe, TextDoc, Value};
use effectcraft_engine::playback_caps::PlaybackCaps;
use effectcraft_engine::project::build::{self, Ids};
use effectcraft_engine::project::{Comp, Footage, ItemId, ItemKind, Layer, LayerSource, Project, Solid};
use effectcraft_engine::sysinfo;
use effectcraft_gpu::Gpu;
use effectcraft_media::{MediaPool, hwdec, probe_single};
use effectcraft_render::{Backend, LayerCache, RenderOpts, Renderer};
use effectcraft_time::{FrameRate, Tick};
use effectcraft_ui_egui::frames::to_color_image;
use serde_json::{Value as Json, json};

use super::Failure;

fn note(msg: &str) {
    let _ = writeln!(std::io::stderr(), "{msg}");
}

fn stats(times: &[f64]) -> (f64, f64, f64, f64) {
    if times.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let n = times.len() as f64;
    let mean = times.iter().sum::<f64>() / n;
    let min = times.iter().copied().fold(f64::INFINITY, f64::min);
    let max = times.iter().copied().fold(0.0_f64, f64::max);
    let fps = if mean > 1e-9 { 1000.0 / mean } else { 0.0 };
    (mean, min, max, fps)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn stage_json(scenario: &str, codec: &str, w: u32, h: u32, layers: u32, scale: f64, stage: &str, times: &[f64]) -> Json {
    let (mean, min, max, fps) = stats(times);
    json!({
        "scenario": scenario,
        "codec": codec,
        "width": w,
        "height": h,
        "layers": layers,
        "scale": scale,
        "stage": stage,
        "frames": times.len(),
        "meanMs": round2(mean),
        "minMs": round2(min),
        "maxMs": round2(max),
        "fps": round2(fps),
        "realtime25": fps + 0.05 >= 25.0,
        "realtime2997": fps + 0.05 >= 29.97,
    })
}

fn print_stage(r: &Json) {
    let stage = r["stage"].as_str().unwrap_or("?");
    let mean = r["meanMs"].as_f64().unwrap_or(0.0);
    let fps = r["fps"].as_f64().unwrap_or(0.0);
    let rt = if r["realtime25"].as_bool().unwrap_or(false) { "≥25 fps" } else { "NOT 25 fps" };
    note(&format!("    {stage:<28} {mean:>8.2} ms/frame  {fps:>7.1} fps  {rt}"));
}

fn simd_level() -> Vec<&'static str> {
    #[cfg(target_arch = "x86_64")]
    {
        let mut v = Vec::new();
        if is_x86_feature_detected!("sse2") {
            v.push("sse2");
        }
        if is_x86_feature_detected!("ssse3") {
            v.push("ssse3");
        }
        if is_x86_feature_detected!("avx2") {
            v.push("avx2");
        }
        if is_x86_feature_detected!("avx512f") {
            v.push("avx512f");
        }
        v
    }
    #[cfg(target_arch = "aarch64")]
    {
        vec!["neon"]
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Vec::new()
    }
}

fn host_json(gpu_name: Option<&str>, gpu_requested: bool, caps: &PlaybackCaps) -> Json {
    let mem = sysinfo::memory();
    let gb = |b: u64| (b as f64 / (1u64 << 30) as f64 * 10.0).round() / 10.0;
    json!({
        "os": sysinfo::os_version(),
        "arch": std::env::consts::ARCH,
        "cpu": sysinfo::cpu_name(),
        "cores": sysinfo::cpu_cores(),
        "simd": simd_level(),
        "ramTotalGb": mem.map(|m| gb(m.total)),
        "ramAvailableGb": mem.map(|m| gb(m.available)),
        "gpu": gpu_name,
        "gpuRequested": gpu_requested,
        "playbackCaps": caps.to_json(),
        "poolThreads": caps.decode_pool_threads,
        "prefetchDooh6880": caps.prefetch_depth(108 << 20, 1 << 30),
        "prefetch1080p": caps.prefetch_depth(1920 * 1080 * 16, 1 << 30),
        "hwDecodeProfiles": hwdec::probe(caps.gpu.as_ref().map(|g| g.vendor_id).unwrap_or(0), caps.gpu.as_ref().map(|g| g.device.as_str()).unwrap_or("")).len(),
        "note": "Cloud VM: CPU-only unless --gpu finds an adapter. RTX 3070 Ti: GPU present (no readback), D3D11VA probe for in-spec H.264/HEVC with CPU fallback; ProRes stays CPU; 6880-wide H.264 never claims Ampere NVDEC (4096 max).",
    })
}

fn ffmpeg_ok() -> Result<(), String> {
    let st = Command::new("ffmpeg").args(["-hide_banner", "-version"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    match st {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("ffmpeg is installed but failed to start".into()),
        Err(e) => Err(format!("ffmpeg is required to generate profile clips (external oracle only): {e}")),
    }
}

fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let out = Command::new("ffmpeg").args(args).output().map_err(|e| format!("ffmpeg: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let tail = err.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(format!("ffmpeg failed: {tail}"))
}

fn time_ffmpeg_decode(path: &Path, frames: usize) -> Option<f64> {
    let t0 = Instant::now();
    let st = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", &path.to_string_lossy(), "-frames:v", &frames.to_string(), "-f", "null", "-"])
        .status()
        .ok()?;
    if !st.success() {
        return None;
    }
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    Some(ms / frames.max(1) as f64)
}

fn write_still(path: &Path, w: u32, h: u32) -> Result<(), String> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([42, 74, 106]));
    img.save(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn generate_clips(dir: &Path, n_prores: usize, n_h264: usize) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let prores = dir.join("prores_6880x1032_hq.mov");
    let h264 = dir.join("h264_1920x1080.mp4");
    let still = dir.join("still_640x360.png");
    run_ffmpeg(&[
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=6880x1032:rate=30000/1001",
        "-frames:v",
        &n_prores.to_string(),
        "-c:v",
        "prores_ks",
        "-profile:v",
        "3",
        "-pix_fmt",
        "yuv422p10le",
        &prores.to_string_lossy(),
    ])?;
    run_ffmpeg(&[
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1920x1080:rate=25",
        "-frames:v",
        &n_h264.to_string(),
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-pix_fmt",
        "yuv420p",
        &h264.to_string_lossy(),
    ])?;
    write_still(&still, 640, 360)?;
    Ok((prores, h264, still))
}

fn apply_fx(p: &mut Project, l: &mut Layer, id: &str, vals: &[(&str, Value)]) {
    let Some(spec) = effectcraft_engine::effects::find(id) else { return };
    let mut next = p.next_id;
    let size = effectcraft_render::source_size(p, l);
    let mut g = effectcraft_engine::effects::instantiate(spec, &mut Ids(&mut next), spec.name, [size.0 as f64, size.1 as f64]);
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

fn add_footage_layer(p: &mut Project, c: &Comp, name: &str, footage: Footage) -> (ItemId, Layer) {
    let id = p.add_item(name, Label::Aqua, None, ItemKind::Footage(footage.clone()));
    let l = build::layer(p, c, name, LayerSource::Footage { item: id }, (footage.width, footage.height), None);
    (id, l)
}

fn one_layer_comp(p: &mut Project, name: &str, footage: Footage) -> ItemId {
    let (w, h) = (footage.width, footage.height);
    let mut c = Comp::new(w, h, FrameRate::FPS_25, footage.duration);
    c.work_area = (Tick::ZERO, c.duration);
    let (_, plate) = add_footage_layer(p, &c, &format!("{name} plate"), footage);
    c.layers = vec![plate];
    p.add_item(name, Label::Sandstone, None, ItemKind::Comp(c.into()))
}

fn four_layer_comp(p: &mut Project, name: &str, video: Footage, still: Footage) -> ItemId {
    let (w, h) = (video.width, video.height);
    let mut c = Comp::new(w, h, FrameRate::FPS_25, video.duration.max(Tick::from_seconds_f64(2.0)));
    c.work_area = (Tick::ZERO, c.duration);
    let mut plate = add_footage_layer(p, &c, "Video plate", video).1;
    plate.blend_mode = BlendMode::Normal;
    if let Some(pr) = plate.props.prop_mut("transform/position") {
        pr.keys = vec![
            Keyframe::new(Tick::ZERO, Value::Vec3([w as f64 / 2.0, h as f64 / 2.0, 0.0])),
            Keyframe::new(c.duration, Value::Vec3([w as f64 / 2.0 + 12.0, h as f64 / 2.0, 0.0])),
        ];
    }
    apply_fx(p, &mut plate, "ec.color.levels", &[("inBlack", Value::Scalar(0.05)), ("inWhite", Value::Scalar(0.95))]);
    let mut graphic = add_footage_layer(p, &c, "Still graphic", still).1;
    graphic.blend_mode = BlendMode::Screen;
    set_pos(&mut graphic, w as f64 * 0.22, h as f64 * 0.28);
    apply_fx(p, &mut graphic, "ec.blur.gaussian", &[("blurriness", Value::Scalar(4.0))]);
    let bar_w = (w * 3 / 4).max(8);
    let bar_h = (h / 8).max(8);
    let sid = p.add_item(
        &format!("{name} bar"),
        Label::Red,
        None,
        ItemKind::Solid(Solid { color: [0.08, 0.12, 0.22], width: bar_w, height: bar_h, pixel_aspect: 1.0 }),
    );
    let mut bar = build::layer(p, &c, "Lower third", LayerSource::Solid { item: sid }, (bar_w, bar_h), None);
    bar.blend_mode = BlendMode::Multiply;
    set_pos(&mut bar, w as f64 * 0.5, h as f64 * 0.82);
    apply_fx(p, &mut bar, "ec.stylize.glow", &[("threshold", Value::Scalar(50.0)), ("radius", Value::Scalar(8.0))]);
    let mut title = build::layer(p, &c, "Title", LayerSource::Text, (w, h), None);
    if let Some(pr) = title.props.prop_mut("text/sourceText") {
        pr.value = Value::Text(Box::new(TextDoc {
            text: "TONIGHT  ·  CITY CENTRE".into(),
            size: (h as f64 * 0.07).clamp(18.0, 72.0),
            style: "SemiBold".into(),
            justify: Justify::Center,
            ..Default::default()
        }));
    }
    set_pos(&mut title, w as f64 * 0.5, h as f64 * 0.84);
    c.layers = vec![title, bar, graphic, plate];
    p.add_item(name, Label::Sandstone, None, ItemKind::Comp(c.into()))
}

fn load_footage(path: &Path) -> Result<Footage, String> {
    probe_single(path).map_err(|e| e.to_string())
}

fn open_ms(pool: &MediaPool, footage: &Footage) -> Result<(f64, usize), String> {
    pool.clear();
    let t0 = Instant::now();
    let img = pool.frame_at(footage, Tick::ZERO).map_err(|e| e.to_string())?;
    Ok((t0.elapsed().as_secs_f64() * 1e3, img.data.len() * 16))
}

fn time_decode(pool: &MediaPool, footage: &Footage, n: usize, scale: f64) -> Result<Vec<f64>, String> {
    pool.clear_frames();
    pool.set_read_ahead(false);
    let mut times = Vec::with_capacity(n);
    for i in 0..n {
        let t = footage.frame_rate.tick_of(i as i64);
        let t0 = Instant::now();
        let img = pool.frame_at_scaled(footage, t, scale).map_err(|e| e.to_string())?;
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&img);
    }
    Ok(times)
}

fn prime_decode(pool: &MediaPool, footage: &Footage, n: usize, scale: f64) -> Result<(), String> {
    pool.set_read_ahead(false);
    for i in 0..n {
        let t = footage.frame_rate.tick_of(i as i64);
        let _ = pool.frame_at_scaled(footage, t, scale).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn time_composite(p: &Project, pool: &MediaPool, cid: ItemId, n: usize, scale: f64, cache: bool, gpu: Option<&Gpu>) -> Vec<f64> {
    let Some(comp) = p.comp(cid) else { return Vec::new() };
    let layer_cache = LayerCache::default();
    let backend = if gpu.is_some() { Backend::Gpu } else { Backend::Cpu };
    let mut times = Vec::with_capacity(n);
    for i in 0..n as i64 {
        let t = comp.frame_rate.tick_of(i);
        let mut r = Renderer::new(p, pool, RenderOpts { scale, parallel: true, backend, motion_blur: false, ..Default::default() });
        if cache {
            r.cache = Some(&layer_cache);
        }
        r.accel = gpu.map(|g| g as &dyn effectcraft_render::Accelerator);
        let t0 = Instant::now();
        let img = r.comp_frame(cid, t);
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&img);
    }
    times
}

fn time_upload_cpu(p: &Project, pool: &MediaPool, cid: ItemId, n: usize, scale: f64) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), String> {
    let Some(comp) = p.comp(cid) else { return Ok((Vec::new(), Vec::new(), Vec::new())) };
    let mut convert = Vec::with_capacity(n);
    let mut memcpy_u8 = Vec::with_capacity(n);
    let mut memcpy_f32 = Vec::with_capacity(n);
    for i in 0..n as i64 {
        let t = comp.frame_rate.tick_of(i);
        let r = Renderer::new(p, pool, RenderOpts { scale, parallel: true, motion_blur: false, ..Default::default() });
        let img = r.comp_frame(cid, t);
        let t0 = Instant::now();
        let ci = to_color_image(&img);
        convert.push(t0.elapsed().as_secs_f64() * 1e3);
        let t1 = Instant::now();
        let copy = ci.pixels.clone();
        memcpy_u8.push(t1.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&copy);
        let t2 = Instant::now();
        let raw = img.data.clone();
        memcpy_f32.push(t2.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&raw);
    }
    Ok((convert, memcpy_u8, memcpy_f32))
}

fn time_gpu_upload(gpu: &Gpu, pool: &MediaPool, footage: &Footage, n: usize) -> Result<Vec<f64>, String> {
    let img = pool.frame_at(footage, Tick::ZERO).map_err(|e| e.to_string())?;
    if gpu.context().upload_image(&img).is_none() {
        return Err("GPU upload declined (image does not fit the device)".into());
    }
    gpu.wait();
    let mut times = Vec::with_capacity(n);
    for _ in 0..n {
        let t0 = Instant::now();
        if gpu.context().upload_image(&img).is_none() {
            return Err("GPU upload declined after warmup".into());
        }
        gpu.wait();
        times.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    Ok(times)
}

fn time_e2e(p: &Project, pool: &MediaPool, cid: ItemId, n: usize, scale: f64) -> Result<Vec<f64>, String> {
    let Some(comp) = p.comp(cid) else { return Ok(Vec::new()) };
    pool.clear_frames();
    pool.set_read_ahead(false);
    let mut times = Vec::with_capacity(n);
    for i in 0..n as i64 {
        let t = comp.frame_rate.tick_of(i);
        let t0 = Instant::now();
        let r = Renderer::new(p, pool, RenderOpts { scale, parallel: true, motion_blur: false, ..Default::default() });
        let img = r.comp_frame(cid, t);
        let ci = to_color_image(&img);
        let copy = ci.pixels.clone();
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&copy);
    }
    Ok(times)
}

fn clip_info(path: &Path, footage: &Footage) -> Json {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let px = u64::from(footage.width) * u64::from(footage.height);
    json!({
        "path": path.display().to_string(),
        "codec": footage.codec,
        "kind": format!("{:?}", footage.kind),
        "width": footage.width,
        "height": footage.height,
        "frameRate": footage.frame_rate.as_f64(),
        "durationS": footage.duration.seconds(),
        "fileBytes": bytes,
        "decodedF32RgbaBytes": px * 16,
        "previewRgba8Bytes": px * 4,
        "hasAudio": footage.has_audio,
        "alpha": format!("{:?}", footage.alpha).to_ascii_lowercase(),
    })
}

fn profile_movie(scenario: &str, path: &Path, n: usize, gpu: Option<&Gpu>, runs: &mut Vec<Json>) -> Result<Footage, String> {
    let footage = load_footage(path)?;
    let pool = MediaPool::with_budget(4 << 30);
    pool.set_read_ahead(false);
    let (w, h) = (footage.width, footage.height);
    let codec = footage.codec.clone();
    note(&format!("  {scenario}: {}  {w}×{h}  {}  {:.2} fps", path.display(), codec, footage.frame_rate.as_f64()));

    let (open, decoded_bytes) = open_ms(&pool, &footage)?;
    note(&format!("    open+first-frame            {open:>8.1} ms  (decoded ~{:.1} MB f32 RGBA)", decoded_bytes as f64 / 1e6));

    let decode = time_decode(&pool, &footage, n, 1.0)?;
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "decode+yuv_to_f32", &decode);
    print_stage(&row);
    runs.push(row);

    let half = time_decode(&pool, &footage, n, 0.5)?;
    let row = stage_json(scenario, &codec, w, h, 1, 0.5, "decode+yuv_to_f32_half", &half);
    print_stage(&row);
    runs.push(row);

    let quarter = time_decode(&pool, &footage, n, 0.25)?;
    let row = stage_json(scenario, &codec, w, h, 1, 0.25, "decode+yuv_to_f32_quarter", &quarter);
    print_stage(&row);
    runs.push(row);

    prime_decode(&pool, &footage, n, 1.0)?;
    let cached = time_decode(&pool, &footage, n, 1.0)?;
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "decode_cached", &cached);
    print_stage(&row);
    runs.push(row);

    if let Some(ms) = time_ffmpeg_decode(path, n) {
        let fps = if ms > 1e-9 { 1000.0 / ms } else { 0.0 };
        let row = json!({
            "scenario": scenario,
            "codec": codec,
            "width": w,
            "height": h,
            "layers": 1,
            "scale": 1.0,
            "stage": "ffmpeg_sw_decode",
            "frames": n,
            "meanMs": round2(ms),
            "minMs": round2(ms),
            "maxMs": round2(ms),
            "fps": round2(fps),
            "realtime25": fps + 0.05 >= 25.0,
            "realtime2997": fps + 0.05 >= 29.97,
        });
        print_stage(&row);
        runs.push(row);
    }

    let mut p = Project::default();
    p.settings.bit_depth = effectcraft_engine::project::BitDepth::Bpc8;
    let cid = one_layer_comp(&mut p, scenario, footage.clone());
    prime_decode(&pool, &footage, n, 1.0)?;
    let composite = time_composite(&p, &pool, cid, n, 1.0, true, None);
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "composite_cpu_warm", &composite);
    print_stage(&row);
    runs.push(row);

    let (convert, memcpy_u8, memcpy_f32) = time_upload_cpu(&p, &pool, cid, n.clamp(1, 4), 1.0)?;
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "to_color_image_u8", &convert);
    print_stage(&row);
    runs.push(row);
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "memcpy_rgba8_upload_standin", &memcpy_u8);
    print_stage(&row);
    runs.push(row);
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "memcpy_f32_upload_standin", &memcpy_f32);
    print_stage(&row);
    runs.push(row);

    if let Some(g) = gpu {
        match time_gpu_upload(g, &pool, &footage, n.clamp(1, 4)) {
            Ok(times) => {
                let row = stage_json(scenario, &codec, w, h, 1, 1.0, "gpu_write_texture_f32", &times);
                print_stage(&row);
                runs.push(row);
            }
            Err(e) => note(&format!("    gpu_write_texture_f32         skipped ({e})")),
        }
        let gpu_comp = time_composite(&p, &pool, cid, n, 1.0, true, Some(g));
        let row = stage_json(scenario, &codec, w, h, 1, 1.0, "composite_gpu_warm", &gpu_comp);
        print_stage(&row);
        runs.push(row);
    }

    let e2e = time_e2e(&p, &pool, cid, n, 1.0)?;
    let row = stage_json(scenario, &codec, w, h, 1, 1.0, "e2e_decode_composite_upload", &e2e);
    print_stage(&row);
    runs.push(row);

    let e2e_half = time_e2e(&p, &pool, cid, n, 0.5)?;
    let row = stage_json(scenario, &codec, w, h, 1, 0.5, "e2e_half", &e2e_half);
    print_stage(&row);
    runs.push(row);

    Ok(footage)
}

fn profile_four_layer(scenario: &str, video: Footage, still: Footage, n: usize, gpu: Option<&Gpu>, runs: &mut Vec<Json>) -> Result<(), String> {
    let (w, h) = (video.width, video.height);
    let codec = format!("4-layer ({})", video.codec);
    let mut p = Project::default();
    p.settings.bit_depth = effectcraft_engine::project::BitDepth::Bpc8;
    let cid = four_layer_comp(&mut p, scenario, video.clone(), still);
    let pool = MediaPool::with_budget(4 << 30);
    pool.set_read_ahead(false);
    note(&format!("  {scenario}: {w}×{h}  4 layers (video + still + solid + text)"));

    let e2e = time_e2e(&p, &pool, cid, n, 1.0)?;
    let row = stage_json(scenario, &codec, w, h, 4, 1.0, "e2e_decode_composite_upload", &e2e);
    print_stage(&row);
    runs.push(row);

    prime_decode(&pool, &video, n, 1.0)?;
    let composite = time_composite(&p, &pool, cid, n, 1.0, true, None);
    let row = stage_json(scenario, &codec, w, h, 4, 1.0, "composite_cpu_warm", &composite);
    print_stage(&row);
    runs.push(row);

    let half = time_e2e(&p, &pool, cid, n, 0.5)?;
    let row = stage_json(scenario, &codec, w, h, 4, 0.5, "e2e_half", &half);
    print_stage(&row);
    runs.push(row);

    let quarter = time_e2e(&p, &pool, cid, n, 0.25)?;
    let row = stage_json(scenario, &codec, w, h, 4, 0.25, "e2e_quarter", &quarter);
    print_stage(&row);
    runs.push(row);

    if let Some(g) = gpu {
        let gpu_comp = time_composite(&p, &pool, cid, n, 1.0, true, Some(g));
        let row = stage_json(scenario, &codec, w, h, 4, 1.0, "composite_gpu_warm", &gpu_comp);
        print_stage(&row);
        runs.push(row);
    }
    Ok(())
}

/// `bench --playback-profile [--play N] [--gpu] [--json] [--out FILE]`.
pub(crate) fn run(args: &super::Args) -> Result<(), Failure> {
    ffmpeg_ok().map_err(Failure::Error)?;
    let n = args.num("--play").map_err(Failure::Error)?.unwrap_or(8.0).max(1.0) as usize;
    let n_prores = n.min(8);
    let n_h264 = n.clamp(8, 24);
    let want_gpu = args.flag("--gpu");
    let gpu = want_gpu.then(Gpu::headless).flatten();
    let gpu_name = gpu.as_ref().map(effectcraft_render::Accelerator::name);
    let dir = std::env::temp_dir().join(format!("effectcraft-playback-profile-{}", std::process::id()));
    note(&format!("playback profile: {n_prores} ProRes frames, {n_h264} H.264 frames, dir {}", dir.display()));
    let (prores, h264, still) = generate_clips(&dir, n_prores, n_h264).map_err(Failure::Error)?;
    let mut runs = Vec::new();
    let mut clips = Vec::new();

    let a = profile_movie("a-prores-6880x1032", &prores, n_prores, gpu.as_ref(), &mut runs).map_err(Failure::Error)?;
    clips.push(clip_info(&prores, &a));
    let b = profile_movie("b-h264-1920x1080", &h264, n_h264, gpu.as_ref(), &mut runs).map_err(Failure::Error)?;
    clips.push(clip_info(&h264, &b));
    let still_f = load_footage(&still).map_err(Failure::Error)?;
    clips.push(clip_info(&still, &still_f));
    profile_four_layer("c-4layer-1920x1080", b.clone(), still_f.clone(), n_h264.min(8), gpu.as_ref(), &mut runs).map_err(Failure::Error)?;
    profile_four_layer("c-4layer-6880x1032", a.clone(), still_f, n_prores, gpu.as_ref(), &mut runs).map_err(Failure::Error)?;

    let mut caps = PlaybackCaps::probe_host();
    if let Some(g) = &gpu {
        let a = g.adapter_caps();
        caps.with_gpu(effectcraft_engine::playback_caps::GpuCaps {
            vendor: if a.nvidia { "NVIDIA".into() } else { String::new() },
            device: a.name.clone(),
            backend: a.backend.clone(),
            vram_bytes: None,
            vendor_id: a.vendor_id,
            nvenc: a.nvidia,
            f16_storage: a.f16_storage,
        });
        let profiles = hwdec::probe(a.vendor_id, &a.name);
        caps.with_decode(
            profiles
                .into_iter()
                .map(|p| effectcraft_engine::playback_caps::DecodeCaps {
                    api: p.api,
                    codec: p.codec,
                    chroma: p.chroma,
                    bit_depth: p.bit_depth,
                    min_w: p.min_w,
                    min_h: p.min_h,
                    max_w: p.max_w,
                    max_h: p.max_h,
                })
                .collect(),
        );
    }
    caps.install_rayon();
    hwdec::register();

    let report = json!({
        "targetFps": 25.0,
        "host": host_json(gpu_name.as_deref(), want_gpu, &caps),
        "pipeline": {
            "decode": "Streamed FileReader (no whole-file fs::read). D3D11VA/Vulkan Video probed; factory returns None so FilmCraft CPU runs. ProRes never HW. Ampere H.264 max 4096 so 6880-wide H.264 stays CPU.",
            "color": "CPU YUV→premultiplied f32 RGBA (BT.601/709/2020, limited/full), row-parallel. Viewer presents RGBA8 (no readback when gpu_display).",
            "prefetch": "Sized from bytes/frame and RAM (2–32); this profile still disables read-ahead so decode timings are honest.",
            "cache": "MediaPool LRU 1 GiB decoded f32 frames; RAM preview up to ~3 GiB; JPEG half-res proxy cache (20 GiB default).",
            "composite": "GPU texture present (no readback) when an adapter exists; CPU SIMD fallback. Preview precision RGBA8 display / f16 when the adapter stores it; renders stay f32.",
            "pool": "One rayon work-stealing pool, max(1, cores−2).",
            "audioSync": "Audio-master clock; Drop frames to keep sound in sync (default ON). Adaptive Auto res Full→Half→Quarter while playing.",
        },
        "clips": clips,
        "runs": runs,
    });

    if let Some(path) = args.opt("--out") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap_or_else(|_| report.to_string()))
            .map_err(|e| Failure::Error(format!("{path}: {e}")))?;
        note(&format!("wrote {path}"));
    }
    if args.flag("--json") {
        crate::emit(&report, true);
    } else {
        note("playback profile done (pass --json for the machine-readable report)");
    }
    Ok(())
}
