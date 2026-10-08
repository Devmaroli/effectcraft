# Screen Suite — analysis (no implementation)

Status: **analysis only**. Nothing from the After Effects Screen Suite, the DOOH planner, or
`studio-overrides.json` has been ported. This document is the design brief so a UX direction can
be chosen first. Do not treat it as a shipping spec until that choice is recorded.

Sources (read-only, not in this repo): the attached `ae_scripts_bundle` (Launch Pad, Screen
Manager v2.7 / config v2.6, Screen Adapter 1.8.11, SizeMaster 1.8.0, Freeze, Screenshot, Video
Renamer, planner v1.0.87, `studio-overrides.json`, `reference.md`, `tools.md`). Behaviour is
taken from those tools and the studio notes — not from Adobe’s code.

Studio rules that every option below should honour:

- Comps and deliveries are **25 fps**.
- Default spot length is **10 s**. **15 s** venues: AL Kout, 360 Mall, Khairan Mall (SizeMaster
  also sets Warehouse Mall to 15 s; Khairan Outdoor stays 10 s). The planner’s Size matcher still
  *flags* anything other than 10 s — that is a QC warning, not a build rule.
- Paste name-map **before** Screen Manager: **Top Gear → Baitak**, **Avenues Quartz → Diamond**.
- Where planner / Screen Manager / SizeMaster / Adapter disagree on a pixel size,
  **`studio-overrides.json` wins**. Remaining conflicts are listed in §4, not guessed.

EncodeCraft (`encodecraft.queue`) is the Media Encoder stand-in wherever `reference.md` §17
hands off to AME.

---

## 1. What the suite is

A Kuwait DOOH production pipeline, not a generic “screen tools” pack.

```
booking names  →  Size sorter (gate 1: any flag → stop)
               →  paste list (+ name-map)
               →  Screen Manager / Adapter / SizeMaster  (build comps)
               →  layout, bilingual AR/EN
               →  freeze / stills / rename
               →  EncodeCraft or Render Queue @ 25 fps, exact W×H
               →  Size matcher (gate 2: FAIL → fix and re-export until PASS)
```

Two different classes of tool share one inventory:

| Class | Tools | Job |
|---|---|---|
| **Planner** (browser) | Screen database, Size sorter, Production list, Size matcher | Turn a booking into sizes; QC exported files |
| **Build** (AE palettes) | Launch Pad, Screen Manager + Combiner, Screen Adapter, SizeMaster, Freeze, Screenshot, Video Renamer | Make comps, retarget layouts, stills, names |

The planner is the **first and last** gate. Screen Manager’s MISSING/close-match report is useful
but is **not** the final check (`tools.md` §1c).

---

## 2. Shared screen library (the one data module to build)

A native port should **not** keep three JSON lists in sync. Today:

| Source | Count | Role |
|---|---|---|
| Planner `screens-data.js` | **133** named faces | Sorter + matcher inventory |
| `AE_ScreenManager_Config.json` | **84** presets | Comp-builder targets |
| `SizeMaster_Config.json` | **75** | One-click resize of the active comp |
| `ScreenAdapter_Config.json` | **72** | Tagged re-layout targets (no `duration` in the shipped file) |
| `AE_ScreenManager_Combiners.json` | **15** layouts | Multi-face master comps |
| `studio-overrides.json` | overlays | Confirmed corrections, custom screens, not-in-planner groups |

### Proposed crate

`effectcraft-screens` (L0/L2, serde-only, wasm-safe):

- Canonical **screen** `{ id, name, aliases[], group, w, h, duration_s, fps: 25, flags }`
- **Pools** (1.7HD = 1920×1080, 2.6 = 1536×576, 4.3 = 2624×608 minus Thuraya, Marina Balcony =
  1200×240, Avenues QD = 1440×1800) plus special pools (Avenues Frame, Yaal/Slayel Indoor)
