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
- In WASM era: image data stays in WASM linear memory, never surfaces as JS `Image` objects or blob URLs

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
- **read-watermark** (planned): input suspected leaked image → scan for LSB magic number → extract session payload; also attempt frequency-domain extraction
- Future: `gallery-config.toml` output from packg, read by WASM build step to bake constants

---

## Current UI — Implemented Features

### Layout

- CSS grid, responsive to viewport aspect ratio (`orientation: landscape` media query)
- **Portrait**: thumbnail carousel strip across the top, viewer fills remainder
- **Landscape**: thumbnail carousel column on the left (`auto` width), viewer fills right
- Carousel and viewer areas resize/rebuild on orientation change (detected via `window.resize` + `landscape_mq.matches` comparison — more reliable than `matchMedia` change event on mobile)
- Viewport meta tag present (`width=device-width, initial-scale=1`)
- No footer — removed; info popup planned

### Thumbnail Carousel

- Thumbnails sized to `min(18% of relevant viewport dimension, G_CAROUSEL_SIZE_MAX=160px)`
- Portrait: scaled to fixed height; landscape: scaled to fixed width — consistent cross-axis size
- Canvas elements initialised to 0×0 to avoid 300px default causing layout flash
- Drag-to-scroll (mouse + touch) on correct axis per orientation
- Scroll wheel scrolls carousel without changing selection
- Scrollbar hidden (`scrollbar-width: none` + `::-webkit-scrollbar`)
- Active thumbnail: red border, dimmed to 75% brightness
- Scrolls to keep active thumbnail visible on navigation

### Image Viewer

- Click/tap **left or right third**: navigate previous/next with slide animation (250ms ease-out)
- Click/tap **center third**: enter zoom mode at 1:1 pixel scale
- Click/tap **center third while in zoom mode**: exit zoom (returns to fit view regardless of current zoom level)
- **Swipe/drag**: pan in zoom mode; slide-navigate in normal mode (25% threshold)
- **Hover indicators**: `<` / `>` arrows fade in/out on left/right thirds, idle-timeout fade

### Zoom Mode

- **Entry**: tap center (1:1), scroll wheel up, pinch outward, or Ctrl+= — all enter at fit-scale seamlessly
- **Exit**: tap while in zoom, scroll/pinch back to fit-scale (automatic), or press `0`
- Range: fit-scale (image fills viewport) → 2.0× (double pixel size)
- **Scroll wheel**: enters zoom if not already in it; zooms toward cursor; exits automatically at fit-scale
- **Pinch to zoom**: enters zoom if not already in it; zooms toward pinch midpoint; exits at fit-scale
- **Ctrl+= / Ctrl+−**: 25% steps toward viewport center; Ctrl+= enters zoom, Ctrl+− exits at fit-scale
- **Arrow keys in zoom mode**: pan image 80 screen-pixels per press in each direction (constant visual distance regardless of zoom level)
- Drag pans image (adjusted for zoom_scale so drag distance feels consistent)

### Loading Screen

- Shown on initial load and whenever a new zip is loaded
- Two lines of large text (`5vw`), wave-bounce CSS animation (per-character staggered delays)
- "Welcome to Glimpse-o-Matic!" + "Loading now ...."
- Hides (not removed) when first image is ready; reappears on new zip load

### Floating Action Buttons

Bottom-right corner, `position: fixed`, horizontal row layout: `[🖼] [⛶] [⬇]`

**Sizing**: `min(max(100vw, 100vh) / 20, 48px)` — responsive, capped at 48px on desktop  
**Style**: white outline (`border: 2px solid white`), white icon, transparent background, rounded corners (`border-radius: 12px`), drop shadow, 33% opacity by default  
**Flash animation**: tap → 100% opacity, eases back to 33% (or 60% if toggling on) over 0.35s

- **🖼 Load archive** (`btn-load`): opens file picker (`<input type="file" accept=".zip">`), loads selected zip through full `load_zip()` pipeline
- **⛶ Fullscreen** (`btn-fullscreen`): toggles `document.requestFullscreen()` / `exitFullscreen()`; stays at 60% opacity while fullscreen is active; `fullscreenchange` event updates state
- **⬇ Download** (`btn-download`): downloads current image via blob URL with correct `.jpg` filename

### Keyboard Shortcuts

- `←` / `→` arrows: navigate previous/next (outside zoom mode)
- `←` / `→` / `↑` / `↓` arrows: pan image while in zoom mode
- `0`: exit zoom mode
- `f` / `F`: toggle fullscreen
- `Ctrl+=` / `Ctrl++`: enter zoom (if needed) and zoom in
- `Ctrl+−`: zoom out; exits zoom at fit-scale
- `Esc`: exit fullscreen (browser native, `fullscreenchange` updates button state)

### Logo Watermarks

