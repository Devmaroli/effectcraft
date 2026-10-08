//! Compute-pipeline compilation: core kernels at device init, effect families on first use.
//!
//! Compiling every effect kernel up front is what made the first GPU use stall for ~10 s on
//! Windows (FXC / a huge concatenated WGSL module). Playback of footage only needs the
//! compositing kernels in [`ENTRIES`]; the rest wait until an effect actually runs. wgpu's
//! pipeline-cache API is `unsafe` (Vulkan-only) so this crate does not call it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::context::{ENTRIES, EXT_ENTRIES, init_resource};

/// Common WGSL prepended to every family (types and helpers).
const COMMON: &str = include_str!("shaders/common.wgsl");

fn pack(family: &'static str) -> String {
    [COMMON, family].concat()
}

/// One WGSL family: its source and the compute entry points it defines.
struct Family {
    label: &'static str,
    src: fn() -> String,
    kernels: &'static [&'static str],
    /// Bind group 1 (classic 3D).
    ext: bool,
}

fn families() -> &'static [Family] {
    static F: OnceLock<Vec<Family>> = OnceLock::new();
    F.get_or_init(|| {
        vec![
            Family { label: "core", src: || pack(include_str!("shaders/kernels.wgsl")), kernels: ENTRIES, ext: false },
            Family { label: "classic3d", src: || pack(include_str!("shaders/classic3d.wgsl")), kernels: EXT_ENTRIES, ext: true },
            Family { label: "sky", src: || pack(include_str!("shaders/sky.wgsl")), kernels: crate::adv3d::SKY_KERNELS, ext: false },
            Family { label: "fx_color", src: || pack(include_str!("shaders/fx_color.wgsl")), kernels: crate::fx_color::KERNELS, ext: false },
            Family { label: "fx_distort", src: || pack(include_str!("shaders/fx_distort.wgsl")), kernels: crate::fx_distort::KERNELS, ext: false },
            Family { label: "fx_generate", src: || pack(include_str!("shaders/fx_generate.wgsl")), kernels: crate::fx_generate::KERNELS, ext: false },
            Family { label: "fx_key", src: || pack(include_str!("shaders/fx_key.wgsl")), kernels: crate::fx_key::KERNELS, ext: false },
            Family { label: "fx_stylize", src: || pack(include_str!("shaders/fx_stylize.wgsl")), kernels: crate::fx_stylize::KERNELS, ext: false },
            Family { label: "fx_noise", src: || pack(include_str!("shaders/fx_noise.wgsl")), kernels: crate::fx_noise::KERNELS, ext: false },
            Family { label: "fx_tone", src: || pack(include_str!("shaders/fx_tone.wgsl")), kernels: crate::fx_tone::KERNELS, ext: false },
            Family { label: "fx_warp", src: || pack(include_str!("shaders/fx_warp.wgsl")), kernels: crate::fx_warp::KERNELS, ext: false },
            Family { label: "fx_extra", src: || pack(include_str!("shaders/fx_extra.wgsl")), kernels: crate::fx_extra::KERNELS, ext: false },
            Family { label: "fx_depth", src: || pack(include_str!("shaders/fx_depth.wgsl")), kernels: crate::fx_depth::KERNELS, ext: false },
            Family { label: "fx_lut", src: || pack(include_str!("shaders/fx_lut.wgsl")), kernels: crate::fx_lut::KERNELS, ext: false },
            Family { label: "fx_sim", src: || pack(include_str!("shaders/fx_sim.wgsl")), kernels: crate::fx_sim::KERNELS, ext: false },
            Family { label: "fx_particles", src: || pack(include_str!("shaders/fx_particles.wgsl")), kernels: crate::fx_particles::KERNELS, ext: false },
            Family { label: "fx_vr", src: || pack(include_str!("shaders/fx_vr.wgsl")), kernels: crate::fx_vr::KERNELS, ext: false },
            Family { label: "fx_light", src: || pack(include_str!("shaders/fx_light.wgsl")), kernels: crate::fx_light::KERNELS, ext: false },
            Family { label: "fx_transition", src: || pack(include_str!("shaders/fx_transition.wgsl")), kernels: crate::fx_transition::KERNELS, ext: false },
            Family { label: "fx_text", src: || pack(include_str!("shaders/fx_text.wgsl")), kernels: crate::fx_text::KERNELS, ext: false },
            Family { label: "fx_time", src: || pack(include_str!("shaders/fx_time.wgsl")), kernels: crate::fx_time::KERNELS, ext: false },
            Family { label: "fx_pixel2", src: || pack(include_str!("shaders/fx_pixel2.wgsl")), kernels: crate::fx_pixel2::KERNELS, ext: false },
            Family { label: "fx_gen2", src: || pack(include_str!("shaders/fx_gen2.wgsl")), kernels: crate::fx_gen2::KERNELS, ext: false },
        ]
    })
    .as_slice()
}