- **Associated groups** / combiners (Thuraya pair → 3584×608, … — see §3 Screen Manager)
- Runtime merge of **studio overrides** (never silently edit the shipped inventory)
- `notInPlanner` groups stay buildable, matcher marks them **NOT CHECKABLE** (not a fail)
- `customScreens` (today: **Tawfeer 1200×960**) exist for matcher + optional SM paste

Commands would *use* this crate; the UI would not own a fourth copy of the sizes.

### Override-backed truth (confirmed 2026-09-29)

| Screen | Use this | Do not use |
|---|---|---|
| Piccadilly face | **2027×720** (combined **6080×720**) | Planner `2026×720` |
| Al Salam Sync | **1536×576 and 3072×576** both valid | Planner-only 1536 as the sole deliverable |
| Al Nassar Tower / Khalijiya | **1536×576** (planner + `reference.md` §13) | SM config **2688×1152** / **2304×1008** |
| Tawfeer | **1200×960**, 10 s, 25 fps | Not in planner/SM until added as custom |
| Marina Palm Trees | **4 × 240×960 → 960×960** | Inventory “6 screens” / matcher `faceCount: 6` |
| Quartz | Same pixels as SM preset **Diamond** (1440×1800) | A “Quartz” SM preset (there isn’t one) |
| Top Gear | Same 2624×608 pool as **Baitak**; paste as Baitak | A “Top Gear” SM preset (there isn’t one) |

**Not in planner** (acknowledged): AL Kout, 360 Mall, Khairan Mall, Warehouse Mall, Dayarti,
Print, Custom. Sorter still emits paste names; matcher prints NOT CHECKABLE.

---

## 3. Per tool

Each subsection: what it does, inputs/options, edge cases, EffectCraft mapping. Closest
1:1 command ids are existing ones; proposed ids are *illustrative* until UX is chosen.

### 3.1 Size sorter (planner tab)

**Does.** Paste booking screen names → match to inventory → distil by production size → emit a
paste list for Screen Manager.

**Inputs / options**

- Paste: one name per line; `,` and `;` also split.
- **Flexible** (default): associated groups, aliases, related words, fuzzy ≥45%.
- **Strict:** exact names or exact pixel sizes only.
- **Send all screens** off by default: one 1.7HD + one 2.6 covers those pools; Al Salam Sync
  stays its own pair.
- Buttons: Copy results, Excel, **Copy names only**, Save TXT, Send to production list, Send to matcher.
- Copy-names format: one name per line, no header; shared formats as `1.7HD` / `2.6`; combined
  groups as `<Group> Combined`; 2624×608 → `Baitak` (or `Top Gear` if that is the only booked
  4.3); 1440×1800 → Diamond/Quartz names.

**Edge cases**

- Flags (`needsReview`): confidence &lt;68%, duplicate screen in the paste (≥88%), ambiguous
  matches, unmatched lines. **Any flag → stop and ask** (`tools.md` gate 1). Never auto-resolve.
- `360 ` paste hazard is a *Screen Manager* parser bug (leading digits stripped); sorter should
  still warn (SM-PASTE-PARTIAL).
- Name-map Top Gear / Quartz is applied **after** copy-names, before SM paste.

**Map.** `screen.sorter.parse {lines, mode: flexible\|strict, sendAll?}` → `{rows, flags,
pasteNames}`. UI: a Booking panel or a dialog; not a browser extension. Inventory from
`effectcraft-screens`. Headless: same command from MCP / CLI (replaces `sort.mjs`).

### 3.2 Size matcher (planner tab) — final QC gate

**Does.** After **all** exports, check files against the booking. PASS is the only “final”.

**Inputs / options**

- Selected screens (from sorter or search) + an export folder (recursive).
- Toggles: Separate faces (off); Aspect only (off — never for final); Check images; Screen-specific /
  Send all; Multi Spot (spot × screen board).
- Bilingual **always on**: `_AR` / `_EN` in name or `AR/` `EN/` folders; untagged = General.
- Expected duration **10 s** (flag, not auto-size-fail). Studio wrapper also checks **fps ≠ 25**,
  duration label vs real length, non-square pixels (`[STUDIO RULE]`).