Two `sip.png` instances overlay `#lobjet_pane` at all times (`z-index: 2`, `pointer-events: none`):
- **Bottom**: centered horizontally, 12px from bottom edge
- **Left**: centered vertically, 12px from left edge, rotated 90° clockwise

Sizing: `max-width: min(20vw, 20vh)` and `max-height: min(20vw, 20vh)` — constrains the long dimension to 20% of the shorter viewport axis, adapts automatically to any image aspect ratio.  
Style: `opacity: 0.15`, `mix-blend-mode: soft-light`, `filter: drop-shadow(0 1px 2px rgba(255,255,255,0.4))` for a subtle indent/deboss look.

### Zip Loading Pipeline (`load_zip(buf)`)

- Cancels in-flight animations, shows loading screen
- Revokes old blob URLs (memory management)
- Resets all state: images, blob_urls, image_cache, zoom, current_index, thumbnails
- Passes raw bytes to `GlimrZip` (WASM) which parses, filters, sorts, and XOR-decodes
- JS iterates entries via `entry_name(i)` / `entry_data(i)`, creates blob URLs
- Calls `archive.free()` to release WASM memory after all blob URLs are created
- Used for both initial `Demo.zip` fetch and file-picker loads
- No external CDN dependencies — fflate removed

### Dev Server (`server.py`)

`python server.py` — serves project root on port 8000 with `Cache-Control: no-store` on all responses. Use instead of `python -m http.server` to avoid browser caching stale WASM/JS.

### Build Script (`build.ps1`)

- Stamps `<!-- Build MMDD:HHMM -->` in `index.html` to bust browser cache
- Builds `glimr` WASM via `wasm-pack build glimr --target web --out-dir ../pkg`; removes the `pkg/.gitignore` wasm-pack generates so `pkg/` is committable
- Builds `packg` and `deployg` via `cargo build --release -p packg -p deployg`; copies both to `tools/bin/`

---

## Rust/WASM Migration Plan

Goal: incrementally replace JS with Rust/WASM, keeping a working app at every step. JS becomes a thin bootstrap by the end.

### Phase 1 — Image processing + zip handling in WASM
Replace `xor_decode()` and zip archive parsing with WASM equivalents. `GlimrZip` struct wraps the zip crate: constructor parses, filters and XOR-decodes all entries; JS iterates via `entry_name(i)` / `entry_data(i)`. Single boundary crossing for the entire archive instead of one per image. Eliminates the fflate CDN dependency. `xor_decode` remains exported for future direct use.

**Streaming (future):** Move from `response.arrayBuffer()` (full download before any processing) to `response.body.getReader()` feeding chunks to a stateful WASM streaming parser. Since packg writes entries in sorted display order with full local headers, a local-header-based streaming parser can decode entries as bytes arrive — first thumbnail appears partway through the download rather than after it completes. Main UX benefit: on slow connections or large galleries, the viewer comes up fast and thumbnails pop in progressively during download rather than all at once afterward. The loading screen becomes nearly unnecessary. Architectural implication: `images[]` grows dynamically; JS receives per-entry callbacks from WASM rather than a completed batch.

### Phase 2 — Canvas rendering in WASM
Move `draw()`, `draw_zoomed()`, `draw_image_in_column()` to WASM using `web-sys` canvas bindings. Decoded image bytes stay in WASM linear memory — never surfaced as JS `Image` objects. JS still handles events, calls `wasm.draw()`. This is the key security improvement.

### Phase 3 — State and event handling in WASM
Move state machine (current_index, zoom state, drag state, animation loops) to WASM. JS event listeners become thin wrappers calling e.g. `wasm.pointer_down(x, y)`. JS is now ~orchestration only.

### Phase 4 — Bootstrap only in JS
JS handles: load WASM module, pass control, file picker trigger, `fetch`, `requestFullscreen` — things that require a browser user-gesture context. Everything else is WASM.

**Tooling**: `wasm-pack build glimr --target web` → `pkg/` directory. Integrated into `build.ps1`. `glimr/` crate in workspace.

---

## Status — Milestones

