//! Startup (and device-lost) playback capability probe and the hardware-adaptive fallback
//! matrix. Every probe failure is a `Result` / empty list — never a panic.
//!
//! GPU adapter details are filled in by the frontend (this crate does not depend on wgpu).
//! Hardware-decode APIs are filled in by `effectcraft_media::hwdec` through
//! [`PlaybackCaps::with_decode`]. Last-resort path: CPU SIMD decode + CPU composite.

use serde::{Deserialize, Serialize};

use crate::sysinfo;

/// How a composited viewer frame is presented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompositePath {
    /// wgpu texture drawn by egui-wgpu (no CPU readback).
    GpuTexture,
    /// GPU composite, then a readback to a CPU `ColorImage`.
    GpuReadback,
    /// CPU compositor + SIMD convert.
    #[default]
    CpuSimd,
}

/// Working / presentation precision for a viewer frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewPrecision {
    /// Premultiplied f32 on the CPU (renders and CPU preview).
    #[default]
    F32Cpu,
    /// f16 working buffers when the adapter supports them.
    F16Gpu,
    /// 8-bit display texture (viewer frames and GPU present). Final renders stay f32.
    Rgba8Display,
}

/// A hardware video-decode profile (the NVIDIA `cuvidGetDecoderCaps` test, from public docs).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeCaps {
    /// `d3d11va`, `vulkan-video`, `videotoolbox`. Never `nvdec` / CUDA SDK.
    pub api: String,
    pub codec: String,
    /// e.g. `420`, `422`, `444`.
    pub chroma: String,
    pub bit_depth: u8,
    pub min_w: u32,
    pub min_h: u32,
    pub max_w: u32,
    pub max_h: u32,
}

impl DecodeCaps {
    /// NVIDIA `cuvidGetDecoderCaps`: codec, chroma, bit depth, width/height in \[min, max\].
    pub fn covers(&self, codec: &str, chroma: &str, bit_depth: u8, w: u32, h: u32) -> bool {
        codec_key(codec) == codec_key(&self.codec)
            && chroma_ok(&self.chroma, chroma)
            && bit_depth <= self.bit_depth.max(8)
            && w >= self.min_w
            && h >= self.min_h
            && w <= self.max_w
            && h <= self.max_h
    }
}

fn codec_key(s: &str) -> String {
    s.to_ascii_lowercase().replace(['.', ' ', '_', '-'], "")
}

fn chroma_ok(have: &str, want: &str) -> bool {
    let n = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    let a = n(have);
    let b = n(want);
    a.is_empty() || b.is_empty() || a == b
}

/// GPU adapter as reported by wgpu (filled by the UI / host).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuCaps {
    pub vendor: String,
    pub device: String,
    pub backend: String,
    /// Dedicated VRAM when the adapter reports it; `None` = unknown (do not guess).
    pub vram_bytes: Option<u64>,
    /// DXGI / PCI vendor id when known (`0x10DE` = NVIDIA).
    pub vendor_id: u32,
    /// NVENC encode is available for proxy transcode (NVIDIA adapter). Encode still uses the
    /// JPEG half-res path in this build: no NVIDIA Video Codec SDK is linked.
    pub nvenc: bool,
    /// Adapter can store `Rgba16Float` working textures.
    pub f16_storage: bool,
}

/// Chosen decode path for one clip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DecodePath {
    Hw { api: String, keep_on_gpu: bool },
    Cpu { threads: u32 },
}

/// Detected host capabilities for playback (serde so agents can read it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackCaps {
    pub gpu: Option<GpuCaps>,
    pub video_decode: Vec<DecodeCaps>,
    pub cpu_cores: u32,
    pub simd: Vec<String>,
    pub ram_total: u64,
    pub ram_available: u64,
    pub composite: CompositePath,
    pub preview_precision: PreviewPrecision,
    /// `max(1, cores.saturating_sub(2))` so UI and audio keep cores.
    pub decode_pool_threads: u32,
}

impl Default for PlaybackCaps {
    fn default() -> Self {
        Self::probe_host()
    }
}

impl PlaybackCaps {
    /// CPU, RAM and SIMD only. Safe to call from any thread; never panics.
    pub fn probe_host() -> Self {
        let cores = sysinfo::cpu_cores() as u32;
        let mem = sysinfo::memory();
        PlaybackCaps {
            gpu: None,
            video_decode: Vec::new(),
            cpu_cores: cores.max(1),
            simd: simd_level(),
            ram_total: mem.map(|m| m.total).unwrap_or(0),
            ram_available: mem.map(|m| m.available).unwrap_or(0),
            composite: CompositePath::CpuSimd,
            preview_precision: PreviewPrecision::F32Cpu,
            decode_pool_threads: pool_threads(cores),
        }
    }

