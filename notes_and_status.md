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
- Click/tap **center third**: enter zoom mode (1:1 pixel scale)
- **Swipe/drag**: pan in zoom mode; slide-navigate in normal mode (25% threshold)
- **Hover indicators**: `<` / `>` arrows fade in/out on left/right thirds, idle-timeout fade

### Zoom Mode

- Entry: tap center → zoom_scale resets to 1.0, pan set to keep tapped pixel fixed
- Exit: tap (without drag) while in zoom mode
- Range: fit-scale (image fills viewport) → 2.0× (double pixel size)
- **Scroll wheel**: zooms toward cursor position (`Math.pow(1.001, -deltaY)` factor — smooth for both mice and trackpads)
- **Pinch to zoom**: two-finger touch, zooms toward midpoint between fingers
- **Ctrl+= / Ctrl+−**: 25% steps toward viewport center
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

- `←` / `→` arrows: navigate previous/next
- `f` / `F`: toggle fullscreen
- `Ctrl+=` / `Ctrl++`: zoom in (in zoom mode)
- `Ctrl+−`: zoom out (in zoom mode)
- `Esc`: exit fullscreen (browser native, `fullscreenchange` updates button state)

### Zip Loading Pipeline (`load_zip(buf)`)

- Cancels in-flight animations, shows loading screen
- Revokes old blob URLs (memory management)
- Resets all state: images, blob_urls, image_cache, zoom, current_index, thumbnails
- Passes raw bytes to `GlimrZip` (WASM) which parses, filters, sorts, and XOR-decodes
- JS iterates entries via `entry_name(i)` / `entry_data(i)`, creates blob URLs
- Calls `archive.free()` to release WASM memory after all blob URLs are created
- Used for both initial `Demo.zip` fetch and file-picker loads
- No external CDN dependencies — fflate removed

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

## TODO

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
- deployg: S3/Wasabi direct upload (access key support)
- deployg: configurable gallery zip name in output (rather than always Demo.zip)
- WASM streaming zip parser (progressive thumbnail display during download)
