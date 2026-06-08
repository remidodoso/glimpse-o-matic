# Glimpse-o-Matic — Notes & Status

## Naming

- Full name: **Glimpse-o-Matic** ('50s–'60s retro charm)
- Short abbreviation: **glim**
- Rust/WASM project name: **glimr**

---

## Design Notes

### Architecture

- Single-page photo gallery viewer; no build system, no framework (JS prototype phase)
- Canvas-based rendering with a double-buffer (`#photo` + hidden `#backing` canvas)
- CSS grid layout: portrait = header/gallery rows; landscape = header column left, gallery fills right
- `lobjet_pane` = "l'objet" (objet d'art) — the main image viewing area
- Rust + WASM target project: **glimr** (Cargo workspace at repo root)

### Purpose

A watermarked image distribution platform. Goals:
- Zero friction for viewers
- Surreptitious per-session source identification via watermarking
- Static hosted, client-side only

### Intended audience / use case

Three-party model: **photographer** (IP holder), **model** (subject, controls distribution), **patron** (viewer). One zip per gallery — no per-audience variants. All copies watermarked; model-mode copies carry a distinct mark rather than being unmarked.

### Watermarking — two-layer model

**Pack-time (zip contents)**
- Simple LSB mark baked into images at pack time
- Identifies the gallery/distribution; erased by re-encoding — low bar, intentional
- Provides minimal protection against direct zip extraction

**View-time (primary, applied by WASM)**
- Frequency-domain watermark (DCT/DWT spread-spectrum): robust, survives recompression/resizing, ~32–128 bit payload
- LSB mark: high capacity, fragile; multiple redundant copies prefixed by magic number — reader tool scans without needing to know placement offsets
- Applied order: decode → frequency-domain mark → LSB mark → blit to canvas
- In WASM era: image data stays in WASM linear memory, never surfaced as JS `Image` objects or blob URLs

### Session data gathered for watermark payload

Passive (no prompt): timestamp, IP (via lightweight outbound call), user-agent, screen/viewport dimensions, timezone, language, WebGL renderer string  
Active (permission requested): geolocation

### Obfuscation (zip contents)

- Each image XOR'd with `0xDEADBEEF` (4-byte cycling key)
- Files renamed to `[8-char hash].dat` before zipping (hash names sort alphabetically, preserving intended image order)
- WASM re-XORs on load; key compiled-in constant — casual friction, not cryptographic security
- Viewer also accepts plain `.jpg`/`.png`/etc. files in unobfuscated zips

### Tools directory (`tools/`)

- **packg** (implemented): takes a directory of `.jpg` files → XOR encodes → renames to `.dat` → zips. Flags: `-o`/`--output`, `-f`/`--force`. Prints summary to stdout, errors to stderr.
- **deployg** (implemented): creates self-contained gallery folder or uploads directly to Cloudflare R2. See deployg section below.
- **watermark-decode** (implemented): input suspected leaked image → blindly recovers the
  frequency-domain (DWT) payload — scale + crop offset auto-recovered, CRC verdict. Default is
  fully blind; `--size`/`--ref` fast overrides; `-v` verbose + live progress bar. See the
  Watermarking checkpoint. (LSB magic-number scan still TODO once the LSB layer ships.)
- Future: `gallery-config.toml` output from packg, read by WASM build step to bake constants

---

## Current UI — Implemented Features

### Layout

- CSS grid, responsive to viewport aspect ratio (`orientation: landscape` media query)
- **Portrait**: thumbnail carousel strip across the top, viewer fills remainder
- **Landscape**: thumbnail carousel column on the left (`auto` width), viewer fills right
- Carousel and viewer areas resize/rebuild on orientation change (detected via `window.resize` + `landscape_mq.matches` comparison — more reliable than `matchMedia` change event on mobile)
- Viewport meta tag present (`width=device-width, initial-scale=1`)

### Thumbnail Carousel

- Thumbnails sized to `min(18% of relevant viewport dimension, G_CAROUSEL_SIZE_MAX=160px)`
- Portrait: scaled to fixed height; landscape: scaled to fixed width — consistent cross-axis size
- Canvas elements initialised to 0×0 to avoid 300px default causing layout flash
- Drag-to-scroll (mouse + touch) on correct axis per orientation
- Scroll wheel scrolls carousel without changing selection
- Scrollbar hidden (`scrollbar-width: none` + `::-webkit-scrollbar`)
- Active thumbnail: red border, dimmed to 75% brightness
- Scrolls to keep active thumbnail visible on navigation and on orientation-change rebuild
- `add_thumbnail(i)` — single-entry function; called per-entry during streaming load and batched in `create_thumbnails()` for orientation-change rebuild. Calls `scroll_carousel_to(i)` inside the `createImageBitmap` callback (once the canvas has its real size) so orientation-change scrolls land correctly.

### Image Viewer

- Click/tap **left or right third**: navigate previous/next with slide animation (250ms ease-out)
- Click/tap **center third**: enter zoom mode at 1:1 pixel scale
- Click/tap **center third while in zoom mode**: exit zoom
- **Swipe/drag**: pan in zoom mode; slide-navigate in normal mode (25% threshold)
- **Hover indicators**: `<` / `>` arrows fade in/out on left/right thirds, idle-timeout fade

### Zoom Mode

- **Entry**: tap center (1:1), scroll wheel up, pinch outward, or Ctrl+= — all enter at fit-scale seamlessly
- **Exit**: tap while in zoom, scroll/pinch back to fit-scale (automatic), or press `0`
- Range: fit-scale (image fills viewport) → 2.0× (double pixel size)
- **Scroll wheel**: enters zoom if not already in it; zooms toward cursor; exits automatically at fit-scale
- **Pinch to zoom**: enters zoom if not already in it; zooms toward pinch midpoint; exits at fit-scale
- **Ctrl+= / Ctrl+−**: 25% steps toward viewport center; Ctrl+= enters zoom, Ctrl+− exits at fit-scale
- **Arrow keys in zoom mode**: pan image 80 screen-pixels per press
- Drag pans image (adjusted for zoom_scale)

### Loading Screen

- Shown on initial load and whenever a new zip is loaded
- Two lines of large text (`5vw`), wave-bounce CSS animation (per-character staggered delays)
- Hides when first image is decoded and drawn; reappears on new zip load
- Error div (`#progress-error`) shown inside loading screen on parse/fetch errors

### Floating Progress Bar (`#stream-progress`)

- Separate from the loading screen — floats over the image viewer while the archive is downloading
- Positioned above the action button bar, centred horizontally; `pointer-events: none` (transparent to all interaction)
- Styled like the action buttons: translucent white fill, `border: 1.5px solid rgba(255,255,255,0.55)`, `border-radius: 6px`, `box-shadow: 2px 4px 12px rgba(0,0,0,0.7)`
- Fill width tracks network bytes received / `Content-Length` (percentage visible when header is available)
- Fades out (opacity transition 0.4s) and hides when stream completes or errors
- Hidden while in zoom mode; restored on zoom exit or navigation
- `stream_loading` JS boolean tracks whether a load is in progress (used by zoom show/hide logic)

### Floating Action Buttons

Bottom-right corner, `position: fixed`, horizontal row: `[🖼] [⛶] [⬇] [i]`  
Bottom-left corner, `position: fixed`: `[α]`

**Sizing**: `min(max(100vw, 100vh) / 20, 48px)` — responsive, capped at 48px on desktop  
**Style**: white outline, white icon, transparent background, `border-radius: 12px`, drop shadow, 33% opacity by default  
**Flash animation**: tap → 100% opacity, eases back to 33% (or 60% if toggling on) over 0.35s

- **🖼 Load archive** (`btn-load`): opens file picker, loads selected zip via `File.stream()`
- **⛶ Fullscreen** (`btn-fullscreen`): toggles fullscreen; stays at 60% opacity while active
- **⬇ Download** (`btn-download`): downloads the **watermarked** current image as a
  high-quality JPEG (q 0.92). Pulls the native-resolution watermarked RGBA from WASM
  (`watermarked_pixels`), encodes JPEG in-browser via `OffscreenCanvas.convertToBlob`,
  forces a `.jpg` name, then saves. The un-watermarked source is never exported.
  - Chrome/Edge: `showSaveFilePicker` → native OS Save As dialog
  - Desktop Firefox: `window.confirm(name + size)` then silent download
  - Mobile/touch (`navigator.maxTouchPoints > 0`): direct download — browser's native download UI serves as confirmation; no extra dialog
- **i Info** (`btn-info`): shows info overlay with filename, dimensions, file size
- **α About** (`btn-about`): bottom-left; shows about overlay loaded from `about.html`

### Info Overlay

Modal overlay (`#info-overlay`) with filename, pixel dimensions, file size. Closes on backdrop click, × button, or pressing `i`.

### About Overlay

Modal overlay (`#about-overlay`) with the same rounded-corner, burlywood/bisque styling as the info box. Content loaded lazily from `about.html` via `fetch()` on first open; result cached for subsequent opens. Body has `max-height: 70vh; overflow-y: auto` for long content. If `about.html` is absent or fails to load, shows a neutral fallback message. Closes on backdrop click, × button, or any keypress. New zip load also closes it.

### Keyboard Shortcuts

- `←` / `→`: navigate; pan in zoom mode
- `↑` / `↓`: pan vertically in zoom mode
- `0`: exit zoom
- `f` / `F`: toggle fullscreen
- `i` / `I`: toggle info overlay
- Any key: close about overlay (if open)
- `Ctrl+=` / `Ctrl+−`: zoom in/out

### Logo Watermarks

Two `sip.png` instances overlay `#lobjet_pane` (`z-index: 2`, `pointer-events: none`):
- **Bottom**: centred horizontally, 12px from bottom
- **Left**: centred vertically, 12px from left, rotated 90°

### Build Script (`build.ps1`)

- Stamps `<!-- Build MMDD:HHMM -->` in `index.html` to bust browser cache
- `wasm-pack build glimr --target web --out-dir ../pkg`; removes `pkg/.gitignore`
- `cargo build --release -p packg -p deployg`; copies both to `tools/bin/`

### Dev Server

Local static serving is now handled by a separate external tool (outside this repo).
`server.py` (was: `python server.py` serving the project root on :8000 with
`Cache-Control: no-store`) has been **removed** — point your external server at the repo
root; just ensure no-store / no-cache during development so WASM/JS edits aren't stale.

---

## Rust/WASM Migration Plan

Goal: incrementally replace JS with Rust/WASM, keeping a working app at every step.

### Phase 1 — Image processing + zip handling in WASM ✓
XOR decode + zip parsing in WASM. `GlimrZip` struct (now removed). Single JS/WASM boundary crossing per archive. fflate CDN eliminated.

### Phase 2 — Canvas rendering in WASM (in progress, steps 1–7 done)
`draw()`, `draw_zoomed()`, `draw_image_in_column()` in WASM via `web-sys` canvas bindings. Decoded image bytes stay in WASM linear memory. JS still handles events.

### Phase 3 — State and event handling in WASM
Move state machine (current_index, zoom state, drag state, animation loops) to WASM. JS event listeners become thin wrappers.

### Phase 4 — Bootstrap only in JS
JS handles only: load WASM module, file picker, `fetch`, `requestFullscreen`. Everything else is WASM.

---

## Status — Milestones

- **Phase 1 WASM complete**: `xor_decode` + zip parsing in Rust; fflate CDN removed
- **deployg tool**: creates self-contained gallery folder; deployed to Wasabi S3 bucket initially
- **Phase 2 steps 1–7 complete**: `GlimrRenderer` wired to JS; only LSB watermark stub remains
- **Logging infrastructure**: `glimr_log` in both Rust and JS; bottleneck confirmed as `zune-jpeg` (~789ms/image after SIMD)
- **WASM SIMD**: `target-feature=+simd128` — ~2× speedup
- **Streaming zip (incremental batch)**: custom sequential parser (`parse_zip_streaming`) replaced `zip` crate; incremental rAF loop with progress bar
- **Hybrid decode complete**: JPEG decode moved from WASM (`zune-jpeg`) to browser (`createImageBitmap`). `image` crate removed — build time ~4s vs ~20s. `get_image_bytes` / `receive_pixels` / `is_decoded` API. `navigate_to()` + `decode_image()` JS pipeline. Concurrent thumbnail fill.
- **True network streaming complete**: `parse_zip_streaming` + incremental batch API replaced by a `StreamState` machine in WASM (`begin_zip_stream` / `feed_bytes` / `is_stream_done`). JS drives a `ReadableStream` pump — entries parsed and decoded as bytes arrive over the network. First image appears as soon as its bytes land. Both `fetch` and `File.stream()` use the same pump. Progress bar tracks network bytes received. Images added to carousel incrementally via `add_thumbnail(i)`. Neighbours of current image prefetched as they arrive.
- **Floating progress bar**: `#stream-progress` element floats over the viewer (separate from loading screen). `pointer-events: none`. Styled like action buttons. Fades out on completion. Hidden in zoom mode.
- **Carousel scroll fix**: `add_thumbnail` calls `scroll_carousel_to(i)` after `createImageBitmap` resolves so orientation-change rebuilds scroll to the right position (canvases have their real size by then).
- **Download dialog mobile fix**: `navigator.maxTouchPoints > 0` skips `window.confirm()` on touch devices — Android/iOS show their own native download UI; desktop Firefox keeps the confirm dialog.
- **Deployed to Cloudflare R2** with custom domain. Same-origin serving — `Content-Length` visible, progress bar percentage works, no CORS config needed.
- **deployg R2 upload**: `deployg -b <bucket> -p <prefix>` uploads viewer files + archive directly to R2 via SigV4-signed S3 API. List/delete/upload all working. Cache purge code present but disabled pending Cloudflare API token permission setup.
- **deployg `--dryrun`**: simulates full operation (reads files, computes hashes, runs S3 list) without writing, uploading, or deleting anything. Confirmation prompt shown with auto-`y (dryrun)`.
- **deployg `-o` required**: removed default `./deploy` fallback; destination must now be specified explicitly via `-o` or `-b`.
- **About overlay**: α button (bottom-left, serif bold) fetches `about.html` lazily and displays it in an info-style modal with scrolling body. `deployg` includes `about.html` in deploys when present.

---

## Watermarking — Status Checkpoint (2026-06-07)

Detailed design + tuning rationale live in `watermarking.md`; measured data in
`tests/reports/`. This is the milestone-level snapshot.

**Algorithm (shipped; WASM-active via `receive_pixels` → `embed_y`):**
- Spread-spectrum in **CDF 5/3 DWT** detail bands **LH2/HL2/LH3/HL3** (Y channel), with a
  modulo-tiled 64² PN sequence per payload bit.
- **ALPHA 0.15**, **EMBED_LEVELS [2,3]**, **perceptual masking** (`MASK_STRENGTH 0.5`,
  mean-1 / energy-neutral). Imperceptibility much improved: PSNR ≈ 45.5 dB, smooth
  film-grain (CDF 5/3) rather than Haar "popcorn", hidden in texture by masking.
- Tuning journey (all measured): Haar → **CDF 5/3**; levels [3,4] → **[2,3]**; ALPHA
  1.0 → 0.3 → **0.15**; **modulo** (not stretched/normalized) PN tiling; masking blend.

**Payload — 192-bit format (CRC shipped, ECC reserved):**
- **192 bits = 128 data + 32 CRC-32 + 32 reserved (ECC, zero for now)**. The CRC-32 (IEEE,
  reflected poly `0xEDB88320`) is appended **inside WASM** (`embed_y` calls `crc32` →
  `full_payload`), so the JS/WASM boundary stays the same **16 data bytes** (`build_payload`
  unchanged). `Decoded { data: [u8;16], verified: bool }` is the decode result type;
  `split_payload` checks the CRC and sets `verified`.
- **CRC is the definitive verdict** — it replaced the old prominence/version-byte heuristic.
  Empirically a *perfect oracle*: across the 40-cell blind sweep, CRC-verified count ==
  clean-decode count (zero false accept, zero false reject). The reserved 32 bits are sized
  for the few-bit ECC to come.

**Decoding — blind is now the default (`glimr` + `tools/watermark-decode`):**
- The critically-sampled DWT is **shift-variant** → recovery needs the *exact* original
  pixel grid. `decode_y_at_size` resamples the suspect back to original dims → matched decode.
- **`decode_blind_auto` (shipped, feature-gated `registration`):** fully blind — spectral-
  whitened autocorrelation recovers **scale** (`SCALE_BLOCK 1024` excerpt → 4 PN periods),
  then folds the suspect into one tile (`FOLD 512`, LCM of the L2/L3 periods) and runs a keyed
  per-bit cross-correlation for **offset + payload signs**, with a ±2% scale refinement ladder
  (`REFINE_STEPS 2`, `REFINE_FRAC 0.005`). CRC gates the result. So a **cropped and/or rescaled**
  suspect decodes with no side information.
- **CLI (`tools/watermark-decode`) — simplified this cycle:**
  - **Blind auto is the default** ("the way it just works"); no flag needed. A **CRC fast-path**
    tries a native matched decode first and returns instantly if it verifies.
  - `--size WxH` / `--ref <orig>` remain as mutually-exclusive fast overrides when dims are known.
  - `--scan` (brute-force size; rayon/ctrlc) **removed** — strictly inferior to blind auto.
    `--auto` kept as an accepted no-op alias for muscle memory.
  - **`-v`/`--verbose`** narrates the search (templates → scale → per-rung scale/prominence/CRC);
    otherwise a **live one-line progress bar** renders on an interactive TTY (`IsTerminal`-gated,
    on stderr, erased before the result) and is suppressed when redirected. Lib stays UI-agnostic /
    WASM-safe via a `Progress` callback (`decode_blind_auto_cb`); results print to stdout.
  - Verdict bands: `verified (CRC ok)` / `likely — CRC failed` (confidence ≥ 3) / `not detected`.

**Robustness (measured + real-world):**
- JPEG q70–90: **0 errors**. Resize 50–120% (size known): **0 errors**.
- Crop is a **registration** problem, not signal loss (`crop_tolerance.md`): pad-at-known-offset
  decodes 0 errors to a 10% edge crop; blind auto now recovers the offset itself.
- **Blind sweep (`blind_auto_sweep.md`): 36/40 cells clean & CRC-verified** after the
  scale-window fix (512→1024 block recovered 10 of 12 full-scale failures). Remaining 4 misses:
  2 are 1-bit ECC targets (test_a 0.50× + crop), 2 are hard-scale (test_e q80 1.0×).
- **Real-world: every screenshot capture CRC-verified blind** — downscale (to 0.42×), crop,
  partial occlusion (gray bar), JPEG recompression; consistent browser fp `6effd55f`.
- **`sstest7` — first wild few-bit failure (ECC poster child):** significant crop at a
  different scale, saved JPEG. Registration **locked** (refine 3/5 prominence 3.7 vs ~1.6
  floor, scale 0.953); fp `6effd55f` + version + a coherent timestamp decoded correctly, but
  a stray `0x80` high bit in the IP (≈1 bit-flip) → CRC correctly **refused** to certify
  (`likely — CRC failed, confidence 3.7`). Exactly the regime ECC is sized to rescue.

**Infrastructure this cycle:**
- **Feature-gated registration**: `rustfft` is an *optional* dep behind the `registration`
  feature; the **WASM build (no feature) stays FFT-free**; `watermark-decode` enables it.
- **Memory**: `pixel_cache` capped at **250 MB** (`enforce_cache_budget`, farthest-from-current eviction).
- **Download**: exports the **watermarked** image as JPEG (q0.92, `watermarked_pixels`);
  `raw_bytes` removed — closed the one-click un-watermarked-original leak.
- **Test tiers**: fast correctness + robustness regression (assert) run always;
  **characterization sweeps** are `#[ignore]` and write `tests/reports/*.md` (`crop_tolerance`,
  `registration_stage1/2`, `blind_auto_sweep`). `embed_y_masked(strength)` exposes the masking knob.
- **`tests/reports/`**: `.md` tracked as living docs, heatmap PNGs gitignored. `tests/test_a.jpg`
  force-tracked past the `*.jpg` ignore.

**Next — ECC (queued, design agreed):**
- Pipeline: receive 192 hard bits → **BCH-correct** → split 160 → CRC32 over the 128 data bits →
  certify. CRC stays the final oracle (ECC proposes, CRC disposes → no false-certification risk).
- **v1: shortened BCH(192, 160) over GF(2⁸), t = 4** (32 parity bits = the reserved field; t=4 is
  the ceiling for 32 parity at m=8). Corrects ≤4 scattered bit-flips anywhere in the codeword —
  covers the marginal band (the 1-bit sweep cells, `sstest7`). >4-error cases stay uncorrectable
  but are sliding toward registration-failure anyway, and CRC still rejects them.
- **Follow-ons (not v1):** (a) **soft-decision / CRC-aided retry** — we already compute per-bit
  correlation *magnitudes* but keep only signs; flipping the least-confident bits + CRC-checking
  corrects beyond hard t=4 cheaply. (b) **ECC-in-the-loop scale ladder** — accept the first refine
  rung that CRC-verifies *after* correction (sstest7's rung 3 would likely pass).
- Consider keeping `sstest7` as the first real-world few-bit regression fixture.

---

## Phase 2 — In-Progress Detail

### Step progress

```
[x] Step 1 — GlimrRenderer scaffold: load_zip, image_count, image_name, raw_bytes
[x] Step 2 — draw(index, offset): single-image fit-scale draw
[x] Step 3 — draw(): add slide offset (prev/next image in adjacent columns)
[x] Step 4 — draw_zoomed(index, scale, pan_x, pan_y)
[x] Step 5 — draw_thumbnail (removed; thumbnails rendered in JS via createImageBitmap)
[x] Step 6 — draw_hover_indicator(index, zone: &str, opacity: f64)
[x] Step 7 — Wire up JS; streaming zip state machine; hybrid decode
[ ] Step 8 — LSB watermark stub (magic number + zero payload, structured for read-watermark tool)
```

### Dependencies (`glimr/Cargo.toml`)

- `wasm-bindgen = "0.2"`
- `flate2 = { version = "1", default-features = false, features = ["rust_backend"] }` — raw deflate (miniz_oxide); no `zip` crate
- `js-sys = "0.3"` — `js_sys::Date` for timestamps, `js_sys::Uint8Array` for `get_image_bytes`
- `web-sys = "0.3"` with features: Document, Element, HtmlElement, HtmlCanvasElement, CanvasRenderingContext2d, ImageData, Window
- `glimr/.cargo/config.toml` — `target-feature=+simd128`
- ~~`image` crate~~ — removed; JPEG decode now browser `createImageBitmap`

### `GlimrRenderer` struct fields

- `names: Vec<String>` — display/download filenames in zip entry order
- `image_bytes: Vec<Vec<u8>>` — XOR-decoded JPEG/PNG bytes, per image
- `pixel_cache: HashMap<usize, (u32, u32, Vec<u8>)>` — watermarked RGBA (width, height, bytes); capped at 250 MB via `enforce_cache_budget` (evicts farthest-from-current)
- `canvas: HtmlCanvasElement` — `#photo` (final display surface)
- `backing: HtmlCanvasElement` — `#backing` (offscreen compositing)
- `decode: HtmlCanvasElement` — hidden canvas created internally; holds one image at native resolution for scale-blitting
- `stream_buf: Vec<u8>` — byte accumulator for streaming parse; drained after each entry
- `stream_state: StreamState` — private enum: `NeedHeader` / `NeedFilename{...}` / `NeedData{...}`
- `stream_done: bool` — true once central directory or EOCD signature seen

### `GlimrRenderer` public API (exported to JS)

**Streaming load:**
- `begin_zip_stream()` — clears all image state and parser state; call before first chunk
- `feed_bytes(chunk: &[u8]) -> Result<u32, JsValue>` — appends chunk to accumulator; advances state machine as far as possible; decompresses (deflate) + XOR-decodes each complete entry; drains consumed bytes; returns total entries ready so far. Errors on bit-3 flags, zip64, unknown compression, bad signature.
- `is_stream_done() -> bool` — true once central directory or EOCD seen

**Image access:**
- `image_count() -> usize`
- `image_name(i) -> String`
- `image_file_size(i) -> usize`
- `image_width(i) -> u32` / `image_height(i) -> u32` — from pixel_cache; 0 if not yet decoded
- `watermarked_pixels(i) -> Uint8Array` — native-resolution **watermarked** RGBA from
  `pixel_cache` (empty if not decoded); used for download (JS encodes JPEG via
  `OffscreenCanvas`). Replaced `raw_bytes` — there is no longer any API that hands the
  un-watermarked source bytes to JS for export.
- `get_image_bytes(i) -> Uint8Array` — for `createImageBitmap`; momentary JS exposure acceptable under "moderate inconvenience" model
- `receive_pixels(i, width, height, data: &[u8]) -> Result<(), JsValue>` — stores RGBA in pixel_cache; watermark applied here (currently no-op stub)
- `is_decoded(i) -> bool`

**Drawing:**
- `draw(index, offset) -> Result<(), JsValue>`
- `draw_zoomed(index, scale, pan_x, pan_y) -> Result<(), JsValue>`
- `draw_hover_indicator(index, zone, opacity) -> Result<(), JsValue>`

**Also exported (free functions):**
- `glimr_log(func, msg)` — timestamped console log
- `xor_decode(input: &[u8]) -> Vec<u8>` — exported for potential direct use

### Streaming zip state machine

`StreamState` enum (private to lib.rs):
- `NeedHeader` — wait for 30 bytes; parse local file header signature + fields; validate flags/compression; drain 30 bytes; → `NeedFilename`
- `NeedFilename { compression, comp_size, fname_len, extra_len }` — wait for `fname_len + extra_len` bytes; extract filename; drain; → `NeedData`
- `NeedData { name, compression, comp_size }` — wait for `comp_size` bytes; decompress (deflate or store); XOR-decode if `.dat`; push to `names`/`image_bytes`; drain; → `NeedHeader`

`std::mem::replace` used to take state out of `self` before match, avoiding simultaneous borrow conflicts with `self.stream_buf`.

On central directory signature (`0x02014b50` / `0x06054b50`): set `stream_done = true`, clear buf, break.

Display order = zip entry order (no sort). `packg` writes entries in hash-sorted order which is the intended display order.

### JS streaming pump (`load_zip(stream, content_length)`)

- `stream` is a `ReadableStream` — from `fetch().body` or `File.stream()`
- `content_length` passed from `Content-Length` header (0 if unavailable)
- `renderer.begin_zip_stream()` initialises WASM state
- `++load_gen` / `++thumb_gen` cancel stale in-flight operations
- `reader.read()` loop: feed chunk to `feed_bytes`, get new entry count, call `add_thumbnail(i)` for each new entry
- First entry: `set_current_index(0)` + `decode_image(0, callback)` → on decode: `draw(0)`, hide loading screen, prefetch image 1
- Newly arrived neighbours of `current_index` prefetched via `decode_image(j, null)`
- On `result.done || is_stream_done()`: call `hide_stream_progress()`
- On error: `hide_stream_progress()` + show error in `#progress-error`

### JS decode pipeline

1. `navigate_to(index)` → `set_current_index(index)` + `decode_image(index, callback)`
2. `decode_image`: `is_decoded(i)` → callback immediately; else `get_image_bytes(i)` → `Blob` → `createImageBitmap` → `OffscreenCanvas` → `getImageData` → `receive_pixels(i, w, h, data)` → callback
3. callback: `draw(0)` + fire `decode_image` for neighbours (no callback = prefetch only)
4. Thumbnail fill: `add_thumbnail(i)` fires `get_image_bytes(i)` → `createImageBitmap` → draw to thumbnail canvas at carousel scale. All in-flight concurrently.

### Three-canvas draw pipeline

1. Resize `backing` to viewport W×H, fill `#777777`
2. `draw_image_in_column` → check `pixel_cache` (grey placeholder on miss) → put RGBA into `decode` at native res → `drawImage` (scaled) into `backing`
3. Resize `canvas` to W×H, `drawImage(backing, 0, 0)`

### JPEG Decode Performance History

- Baseline: `zune-jpeg` pure-Rust in WASM → ~789ms/image
- WASM SIMD: ~2× → ~400ms/image
- Hybrid decode (`createImageBitmap`): ~5–30ms/image, concurrent — `image` crate removed

### Security Model — Devtools Access

**Goal**: "Security by moderate inconvenience."

- **Network tab**: `.dat` XOR encoding — no raw JPEG in transit
- **Canvas (`#photo`)**: watermarked version only — acceptable
- **Download**: exports the watermarked image (JPEG, in-browser encode) only. (Previously
  served `raw_bytes` = the un-watermarked original — a one-click leak, now closed; `raw_bytes` removed.)
- **`decode` canvas**: created programmatically, never appended to DOM — not visible in element inspector
- **`#backing`**: has `hidden` attribute but still in DOM — minor gap; TODO: create programmatically
- **WASM linear memory**: `pixel_cache` raw RGBA — inspectable only by knowing byte offset
- **Hybrid decode weakness**: un-watermarked RGBA briefly exists as JS `ImageData` during `getImageData` → `receive_pixels`. Acceptable at current security model. Thumbnail canvases hold un-watermarked pixels at thumbnail resolution — also acceptable.
- **mozjpeg-sys path**: would keep decode entirely in WASM; revisit if security model tightens

---

## Streaming Zip Design

**Approach: streaming is the only path; error out if not streamable.**

No fallback. Clean implementation, honest errors. Windows Explorer zips, macOS Archive Utility, 7-Zip, packg — all write complete local headers (bit 3 unset) and are compatible.

**What makes a zip streaming-compatible**: Bit 3 of general-purpose flags must be 0 (sizes in local header, not data descriptor). Supported compression: 0 (store) or 8 (deflate). No zip64. No encryption.

**Network streaming** (`ReadableStream` pump in JS): chunks fed to `feed_bytes` as they arrive. First image shown as soon as its compressed bytes land — no need to wait for full download. Progress bar tracks bytes received / `Content-Length`.

**Local file** (`File.stream()`): same code path; resolves instantly since file is already in memory. No meaningful streaming but code is unified.

---

### Parallelism — Rayon (future)

`wasm-bindgen-rayon` + `SharedArrayBuffer` requires:
```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```
Don't add now — breaks cross-origin resources without `Cross-Origin-Resource-Policy`. Save for when watermark computation is expensive enough to justify it. `coi-serviceworker` is the GitHub Pages workaround if needed.

---

## deployg — Deploy Tool

### Local output mode
```
deployg -o <output-dir> [-a archive.zip] [-f] [--dryrun]
```
Copies viewer files + archive into a local directory. `-f` clears and overwrites. `-o` is required; there is no default output directory.

### R2 upload mode
```
deployg -b <bucket> -p <prefix> [-a archive.zip] [-f] [-y] [--dryrun]
```
Uploads directly to Cloudflare R2 via S3-compatible API.

**Flags:**
- `-b`/`--bucket` — R2 bucket name (looks up credentials stanza)
- `-p`/`--prefix` — key prefix, e.g. `2020/Phoenix` → files land at `https://domain/2020/Phoenix/...`
- `-f`/`--force` — if prefix is occupied: list files, confirm, delete, then upload
- `-y`/`--yes` — skip confirmation prompt (safe for scripting)
- `--dryrun` — simulate without modifying anything; reads files and computes hashes for accurate size output; prints `(dryrun)` on each affected line; still runs S3 list (read-only)
- `-a`/`--archive` — source archive (default: `Demo.zip` in viewer root)
- `-o`/`--output` — local directory output (mutually exclusive with `-b`; required)
- `-?`/`--help` — usage

**Credentials file**: `%USERPROFILE%\.r2\credentials.txt` — INI format, one stanza per bucket:
```ini
[si-p]
auth_token        = ...   ; Cloudflare API token (cache purge, when enabled)
access_key_id     = ...   ; R2 S3 access key
secret_access_key = ...   ; R2 S3 secret key
endpoint          = https://<account_id>.r2.cloudflarestorage.com
domain            = https://si-p.jayenh.com
zone_id           = ...   ; Cloudflare zone ID for cache purge
```

**Files deployed** (both modes): `index.html`, `main.js`, `main.css`, `sip.png`, `pkg/glimr.js`, `pkg/glimr_bg.wasm`, archive (`Demo.zip`), and `about.html` if present in viewer root (optional — silently omitted if absent).

**Upload flow:**
1. List objects under `{bucket}/{prefix}/` (S3 ListObjectsV2)
2. If occupied and no `-f` → error, no action taken
3. If occupied and `-f` → print deletion list, confirm (unless `-y` or `--dryrun`), delete (S3 DeleteObjects)
4. Upload viewer files then archive (+ `about.html` if present), each with interactive size + "done" display
5. _(Cloudflare cache purge — code present in `cloudflare.rs`, temporarily commented out pending API token permission verification — token needs Zone:Cache Purge scope)_

**Source modules** (`tools/deployg/src/`):
- `main.rs` — arg parsing, credentials reader, main flow
- `sigv4.rs` — AWS Signature V4: `sign()`, `sign_with_hash()`, `sha256_hex()`, `uri_encode()`, `utc_now()`
- `s3.rs` — `list_prefix()`, `delete_objects()`, `put_object()`, `fmt_size()`
- `cloudflare.rs` — `purge_cache()` (disabled; uncomment in `main.rs` when token is ready)

**Cargo deps added:** `ureq = "2"`, `hmac = "0.12"`, `sha2 = "0.10"`

**Note on Content-Length:** R2 requires `Content-Length` for PutObject; `ureq::send(reader)` uses chunked transfer which R2 rejects. Files are loaded into memory via `send_bytes`. Acceptable for gallery-sized assets.

---

## Deployment

- **Cloudflare R2** (`si-p` bucket, custom domain `si-p.jayenh.com`) — same-origin serving; `Content-Length` visible; no CORS config needed; WASM served as `application/wasm`
- **`r2.dev` dev URL**: cache disabled — convenient for iterating without manual purges
- **Custom domain**: Cloudflare edge CDN; fast global delivery
- **Deploy tool**: `deployg -b si-p -p <gallery-name> -a <archive.zip>` — uploads all viewer files + archive, cleans old prefix with `-f`
- **Cache strategy**: Cloudflare cache purge via API after deploys (code ready in `cloudflare.rs`; needs Zone:Cache Purge API token). Alternatively: `r2.dev` URL during development (no cache), custom domain for distribution.
- **Build stamp**: `<!-- Build MMDD:HHMM -->` in `index.html` — busts browser cache on HTML. Asset filenames are static — stale WASM/JS possible on Cloudflare edge until purge or TTL expires.

---

## Notes / Gotchas

- **`client_width` vs `width`**: always use `client_width/height` for viewport size, then `set_width/height` to match.
- **`set_fill_style_str`**: use this, not the deprecated `set_fill_style(&JsValue)`, on web-sys 0.3.99+.
- **`ImageData` constructor**: takes `Clamped<&[u8]>`, not `&Uint8ClampedArray`.
- **9-arg `drawImage` web-sys name**: `draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh` — `sx` and `sy` are positional args #2 and #3 despite not being in the method name.
- **Borrow checker / pixel cache**: `draw_image_in_column` uses an explicit block scope to drop the `pixel_cache` borrow before the `self.decode` borrow.
- **`StreamState` borrow**: `std::mem::replace(&mut self.stream_state, StreamState::NeedHeader)` takes state out before match so `self.stream_buf` can be mutated without conflict. Placeholder `NeedHeader` is immediately overwritten before any `break`.
- **Stream buf draining**: bytes drained after each state transition (`drain(0..n)` is O(remaining)); keeps buffer small — only current in-flight entry bytes buffered, not the whole zip.
- **`OffscreenCanvas`**: used in `decode_image()` to extract RGBA from `ImageBitmap` for `receive_pixels`. Supported in all modern browsers (Chrome, Firefox, Edge, Safari 16.4+).
- **Carousel closure bug (fixed)**: `create_thumbnails` loop previously used `var canvas` (function-scoped) — all click handlers referenced the last canvas. Fixed with `let canvas` (block-scoped).
- **`navigate_to` vs `draw`**: `draw(offset)` called only for animation frames and decode callbacks. All index-changing navigation goes through `navigate_to`.
- **Prefetch on navigate**: after displaying image N, `decode_image` fired for N±1 (no callback) for snappy swipe transitions.
- **load_gen guard**: `++load_gen` at start of each `load_zip`; pump checks `gen !== load_gen` and cancels reader if a new load starts.
- **thumb_gen guard**: `++thumb_gen` at start of `load_zip` and `create_thumbnails`; `createImageBitmap` callbacks check and close bitmap if stale.
- **stream_loading flag**: JS boolean set true at `load_zip` start, false in `hide_stream_progress()`. Used by zoom enter/exit and `set_current_index` to decide whether to show/hide `#stream-progress`.
- **Carousel scroll on orientation change**: `add_thumbnail` calls `scroll_carousel_to(i)` inside the `createImageBitmap.then` callback — at that moment the canvas has its actual size and the scroll lands correctly. Calling it synchronously after `create_thumbnails()` doesn't work (0×0 canvases).
- **Download dialog on mobile**: `navigator.maxTouchPoints > 0` detects touch devices. These get native browser download UI and don't need our `window.confirm()`. Edge case: touchscreen laptops with desktop Firefox skip the dialog (minor; acceptable).
- **Memory**: full-resolution RGBA in `pixel_cache`, **capped at 250 MB** (`enforce_cache_budget`, called from `decode_image` after each `receive_pixels`, anchored on the displayed image). Over budget → evicts the cached image farthest by index from current; never evicts current. Evicted images re-decode (~300ms) if revisited. ~10 images at 6 MP, more at lower res. See *Background Pre-Watermarking & Cache Memory*.
- **wasm-pack build**: must use `--out-dir ../pkg` (or `build.ps1`) — root `pkg/` is what the server serves.

---

## Background Pre-Watermarking & Cache Memory

### What exists
- `navigate_to()` ([main.js](main.js)) decodes+watermarks the current image, then fires `decode_image(index±1)` to pre-watermark immediate neighbours. The streaming loader also prefetches arriving neighbours of the current image.
- `decode_image()` is idempotent (`renderer.is_decoded` guard); the embed itself — `receive_pixels` → `extract_y` + `embed_y` — is a **synchronous WASM call on the main thread** (~100–500ms for a 6MP image).
- Results live in `pixel_cache: HashMap<usize,(w,h,Vec<u8>)>` as **watermarked RGBA, unbounded** (cleared only on full reset).

### Observation (this discussion)
The ±1 pre-watermarking works well — scrolling to a neighbour is usually delay-free. But it surfaced that the cache holds **uncompressed RGBA indefinitely** (~8–24MB/image). This is a latent memory problem regardless of any new feature: a long viewing session already accumulates every visited image. The original idea was to *extend* background watermarking to **all** images; on reflection the more important first step is the opposite — **evict**, so the cache stays bounded.

### Decision / priority order
1. ~~**Eviction first.**~~ DONE — `enforce_cache_budget(current)` caps `pixel_cache` at a
   **250 MB byte budget** (chosen over a fixed image count so it self-adjusts to image
   resolution / device), evicting farthest-from-current first, never current. Called from
   `decode_image` after each `receive_pixels`. No extra anticipatory prefetch was added
   (kept the existing ±1) — the app's navigation is well-behaved; the goal was just to
   stop a large catalog blowing up the browser.
2. **Background sweep to all (optional, gated on #1).** Only after the cache is bounded does a full outward sweep make sense. Schedule one image per idle slice (`requestIdleCallback`, `setTimeout` fallback), concurrency 1, nearest-first, re-seeded on navigation, driven also by stream arrival with a post-stream mop-up, abandoned on new gallery (`load_gen`). With an LRU cap in place, "all" effectively means "the window stays warm as you move," not "all retained at once."
3. **Compact retention — DEFERRED (likely the eventual right answer).** Instead of
   evicting, watermark *every* image and cache it as a **high-quality JPEG blob** — the
   *same* encode the download path now produces (`watermarked_pixels` →
   `OffscreenCanvas.convertToBlob`, q 0.92, ~10× smaller than RGBA). Then a whole large
   catalog fits in memory and navigation only pays a fast JPEG-decode (`createImageBitmap`)
   on draw instead of a full re-watermark. This would largely retire the byte-budget
   eviction. Deferred for now; the 250 MB cap is the simple stopgap.
4. **Web Worker embed (heavyweight, separate concern).** Moving `receive_pixels` to a worker with its own WASM instance removes *all* main-thread hitching during embeds. Worth it only if idle-time embeds still cause a noticeable hitch.

### Notes
- **Security: neutral.** Un-watermarked originals already sit resident as the obfuscated zip bytes (`image_bytes`); pre-watermarking more (or evicting) doesn't widen dev-tools exposure. Transient un-watermarked RGBA still only exists momentarily per embed.
- **Open question (drives #2/#3):** typical and worst-case gallery sizes, image dimensions, and whether mobile is a target. Under ~20–30 images a generous LRU window effectively retains everything; hundreds → the window/eviction matters and full retention needs #3.

---

## TODO

- ~~**`pixel_cache` eviction**~~: DONE — 250 MB byte budget (`enforce_cache_budget`).
- **Compact retention (HQ-JPEG cache)** — DEFERRED: cache every watermarked image as a
  q0.92 JPEG blob (same encode as download) instead of evicting RGBA; ~10× smaller, fast
  decode-on-draw, fits a whole large catalog. Would largely retire the byte budget. See
  *Background Pre-Watermarking & Cache Memory* §3.
- **Step 8 — LSB watermark stub**: In `receive_pixels`, write `"GLIM"` magic + 28 zero payload bytes into RGBA LSBs. Structured for `read-watermark` tool.
- **mozjpeg-sys**: If security model tightens (pixels must not pass through JS), revisit C build toolchain (NASM + CMake + clang wasm32 target) on Windows.
- **Rayon parallelism**: `wasm-bindgen-rayon` + `coi-serviceworker`. Less critical now; revisit when `receive_pixels` applies a real watermark algorithm.
- **Move `#backing` off DOM**: create programmatically like `decode` canvas — minor security cleanup.
- **Cache-busting for assets**: short TTL on `index.html`, longer on WASM/JS. Purge via `cloudflare::purge_cache` — uncomment in `main.rs` once API token has Zone:Cache Purge scope.
- **deployg — configurable gallery zip name**: archive always output as `Demo.zip`; could use source filename or a flag instead.
- Slideshow / 3-state fullscreen: normal → fullscreen carousel+image → fullscreen image-only with play/pause
- Desktop app — Tauri (Rust backend, system WebView, native file dialogs); near-term option: local HTTP server binary
- `?zip=` URL param to select archive
- Animate zoom transitions (smooth zoom on wheel/pinch/keyboard)
- `gallery-config.toml` output from packg; WASM build bakes constants
- **Per-gallery XOR key (bespoke app instances).** packg/deployg optionally generate a
  *random* XOR key (instead of the shared compiled-in `0xDEADBEEF`) used to obfuscate that
  zip's `.dat` images; bake the key into *that gallery's* WASM build (hard to extract from
  the binary) so each deployed gallery is a bespoke app instance matched to its own zip —
  extracting/decoding one gallery's images doesn't help with another. Fits the
  `gallery-config.toml` → WASM-build-bakes-constants flow above; raises the obfuscation bar
  from "one shared key" to "per-gallery key" while staying "moderate inconvenience" (still
  recoverable by a determined reverser of that gallery's WASM).
- `read-watermark` tool
- LSB watermarking implementation in WASM
- Frequency-domain watermarking implementation in WASM
- PWA manifest for iOS home-screen fullscreen
- Social preview (Open Graph): `meta.json` in source dir → included in zip by packg; deployg extracts preview image, stamps `og:title`/`og:image` into `index.html`
- Recursion option for packg (currently flat directory only)
- `<glimr-player src="...">` Web Component (Phase 3–4)