/// Kernels compiled before the first frame (2D composite + display). Effect families wait.
pub const EAGER: &[&str] = &["fill", "warp_blend", "blend_full", "matte", "preserve", "knockout", "channel_mix", "quantize", "convert", "half", "unpack"];

/// Compiled compute pipelines for one device.
pub(crate) struct Kernels {
    layout: wgpu::PipelineLayout,
    layout_ext: wgpu::PipelineLayout,
    modules: Mutex<HashMap<&'static str, wgpu::ShaderModule>>,
    pipes: Mutex<HashMap<&'static str, wgpu::ComputePipeline>>,
}

impl Kernels {
    pub(crate) fn new(device: &wgpu::Device, layout: wgpu::PipelineLayout, layout_ext: wgpu::PipelineLayout) -> Result<Kernels, String> {
        let k = Kernels { layout, layout_ext, modules: Mutex::new(HashMap::new()), pipes: Mutex::new(HashMap::new()) };
        for e in EAGER {
            k.pipeline(device, e).ok_or_else(|| format!("GPU initialization ({e}): kernel missing"))?;
        }
        Ok(k)
    }

    /// Pipelines created so far (eager plus any effect family that has run).
    pub(crate) fn compiled(&self) -> usize {
        self.pipes.lock().map(|p| p.len()).unwrap_or(0)
    }

    pub(crate) fn pipeline(&self, device: &wgpu::Device, entry: &str) -> Option<wgpu::ComputePipeline> {
        if let Ok(p) = self.pipes.lock()
            && let Some(pipe) = p.get(entry)
        {
            return Some(pipe.clone());
        }
        let fam = families().iter().find(|f| f.kernels.contains(&entry))?;
        let key = fam.kernels.iter().copied().find(|k| *k == entry)?;
        let module = self.module(device, fam)?;
        let layout = if fam.ext { &self.layout_ext } else { &self.layout };
        let pipe = init_resource(device, key, || {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(key),
                layout: Some(layout),
                module: &module,
                entry_point: Some(key),
                compilation_options: Default::default(),
                cache: None,
            })
        })
        .map_err(|e| log::error!("{e}"))
        .ok()?;
        if let Ok(mut p) = self.pipes.lock() {
            p.insert(key, pipe.clone());
        }
        Some(pipe)
    }

    fn module(&self, device: &wgpu::Device, fam: &Family) -> Option<wgpu::ShaderModule> {
        if let Ok(m) = self.modules.lock()
            && let Some(module) = m.get(fam.label)
        {
            return Some(module.clone());
        }
        let src = (fam.src)();
        let module = init_resource(device, fam.label, || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(fam.label), source: wgpu::ShaderSource::Wgsl(src.into()) })
        })
        .map_err(|e| log::error!("{e}"))
        .ok()?;
        if let Ok(mut m) = self.modules.lock() {
            m.insert(fam.label, module.clone());
        }
        Some(module)
    }
}

/// Named compute entry points the compositor knows (core + every effect family).
#[cfg(test)]
fn kernel_names() -> impl Iterator<Item = &'static str> {
    families().iter().flat_map(|f| f.kernels.iter().copied())
}

#[cfg(test)]
mod tests {
    #[test]
    fn eager_kernels_are_in_the_core_family() {
        for e in super::EAGER {
            assert!(super::ENTRIES.contains(e), "{e} is not a core compositing kernel");
        }
        let names: Vec<_> = super::kernel_names().collect();
        assert!(names.len() > super::EAGER.len() + 8, "effect families should outnumber eager kernels");
        let mut seen = std::collections::BTreeSet::new();
        for n in &names {
            assert!(seen.insert(*n), "duplicate kernel name {n}");
        }
    }
}