- **Phase 1 WASM complete**: `xor_decode` + `GlimrZip` in Rust; fflate CDN removed; single JS/WASM boundary crossing per zip load
- **deployg tool**: creates self-contained gallery folder (index.html, main.js, main.css, pkg/, Demo.zip); tested and successfully deployed to Wasabi S3 bucket
- **End-to-end flow working**: pack with `packg`, deploy with `deployg`, serve from Wasabi, view in browser
- **Phase 2 in progress**: `GlimrRenderer` wired to JS; steps 1–7 of 8 done; only LSB watermark stub remains.
- **Logging infrastructure**: `glimr_log` in both Rust and JS; bottleneck confirmed as `zune-jpeg` decode (~789ms/image after SIMD); agreed roadmap: WASM SIMD ✓ → streaming zip ✓ → hybrid/mozjpeg
- **Streaming zip complete**: custom sequential parser replaces `zip` crate; `GlimrZip` removed; incremental load API (`begin_zip_load` / `load_next_entry` / `load_bytes_done` / `finish_zip_load`) drives a rAF loop with progress bar; friendly error display on unsupported zip format. Verified with Windows Explorer zips and packg zips.
- **Hybrid decode complete**: JPEG decode moved from WASM (`zune-jpeg`) to browser (`createImageBitmap`). `image` crate removed — smaller WASM binary, faster build (~4s vs ~20s). `ensure_decoded` and `draw_thumbnail` removed from Rust. New WASM API: `get_image_bytes` / `receive_pixels` / `is_decoded`. JS side: `decode_image()` (async decode helper) + `navigate_to()` (decode + draw + prefetch neighbours). Thumbnail fill now fires all `createImageBitmap` calls concurrently — thumbnails appear as ready instead of one per rAF frame. Carousel `var`→`let` closure bug fixed (all thumbnails previously navigated to last image).
- **Build fix**: `wasm-pack build` must use `--out-dir ../pkg` (or `build.ps1`) — root `pkg/` is what the server serves; `glimr/pkg/` is orphaned

---

## Phase 2 — In-Progress Detail

Goal: move canvas rendering into WASM so image pixels never surface as JS `Image` objects or blob URLs. JS keeps all state and event handling for now (that moves in Phase 3). The incremental step plan:

```
[x] Step 1 — GlimrRenderer scaffold: load_zip, image_count, image_name, raw_bytes
[x] Step 2 — draw(index, offset): single-image fit-scale draw; offset ignored until step 3
[x] Step 3 — draw(): add slide offset (prev/next image in adjacent columns)
[x] Step 4 — draw_zoomed(index, scale, pan_x, pan_y)
[x] Step 5 — draw_thumbnail (removed; thumbnails now rendered in JS via createImageBitmap)
[x] Step 6 — draw_hover_indicator(index, zone: &str, opacity: f64)
[x] Step 7 — Wire up JS (replace GlimrZip pipeline with GlimrRenderer; remove image_cache / blob_urls)
[ ] Step 8 — LSB watermark stub (magic number + zero payload, structured for read-watermark tool)
```

### What exists in glimr/src/lib.rs now

**Dependencies (`glimr/Cargo.toml`)**:
- `flate2 = { version = "1", default-features = false, features = ["rust_backend"] }` — raw deflate decompression (pure Rust via miniz_oxide); replaced the `zip` crate
- `js-sys = "0.3"` — used for `js_sys::Date` in `glimr_log` timestamp, and `js_sys::Uint8Array` in `get_image_bytes`
- `web-sys = "0.3"` with features: Document, Element, HtmlElement, HtmlCanvasElement, CanvasRenderingContext2d, ImageData, Window
- `glimr/.cargo/config.toml` — `target-feature=+simd128` applied to all WASM builds
- ~~`image` crate~~ — **removed**; JPEG decode now handled by browser via `createImageBitmap`; this removed `zune-jpeg` and `png` from the dep tree, shrinking the WASM binary noticeably and cutting build time from ~20s to ~4s

**`GlimrRenderer` struct fields:**
- `names: Vec<String>` — display/download filenames in sort order
- `image_bytes: Vec<Vec<u8>>` — XOR-decoded JPEG/PNG bytes, stored per image
- `pixel_cache: HashMap<usize, (u32, u32, Vec<u8>)>` — lazily decoded RGBA (width, height, raw bytes); decode-once, cache-forever (no LRU yet)
- `canvas: HtmlCanvasElement` — the `#photo` canvas (final display surface)
- `backing: HtmlCanvasElement` — the `#backing` canvas (offscreen compositing)
- `decode: HtmlCanvasElement` — hidden canvas created internally at `new()` time; holds one image at native resolution for scale-blitting
- `pending_zip: Vec<u8>` — zip bytes buffered during incremental load; cleared by `finish_zip_load`
- `pending_pos: usize` — current parse position within `pending_zip`
- `pending_entries: Vec<(String, Vec<u8>)>` — accumulating parsed image entries during incremental load