- Combined default sizes: Thuraya 3584×608, Platinum 2880×1008, Marina Crescent 960×1440,
  Piccadilly 6080×720, Palm Trees 960×960, Eye of Kuwait 7560×2100, Kuwait Gate 4460×378
  (also 4459×378). Separate-faces: exact preferred, ±5 px flagged.
- Yaal/Slayel also need filename match ≥80%.

**Edge cases**

- Combined vs face files: a 2027 face in combined mode is “outside”; 2026 fails against override.
- NOT CHECKABLE groups are not FAIL.
- Unresolved sorter flags in the booking JSON fail the matcher until `--sorter-flags-cleared`.
- Aspect-only is for rough packs only.

**Map.** `screen.matcher.check {booking, folder, separateFaces?, aspectOnly?, checkImages?,
multiSpot?}`. Probe files with FilmCraft/`effectcraft-media` (not ffmpeg-linked). Report as a
panel + Excel-like CSV/XLSX export. Desktop-only (folder walk). Do not embed the Edge PWA.

### 3.3 Screen database / production list

**Does.** Search/filter the 133-face inventory; a running production-size list.

**Map.** Read-only views of `effectcraft-screens` (search, group, size). Production list can be
session state (`serde` UI) or a small project sidecar — decide with UX.

### 3.4 Launch Pad (`ScreenSuite_LaunchPad.jsx` v1.22)

**Does.** Dockable strip that `evalFile`s six palettes (Alt+1…6). Discovers `.jsx` by filename
pattern under `toolsRoot`. Installs a stub into AE’s ScriptUI Panels (`install_dock_panel.ps1`,
admin, writes into Adobe’s Program Files).

**Inputs / options.** Layout breakpoints (stack / rail / bar / icons); Install / Remove dock;
focus an already-open tool instead of relaunching.

**Edge cases.** Missing tool → disabled `!` button. `Screen Suite.root.json` in the bundle still
points at `C:\Users\FriendName\Documents\ScreenSuite` (placeholder). Bare 1–6 only if the pad
stole focus (collision with AE resolution shortcuts).

**Map.** **Do not port the stub installer or `$.evalFile`.** EffectCraft already has dockable
panels, Window menu, workspaces, and a command registry. The pad is a *navigation* problem
(§6), not a script host. Keyboard: register `screen.suite.focus {tool}` if a multi-tool panel
is chosen.

### 3.5 Screen Manager + Combiner (`AE_ScreenManager_Unified_V2_6.jsx`, UI **v2.7**)

**Does.** Select footage → select presets (list, filter, **paste box**) → optionally **Create
Comps** → **Apply to Footage**. Matches footage to presets by size, builds one 25 fps comp per
preset, time-stretches video to 10 s or 15 s, and when every column of a combiner layout is
selected, builds one master mosaic. Renames source items. Excel report (Summary / Screen Report /
Comps Created / Log).

**Inputs / options**

- Prefix / Suffix; JSON/CSV import/export of presets; filter; group dropdown (DOOH, Marina,
  Avenues, AL Kout, 360 Mall, Khairan, Warehouse, Dayarti, Print).
- Paste box: **Select pasted names** / Clear. **Replaces** the whole selection and **writes
  `AE_ScreenManager_Config.json`**.
- Create Comps (default **off**); Comp FPS 25 (fixed); Tolerance px (default 60, reporting);
  Custom duration 0.1–600 s (**10 s groups only**; 15 s venues never overridden); Aspect Scale
  to Fit ±2%.
- Combiners tab: 15 layouts, column editor, Save/Reload JSON.

**Paste parser**

1. Split on newline / `,` / `;`.
2. Strip list markers — **including `1–4 digits + space`**, unless the token looks like a size
   (`1234x567`, `1.7HD`, ordinal `1st`). **Hazard:** `360 Grand entrance` → `Grand entrance`.
3. `PASTE_SELECTION_ALIASES` (e.g. `2.6` → 2.6 Screen; `Thuraya Combined` → both Thuraya
   presets; `Marina Palms Full` → Palm Trees; Piccadilly Sync; Topaz Sync; …).
