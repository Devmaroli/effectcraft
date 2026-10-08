# Testing

```sh
cargo test --workspace     # everything
cargo xtask ci             # fmt, clippy -D warnings, tests, layers, assets, wasm
```

## What the tests cover

- **Engine commands** (`crates/engine/src/tests*.rs`): every command family, including undo and
  redo, and that every menu entry resolves to a registered command.
- **Rendering** (`crates/render`): pixel checks on small compositions, for blend modes, masks,
  mattes, time remapping, 3D projection, lighting and shadows. Fast paths (the layer cache, the
  affine compositor, blurs, motion blur accumulation) are checked against their reference
  implementations.
- **Effects** (`crates/effects`): each effect has a determinism test and at least one behaviour
  test (identity at neutral settings, known pixel results, or simulations giving the same frame
  whether you seek straight to it or play up to it). A registry-wide test checks that effects
  which read the clock are declared time-dependent. `crates/effects/tests/sim_golden.rs` pins
  exact pixel hashes of the simulation effects the GPU shares plans with (CC Rainfall … Card
  Wipe) at several settings, times, bit depths and resolutions, so any change to their CPU output
  fails (re-pin with `SIM_GOLDEN_PRINT=1` after an intended change; pinned for aarch64 macOS).
- **Export** (`crates/export/tests`): every format is encoded and decoded back, checking frame
  count, size and pixels. When `ffmpeg`/`ffprobe` are installed they are used as an outside
  check; they are never linked or shipped.
- **Automation** (`crates/automation`, `apps/effectcraft-cli/tests`): MCP protocol round trips and
  the command-line tool's JSON output.
- **Interface** (`crates/ui-egui`): headless egui_kittest tests for panels and dialogs.

## Looking at the interface without a window

```sh
cargo run -p effectcraft-ui-egui --example snapshot -- --out ui.png \
  --step '{"method":"engine.execute","params":{"command":"layer.select","params":{"layers":["#2"]}}}'
```

Each `--step` is a control-channel request; `{"method":"snap","params":{"path":"x.png"}}` writes an
intermediate image.

## Benchmarks

```sh
cargo run --release -p effectcraft-cli -- bench --n 10 --play 30
cargo run --release -p effectcraft-cli -- bench --dooh --play 25 --gpu
cargo run --release -p effectcraft-cli -- bench --playback-profile --play 8 --gpu --json --out /tmp/playback-profile.json
```

`bench` prints per-layer and per-effect timings for one frame, and playback timings with and
without the layer cache. `bench --dooh` builds 25 fps digital-out-of-home compositions
(1920×1080, 3072×576, 6080×720, 960×960) with a video plate, stills, a title, Gaussian Blur /
Levels / Glow and blend modes, then reports achieved fps against 25 fps at Full, Half and
Quarter resolution. `--serial` is the old one-layer-at-a-time walk; `--gpu` tries the wgpu
compositor (skips with a note when this machine has no adapter). `--json` writes a machine-
readable report. `bench --playback-profile` generates short ProRes HQ 6880×1032 and H.264
1080p clips with ffmpeg (external oracle only) and reports decode, composite, viewer-upload
stand-in and end-to-end frame times, plus a 4-layer 25 fps comp; `--out` writes JSON.
First numbers (Linux cloud VM, 4 cores, no GPU) are in [gaps.md](gaps.md) G5:
Full 1080p and 6080×720 were not real-time; Half was real-time on all four sizes.