**`GlimrRenderer` public API (exported to JS):**
- `new(canvas, backing) -> Result<GlimrRenderer, JsValue>` — takes DOM canvas elements from JS, creates hidden decode canvas via `document.createElement`
- `load_zip(zip_bytes: &[u8]) -> Result<(), JsValue>` — synchronous load (kept for compatibility; JS now uses the incremental API instead)
- `image_count() -> usize`
- `begin_zip_load(zip_bytes: &[u8]) -> u32` — copies bytes into `pending_zip`, resets state, returns total bytes (progress denominator)
- `load_next_entry() -> Result<bool, JsValue>` — parses one local file header; decompresses + XOR-decodes if image; returns `true` when central directory reached, `false` while more entries remain, `Err` on bad zip
- `load_bytes_done() -> u32` — current byte position; divide by `begin_zip_load` result for 0.0–1.0 progress
- `finish_zip_load()` — sorts accumulated entries, populates `names`/`image_bytes`, frees `pending_zip`
- `image_name(i) -> String` — used by JS for download filename (`.dat` → `.jpg` replacement still done in JS for now)
- `raw_bytes(i) -> Vec<u8>` — XOR-decoded JPEG bytes; JS uses for download button via ephemeral blob URL (create → click → revoke immediately, never stored)
- `get_image_bytes(i) -> Uint8Array` — returns `image_bytes[i]` as a JS Uint8Array; JS wraps in a Blob and passes to `createImageBitmap`. Momentary exposure of raw bytes to JS is acceptable under the "moderate inconvenience" security model.
- `receive_pixels(i, width, height, data: &[u8]) -> Result<(), JsValue>` — stores watermarked RGBA for image `i` in `pixel_cache`. Called by JS after browser decode. Watermark applied here (currently a no-op stub; real implementation is Step 8).
- `is_decoded(i) -> bool` — `pixel_cache.contains_key(&i)`; JS checks this before deciding whether to decode or draw immediately.
- `draw(index: usize, offset: f64) -> Result<(), JsValue>` — draws image at `index` onto `self.canvas` via `self.backing`; `offset > 0` draws prev at `col_x = offset - W`, `offset < 0` draws next at `col_x = offset + W`; draws grey placeholder if image not yet in `pixel_cache`
- `draw_zoomed(index: usize, scale: f64, pan_x: f64, pan_y: f64) -> Result<(), JsValue>` — 9-arg drawImage crop+scale; returns early (no draw) if image not in `pixel_cache`

**Private helpers:**
- `draw_image_in_column(ctx, index, col_x, col_w, col_h)` — fit-scales image into a column rect; draws grey `#777777` placeholder if not in `pixel_cache`; otherwise puts RGBA into `self.decode` at native res, then `drawImage` (scaled) into target ctx
- `get_2d_context(canvas)` — free fn; calls `get_context("2d")` + `dyn_into`

**Three-canvas draw pipeline** (current):
1. Resize `backing` to viewport W×H, fill `#777777`
2. `draw_image_in_column` → check `pixel_cache` (grey placeholder if miss) → put RGBA into `decode` canvas at native res → `drawImage` (scaled) into `backing`
3. Resize `canvas` to W×H, `drawImage(backing, 0, 0)`

**JS decode pipeline** (hybrid decode, introduced with hybrid decode milestone):
1. `navigate_to(index)` → `set_current_index(index)` + `decode_image(index, callback)`
2. `decode_image`: if `renderer.is_decoded(i)` → callback immediately; else `get_image_bytes(i)` → `Blob` → `createImageBitmap` → `OffscreenCanvas` → `getImageData` → `renderer.receive_pixels(i, w, h, data)` → callback
3. callback: `draw(0)` + fire `decode_image` for neighbours (prefetch, no callback)
4. Thumbnail fill: for each image, fire `get_image_bytes(i)` → `createImageBitmap` → draw to thumbnail canvas at carousel scale. All N images in flight simultaneously.

W and H come from `self.canvas.client_width() / client_height()` (CSS rendered size, same as the JS `photo_box.clientWidth`).

**Still in JS / not yet moved:**
- `steg(canvas)` placeholder — removed from JS; replaced in step 8 when WASM does it properly
- `GlimrZip` — still exported from `lib.rs` but no longer called from JS; can be removed after step 8

### JPEG Decode Performance — History and Resolution

**Original symptom**: Each thumbnail ~1 second to appear; first image ~5s after zip load.
```
[01:05:07.782] <ensure_decoded> start image 1 (2704159 bytes)
[01:05:09.081] <ensure_decoded> done  image 1 → 2133×3200
```
Root cause: `zune-jpeg` pure-Rust decoder with no hardware acceleration in WASM.

**Logging infrastructure (June 2026)**:
- Rust: `glog(func, msg)` + `#[wasm_bindgen] pub fn glimr_log` using `js_sys::Date` for `[HH:MM:SS.SSS] <func> msg`.
- JS: `function glimr_log(func, msg)` with identical format.

**Performance roadmap — status**:

**Step 1 — WASM SIMD** ✓: `target-feature=+simd128` via `glimr/.cargo/config.toml`. ~2× speedup (789ms → ~400ms). Hit ceiling; 10× gap remained.

**Step 2 — Streaming zip** ✓: Incremental extraction API + progress bar. First-image latency decoupled from total gallery load time.

**Step 3 — Hybrid decode** ✓: `createImageBitmap` (browser GPU-accelerated) replaces `zune-jpeg`. Typical decode now ~5–30ms/image, concurrent. `image` crate removed. Chosen over `mozjpeg-sys` (complex Windows C build) for now; `mozjpeg-sys` remains an option if pixel security must be tightened (see security notes).