4. Exact normalized preset name, else unique partial (≥4 chars, **exactly one** hit).
5. Unmatched names in an alert.

**Comp rules**

| | |
|---|---|
| FPS | 25 |
| Duration | 15 s if group ∈ {AL Kout, 360 Mall, Khairan Mall}; else 10 s or custom |
| Video | Time stretch to target; stills just fill the comp |
| Smart scale | max Δ ≤ **3 px** → non-uniform scale to size |
| Aspect downscale | optional; source ≥ target both axes; aspect ±2%; uniform min scale |
| Slayel and Yaal | Comp **1920×1080**, footage rotated **90°** (preset stored 1080×1920) |
| Naming | `[prefix\|source]_[Preset]_[NN]Sec[_suffix]`; combined `[prefix]_<Layout>_[NN]Sec[_suffix]` |
| Duplicate | Exact comp name already exists → skip |
| Guide | Locked guide text “SCALING AND RETIMING NOTES (GUIDE)” (do not render) |

**Combiner (15 layouts)** — activate only if **every** column’s screen is selected; those
screens leave the individual pass. Column match ±**2 px**. Width = sum of columns; height =
max. h-dup / v-dup; Thuraya side vAlign top (68 px gap). Piccadilly: `forceScale`, `compW`
6080, 3× 2027×720. Al Salam: 2× 1536×576 → 3072×576.

Layouts: `Thuraya_Full` 3584×608, `Boursa_Full` 3840×720, `Al_Zawaya_Full` 1900×720,
`Platinum_Screen` 2880×1008, `Al_Meera_Full` 3600×1008, `Blajat_Full` 3600×864,
`Avenues_Gate_Full` 7488×864, `Marina_Crescent_Sync` 960×1440, `Marina_Palms_Full` 960×960
(4×240×960), `Mourouj_Wall_AB` 1680×1440, `Mourouj_Wall_C` 1680×1920, `Capital_HUB` 2880×630,
`Piccadilly_Sync` 6080×720, `Topaz_Sync` 1728×864, `Al_Salam_Sync` 3072×576.

**Footage report.** Per selected preset: exact W×H vs closest; MISSING vs Close (≤ tolerance);
PAR warning if `pixelAspect ≠ 1`.

**Edge cases**

- No AME in this script (export is a later step).
- Nine presets named “Sultan Column” (different sizes) — paste partial match can hit none.
- Combiner `Marina - Palm Trees (6 screens)` vs preset `Marina - Palm Trees` → missing-preset
  warning; JSON `match` still used at runtime.
- SM config still has **outdated** Al Nassar / Khalijiya — a port must not copy those pixels.
- Writes the user’s config JSON on almost every checkbox; a native port should keep selection
  in **session / project** unless the user hits Save Library.

**Map.**

- `screen.manager.selectPasted {names}` (apply name-map + aliases; warn on 360-strip / NOT FOUND)
- `screen.manager.apply {createComps, aspectFit, customDuration?, prefix?, suffix?}`
- Reuse: `comp.new` (w/h/fps/duration), `layer.timeStretch`, `layer` transform/rotation,
  `project.newFolder`, `project.duplicate`, `layer.setComment` for notes.
- Combiner: new comps + positioned precomp or footage layers (column layout from the crate).
- Report: `screen.manager.report` → JSON the UI can save as xlsx/csv.
- EncodeCraft is **after** comps exist, not inside Apply.

### 3.6 Screen Adapter (`ScreenAdapter.jsx` v1.8.11)

**Does.** Tag layers on a **master** comp (`@bg @logo @headline @cta @product @panel` plus
overrides) → generate one retargeted comp per ticked screen into `ScreenAdapter_Output`.

**Inputs / options**

- Tabs: Tag / Generate / Log.
- Tag: role, adapt (auto/keep/stretch/cover/contain), pin, flush T/B/L/R, keep-clear + gap px/%,
  orientation class, product zone `%` + side, product fit.
