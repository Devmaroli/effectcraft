# effectcraft-screens

Canonical Kuwait DOOH screen library for EffectCraft's Screen Suite panel.

Inventory comes from the studio planner (`screens-data.js`, 133 faces) with
`studio-overrides.json` applied in memory: Piccadilly is **2027×720**, Al Salam Sync
accepts **1536×576 and 3072×576**, Al Nassar / Khalijiya are **1536×576**, Tawfeer is
**1200×960**. Comps are always **25 fps**. Configured combiner duplication (Al Salam
1536×576 × 2 → 3072×576, Marina Palm Trees 240×960 × 4 → 960×960) is the normal
deliverable and does not warn. Extra source comps beyond a combiner's configured
slots are stacked vertically and flagged (expected vs actual size, extra piece count).

This crate is L0 (serde only, wasm-safe). Commands live in `effectcraft-engine`.
Provenance: the user's Screen Suite bundle (planner + AE tools), licensed to
EffectCraft under MIT OR Apache-2.0.