**Step 4 — Rayon inter-image parallelism**: `wasm-bindgen-rayon` + `coi-serviceworker` for GitHub Pages. Now less critical since thumbnail fill is already concurrent via `createImageBitmap`. Could still benefit the `receive_pixels` / watermark path when that is non-trivial. Orthogonal; add when needed.

---

### Security Model — Devtools Access

**Goal**: "Security by moderate inconvenience." Casual devtools inspection should not expose un-watermarked images. Determined users who decode the archive directly are an accepted risk.

**Threat surfaces**:
- **Network tab**: Handled — `.dat` XOR encoding, no raw JPEG in transit.
- **Canvas elements**: `#photo` is in the DOM and readable via `canvas.toDataURL()`. Since the watermark is applied in `ensure_decoded` before pixels ever reach a canvas, the canvas **already contains the watermarked version** — this is acceptable.
- **Hidden canvases**: `#backing` has the `hidden` attribute but is still in the DOM (inspectable). The `decode` canvas is created programmatically and never appended to the DOM — not visible in devtools element inspector.
- **WASM linear memory**: `pixel_cache` raw RGBA lives here. Inspectable via the WASM memory viewer, but requires knowing the byte offset — not a casual operation.
- **JS heap**: Any `ImageData` or typed array that passes through JS is visible to a console breakpoint. This is the key distinction between the two decode approaches.

**Implication for hybrid approach (current implementation)**: With `createImageBitmap` → `getImageData` → `receive_pixels`, un-watermarked RGBA briefly exists as a JS `ImageData`. A breakpoint on `receive_pixels` or a monkey-patched `putImageData` would expose it. Acceptable for "moderate inconvenience" but weaker than the pure-WASM path. Thumbnails are drawn directly from `ImageBitmap` without going through `receive_pixels` — un-watermarked pixels can be read from thumbnail canvases. Also acceptable at thumbnail resolution.

**Implication for mozjpeg-sys (not yet implemented)**: Pixels decoded entirely inside WASM. Un-watermarked data never exists as a JS object. Stronger guarantee. Revisit if the security model tightens.

**Minor cleanup needed**: Move `#backing` off the DOM (create it programmatically like `decode`) to close the canvas inspection gap.

---

### Streaming Zip Design

**Decided approach: streaming is the only path; error out if not streamable.**

No fallback to the `zip` crate random-access path. One clean implementation, honest errors. Windows Explorer zips (a primary local use case) are well-formed — bit 3 unset, complete local headers, deflate compression — so they parse correctly without the central directory. Most ordinary photo archives work the same way.

**What makes a zip streaming-compatible**: Every local file header contains the filename, compression method, and compressed/uncompressed sizes. If general-purpose bit 3 is unset, those sizes are correct and the entry can be decompressed without seeking. Bit 3 = 1 means sizes are deferred to a data descriptor after the compressed data — that's the error case. Other error cases: unsupported compression method, zip64 entries (sizes = 0xFFFFFFFF), encrypted entries, bad local header signature.

**Windows Explorer zips**: Always well-formed. Bit 3 unset, deflate or stored compression, no zip64 for typical photo files (<4 GB each). Confirmed compatible.

**packg zips**: Already streaming-compatible for the same reasons — the `zip` crate writes complete local headers when file sizes are known upfront. No changes needed to packg format for this to work.

**The `zip` crate is removed**: A custom sequential parser replaces it. `flate2` (already in the dep tree transitively) is added as a direct dependency for raw deflate decompression. `GlimrZip` is removed at the same time (dead code).

**Two-phase progress reporting**:

*Download phase* (network fetch): `Content-Length` is present on static file servers (GitHub Pages, S3/Wasabi, nginx). Reading it from `response.headers.get('content-length')` before consuming the body gives the total bytes upfront — percentage progress is computable during download. Local file open has no download phase (buffer is in RAM instantly).

*Extraction phase* (sequential parse): byte position / total bytes = fraction complete. Since `begin_zip_load` receives the full buffer, `total_bytes = buffer.byteLength`. Updated once per image via JS rAF loop.

**Progress bar**: Lives inside the existing loading screen overlay. A track `<div>` + fill `<div>` below the wave-text lines. Fill width updates each rAF frame (one entry processed per frame). Disappears when extraction is complete (loading screen hides as today). Friendly error message shown if streaming fails: "This zip file isn't supported — try a zip created by Windows or macOS."

**Incremental WASM API** (replaces synchronous `load_zip`):
- `begin_zip_load(zip_bytes: &[u8]) -> u32` — stores bytes in renderer, resets state, returns total_bytes
- `load_next_entry() -> Result<bool, JsValue>` — processes one entry (decompress + XOR-decode), returns `true` when done, `false` when more remain, Err on bad zip
- `load_bytes_done() -> u32` — current byte position for progress numerator
- `finish_zip_load()` — sorts entries, populates `names`/`image_bytes`, clears pending state