- Generate: search, select all/none, **Select from pasted** (names or `WxH` — **no** SM alias
  table, so `Thuraya Combined` may be “Not found”), Load sizes JSON (session only; ticks “name
  by pixel size”), Back to saved list, Prefix/Suffix, Name by pixel size, **Fork nested
  precomps** (off), Duration from size list vs master.
- Crop ROI on Tag tab (mutates the **source** precomp).

**Tag grammar (high level)**

- Aliases: `background→bg`, `brand→logo`, `text/copy→headline`, `hero/fit/safe/image/visual→product`,
  `block→panel`, `scaleP/sp`.
- Defaults: bg cover, panel stretch, others keep.
- Overrides: `.keep .stretch .cover .contain`, pins `.tl…`, `.flushb` / `.ground`, `.clear` /
  `.clear24` / `.clear5p`, aspect `.h .v .uv…`, `@product.60.r`.
- Unknown tokens ignored. Animated layers are **not** written: parented to `<layer> ADAPT` null
  so the motion path is mapped. Master sampled at **playhead**. Controller nulls without a role
  are copied if a descendant is tagged.

**Edge cases**

- Fork depth cap 40; `comp("Old")` rewrite is literal-name only.
- Crop ROI: full-frame ROI skipped; 3D content pre-composed first; expression-locked
  position/anchor skipped.
- AE `copyToComp` + parent + undo group: Adapter detaches parent for the copy. EffectCraft
  `edit.duplicate` / `layer.precompose` do not have that AE restriction — map to native copy
  without the detach hack unless we find an equivalent bug.
- Shipped Adapter JSON has **no** durations; 15 s malls need either the size-list toggle or a
  merged library that carries SM/SizeMaster durations.

**Map.**

- Store tags in **`layer.comment`** (or a small `dooh/tags` property group) — **not** by
  rewriting `layer.name`, so Project/timeline stay readable. Keep a compatibility parse of
  `@role` in the name for imported AE masters.
- `screen.adapter.tag {layer, role, …}` / `screen.adapter.generate {screens, fork?, durationFrom,
  prefix?, suffix?}`
- Placement is new L4 math (master box as fractions of frame → target rect, then
  keep/cover/contain, flush, clear). Tests can port the **behaviour** of
  `tools/tag-grammar.test.js` as Rust unit tests without copying the JSX.
- Fork: `project.duplicate` per nested comp + expression rewrite for `comp("…")` via the
  existing expression engine (boa), with the same literal-name limit documented.
- Crop ROI: `view.setRegionOfInterest` + `comp.cropToRegionOfInterest`, then a command that
  re-places selected precomp layers (2D first; 3D as a later slice).

### 3.7 SizeMaster (`SizeMaster.jsx` v1.8.0)

**Does.** Click a screen → resize the **active** comp; optionally uniform scale-to-fit + centre
layers; optionally rename the comp to the screen name.

**Inputs / options.** Category filter, search, Scale to fit (default on), Rename comp (default
off). Library CRUD + Save/Load JSON.

**Duration (differs from Screen Manager).** 15 s for groups 360 Mall, Warehouse Mall, AL Kout,
Khairan **except** name contains `Outdoor` (10 s). **Every other screen: duration is left
alone.** SM always writes 10/15.

**Edge cases.** No active comp → error. Duplicate library names blocked. `PAIR_DEFS` mentions
Al Nassar Tower + Vertical but config only has Vertical. Name typos in the JSON (`Aveneus`,
`Avnues`) — fix in the canonical library, do not reproduce. `selected` in JSON is unused.

**Map.** `screen.sizeMaster.apply {screen, scaleToFit?, rename?}` → `comp.settings` + layer
scale/position. Same library as SM; duration policy must be **one** table (flag the Warehouse
Mall / “keep duration” split in the UI as a checkbox defaulting to SM’s 10/15 if we unify).

### 3.8 Freeze frame (`Freezframe Script.jsx` v8)

