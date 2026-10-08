//! `--gpu` attaches the shared GPU compositor (`Gpu` as `Accelerator`) with automatic CPU
//! fallback, including an 8 GB VRAM budget for 6880×1032. EncodeCraft streaming uses
//! `Accelerator::comp_frame` (viewer present uses `Gpu::render_display`). No second compositor.

use std::sync::Arc;

use effectcraft_engine::Session;
use effectcraft_gpu::Gpu;
use serde_json::{json, Value};

pub struct GpuSlot {
    gpu: Option<Arc<Gpu>>,
}

impl GpuSlot {
    /// Attach a headless GPU compositor when `--gpu` is set. Missing adapters, or a frame that
    /// would not fit, leave the session on the CPU and emit a `gpu` event — they are not errors.
    pub fn attach(s: &mut Session, want: bool, events: &mut dyn FnMut(Value)) -> GpuSlot {
        if !want {
            s.accel = None;
            return GpuSlot { gpu: None };
        }
        match Gpu::headless() {
            Some(g) => {
                let g = Arc::new(g);
                s.accel = Some(g.clone() as Arc<dyn effectcraft_render::Accelerator>);
                GpuSlot { gpu: Some(g) }
            }
            None => {
                events(json!({
                    "event": "gpu",
                    "used": false,
                    "reason": "no usable GPU adapter; rendering on the CPU",
                }));
                s.accel = None;
                GpuSlot { gpu: None }
            }
        }
    }

    /// Drop the GPU for this job when the output size is beyond the device or the 6 GiB working
    /// set (8 GB RTX 3070 Ti class, leaving headroom for the OS).
    pub fn fit(&self, s: &mut Session, width: u32, height: u32, events: &mut dyn FnMut(Value)) -> bool {
        let Some(g) = &self.gpu else {
            return false;
        };
        if g.can_composite(width, height) {
            if s.accel.is_none() {
                s.accel = Some(g.clone() as Arc<dyn effectcraft_render::Accelerator>);
            }
            return true;
        }
        s.accel = None;
        events(json!({
            "event": "gpu",
            "used": false,
            "reason": format!("{width}×{height} exceeds the GPU texture or 6 GiB working-set budget; rendering on the CPU"),
            "width": width,
            "height": height,
        }));
        false
    }

    pub fn used(&self, s: &Session) -> bool {
        self.gpu.is_some() && s.accel.is_some()
    }
}