**Implementation steps**:
1. Remove `GlimrZip` from `lib.rs` and `index.html`
2. Write `parse_zip_streaming` (sequential parser, single synchronous call) — replace `ZipArchive` in `load_zip`, remove `zip` crate, add `flate2`; behavior unchanged, verify with Windows + packg zips
3. Add incremental load API to `GlimrRenderer` (`begin_zip_load`, `load_next_entry`, `load_bytes_done`, `finish_zip_load`)
4. Progress bar HTML/CSS + JS rAF loop + error handling; switch JS to incremental API

**Future — true network streaming**: Once the sequential parser exists, adapting it to consume `ReadableStream` chunks is a natural evolution — the parser becomes a state machine fed bytes incrementally. Download progress and extraction progress can then be shown in a unified bar. Not needed for local-file use case.

---

### Parallelism — Rayon + wasm-bindgen-rayon

**Opportunity**: Inter-image parallelism — decode multiple JPEGs simultaneously on different hardware threads. On a 4-core device this gives ~4× throughput on the thumbnail fill phase. The `image` crate doesn't use Rayon internally, but decoding across images can be parallelized trivially with `par_iter()`.

**Mechanism**: `wasm-bindgen-rayon` implements Rayon's thread pool using Web Workers + SharedArrayBuffer. Requires two HTTP response headers:
```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

**GitHub Pages workaround — `coi-serviceworker`**: GitHub Pages does not set these headers. However, a service worker can inject them client-side. `coi-serviceworker` (by gzuidhof, well-maintained, used by many production WASM projects) is the standard solution recommended by the `wasm-bindgen-rayon` docs themselves. It's a single small JS file included in the project. First-ever page load triggers an automatic reload after service worker registration; subsequent loads are seamless. This means **Rayon parallelism is available on GitHub Pages** — the deployment constraint is solved.

**What parallelism does and doesn't fix**: Inter-image parallelism improves thumbnail fill rate (all images decode in N_threads batches rather than sequentially). It does NOT improve first-image latency — image 0 still waits one full single-thread decode. Prioritizing image 0 on one thread while others start in parallel is the right approach.

**How the options stack (multiplicative)**:
- WASM SIMD: ~2× (done)
- mozjpeg-sys: ~10× estimated
- Rayon (4 threads): ~4× on thumbnail fill rate
- Combined: potentially 80× over baseline — 789ms/image → ~10ms/image

**Sequencing**: Rayon is orthogonal to streaming and mozjpeg — can be added at any point. The COOP/COEP header question (whether to use `coi-serviceworker` or configure the deployment infrastructure) needs to be resolved first.

### Step 3 — Slide offset detail

`draw(index, offset)` with `offset != 0`:
- `offset > 0` (dragging right, revealing prev): draw prev image (`index - 1`) at `col_x = offset - W`
- `offset < 0` (dragging left, revealing next): draw next image (`index + 1`) at `col_x = offset + W`
- Guard: no prev column if `index == 0`; no next column if `index == image_count() - 1`
- Both columns drawn into `backing` before the final blit to `canvas`
- `ensure_decoded` on adjacent image will trigger a JPEG decode on first drag — first drag of a new image will be slightly slower; acceptable for now

### Step 4 — draw_zoomed detail

New exported method:
```rust
pub fn draw_zoomed(&mut self, index: usize, scale: f64, pan_x: f64, pan_y: f64) -> Result<(), JsValue>
```
Mirrors the JS `draw_zoomed()` logic:
- Source rect: `(pan_x, pan_y, min(img_w - pan_x, W/scale), min(img_h - pan_y, H/scale))`
- Destination rect: centered in viewport
- Draw from `decode` canvas (put only the source sub-rect? Or put full image and use the 9-arg drawImage?)
- Easiest: put full RGBA into `decode`, then use `draw_image_with_html_canvas_element_and_sx_and_sy_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh` (9-arg form) to crop+scale in one call

### Step 5 — draw_thumbnail (removed; superseded by hybrid decode)

`draw_thumbnail` was removed as part of the hybrid decode milestone. Thumbnails are now drawn entirely in JS:
- `create_thumbnails()` fires one `createImageBitmap(new Blob([renderer.get_image_bytes(i)]))` per image concurrently
- Each promise callback draws the `ImageBitmap` to the thumbnail `<canvas>` at carousel scale using `ctx.drawImage(bitmap, 0, 0, tw, th)` — browser handles downscaling
- `bitmap.close()` is called immediately after to release GPU memory
- No WASM involvement in thumbnail rendering; `pixel_cache` is not used for thumbnails

### Step 6 — draw_hover_indicator detail

New exported method:
```rust
pub fn draw_hover_indicator(&mut self, zone: &str, opacity: f64) -> Result<(), JsValue>
```
- `zone`: `"left"`, `"right"`, or `""` (none)
- Draws `<` / `>` text (or `>>` / `<<` at ends) onto `self.canvas` directly (not backing — drawn on top after blit)
- Font, positioning, alpha all match the JS implementation
- JS animation loop calls `draw()` then `draw_hover_indicator()` (or `draw_zoomed()` then `draw_hover_indicator()`)

### Step 7 — JS wiring detail

Changes to `main.js`:

**`init()`**: create renderer once:
```js
window.renderer = new window.glimr.GlimrRenderer(
    document.getElementById('photo'),
    document.getElementById('backing')
);
```

**`load_zip(buf)`**: replace `GlimrZip` block with:
```js
renderer.load_zip(new Uint8Array(buf));
// No more blob_urls / image_cache / file_sizes setup
// No more archive.free() — renderer.load_zip owns the data
```
Remove `preload_images()` call — first `draw()` triggers lazy decode.

**Global `draw(offset)`** → becomes:
```js
function draw(offset) {
    if (zoom_mode) { draw_zoomed(); return; }
    renderer.draw(current_index, offset || 0);
    renderer.draw_hover_indicator(hover_zone || '', hover_opacity);
}
```

**Global `draw_zoomed()`** → becomes:
```js
function draw_zoomed() {
    renderer.draw_zoomed(current_index, zoom_scale, zoom_pan_x, zoom_pan_y);
    renderer.draw_hover_indicator(hover_zone || '', hover_opacity);
}
```

**`create_thumbnails()`**: still creates canvas DOM elements, but instead of loading `Image` objects:
```js
renderer.draw_thumbnail(canvas, i, carousel_size);
```
(called immediately — decode is synchronous in WASM, no onload needed)

**`download_current()`**:
```js
var bytes = renderer.raw_bytes(current_index);
var url = URL.createObjectURL(new Blob([bytes], {type: 'image/jpeg'}));
var a = document.createElement('a');
a.href = url;
a.download = renderer.image_name(current_index).replace(/\.dat$/i, '.jpg');
document.body.appendChild(a);
a.click();
document.body.removeChild(a);
URL.revokeObjectURL(url);
```

**Remove entirely from JS**: `image_cache`, `blob_urls`, `file_sizes` dicts; `preload_images()`; `draw_image_in_column()`; `steg()`; `draw_hover_indicator()` (JS version).

**`show_info()`**: currently reads `img.naturalWidth/Height` from `image_cache`. After step 7, get dimensions from `renderer` — needs a new exported method `image_dimensions(i) -> Vec<u32>` (returns `[w, h]`) or two methods `image_width(i)` / `image_height(i)`.

**`GlimrZip`** can be removed from `lib.rs` once step 7 is complete and tested.

### Step 8 — LSB watermark stub detail

In `ensure_decoded`, after `rgba.into_raw()` and before inserting into `pixel_cache`, apply a stub mark:
- Magic number: `[0x47, 0x4C, 0x49, 0x4D]` = ASCII "GLIM"  
- Write N copies at evenly-spaced row positions (e.g. every 10% of image height, at the start of that row in the alpha channel or LSB of R channel)
- Each copy: magic (4 bytes) + 28 zero bytes of payload = 32 bytes total, written into LSBs of consecutive R-channel pixels
- Structure chosen so `read-watermark` tool can scan linearly for the magic without knowing placement — find first copy, validate, extract payload
- When real session data is ready: replace the zero payload with timestamp + IP + fingerprint bytes (packed into 28 bytes or extend to more copies)
- This replaces the JS `steg()` placeholder entirely

### Notes / Gotchas

- **`client_width` vs `width`**: `canvas.client_width()` is the CSS rendered size (what the JS `photo_box.clientWidth` returns). `canvas.width()` is the backing buffer attribute. Always use `client_width/height` to get viewport size, then set `canvas.set_width/height` to match — same pattern as the JS.
- **`set_fill_style_str`**: use this, not the deprecated `set_fill_style(&JsValue)`, on web-sys 0.3.99+.
- **`ImageData` constructor**: takes `Clamped<&[u8]>` (from `wasm_bindgen`), not `&Uint8ClampedArray`.
- **Borrow checker / pixel cache**: `draw_image_in_column` uses an explicit block scope to drop the `pixel_cache` borrow before the `self.decode` borrow in the final `draw_image_with_html_canvas_element_and_dw_and_dh` call.
- **Memory**: full-resolution RGBA cache — no LRU yet. Acceptable for typical galleries; add eviction if needed. A 20MP image = 80MB decoded; 10 such images = 800MB. For normal photo galleries (2–5MP) this is fine.
- **WASM binary size**: removing the `image` crate (zune-jpeg + png) shrank the binary noticeably. Build time dropped from ~20s to ~4s.
- **`GlimrZip` removed** — was dead code after Step 7; removed in streaming zip Step 1 along with the `zip` crate dep.
- **`load_zip` still present** in `lib.rs` but no longer called from JS; can be removed in a future cleanup.
- **Borrow conflict in `load_next_entry`**: the immutable borrow of `self.pending_zip` must end before `self.pending_pos` is mutated. Solved with an explicit block scope that computes `(new_pos, maybe_entry)` and drops the borrow before any mutation.
- **Incremental load generation guard**: `load_gen` in JS (mirrors `thumb_gen`) — incremented at the start of each `load_zip` call; the rAF `step` function checks `gen !== load_gen` and returns early if a new load has started.
- **Thumbnail gen guard**: `thumb_gen` incremented on each `create_thumbnails()` call; concurrent `createImageBitmap` promise callbacks check `gen !== thumb_gen` and call `bitmap.close()` before returning if stale.
- **Progress bar lives in loading screen**: created by `create_loading_screen()` — `#progress-track` / `#progress-fill` divs. Fill width updated once per rAF frame. Error div (`#progress-error`) shown in place of bar on failure; both reset on next load.
- **9-arg `drawImage` web-sys name**: `draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh` — `sx` and `sy` are NOT in the method name but are still positional args #2 and #3. The compiler's error suggestion gives the exact name.
- **`OffscreenCanvas` in decode path**: used in `decode_image()` (JS) to extract RGBA from `ImageBitmap` for `receive_pixels`. Supported in all modern browsers (Chrome, Firefox, Edge, Safari 16.4+). No fallback implemented.
- **Carousel closure bug fixed**: `create_thumbnails` loop used `var canvas` (function-scoped) — all click handlers referenced the last canvas created, always navigating to the last image. Fixed with `let canvas` (block-scoped). `navigate_to(i)` now used directly in handlers.
- **`navigate_to` vs `draw`**: `draw(offset)` is now only called for animation frames (drag, hover tick, slide animation) and from within `decode_image`'s callback. All index-changing navigation goes through `navigate_to` which handles the async decode + prefetch.
- **Prefetch on navigate**: after displaying image N, `decode_image` is fired for N-1 and N+1 (no callback) so swipe transitions have pixels ready.