**Does.** (1) Set playhead of **every comp in the project** to N seconds (0–20, clamped to last
frame). (2) Native **Freeze Frame** on every video layer of **selected** comps.

**Inputs.** Slider 0–20 s. Two buttons. Skips adjustment layers / no-video; restores shy/lock.

**Edge cases.** Freeze uses **current** time on each opened comp — run playhead first. Combined
masters: all slot layers at the same time (`reference.md` §17). Stills/text skip. Menu id
fallback 2036 is AE-specific.

**Map.** `comp.setTime` / editor CTI per comp; `layer.freezeFrame` (already exists — time
remapping at CTI). Batch: `screen.freeze {seconds, comps: all\|selected, freeze?: bool}`.
Do not call a menu command id. After Screen Manager retime, then freeze, then EncodeCraft
(§17 order).

### 3.9 Screenshot (`Screenshot Script.jsx`)

**Does.** Selected footage → one comp per file → shared time → **full-res PNG** per comp.
Stills: ≥5 s @ 25 fps. Never overwrite (`name.png`, `name_2.png`, …). Session folder remembered.

**Inputs.** Time field/slider (default 3 s, slider 0–60, field can exceed). Create comps /
Save PNGs. Fallback capture targets: selected comps → session comps → active comp.

**Edge cases.** Time past the end → last frame. Video comps keep source fps/duration. Needs
write permission in AE; EffectCraft just writes.

**Map.** `file.newCompFromSelection` / `comp.new` + `comp.saveFrameAs {path, time}`. Batch
command `screen.screenshot {time, folder, items?}`. Unique filenames in the command.

### 3.10 Video Renamer (`VideoRenamer_AE.jsx` v5)

**Does.** Batch rename **disk files** (folder of names containing `_`) **or** selected project
items, by underscore parts: swap text, lock parts, set part N, insert segment, suffix.
Preview (≤80 rows) + confirm (≤8 examples). Sample:
`Jazeera Airways_Salala_1St Ring Road_10 Sec_En`.

**Edge cases.** Folder mode **renames on disk** (destructive). AE mode is undoable item
rename only. Insert shifts lock indices. Whole-part vs substring swap. Language swap En→Ar
is a common studio move; duration labels in the name must stay true (`[STUDIO RULE]`).

**Map.** `screen.rename.preview` / `screen.rename.apply {source: folder\|selection, …}`. Disk
mode is desktop-only and should stay behind an explicit confirm. Prefer EffectCraft
`project` item rename for in-app work; disk rename is a post-export utility (pairs with matcher).

### 3.11 `install_dock_panel.ps1`

AE Program Files writer. **Not ported.** Window ▸ panel + workspaces replace it.

---

## 4. Conflicts to surface in the UI, not silently fix

| Topic | Sources | Port rule |
|---|---|---|
| Piccadilly 2026 vs 2027 | Planner inventory vs overrides + SM | **2027 / 6080** |
| Al Nassar / Khalijiya | SM JSON outdated vs planner + §13 | **1536×576** |
| Al Salam 1536 vs 3072 | Planner row vs combiner + overrides | **both accepted** in matcher |
| Palm Trees 6 vs 4 | Inventory label vs §14 | **4 faces, 960×960** |
| Duration 10 vs 15 vs “keep” | Matcher vs SM vs SizeMaster | Build with SM 10/15 table; matcher flags ≠10 s; SizeMaster “keep” as an option |
| 133 faces vs 84 SM presets | Pools vs named presets | Library holds both; sorter emit pools; SM paste expands aliases |
| Adapter paste | No SM aliases | Share one alias table |
| `360 ` list-marker | SM parser | Stop stripping 3-digit tokens that match a preset prefix; warn |

---

## 5. How this sits on EffectCraft

```
L0/L2  effectcraft-screens     inventory, pools, combiners, overrides
L3     media / export          probe W×H×duration×fps; PNG/H.264
L4     engine commands         screen.*  + existing comp/layer/RQ/encodecraft
L5     ui-egui panels          chosen UX from §6
L6     apps                    desktop folder matcher; wasm gets library+sorter only
```

