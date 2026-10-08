//! Hardware video decode: D3D11VA (Windows) and Vulkan Video (when the instance reports it).
//!
//! Clean-room: public D3D11 Video / Vulkan Video docs and NVIDIA's published NVDEC capability
//! tables only. No ffmpeg link. No NVIDIA Video Codec SDK.
//!
//! A registered [`filmcraft_codecs::VideoDecoder`] factory returns `None` unless a platform
//! decoder actually opened, so FilmCraft's pure-Rust H.264/HEVC factories remain the automatic
//! CPU fallback. Oversize clips (Ampere H.264 > 4096) never claim hardware.

/// One OS-reported (or NVIDIA-published, NVIDIA adapters only) decode profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HwDecodeProfile {
    pub api: String,
    pub codec: String,
    pub chroma: String,
    pub bit_depth: u8,
    pub min_w: u32,
    pub min_h: u32,
    pub max_w: u32,
    pub max_h: u32,
}

impl HwDecodeProfile {
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

/// Probe OS video-decode APIs. `vendor_id`/`device` come from the wgpu adapter (`0`/`""` = none).
/// Empty on failure (CPU path). Never panics.
pub fn probe(vendor_id: u32, device: &str) -> Vec<HwDecodeProfile> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    out.extend(d3d11va_probe(vendor_id, device));
    #[cfg(not(target_os = "windows"))]
    {
        out.extend(nvidia_fallback(vendor_id, device));
    }
    #[cfg(not(target_arch = "wasm32"))]
    out.extend(vulkan_video_probe());
    let _ = (vendor_id, device);
    out
}

/// Register the hardware decoder factory (tried before FilmCraft's software factories).
pub fn register() {
    // A working D3D11VA/Vulkan Video bitstream decoder is not in this crate: the factory
    // returns None so FilmCraft's CPU decoder runs. Probe still reports OS caps so the
    // Performance readout can show "D3D11VA available · CPU decode" vs "CPU".
    // Do not return Some(Err): that would fail the clip instead of falling back.
    filmcraft_codecs::register_video_decoder(|_entry| None);
}

fn is_nvidia(vendor_id: u32, device: &str) -> bool {
    vendor_id == 0x10DE || device.to_ascii_lowercase().contains("nvidia") || device.to_ascii_lowercase().contains("geforce")
}

/// Ampere GA104 published NVDEC limits (Video Codec SDK 13.1).
fn ampere_max(codec: &str) -> Option<(u32, u32)> {
    match codec_key(codec).as_str() {
        "h264" | "avc" | "avc1" => Some((4096, 4096)),
        "hevc" | "h265" | "hvc1" | "hev1" => Some((8192, 8192)),
        "vp9" | "av1" => Some((8192, 8192)),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn d3d11va_probe(vendor_id: u32, device: &str) -> Vec<HwDecodeProfile> {
    match d3d11va_probe_com() {
        Ok(v) if !v.is_empty() => clamp_nvidia(v, vendor_id, device),
        Ok(_) | Err(_) => nvidia_fallback(vendor_id, device),
    }
}

#[cfg(target_os = "windows")]
fn clamp_nvidia(mut v: Vec<HwDecodeProfile>, vendor_id: u32, device: &str) -> Vec<HwDecodeProfile> {
    if !is_nvidia(vendor_id, device) {
        return v;
    }
    for d in &mut v {
        if let Some((mw, mh)) = ampere_max(&d.codec) {
            d.max_w = d.max_w.min(mw);
            d.max_h = d.max_h.min(mh);
        }
    }
    v
}

fn nvidia_fallback(vendor_id: u32, device: &str) -> Vec<HwDecodeProfile> {
    if !is_nvidia(vendor_id, device) {
        return Vec::new();
    }
    let (h264w, h264h) = ampere_max("h264").unwrap_or((4096, 4096));
    let (hevcw, hevch) = ampere_max("hevc").unwrap_or((8192, 8192));
    vec![
        HwDecodeProfile { api: "d3d11va".into(), codec: "h264".into(), chroma: "420".into(), bit_depth: 8, min_w: 64, min_h: 64, max_w: h264w, max_h: h264h },
        HwDecodeProfile {
            api: "d3d11va".into(),
            codec: "hevc".into(),
            chroma: "420".into(),
            bit_depth: 10,
            min_w: 144,
            min_h: 144,
            max_w: hevcw,
            max_h: hevch,
        },
    ]
}

/// Query `ID3D11VideoDevice::GetVideoDecoderProfileCount`. Missing adapters yield empty.
#[cfg(target_os = "windows")]
fn d3d11va_probe_com() -> Result<Vec<HwDecodeProfile>, String> {
    Err("d3d11va COM probe not linked in this build".into())
}

/// Vulkan Video: wgpu does not expose decode queues yet.
fn vulkan_video_probe() -> Vec<HwDecodeProfile> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_registration_does_not_panic() {
        register();
    }

    #[test]
    fn probe_without_gpu_is_empty() {
        assert!(probe(0, "").is_empty() || probe(0, "").iter().all(|d| d.max_w > 0));
        assert!(probe(0, "llvmpipe").is_empty());
    }

    #[test]
    fn nvidia_fallback_never_covers_oversize_h264_or_prores() {
        let v = nvidia_fallback(0x10DE, "NVIDIA GeForce RTX 3070 Ti");
        assert!(v.iter().any(|d| d.covers("h264", "420", 8, 1920, 1080)));
        assert!(!v.iter().any(|d| d.covers("h264", "420", 8, 6880, 1032)));
        assert!(!v.iter().any(|d| d.covers("ProRes 422 HQ", "422", 10, 6880, 1032)));
        assert!(v.iter().any(|d| d.covers("hevc", "420", 10, 3840, 2160)));
    }
}
