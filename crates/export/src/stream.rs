//! Raw video and WAV audio streams for EncodeCraft: frames go to a writer (stdout, a named pipe,
//! or a file) with no temp movie. Progress is reported after every written frame, starting with
//! frame 1, so a consumer can show `n/total` from the first frame.

use std::io::Write;

use effectcraft_project::Comp;
use effectcraft_project::render_queue::{AudioFormat, Channels};
use effectcraft_raster::Image;
use web_time::Instant;

use crate::{Cx, ExportError, Job, Progress, Report, Result, State, io, output_size, wants_audio};

/// Pixel layout of a raw video stream (FFmpeg `-f rawvideo -pix_fmt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixFmt {
    /// Planar BT.709 limited-range 8-bit 4:2:0 (Y, then U, then V). Fastest typical pipe.
    Yuv420p,
    /// Packed 8-bit RGB, no alpha.
    Rgb24,
    /// Packed 8-bit RGBA (straight alpha over the requested channels).
    Rgba,
}

impl PixFmt {
    pub fn as_str(self) -> &'static str {
        match self {
            PixFmt::Yuv420p => "yuv420p",
            PixFmt::Rgb24 => "rgb24",
            PixFmt::Rgba => "rgba",
        }
    }

    pub fn parse(s: &str) -> Option<PixFmt> {
        match s.trim().to_ascii_lowercase().as_str() {
            "yuv420p" | "yuv420" | "raw" => Some(PixFmt::Yuv420p),
            "rgb24" | "rgb" => Some(PixFmt::Rgb24),
            "rgba" | "rgb32" => Some(PixFmt::Rgba),
            _ => None,
        }
    }

    pub fn bytes_per_frame(self, w: u32, h: u32) -> u64 {
        let (w, h) = (w as u64, h as u64);
        match self {
            PixFmt::Yuv420p => {
                let y = w.saturating_mul(h);
                let c = w.div_ceil(2).saturating_mul(h.div_ceil(2));
                y.saturating_add(c.saturating_mul(2))
            }
            PixFmt::Rgb24 => w.saturating_mul(h).saturating_mul(3),
            PixFmt::Rgba => w.saturating_mul(h).saturating_mul(4),
        }
    }
}

/// What a consumer needs before the first frame (header / sidecar JSON).
#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: u64,
    pub pix_fmt: PixFmt,
    pub bytes_per_frame: u64,
    pub audio: bool,
    pub sample_rate: u32,
    pub audio_channels: u8,
    pub comp: String,
}

/// Size, rate and audio flags for a raw stream of `pix_fmt` (yuv420p is cropped to even).
pub fn stream_info(job: &Job<'_>, pix_fmt: PixFmt) -> Result<StreamInfo> {
    let comp = job.project.comp(job.comp).ok_or(ExportError::NoComp)?;
    let (w, h) = stream_size(comp, job, pix_fmt);
    let fps = job.settings.rate(comp).as_f64();
    let frames = job.settings.frame_count(comp);
    Ok(StreamInfo {
        width: w,
        height: h,
        fps,
        frames,
        pix_fmt,
        bytes_per_frame: pix_fmt.bytes_per_frame(w, h),
        audio: effectcraft_render::audio::comp_has_audio(job.project, job.comp),
        sample_rate: job.output.audio_sample_rate.clamp(8_000, 192_000),
        audio_channels: if job.output.audio_channels == 1 { 1 } else { 2 },
        comp: job.project.item(job.comp).map(|i| i.name.clone()).unwrap_or_default(),
    })
}

/// Render the job's frames as packed `pix_fmt` bytes, in order, flushing after each frame.
pub fn export_raw(job: &Job<'_>, pix_fmt: PixFmt, video: &mut dyn Write, progress: &mut dyn FnMut(&Progress) -> bool) -> Result<Report> {
    raw_run::block_on_raw(job, pix_fmt, video, progress)
}

/// Mix the job's audio span to a complete WAV (PCM 16-bit little-endian unless the module says
/// otherwise) and write it. A 10 s stereo 48 kHz mix is a couple of megabytes.
pub fn export_wav(job: &Job<'_>, audio: &mut dyn Write, progress: &mut dyn FnMut(&Progress) -> bool) -> Result<Report> {
    let t0 = Instant::now();
    let cx = Cx::new(job);
    let comp = cx.comp().ok_or(ExportError::NoComp)?;
    let total = cx.settings.frame_count(comp).max(1);
    let mut st = State::new(t0, total, progress);
    st.advance(0)?;
    let bytes = write_wav(&cx, comp, audio, &mut st)?;
    Ok(Report {
        path: job.path.to_string(),
        frames: total,
        width: 0,
        height: 0,
        seconds: t0.elapsed().as_secs_f64(),
        bytes,
        audio: true,
        log: None,
        overflow: vec![],
    })
}