    /// GPU adapter appeared (or was rebuilt after device-lost).
    pub fn with_gpu(&mut self, gpu: GpuCaps) {
        self.preview_precision = if gpu.f16_storage { PreviewPrecision::F16Gpu } else { PreviewPrecision::Rgba8Display };
        // Viewer present stays on a GPU texture (no readback). Auto still measures CPU vs GPU
        // per comp; [`Self::pick_from_times`] records the probe's choice.
        self.composite = CompositePath::GpuTexture;
        self.gpu = Some(gpu);
    }

    /// Hardware probe: keep GPU present only when the display path beat the CPU.
    pub fn pick_from_times(&mut self, cpu_ms: f64, gpu_display_ms: f64) {
        if self.gpu.is_none() {
            self.composite = CompositePath::CpuSimd;
            self.preview_precision = PreviewPrecision::F32Cpu;
            return;
        }
        if effectcraft_render::AutoPick::gpu_wins(cpu_ms, gpu_display_ms) {
            self.composite = CompositePath::GpuTexture;
        } else {
            self.composite = CompositePath::CpuSimd;
        }
    }

    /// Hardware-decode profiles from a platform probe (empty = CPU only).
    pub fn with_decode(&mut self, caps: Vec<DecodeCaps>) {
        self.video_decode = caps;
    }

    /// Drop GPU and hardware-decode state after device-lost; CPU path remains.
    pub fn clear_gpu(&mut self) {
        self.gpu = None;
        self.video_decode.clear();
        self.composite = CompositePath::CpuSimd;
        self.preview_precision = PreviewPrecision::F32Cpu;
    }

    /// Pick a decode path for one clip. ProRes never matches a hardware profile.
    pub fn pick_decode(&self, codec: &str, chroma: &str, bit_depth: u8, w: u32, h: u32) -> DecodePath {
        if codec_key(codec).contains("prores") {
            return DecodePath::Cpu { threads: self.decode_pool_threads };
        }
        if let Some(d) = self.video_decode.iter().find(|d| d.covers(codec, chroma, bit_depth, w, h)) {
            DecodePath::Hw { api: d.api.clone(), keep_on_gpu: matches!(self.composite, CompositePath::GpuTexture) }
        } else {
            DecodePath::Cpu { threads: self.decode_pool_threads }
        }
    }

    /// Size a sequential prefetch depth from decoded frame bytes and available RAM.
    pub fn prefetch_depth(&self, bytes_per_frame: usize, budget: usize) -> usize {
        size_prefetch(bytes_per_frame, self.ram_available, budget)
    }

    /// Install the process-wide rayon pool (`max(1, cores−2)`). Harmless if already set.
    pub fn install_rayon(&self) {
        install_rayon_pool(self.decode_pool_threads);
    }

    /// JSON for Help ▸ System Compatibility / the Performance readout.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

/// Blumofe/Leiserson: one work-stealing pool; leave 2 cores for UI/audio.
pub fn pool_threads(cores: u32) -> u32 {
    cores.saturating_sub(2).max(1)
}

/// Prefetch depth: `budget / bytes_per_frame` and `RAM/8 / bytes_per_frame`, clamped 2–32.
pub fn size_prefetch(bytes_per_frame: usize, ram_available: u64, budget: usize) -> usize {
    let bpf = bytes_per_frame.max(1);
    let from_budget = budget / bpf;
    let from_ram = (ram_available / 8).saturating_div(bpf as u64) as usize;
    from_budget.min(from_ram).clamp(2, 32)
}

/// Install rayon's global pool once. Failures (already initialised, OS refusal) are logged.
pub fn install_rayon_pool(threads: u32) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let n = threads.max(1) as usize;
        if let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .thread_name(|i| format!("ec-cpu-{i}"))
            .panic_handler(|_| log::error!("a CPU job panicked; that work was skipped"))
            .build_global()
        {
            log::info!("playback: rayon global pool not replaced ({e})");
        } else {
            log::info!("playback: rayon pool {n} threads (cores−2)");
        }
    });
}

fn simd_level() -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        let mut v = Vec::new();
        if is_x86_feature_detected!("sse2") {
            v.push("sse2".into());
        }
        if is_x86_feature_detected!("ssse3") {
            v.push("ssse3".into());
        }
        if is_x86_feature_detected!("avx2") {
            v.push("avx2".into());
        }
        if is_x86_feature_detected!("avx512f") {
            v.push("avx512f".into());
        }
        v
    }
    #[cfg(target_arch = "aarch64")]
    {
        vec!["neon".into()]
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Vec::new()
    }
}

