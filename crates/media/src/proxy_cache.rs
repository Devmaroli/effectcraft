//! Automatic JPEG half-resolution proxies for ProRes HQ and oversize/heavy files.
//!
//! Stored in a size-capped cache folder. Playback may use them; Render Queue and EncodeCraft
//! handoff keep the originals (`ProxyUse::UseNone` on export). Codec: JPEG (the study's JPEG
//! option — engine does not depend on the export crate's ProRes encoder). NVENC is probed on
//! the GPU caps but is not used: no NVIDIA Video Codec SDK.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use effectcraft_project::{Footage, FootageKind, ItemId};
use effectcraft_raster::Image;
use effectcraft_render::FootageSource;
use effectcraft_time::Tick;

use crate::{MediaError, Result};

/// Default proxy-cache budget: 20 GiB.
pub const DEFAULT_MAX_BYTES: u64 = 20 << 30;

/// Progress of one background transcode (0–1000 = 0–100%).
#[derive(Clone, Debug)]
pub struct ProxyJob {
    pub path: String,
    pub item: u64,
    pub progress: u32,
    pub status: ProxyStatus,
    pub proxy_path: Option<PathBuf>,
    pub error: Option<String>,
    /// `file.setProxy` already pointed the Project item at `proxy_path`.
    pub attached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyStatus {
    Queued,
    Running,
    Ready,
    Failed,
    Cancelled,
}

/// Background JPEG half-res proxy generator, size-capped.
#[derive(Clone)]
pub struct ProxyCache {
    inner: Arc<Inner>,
}

struct Inner {
    folder: Mutex<PathBuf>,
    max_bytes: std::sync::atomic::AtomicU64,
    jobs: Mutex<HashMap<String, ProxyJob>>,
}

impl ProxyCache {
    pub fn new(folder: PathBuf, max_bytes: u64) -> Self {
        ProxyCache {
            inner: Arc::new(Inner { folder: Mutex::new(folder), max_bytes: std::sync::atomic::AtomicU64::new(max_bytes.max(1 << 20)), jobs: Mutex::default() }),
        }
    }

    pub fn set_folder(&self, folder: PathBuf) {
        *lock(&self.inner.folder) = folder;
    }

    pub fn set_max_bytes(&self, bytes: u64) {
        self.inner.max_bytes.store(bytes.max(1 << 20), Ordering::Relaxed);
    }

    pub fn folder(&self) -> PathBuf {
        lock(&self.inner.folder).clone()
    }

    pub fn job(&self, path: &str) -> Option<ProxyJob> {
        lock(&self.inner.jobs).get(path).cloned()
    }

    pub fn job_for_item(&self, item: u64) -> Option<ProxyJob> {
        lock(&self.inner.jobs).values().find(|j| j.item == item).cloned()
    }

    pub fn jobs(&self) -> Vec<ProxyJob> {
        lock(&self.inner.jobs).values().cloned().collect()
    }

    pub fn mark_attached(&self, path: &str) {
        if let Some(j) = lock(&self.inner.jobs).get_mut(path) {
            j.attached = true;
        }
    }

    pub fn running_count(&self) -> usize {
        lock(&self.inner.jobs).values().filter(|j| matches!(j.status, ProxyStatus::Queued | ProxyStatus::Running)).count()
    }

    /// ProRes (any profile), DNxHR/DNxHD, or frames larger than 1080p.
    pub fn should_auto(f: &Footage) -> bool {
        if !f.has_video || f.kind == FootageKind::Still || f.missing {
            return false;
        }
        let codec = f.codec.to_ascii_lowercase();
        let pixels = u64::from(f.width).saturating_mul(u64::from(f.height));
        codec.contains("prores") || codec.contains("dnx") || codec.contains("apch") || codec.contains("apcn") || pixels > 1920 * 1080
    }

