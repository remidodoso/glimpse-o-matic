# Glimpse-o-Matic — Notes & Status

## Design Notes

- Single-page photo gallery viewer; no build system, no framework
- Canvas-based rendering with a double-buffer (`#photo` + hidden `#backing` canvas)
- Images are loaded from a zip archive (`Demo.zip`) via `fflate` (CDN UMD build)
  - Zip entries are filtered to image extensions, sorted by filename, turned into blob URLs
  - `blob_urls{}` maps filename → blob URL; `image_cache{}` maps filename → Image object
  - The rest of the rendering pipeline uses filenames as keys throughout
- Thumbnail carousel lives in `#header_container`; full viewer in `#lobjet_pane`
- Navigation: swipe/drag with slide animation, keyboard arrows, mouse wheel, click left/center/right thirds of the viewer
- Zoom mode: tap center third → 100% pan view; drag to pan; tap again to exit
- Hover indicators: `<` / `>` arrows fade in/out with idle timeout on the left/right thirds
- `steg()`: zeros the low 4 bits of a 100-pixel horizontal strip at center of the canvas on each draw — steganographic marker
- Intended to migrate to Rust eventually; JS/HTML/CSS is the prototype phase

## Status

Core viewer is fully functional:
- Zip loading, blob URL creation, image preloading
- Thumbnail carousel with drag-to-scroll and active-image highlight
- Full viewer with swipe, zoom/pan, hover indicators, keyboard/wheel navigation
- Loading screen shown during zip fetch + first image decode
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
- Migration to Rust