## TODO

- **Carousel rewrite**: Move from canvas-based thumbnails (WASM draw) to pure-JS DOM elements (already done for click handling with `let` fix). Evaluate whether to keep `<canvas>` per thumbnail or switch to `<img>` elements (simpler). Drag-vs-click detection can be simplified with per-element pointer tracking.
- **Step 8 — LSB watermark stub**: In `receive_pixels`, write `"GLIM"` magic + 28 zero payload bytes into RGBA LSBs. Structured for `read-watermark` tool.
- **mozjpeg-sys**: If security model tightens (pixels must not pass through JS), revisit C build toolchain (NASM + CMake + clang wasm32 target) on Windows. Would move decode back into WASM at ~10× speed of zune-jpeg.
- **Rayon parallelism**: `wasm-bindgen-rayon` + `coi-serviceworker`. Less critical now that thumbnail decode is concurrent via `createImageBitmap`. Will matter more once `receive_pixels` applies a real (slow) watermark algorithm.
- **Move `#backing` off DOM**: Create programmatically (like `decode` canvas) — minor security cleanup.
- **Streaming zip — true network streaming**: State-machine parser consuming `ReadableStream` chunks. Enables images appearing before the full zip downloads. Low priority for local-file use case.
- Slideshow / 3-state fullscreen: state 1 = normal; state 2 = fullscreen carousel+image (current); state 3 = fullscreen image-only with play/pause + advance/retreat, no zoom. Separate slideshow entry button desirable despite overlap with 3-state button.
- Desktop (/mobile?) app — Tauri is the natural fit (Rust backend, system WebView, native file dialogs, single distributable); near-term option: local HTTP server binary (`serve` tool)
- Consider support for embedding (iframe now works; longer term: Web Component / `<glimr-player src="...">` once WASM migration reaches Phase 3-4; near-term: `?zip=` URL param to select archive)
- Rename `index.html` → `glimr.html`
- Animate zoom transitions (smooth zoom on wheel/pinch/keyboard)
- Additional zoom options
- Info popup (image dimensions, filename — replaces removed footer)
- `gallery-config.toml` output from packg; WASM build bakes constants
- `read-watermark` tool
- LSB watermarking implementation in WASM
- Frequency-domain watermarking implementation in WASM
- Cache-busting for `main.js`, `main.css`, `Demo.zip`
- Recursion option for packg (currently flat directory only — "maybe" noted)
- PWA manifest for iOS home-screen fullscreen
- Social preview (Open Graph): `meta.json` in source dir → included in zip by packg; deployg extracts designated preview image (XOR-decodes it), stamps `og:title`/`og:image` into output `index.html`; needs `--url` flag or similar for the absolute `og:image` URL
- deployg: S3/Wasabi direct upload (access key support)
- deployg: configurable gallery zip name in output (rather than always Demo.zip)
- WASM true-streaming zip parser (progressive thumbnail display during network download — see "Streaming zip — true network streaming" in TODO above)