    /// Queue a half-res JPEG. No-op if a job is already running or ready for `path`.
    pub fn start(&self, pool: Arc<dyn FootageSource>, footage: &Footage, item: ItemId) {
        if footage.path.is_empty() {
            return;
        }
        {
            let mut jobs = lock(&self.inner.jobs);
            if let Some(j) = jobs.get(&footage.path)
                && matches!(j.status, ProxyStatus::Queued | ProxyStatus::Running | ProxyStatus::Ready)
            {
                return;
            }
            jobs.insert(
                footage.path.clone(),
                ProxyJob { path: footage.path.clone(), item: item.0, progress: 0, status: ProxyStatus::Queued, proxy_path: None, error: None, attached: false },
            );
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = pool;
            if let Some(j) = lock(&self.inner.jobs).get_mut(&footage.path) {
                j.status = ProxyStatus::Failed;
                j.error = Some("automatic proxies need a native build".into());
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (cache, f) = (self.clone(), footage.clone());
            let spawned = std::thread::Builder::new().name("ec-proxy".into()).spawn(move || cache.run(pool.as_ref(), &f, item));
            if spawned.is_err()
                && let Some(j) = lock(&self.inner.jobs).get_mut(&footage.path)
            {
                j.status = ProxyStatus::Failed;
                j.error = Some("could not start proxy thread".into());
            }
        }
    }

    /// Remove the cached files for `path` (the Project item proxy is cleared by the caller).
    pub fn remove(&self, path: &str) {
        if let Some(j) = lock(&self.inner.jobs).remove(path)
            && let Some(p) = j.proxy_path
        {
            let _ = std::fs::remove_file(&p);
            if let Some(dir) = p.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    fn run(&self, pool: &dyn FootageSource, footage: &Footage, item: ItemId) {
        let path = footage.path.clone();
        let set = |st: ProxyStatus, prog: u32, proxy: Option<PathBuf>, err: Option<String>| {
            if let Some(j) = lock(&self.inner.jobs).get_mut(&path) {
                j.status = st;
                j.progress = prog;
                if proxy.is_some() {
                    j.proxy_path = proxy;
                }
                j.error = err;
            }
        };
        set(ProxyStatus::Running, 0, None, None);
        match transcode(pool, footage, item, &self.folder(), |p| {
            set(ProxyStatus::Running, p, None, None);
        }) {
            Ok(out) => {
                self.enforce_size_limit();
                set(ProxyStatus::Ready, 1000, Some(out), None);
            }
            Err(e) => set(ProxyStatus::Failed, 0, None, Some(e.to_string())),
        }
    }

    fn enforce_size_limit(&self) {
        let folder = self.folder();
        let max = self.inner.max_bytes.load(Ordering::Relaxed);
        let Ok(rd) = std::fs::read_dir(&folder) else { return };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
        let mut total = 0u64;
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if !m.is_file() {
                continue;
            }
            total = total.saturating_add(m.len());
            files.push((m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH), m.len(), p));
        }
        if total <= max {
            return;
        }
        files.sort_by_key(|(t, _, _)| *t);
        for (_, len, p) in files {
            if total <= max {
                break;
            }
            if std::fs::remove_file(&p).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cache_key(path: &str) -> String {
    let meta = std::fs::metadata(path).ok();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.as_ref().map(|m| m.len()), meta.and_then(|m| m.modified().ok())).hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Half-res JPEG of the first movie frame (one file). Sequences can be added later; a still
/// JPEG is enough for the Project-panel proxy and Half/Quarter playback of heavy plates.
fn transcode(pool: &dyn FootageSource, footage: &Footage, item: ItemId, folder: &Path, progress: impl Fn(u32)) -> Result<PathBuf> {
    progress(10);
    let img = pool.frame(item, footage, Tick::ZERO).ok_or_else(|| MediaError::Decode(format!("{}: no frame at start", footage.path)))?;
    progress(400);
    let (w, h) = (img.width.max(1), img.height.max(1));
    let (tw, th) = ((w / 2).max(1), (h / 2).max(1));
    let small = if tw == w && th == h { (*img).clone() } else { effectcraft_raster::resample(&img, tw, th) };
    progress(700);
    let jpeg = encode_jpeg(&small)?;
    let dir = folder.join(cache_key(&footage.path));
    std::fs::create_dir_all(&dir).map_err(|e| MediaError::Io(e.to_string()))?;
    let out = dir.join("proxy.jpg");
    atomic_write(&out, &jpeg)?;
    progress(1000);
    Ok(out)
}

fn encode_jpeg(img: &Image) -> Result<Vec<u8>> {
    let (w, h) = (img.width, img.height);
    let mut rgb = vec![0u8; w as usize * h as usize * 3];
    for (i, px) in img.data.iter().enumerate() {
        let a = px[3].clamp(0.0, 1.0);
        let unpre = |v: f32| -> u8 {
            let x = if a > 1e-6 { (v / a).clamp(0.0, 1.0) } else { 0.0 };
            (x * 255.0 + 0.5) as u8
        };
        let o = i.saturating_mul(3);
        if let Some(s) = rgb.get_mut(o..o + 3) {
            s[0] = unpre(px[0]);
            s[1] = unpre(px[1]);
            s[2] = unpre(px[2]);
        }
    }
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75);
    enc.encode(&rgb, w, h, image::ExtendedColorType::Rgb8).map_err(|e| MediaError::Decode(e.to_string()))?;
    Ok(out)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| MediaError::Io(e.to_string()))?;
        f.write_all(bytes).map_err(|e| MediaError::Io(e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| MediaError::Io(e.to_string()))
}

/// Probe a written JPEG as footage (caller attaches it with `file.setProxy`).
pub fn probe_proxy(path: &Path) -> Result<Footage> {
    crate::probe(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_auto_prores_and_oversize_not_hd_h264() {
        let mut f = Footage { has_video: true, kind: FootageKind::Video, width: 6880, height: 1032, codec: "ProRes 422 HQ".into(), ..Default::default() };
        assert!(ProxyCache::should_auto(&f));
        f.codec = "H.264".into();
        f.width = 1920;
        f.height = 1080;
        assert!(!ProxyCache::should_auto(&f));
        f.width = 3840;
        f.height = 2160;
        assert!(ProxyCache::should_auto(&f));
        f.kind = FootageKind::Still;
        assert!(!ProxyCache::should_auto(&f));
    }

    #[test]
    fn encode_jpeg_tiny_rgb() {
        let mut img = Image::new(4, 4);
        for p in &mut img.data {
            *p = [1.0, 0.0, 0.0, 1.0];
        }
        let j = encode_jpeg(&img).unwrap();
        assert!(j.len() > 32);
        assert_eq!(&j[..2], &[0xFF, 0xD8]);
    }
}