Reuse rather than reinvent:

| Need | Existing |
|---|---|
| New sized comp | `comp.new` / `comp.settings` (`fps` alias) |
| Stretch / freeze | `layer.timeStretch`, `layer.freezeFrame` |
| Still PNG | `comp.saveFrameAs` |
| Nested copy | `project.duplicate`, `layer.precompose` |
| ROI crop | `view.setRegionOfInterest`, `comp.cropToRegionOfInterest` |
| Folders | `project.newFolder`, `project.move` |
| Layer annotation | `layer.setComment`, `layer.rename` |
| In-app encode | `renderQueue.add` / `renderQueue.render` |
| AME-style queue | **`encodecraft.queue`** (saved + clean project) |
| Agent drive | same command ids on control channel / MCP |

Wasm: sorter + library + adapter math can run. Matcher folder walk, disk rename, EncodeCraft
launch stay desktop (`encodecraft.queue` already errors clearly on wasm).

---

## 6. Candidate UX (choose one)

These are whole-suite directions, not per-tool skins. All three still use the **same commands
and `effectcraft-screens`**. The difference is where the operator lives in the app.

### A — Dockable Screen Suite panel (Launch Pad analog)

One `PanelKind::ScreenSuite` (Window ▸ Screen Suite), tabbed or accordion:

1. Booking (sorter + flags + paste list)
2. Build (preset checklist + combiners + Apply / SizeMaster apply)
3. Adapter (tag + generate)
4. Deliver (freeze, screenshot, rename, EncodeCraft / RQ)
5. QC (matcher)

**For:** Matches how the user works today (one dock, Alt+n muscle memory). Dense DOOH session
without hunting menus. Easy to snapshot and agent-drive (`screen.suite.tab`).

**Against:** A fifth mega-panel next to Project/Timeline. Easy to become a “script UI in
egui.” Adapter tagging wants Timeline selection — split attention unless the panel is a
narrow inspector. Sorter+matcher are *ops* tools that do not need an open comp.

### B — Menu commands + dialogs

Composition / File ▸ Screen Suite ▸ each tool as a command that opens a focused dialog
(paste booking, apply presets, generate adapter, batch PNG, rename). Launch Pad becomes a
menu. Library editing under a Settings page or File ▸ Screen Library.

**For:** Maximum EffectCraft-native: everything is already a command. Lightest UI. Best for
agents (`screen.manager.apply {…}` with no panel). Matches “don’t bolt on a script runner.”

**Against:** The real workflow is *stateful* (booking flags, selected presets, session screenshot
folder, matcher board). Dialogs re-ask that state or dump it into prefs. Combiner editing and
the matcher wall are poor as one-shot dialogs. Keyboard discovery is worse than a dock.

### C — “Deliverables” workspace

A saved workspace (like Standard / All Panels): Project + Viewer + Timeline, plus **Booking**,
**Screen Library**, **Deliverables** (queue of comps to encode), **QC**. Screen Manager Apply
is a button on the Library panel; Adapter tags show in the Timeline (comment column / badges)
with a small Generate bar. EncodeCraft/RQ is the Deliverables panel. Matcher is a QC panel
that points at an export folder.

**For:** Feels like EffectCraft, not a floating AE palette. Booking → build → encode → QC is a
place, not a modal. Studio could live in this workspace all day. Maps cleanly to “everything
is a command” *and* “look at the result.”

**Against:** Heaviest first implementation (new `PanelKind`s + `dock::workspace("Deliverables")`).
Must get window-save/load right (existing workspace machinery). Users who only want “resize
this comp” pay for a workspace switch.

### Hybrid worth considering

**C for layout, A for density:** ship the Deliverables workspace whose extra panels *are* the
Screen Suite surfaces, and also list those panels under Window so they can dock into Standard.
Menus still wrap every action (B) so MCP/CLI work with the panel closed. That is three
surfaces on one command set — more work, but it is the pattern EffectCraft already uses
(Render Queue panel + Composition ▸ Add to Render Queue + `renderQueue.add`).