fn stream_size(comp: &Comp, job: &Job<'_>, pix_fmt: PixFmt) -> (u32, u32) {
    let (mut w, mut h) = output_size(comp, job.settings, job.output);
    if pix_fmt == PixFmt::Yuv420p {
        w &= !1;
        h &= !1;
        w = w.max(2);
        h = h.max(2);
    }
    (w.max(1), h.max(1))
}

fn channels_for(pix_fmt: PixFmt, job: &Job<'_>) -> Channels {
    match pix_fmt {
        PixFmt::Rgba => job.output.channels,
        PixFmt::Rgb24 | PixFmt::Yuv420p => Channels::Rgb,
    }
}

fn pack_frame(pix_fmt: PixFmt, rgba: &[u8], w: u32, h: u32, y: &mut Vec<u8>, u: &mut Vec<u8>, v: &mut Vec<u8>, out: &mut Vec<u8>) -> Result<()> {
    let (wu, hu) = (w as usize, h as usize);
    let need = wu.saturating_mul(hu).saturating_mul(4);
    if rgba.len() < need {
        return Err(ExportError::Encode(format!("frame is {} bytes, expected {need} for {w}×{h}", rgba.len())));
    }
    match pix_fmt {
        PixFmt::Rgba => {
            out.clear();
            if let Some(px) = rgba.get(..need) {
                out.extend_from_slice(px);
            }
        }
        PixFmt::Rgb24 => {
            out.clear();
            out.reserve(wu.saturating_mul(hu).saturating_mul(3));
            for px in rgba[..need].as_chunks::<4>().0 {
                if let Some(rgb) = px.get(..3) {
                    out.extend_from_slice(rgb);
                }
            }
        }
        PixFmt::Yuv420p => {
            crate::encode::rgba_to_yuv420(rgba, wu, hu, y, u, v);
            out.clear();
            out.reserve(y.len().saturating_add(u.len()).saturating_add(v.len()));
            out.extend_from_slice(y);
            out.extend_from_slice(u);
            out.extend_from_slice(v);
        }
    }
    Ok(())
}

fn write_all(w: &mut dyn Write, buf: &[u8]) -> Result<()> {
    w.write_all(buf).map_err(io)?;
    w.flush().map_err(io)
}

fn write_wav(cx: &Cx<'_>, comp: &Comp, out: &mut dyn Write, st: &mut State<'_>) -> Result<u64> {
    use crate::encode::{mix, pcm_bytes};
    let sr = cx.output.audio_sample_rate.clamp(8_000, 192_000);
    let channels: u16 = if cx.output.audio_channels == 1 { 1 } else { 2 };
    let fmt = cx.output.audio_format;
    let bytes_per_sample: u16 = match fmt {
        AudioFormat::S16 => 2,
        AudioFormat::S24 => 3,
        AudioFormat::F32 => 4,
    };
    let (span_start, span_end) = cx.settings.span(comp);
    let start = span_start.to_units_floor(sr as i64);
    let end = span_end.to_units_floor(sr as i64);
    let n = (end - start).max(0) as usize;
    let mut pcm = Vec::new();
    let cap = n.saturating_mul(channels as usize).saturating_mul(bytes_per_sample as usize);
    if cap > 512 * 1024 * 1024 {
        return Err(ExportError::Unsupported("audio span is too long to mix in one pass".into()));
    }
    pcm.reserve(cap);
    let block = sr as usize;
    let mut done = 0usize;
    let total_frames = st.total.max(1);
    let mut reported = 0u64;
    while done < n {
        let k = block.min(n - done);
        let t = effectcraft_time::Tick((((start + done as i64) as i128 * effectcraft_time::TICKS_PER_SECOND as i128) / sr as i128) as i64);
        pcm.extend(pcm_bytes(&mix(cx, t, k, sr), fmt, false));
        done = done.saturating_add(k);
        let now = (done as u64).saturating_mul(total_frames) / (n.max(1) as u64);
        let now = now.min(total_frames);
        if now > reported {
            st.advance(now - reported)?;
            reported = now;
        }
    }
    if reported < st.total {
        st.advance(st.total - reported)?;
    }
    let tag: u16 = if fmt == AudioFormat::F32 { 3 } else { 1 };
    let block_align = channels.saturating_mul(bytes_per_sample);
    let mut header = Vec::with_capacity(44 + pcm.len());
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36u32.saturating_add(pcm.len() as u32).saturating_add((pcm.len() % 2) as u32)).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&tag.to_le_bytes());
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sr.to_le_bytes());
    header.extend_from_slice(&(sr.saturating_mul(block_align as u32)).to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&(bytes_per_sample.saturating_mul(8)).to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    header.extend_from_slice(&pcm);
    if pcm.len() % 2 == 1 {
        header.push(0);
    }
    write_all(out, &header)?;
    Ok(header.len() as u64)
}

