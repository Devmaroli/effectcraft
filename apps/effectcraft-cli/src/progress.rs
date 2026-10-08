//! Machine-readable JSON-line events on stderr (CLI) or a TCP writer (serve).

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

/// Write one JSON object and a newline. Failures are dropped (a closed pipe is not a crash).
pub fn emit(event: &Value) {
    let _ = writeln!(std::io::stderr(), "{event}");
    let _ = std::io::stderr().flush();
}

/// A progress callback that emits `{"event":"frame","n":N,"total":T}` from the first frame
/// (`n` is 1-based) and stops when `cancel` is set.
pub fn on_frame<'a>(cancel: &'a AtomicBool, mut events: impl FnMut(Value) + 'a) -> impl FnMut(&effectcraft_export::Progress) -> bool + 'a {
    move |pr: &effectcraft_export::Progress| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if pr.done > 0 {
            events(json!({
                "event": "frame",
                "n": pr.done,
                "total": pr.total,
                "elapsed": (pr.elapsed * 1000.0).round() / 1000.0,
            }));
        }
        true
    }
}

pub fn header_json(info: &effectcraft_export::StreamInfo, gpu: bool, out: &str, audio_out: Option<&str>) -> Value {
    json!({
        "event": "header",
        "version": 1,
        "width": info.width,
        "height": info.height,
        "fps": info.fps,
        "frames": info.frames,
        "pixFmt": info.pix_fmt.as_str(),
        "bytesPerFrame": info.bytes_per_frame,
        "audio": info.audio,
        "sampleRate": info.sample_rate,
        "audioChannels": info.audio_channels,
        "gpu": gpu,
        "comp": info.comp,
        "out": out,
        "audioOut": audio_out,
    })
}