---

## 7. EncodeCraft handoff — security and robustness review

Reviewed: `crates/engine/src/commands/encodecraft.rs`, vendored `encodecraft-job`, inbox
write, process spawn. Findings by severity. **Fixed** items have tests in this fork
(`cargo test -p effectcraft-engine encodecraft` — 14 tests).

### High

| Finding | Status |
|---|---|
| `TcpStream::connect` had no timeout; a dropped SYN to localhost could freeze the UI thread | **Fixed:** `connect_timeout` 800 ms + read/write timeouts |

### Medium

| Finding | Status |
|---|---|
| macOS `.app` launch used `Command::new("open")` (PATH hijack) | **Fixed:** `/usr/bin/open` only, and only if that file exists |
| Inbox `fs::write` could overwrite or follow `{id}.json` | **Fixed:** `create_new`, unique suffix, Unix dir `0o700` / file `0o600` |
| Sibling `encodecraft` next to the GUI | **Fixed:** absolute existing file; Unix sibling must share the current exe uid |
| HTTP 503 treated as “server down” → silent inbox write | **Fixed:** non-2xx is an error |
| Launch retries could block several seconds on the command thread | **Mitigated:** 4 × 200 ms with connect timeouts. Still synchronous (same as other engine commands) |

### Low

| Finding | Status |
|---|---|
| `output` / `format` accepted control chars and `..` | **Fixed:** absolute output, no `..`; format is a short `[A-Za-z0-9_-]` token |
| Invalid `ENCODECRAFT_INBOX` fell through to the default directory | **Fixed:** set-but-invalid env → no inbox |
| Command result echoed up to 64 KiB of HTTP body | **Fixed:** truncate to 1 KiB in the result |
| URL CRLF / `@` / non-loopback / IPv6 / `https://` / path `..` | **Already rejected** (tests) |

### Accepted (not treated as bugs)

- `inbox` / `output` command params can be any absolute path, like `file.saveAs`.
- A same-uid binary named `encodecraft` beside the GUI is trusted (if an attacker can plant
  that, they can replace `effectcraft`).
- Loopback IPC is **token-gated** (`X-EncodeCraft-Token` / `ENCODECRAFT_TOKEN` /
  `<data dir>/ipc-token`). GET `/health` stays unauthenticated so we can detect a running
  encoder. A mock server that enforces token, loopback Host, and Origin / Sec-Fetch-Site
  rules is covered by engine tests.

Screen Suite must not weaken this: freeze/screenshot/rename must not spawn PATH binaries; matcher
folder paths should be absolute user-chosen directories; EncodeCraft remains save-first.

---

## 8. What we will not do (until UX is chosen)

- No JSX in-tree, no ScriptUI host, no Adobe ScriptUI Panels installer.
- No copy of `screens-data.js` as a browser app inside EffectCraft.
- No silent size guesses: flags stay flags.
- No port of a single tool “to show progress” that would lock in panel vs menu vs workspace.

After a direction is picked, implementation order that preserves the studio gates:

1. `effectcraft-screens` + overrides + conflict tests (Piccadilly, Nassar, Al Salam, Tawfeer).
2. Sorter + paste list (gate 1) + name-map.
3. Screen Manager apply + combiners (25 fps, 10/15 s, Yaal rotation).
4. EncodeCraft from the deliverables list (`encodecraft.queue` per comp).
5. Matcher (gate 2).
6. Adapter tags + generate.
7. SizeMaster, freeze, screenshot, renamer as the same command set.

---

## 9. Open questions for the UX choice

1. **A, B, C, or C+A hybrid** (§6)?
2. Unify SizeMaster “keep duration” with SM’s always-10/15, or keep a checkbox?
3. Tags in `layer.comment`, a hidden property group, or visible `@role` in the layer name
   (AE-compatible)?
4. Should the matcher live in-app, or stay a headless `effectcraft-cli screen.matcher` plus a
   thin panel?
5. Is the planner’s Excel export required in v1, or is JSON + a CSV enough?