/// A blocking raw export that still uses the shared frame pipeline (and its async GPU path).
mod raw_run {
    use super::*;
    use crate::batch_size;

    pub(super) fn block_on_raw(job: &Job<'_>, pix_fmt: PixFmt, video: &mut dyn Write, progress: &mut dyn FnMut(&Progress) -> bool) -> Result<Report> {
        effectcraft_render::passes::block_on(raw_async(job, pix_fmt, video, progress))
    }

    async fn raw_async(job: &Job<'_>, pix_fmt: PixFmt, video: &mut dyn Write, progress: &mut dyn FnMut(&Progress) -> bool) -> Result<Report> {
        let t0 = Instant::now();
        let cx = Cx::new(job);
        let comp = cx.comp().ok_or(ExportError::NoComp)?;
        let total = cx.settings.frame_count(comp);
        let (w, h) = stream_size(comp, job, pix_fmt);
        let channels = channels_for(pix_fmt, job);
        let mut st = State::new(t0, total, progress);
        st.advance(0)?;
        let mut y = Vec::new();
        let mut u = Vec::new();
        let mut v = Vec::new();
        let mut packed = Vec::new();
        let mut bytes = 0u64;
        // Frame 0 on its own so the first `frame 1/total` event is not stuck behind a batch.
        if total > 0 {
            let img = cx.frame(comp, 0);
            bytes = bytes.saturating_add(write_one(&cx, &img, comp, channels, pix_fmt, w, h, video, &mut y, &mut u, &mut v, &mut packed)?);
            st.advance(1)?;
        }
        let batch = batch_size();
        let mut i = 1u64;
        while i < total {
            let end = (i + batch).min(total);
            let ks: Vec<u64> = (i..end).collect();
            let frames: Vec<(u64, Image)> = cx.frames(comp, ks, |k, img| (k, img)).await;
            for (_k, img) in frames {
                bytes = bytes.saturating_add(write_one(&cx, &img, comp, channels, pix_fmt, w, h, video, &mut y, &mut u, &mut v, &mut packed)?);
                st.advance(1)?;
            }
            i = end;
        }
        Ok(Report {
            path: job.path.to_string(),
            frames: total,
            width: w,
            height: h,
            seconds: t0.elapsed().as_secs_f64(),
            bytes,
            audio: wants_audio(job),
            log: None,
            overflow: vec![],
        })
    }

    fn write_one(
        cx: &Cx<'_>,
        img: &Image,
        comp: &Comp,
        channels: Channels,
        pix_fmt: PixFmt,
        w: u32,
        h: u32,
        video: &mut dyn Write,
        y: &mut Vec<u8>,
        u: &mut Vec<u8>,
        v: &mut Vec<u8>,
        packed: &mut Vec<u8>,
    ) -> Result<u64> {
        let rgba = cx.pixels(img, comp, channels, w, h);
        pack_frame(pix_fmt, &rgba, w, h, y, u, v, packed)?;
        write_all(video, packed)?;
        Ok(packed.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuv420p_bytes_match_ffmpeg_planar() {
        assert_eq!(PixFmt::Yuv420p.bytes_per_frame(2, 2), 6);
        assert_eq!(PixFmt::Yuv420p.bytes_per_frame(6880, 1032), 6880 * 1032 + 2 * (3440 * 516));
        assert_eq!(PixFmt::Rgb24.bytes_per_frame(64, 48), 64 * 48 * 3);
        assert_eq!(PixFmt::Rgba.bytes_per_frame(64, 48), 64 * 48 * 4);
        assert_eq!(PixFmt::parse("raw"), Some(PixFmt::Yuv420p));
        assert_eq!(PixFmt::parse("RGB24"), Some(PixFmt::Rgb24));
        assert_eq!(PixFmt::parse("nope"), None);
    }
}
