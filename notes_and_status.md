# Glimpse-o-Matic — Notes & Status

## Naming

- Full name: **Glimpse-o-Matic** ('50s–'60s retro charm)
- Short abbreviation: **glim**
- Rust/WASM project name: **glimr**

## Design Notes

### Architecture

- Single-page photo gallery viewer; no build system, no framework (JS prototype phase)
- Canvas-based rendering with a double-buffer (`#photo` + hidden `#backing` canvas)
- Images loaded from a zip archive (`Demo.zip`) via `fflate` (CDN UMD build)
  - Zip entries filtered to image extensions, sorted by filename, turned into blob URLs
  - `blob_urls{}` maps filename → blob URL; `image_cache{}` maps filename → Image object
- Thumbnail carousel in `#header_container`; full viewer in `#lobjet_pane`
- Navigation: swipe/drag with slide animation, keyboard arrows, mouse wheel, click left/center/right thirds
- Zoom mode: tap center third → 100% pan; drag to pan; tap again to exit
- Hover indicators: `<` / `>` arrows fade in/out with idle timeout
- Rust + WASM target: project will be called **glimr**

### Purpose

A watermarked image distribution platform. Goals:
- Zero friction for viewers
- Surreptitious per-session source identification via watermarking
- Static hosted, client-side only

### Intended audience / use case

Three-party model: **photographer** (IP holder), **model** (subject, controls distribution), **patron** (viewer). One zip per gallery — no per-audience variants unless compelling reason. All copies watermarked; model-mode copies carry a distinct mark rather than being unmarked.

### Watermarking — two-layer model

**Pack-time (zip contents)**
- Simple LSB mark baked into images at pack time
- Identifies the gallery/distribution; erased by re-encoding — a low bar, intentionally
- Provides minimal protection for images extracted directly from the zip

**View-time (primary, applied by WASM)**
- Frequency-domain watermark (DCT/DWT spread-spectrum): robust, survives recompression and resizing, ~32–128 bit payload — encodes gallery identity + session fingerprint (timestamp, IP, etc.)
- LSB mark: high capacity, fragile (destroyed by re-encoding), catches direct canvas saves and screenshots; multiple redundant copies around the image, each prefixed by a magic number so a reader tool can scan for them without knowing placement offsets
- Applied in order: decode → frequency-domain mark → LSB mark → blit to canvas
- Images never surface as JS `Image` objects or blob URLs in the WASM era — decoded data stays in WASM memory, only canvas pixels are exposed

### Session data gathered for watermark payload

Passive (no prompt): timestamp, IP (via lightweight outbound call), user-agent, screen/viewport dimensions, timezone, language, WebGL renderer string  
Active (permission requested): geolocation — embedded if granted, skipped if not

### Obfuscation (zip contents)

- Each image XOR'd with a build-specific key (currently notionally `0xAA`; actual key baked into WASM at compile time)
- Files renamed to `[hexdigits].dat` before zipping
- WASM re-XORs on load; key is a compiled-in constant, somewhat opaque in binary
- Not intended to resist determined disassembly — intended to defeat casual extraction

### Tools directory (`tools/`)

CLI tools to be developed:
- **pack-gallery**: input images → XOR encode → rename → zip; outputs `gallery.zip` + `gallery-config.toml` (key, magic number, watermark seed, etc.)
- **read-watermark**: input suspected leaked image → scan for LSB magic number → extract and report session payload; also attempt frequency-domain extraction
- Build step reads `gallery-config.toml` and bakes constants into WASM

## Status

Core JS viewer fully functional:
- Zip loading, blob URL creation, image preloading
- Thumbnail carousel with drag-to-scroll and active-image highlight
- Full viewer with swipe, zoom/pan, hover indicators, keyboard/wheel navigation
- Loading screen (wave-bounce animation) shown during zip fetch + first image decode
- Build script stamps `index.html` to bust browser cache

## Newly Implemented

- `fflate` CDN script tag added to `index.html`
- `Demo.zip` as image source; hardcoded `images[]` array removed
- `blob_urls{}` introduced alongside `image_cache{}`
- `create_thumbnails()` and `preload_images()` updated to use blob URLs
- Footer view/download links updated to use blob URLs (download gets original filename)
- Loading screen: CSS wave-bounce animation, large viewport-relative text (`5vw`), removed on first image ready
- `build.ps1`: stamps `<!-- Build MMDD:HHMM -->` comment in `index.html`; run before each refresh

## TODO

- File picker (`<input type="file">`) to load arbitrary zip archives locally
- Cache-busting for `main.js`, `main.css`, `Demo.zip` (currently only `index.html` is touched by the build script)
- Scroll wheel behavior
- Additional zoom options
- `tools/` directory and initial `pack-gallery` CLI tool
- Migration to Rust → **glimr**