/// Ampere GA104 published NVDEC limits (Video Codec SDK 13.1). Used only as a *cap* on a
/// probed D3D11VA profile, never to invent a decoder that the OS did not report.
pub fn ampere_published_limits(codec: &str) -> Option<(u32, u32)> {
    match codec_key(codec).as_str() {
        "h264" | "avc" | "avc1" => Some((4096, 4096)),
        "hevc" | "h265" | "hvc1" | "hev1" => Some((8192, 8192)),
        "vp9" | "av1" => Some((8192, 8192)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_leaves_two_cores() {
        assert_eq!(pool_threads(24), 22);
        assert_eq!(pool_threads(4), 2);
        assert_eq!(pool_threads(1), 1);
        assert_eq!(pool_threads(2), 1);
    }

    #[test]
    fn prefetch_clamps_and_uses_bytes_per_frame() {
        // 108 MiB f32 DOOH frame, 1 GiB budget, 16 GiB RAM.
        let d = size_prefetch(108 << 20, 16 << 30, 1 << 30);
        assert_eq!(d, 9); // 1024/108 = 9
        assert_eq!(size_prefetch(1, 0, 1 << 20), 2);
        assert_eq!(size_prefetch(64, u64::MAX, usize::MAX), 32);
    }

    #[test]
    fn prores_never_picks_hw() {
        let mut c = PlaybackCaps::probe_host();
        c.with_decode(vec![DecodeCaps {
            api: "d3d11va".into(),
            codec: "h264".into(),
            chroma: "420".into(),
            bit_depth: 8,
            min_w: 64,
            min_h: 64,
            max_w: 4096,
            max_h: 4096,
        }]);
        assert!(matches!(c.pick_decode("ProRes 422 HQ", "422", 10, 6880, 1032), DecodePath::Cpu { .. }));
        assert!(matches!(c.pick_decode("h264", "420", 8, 1920, 1080), DecodePath::Hw { .. }));
        // Ampere H.264 max 4096: 6880-wide H.264 stays on the CPU.
        assert!(matches!(c.pick_decode("h264", "420", 8, 6880, 1032), DecodePath::Cpu { .. }));
    }

    #[test]
    fn device_lost_falls_back_to_cpu() {
        let mut c = PlaybackCaps::probe_host();
        c.with_gpu(GpuCaps {
            vendor: "NVIDIA".into(),
            device: "RTX 3070 Ti".into(),
            backend: "Dx12".into(),
            vram_bytes: Some(8 << 30),
            vendor_id: 0x10DE,
            nvenc: true,
            f16_storage: true,
        });
        c.with_decode(vec![DecodeCaps {
            api: "d3d11va".into(),
            codec: "hevc".into(),
            chroma: "420".into(),
            bit_depth: 10,
            min_w: 144,
            min_h: 144,
            max_w: 8192,
            max_h: 8192,
        }]);
        assert_eq!(c.composite, CompositePath::GpuTexture);
        c.clear_gpu();
        assert!(c.gpu.is_none());
        assert!(c.video_decode.is_empty());
        assert_eq!(c.composite, CompositePath::CpuSimd);
        assert!(matches!(c.pick_decode("hevc", "420", 10, 1920, 1080), DecodePath::Cpu { .. }));
    }

    #[test]
    fn pick_from_times_prefers_cpu_when_gpu_is_slower() {
        let mut c = PlaybackCaps::probe_host();
        c.with_gpu(GpuCaps {
            vendor: "NVIDIA".into(),
            device: "RTX 3070 Ti".into(),
            backend: "Dx12".into(),
            vram_bytes: Some(8 << 30),
            vendor_id: 0x10DE,
            nvenc: true,
            f16_storage: true,
        });
        assert_eq!(c.composite, CompositePath::GpuTexture);
        // Kapildev's 6880 readback path: GPU 6.5 fps (~154 ms) vs CPU ~30 fps (~33 ms).
        c.pick_from_times(33.0, 154.0);
        assert_eq!(c.composite, CompositePath::CpuSimd);
        c.pick_from_times(88.0, 20.0);
        assert_eq!(c.composite, CompositePath::GpuTexture);
    }

    #[test]
    fn probe_host_never_panics() {
        let c = PlaybackCaps::probe_host();
        assert!(c.cpu_cores >= 1);
        assert!(c.decode_pool_threads >= 1);
        let _ = c.to_json();
    }
}
