use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use std::collections::HashMap;
use std::io::Read;
use flate2::read::DeflateDecoder;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

pub mod watermark;

const XOR_KEY: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

/// Max bytes of watermarked RGBA held in `pixel_cache` before eviction kicks in.
/// ~10 images at 6 MP (24 MB each), more at lower resolutions.
const PIXEL_CACHE_BUDGET: usize = 250 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Console logging — [HH:MM:SS.SSS] <func> msg
// ---------------------------------------------------------------------------

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

fn glog(func: &str, msg: &str) {
    let d = js_sys::Date::new_0();
    log(&format!(
        "[{:02}:{:02}:{:02}.{:03}] <{}> {}",
        d.get_hours() as u32,
        d.get_minutes() as u32,
        d.get_seconds() as u32,
        d.get_milliseconds() as u32,
        func, msg
    ));
}

/// Exported so JS can emit timestamped log lines in the same format.
#[wasm_bindgen]
pub fn glimr_log(func: &str, msg: &str) {
    glog(func, msg);
}

fn xor_bytes(input: &[u8]) -> Vec<u8> {
    input.iter().enumerate()
        .map(|(i, &b)| b ^ XOR_KEY[i % 4])
        .collect()
}

fn is_image_ext(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".jpg")  || n.ends_with(".jpeg") ||
    n.ends_with(".png")  || n.ends_with(".gif")  ||
    n.ends_with(".webp") || n.ends_with(".dat")
}

// Reserved social-preview entries: carried in the archive (for a future splash /
// About panel) but never treated as gallery images. Ignored for now.
fn is_reserved(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    matches!(base.as_str(),
        "social_preview.jpg" | "social_preview.jpeg" |
        "social_preview.png" | "social_preview.txt")
}

// NOTE: not exported to JS. `xor_bytes` is used internally by the streaming zip parser
// (`feed_bytes`) to de-obfuscate `.dat` entries. It was previously `#[wasm_bindgen] pub fn
// xor_decode`, but an exported de-obfuscation primitive is a turnkey oracle — anyone could pull a
// `.dat` from the Network tab and call it to recover the pristine original. Removing the export
// forces a would-be deobfuscator to find the key in the WASM and reimplement XOR (more
// inconvenience, which is the whole point of the obfuscation layer). Re-export if a tool ever needs it.

// ---------------------------------------------------------------------------
// Streaming zip parser state machine.
// Advances through local file headers sequentially; errors on data descriptors
// (bit 3), zip64, unknown compression, or bad signatures.
// ---------------------------------------------------------------------------

enum StreamState {
    NeedHeader,
    NeedFilename {
        compression: u16,
        comp_size:   u32,
        fname_len:   usize,
        extra_len:   usize,
    },
    NeedData {
        name:        String,
        compression: u16,
        comp_size:   u32,
    },
}

// ---------------------------------------------------------------------------
// GlimrRenderer — Phase 2+: WASM-side canvas rendering
// ---------------------------------------------------------------------------

fn get_2d_context(canvas: &HtmlCanvasElement) -> Result<CanvasRenderingContext2d, JsValue> {
    canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into::<CanvasRenderingContext2d>()
        .map_err(|_| JsValue::from_str("2d context is wrong type"))
}

fn new_canvas() -> Result<HtmlCanvasElement, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?
        .document().ok_or_else(|| JsValue::from_str("no document"))?
        .create_element("canvas")?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("failed to create canvas"))
}

#[wasm_bindgen]
pub struct GlimrRenderer {
    names:       Vec<String>,
    image_bytes: Vec<Vec<u8>>,                         // XOR-decoded JPEG/PNG bytes
    pixel_cache: HashMap<usize, (u32, u32, Vec<u8>)>, // watermarked RGBA (full-res; download + zoom)
    // Screen-size watermarked drawables for the swipe view, built once per image from
    // pixel_cache. Keyed by index, lifetime-aligned with pixel_cache, flushed when the
    // pane geometry changes (tracked via the backing canvas size in draw()).
    scroll_cache: HashMap<usize, HtmlCanvasElement>,
    canvas:  HtmlCanvasElement,  // #photo
    // #backing — off-screen compose target: each frame composes the gray fill + columns here,
    // then blits to #photo. INTENTIONAL — added in early prototyping to avoid flicker/glitching
    // when compositing the swipe directly to the visible canvas. Keep it; don't "optimize" the
    // indirection away without verifying the swipe stays glitch-free on real devices.
    backing: HtmlCanvasElement,
    decode:  HtmlCanvasElement,  // hidden, used to scale-blit decoded images
    // Streaming zip parse state
    stream_buf:   Vec<u8>,
    stream_state: StreamState,
    stream_done:  bool,
}

#[wasm_bindgen]
impl GlimrRenderer {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement, backing: HtmlCanvasElement) -> Result<GlimrRenderer, JsValue> {
        let decode = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .document()
            .ok_or_else(|| JsValue::from_str("no document"))?
            .create_element("canvas")?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| JsValue::from_str("failed to create decode canvas"))?;
        Ok(GlimrRenderer {
            names:       Vec::new(),
            image_bytes: Vec::new(),
            pixel_cache: HashMap::new(),
            scroll_cache: HashMap::new(),
            canvas,
            backing,
            decode,
            stream_buf:   Vec::new(),
            stream_state: StreamState::NeedHeader,
            stream_done:  false,
        })
    }

    // --- Streaming load API ---

    /// Resets all image state and parser state. Call before feeding the first chunk.
    pub fn begin_zip_stream(&mut self) {
        glog("begin_zip_stream", "start");
        self.names.clear();
        self.image_bytes.clear();
        self.pixel_cache.clear();
        self.scroll_cache.clear();
        self.stream_buf.clear();
        self.stream_state = StreamState::NeedHeader;
        self.stream_done  = false;
    }

    /// Feed the next chunk of zip bytes. Advances the state machine as far as
    /// possible, decompressing and XOR-decoding each complete entry. Returns
    /// the total number of image entries ready so far. Errors on malformed zip.
    pub fn feed_bytes(&mut self, chunk: &[u8]) -> Result<u32, JsValue> {
        self.stream_buf.extend_from_slice(chunk);

        loop {
            // Take state out to avoid simultaneous borrow conflicts on self.
            let state = std::mem::replace(&mut self.stream_state, StreamState::NeedHeader);

            match state {
                StreamState::NeedHeader => {
                    if self.stream_buf.len() < 4 {
                        self.stream_state = StreamState::NeedHeader;
                        break;
                    }
                    let sig = u32::from_le_bytes(
                        self.stream_buf[0..4].try_into().unwrap()
                    );
                    match sig {
                        0x04034b50 => {
                            if self.stream_buf.len() < 30 {
                                self.stream_state = StreamState::NeedHeader;
                                break;
                            }
                            let flags       = u16::from_le_bytes(self.stream_buf[ 6.. 8].try_into().unwrap());
                            let compression = u16::from_le_bytes(self.stream_buf[ 8..10].try_into().unwrap());
                            let comp_size   = u32::from_le_bytes(self.stream_buf[18..22].try_into().unwrap());
                            let fname_len   = u16::from_le_bytes(self.stream_buf[26..28].try_into().unwrap()) as usize;
                            let extra_len   = u16::from_le_bytes(self.stream_buf[28..30].try_into().unwrap()) as usize;

                            if flags & 0x0008 != 0 {
                                return Err(JsValue::from_str(
                                    "zip uses data descriptors — not supported. Try a zip created by Windows or macOS."
                                ));
                            }
                            if comp_size == 0xFFFF_FFFF {
                                return Err(JsValue::from_str("zip64 format is not supported."));
                            }
                            if compression != 0 && compression != 8 {
                                return Err(JsValue::from_str(&format!(
                                    "unsupported compression method {compression} — only stored and deflate are supported."
                                )));
                            }

                            self.stream_buf.drain(0..30);
                            self.stream_state = StreamState::NeedFilename {
                                compression, comp_size, fname_len, extra_len,
                            };
                        }
                        0x02014b50 | 0x06054b50 => {
                            // Central directory or EOCD — done.
                            self.stream_done = true;
                            self.stream_buf.clear();
                            glog("feed_bytes", &format!("done — {} images", self.names.len()));
                            break;
                        }
                        _ => {
                            return Err(JsValue::from_str(&format!(
                                "unrecognised zip signature 0x{sig:08x} — file may be corrupted or an unsupported format."
                            )));
                        }
                    }
                }

                StreamState::NeedFilename { compression, comp_size, fname_len, extra_len } => {
                    let need = fname_len + extra_len;
                    if self.stream_buf.len() < need {
                        self.stream_state = StreamState::NeedFilename {
                            compression, comp_size, fname_len, extra_len,
                        };
                        break;
                    }
                    let name = String::from_utf8_lossy(&self.stream_buf[..fname_len]).into_owned();
                    self.stream_buf.drain(0..need);
                    self.stream_state = StreamState::NeedData { name, compression, comp_size };
                }

                StreamState::NeedData { name, compression, comp_size } => {
                    let need = comp_size as usize;
                    if self.stream_buf.len() < need {
                        self.stream_state = StreamState::NeedData { name, compression, comp_size };
                        break;
                    }

                    if !name.ends_with('/') && is_image_ext(&name) && !is_reserved(&name) {
                        let compressed = &self.stream_buf[..need];
                        let raw: Vec<u8> = match compression {
                            0 => compressed.to_vec(),
                            8 => {
                                let mut out = Vec::new();
                                DeflateDecoder::new(compressed).read_to_end(&mut out)
                                    .map_err(|e| JsValue::from_str(&format!("deflate error: {e}")))?;
                                out
                            }
                            _ => unreachable!(),
                        };
                        let decoded = if name.to_lowercase().ends_with(".dat") {
                            xor_bytes(&raw)
                        } else {
                            raw
                        };
                        glog("feed_bytes", &format!("entry {} ready: {}", self.names.len(), name));
                        self.names.push(name);
                        self.image_bytes.push(decoded);
                    }

                    self.stream_buf.drain(0..need);
                    self.stream_state = StreamState::NeedHeader;
                }
            }
        }

        Ok(self.names.len() as u32)
    }

    /// True once a central directory or end-of-archive signature has been seen.
    pub fn is_stream_done(&self) -> bool {
        self.stream_done
    }

    pub fn image_count(&self) -> usize {
        self.names.len()
    }

    pub fn image_name(&self, i: usize) -> String {
        self.names[i].clone()
    }

    /// Watermarked RGBA pixels for image `i` at native resolution (for export).
    /// Empty if the image hasn't been decoded/watermarked yet.  This is the only
    /// full-resolution image data exposed to JS for download — it is always
    /// watermarked; the un-watermarked source bytes are never handed out for export.
    pub fn watermarked_pixels(&self, i: usize) -> js_sys::Uint8Array {
        match self.pixel_cache.get(&i) {
            Some((_, _, px)) => js_sys::Uint8Array::from(px.as_slice()),
            None             => js_sys::Uint8Array::new_with_length(0),
        }
    }

    pub fn image_width(&self, i: usize) -> u32 {
        self.pixel_cache.get(&i).map(|e| e.0).unwrap_or(0)
    }

    pub fn image_height(&self, i: usize) -> u32 {
        self.pixel_cache.get(&i).map(|e| e.1).unwrap_or(0)
    }

    pub fn image_file_size(&self, i: usize) -> usize {
        self.image_bytes[i].len()
    }

    /// Returns the raw (XOR-decoded) bytes for image i as a Uint8Array.
    pub fn get_image_bytes(&self, i: usize) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.image_bytes[i].as_slice())
    }

    /// Stores watermarked RGBA pixels for image i. Called by JS after
    /// createImageBitmap → OffscreenCanvas → getImageData.
    /// `payload` is the 16-byte watermark payload assembled by JS `build_payload()`.
    pub fn receive_pixels(&mut self, i: usize, width: u32, height: u32, data: &[u8], payload: &[u8]) -> Result<(), JsValue> {
        let t0 = js_sys::Date::now();
        let w = width as usize;
        let h = height as usize;

        let y_orig = watermark::extract_y(data);
        let mut y  = y_orig.clone();

        let mut wm_payload = [0u8; 16];
        let n = payload.len().min(16);
        wm_payload[..n].copy_from_slice(&payload[..n]);
        watermark::embed_y(&mut y, w, h, &wm_payload);

        let mut pixels = data.to_vec();
        watermark::write_y_delta(&mut pixels, &y_orig, &y);

        let ms = js_sys::Date::now() - t0;
        glog("receive_pixels", &format!("image {} {}×{} watermarked in {:.0}ms", i, width, height, ms));
        self.pixel_cache.insert(i, (width, height, pixels));
        // Build the screen-size swipe surface now, as a by-product (no extra watermarking).
        // No-op until the pane has a known size (first draw); draw() rebuilds lazily after that.
        let _ = self.build_scroll_surface(i);
        Ok(())
    }

    /// Build (or refresh) the screen-size watermarked drawable for image `i` from its
    /// full-res pixels, sized to fit the current pane (the backing-canvas geometry).
    /// Cheap downscale of pixels we already watermarked — never triggers a new watermark.
    fn build_scroll_surface(&mut self, i: usize) -> Result<(), JsValue> {
        let pw = self.canvas.width();
        let ph = self.canvas.height();
        if pw == 0 || ph == 0 { return Ok(()); }                 // pane not laid out yet
        let (img_w, img_h) = match self.pixel_cache.get(&i) {
            Some(e) => (e.0, e.1),
            None => return Ok(()),
        };
        let scale = f64::min(pw as f64 / img_w as f64, ph as f64 / img_h as f64);
        let sw = ((img_w as f64 * scale).round() as u32).max(1);
        let sh = ((img_h as f64 * scale).round() as u32).max(1);
        if let Some(c) = self.scroll_cache.get(&i) {             // already current at this size
            if c.width() == sw && c.height() == sh { return Ok(()); }
        }
        self.decode.set_width(img_w);
        self.decode.set_height(img_h);
        let dctx = get_2d_context(&self.decode)?;
        {
            let px = &self.pixel_cache[&i].2;
            let img_data = ImageData::new_with_u8_clamped_array_and_sh(Clamped(px.as_slice()), img_w, img_h)?;
            dctx.put_image_data(&img_data, 0.0, 0.0)?;
        }
        let surf = new_canvas()?;
        surf.set_width(sw);
        surf.set_height(sh);
        get_2d_context(&surf)?.draw_image_with_html_canvas_element_and_dw_and_dh(
            &self.decode, 0.0, 0.0, sw as f64, sh as f64)?;
        self.scroll_cache.insert(i, surf);
        Ok(())
    }

    /// Returns true if image i has been decoded and cached.
    pub fn is_decoded(&self, i: usize) -> bool {
        self.pixel_cache.contains_key(&i)
    }

    /// Cap the watermarked-RGBA cache at PIXEL_CACHE_BUDGET bytes so a large
    /// catalog can't grow `pixel_cache` without bound.  Evicts the cached image
    /// farthest (by index) from `current` first, never evicting `current` itself.
    /// Evicted images are simply re-decoded + re-watermarked if revisited.
    /// JS calls this after each `receive_pixels`, anchored on the displayed image.
    pub fn enforce_cache_budget(&mut self, current: usize) {
        let mut total: usize = self.pixel_cache.values().map(|(_, _, px)| px.len()).sum();
        while total > PIXEL_CACHE_BUDGET && self.pixel_cache.len() > 1 {
            let victim = self.pixel_cache.keys().copied()
                .filter(|&i| i != current)
                .max_by_key(|&i| if i >= current { i - current } else { current - i });
            match victim {
                Some(v) => {
                    if let Some((_, _, px)) = self.pixel_cache.remove(&v) {
                        total -= px.len();
                        self.scroll_cache.remove(&v); // lifetime-aligned with the full-res entry
                    }
                }
                None => break, // only `current` remains
            }
        }
    }

    /// Draw image at `index` onto the photo canvas.
    pub fn draw(&mut self, index: usize, offset: f64) -> Result<(), JsValue> {
        let w = self.canvas.client_width() as u32;
        let h = self.canvas.client_height() as u32;
        if w == 0 || h == 0 { return Ok(()); }

        // Resize buffers only when the pane geometry actually changes — and when it does,
        // flush the (now wrong-size) screen-size scroll surfaces so they rebuild for the new
        // geometry. The backing-canvas size doubles as the "geometry the surfaces were built
        // for" tracker.
        if self.canvas.width() != w || self.canvas.height() != h {
            self.backing.set_width(w);
            self.backing.set_height(h);
            self.canvas.set_width(w);
            self.canvas.set_height(h);
            self.scroll_cache.clear();
        }

        let back_ctx = get_2d_context(&self.backing)?;
        back_ctx.set_fill_style_str("#777777");
        back_ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

        let wf = w as f64;
        let hf = h as f64;

        if offset > 0.0 && index > 0 {
            self.draw_image_in_column(&back_ctx, index - 1, offset - wf, wf, hf)?;
        }
        self.draw_image_in_column(&back_ctx, index, offset, wf, hf)?;
        if offset < 0.0 && index + 1 < self.names.len() {
            self.draw_image_in_column(&back_ctx, index + 1, offset + wf, wf, hf)?;
        }

        get_2d_context(&self.canvas)?
            .draw_image_with_html_canvas_element(&self.backing, 0.0, 0.0)
            .map_err(|e| e)
    }

    /// Renders image `index` in zoom/pan mode.
    pub fn draw_zoomed(&mut self, index: usize, scale: f64, pan_x: f64, pan_y: f64) -> Result<(), JsValue> {
        let w = self.canvas.client_width() as u32;
        let h = self.canvas.client_height() as u32;
        if w == 0 || h == 0 { return Ok(()); }

        let (img_w, img_h) = match self.pixel_cache.get(&index) {
            Some(e) => (e.0, e.1),
            None => return Ok(()),
        };

        let wf = w as f64;
        let hf = h as f64;

        let src_w = f64::min(img_w as f64 - pan_x, wf / scale);
        let src_h = f64::min(img_h as f64 - pan_y, hf / scale);
        let dst_w = src_w * scale;
        let dst_h = src_h * scale;
        let dst_x = (wf - dst_w) / 2.0;
        let dst_y = (hf - dst_h) / 2.0;

        self.backing.set_width(w);
        self.backing.set_height(h);
        let back_ctx = get_2d_context(&self.backing)?;
        back_ctx.set_fill_style_str("#777777");
        back_ctx.fill_rect(0.0, 0.0, wf, hf);

        self.decode.set_width(img_w);
        self.decode.set_height(img_h);
        let decode_ctx = get_2d_context(&self.decode)?;
        let img_data = {
            let pixels = &self.pixel_cache[&index].2;
            ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels.as_slice()), img_w, img_h)?
        };
        decode_ctx.put_image_data(&img_data, 0.0, 0.0)?;

        back_ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            &self.decode,
            pan_x, pan_y, src_w, src_h,
            dst_x, dst_y, dst_w, dst_h,
        ).map_err(|e| e)?;

        self.canvas.set_width(w);
        self.canvas.set_height(h);
        get_2d_context(&self.canvas)?
            .draw_image_with_html_canvas_element(&self.backing, 0.0, 0.0)
            .map_err(|e| e)
    }

    /// Draws the hover arrow directly onto `self.canvas`.
    pub fn draw_hover_indicator(&mut self, index: usize, zone: &str, opacity: f64) -> Result<(), JsValue> {
        if opacity <= 0.0 || zone.is_empty() { return Ok(()); }

        let w = self.canvas.client_width() as f64;
        let h = self.canvas.client_height() as f64;

        let symbol = match zone {
            "left"  => if index == 0                       { ">>" } else { "<" },
            "right" => if index + 1 >= self.names.len()   { "<<" } else { ">" },
            _       => return Ok(()),
        };
        let cx = if zone == "left" { w / 6.0 } else { w * 5.0 / 6.0 };

        let ctx = get_2d_context(&self.canvas)?;
        ctx.save();
        ctx.set_global_alpha(opacity * 0.6);
        ctx.set_fill_style_str("#ddd");
        ctx.set_shadow_color("#555");
        ctx.set_shadow_blur(4.0);
        ctx.set_shadow_offset_x(3.0);
        ctx.set_shadow_offset_y(3.0);
        ctx.set_font(&format!("bold {}px sans-serif", (h * 0.10).round() as u32));
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.fill_text(symbol, cx, h / 2.0)?;
        ctx.restore();
        Ok(())
    }

    fn draw_image_in_column(
        &mut self,
        ctx: &CanvasRenderingContext2d,
        index: usize,
        col_x: f64,
        col_w: f64,
        col_h: f64,
    ) -> Result<(), JsValue> {
        let (img_w, img_h) = match self.pixel_cache.get(&index) {
            Some(e) => (e.0, e.1),
            None => {
                ctx.set_fill_style_str("#777777");
                ctx.fill_rect(col_x, 0.0, col_w, col_h);
                return Ok(());
            }
        };

        // Blit the pre-scaled, screen-size watermarked surface (built once) — no per-frame
        // full-res put_image_data. It's already at the fit-display size, so this is a plain
        // translated blit; `offset` is carried by `col_x`.
        self.build_scroll_surface(index)?; // cheap if already cached at the current geometry

        let iw = img_w as f64;
        let ih = img_h as f64;
        let scale = f64::min(col_w / iw, col_h / ih);
        let dst_w = iw * scale;
        let dst_h = ih * scale;
        let h_pad = (col_w - dst_w) / 2.0;
        let v_pad = (col_h - dst_h) / 2.0;

        if let Some(surf) = self.scroll_cache.get(&index) {
            ctx.draw_image_with_html_canvas_element(surf, col_x + h_pad, v_pad).map_err(|e| e)
        } else {
            ctx.set_fill_style_str("#777777");
            ctx.fill_rect(col_x, 0.0, col_w, col_h);
            Ok(())
        }
    }
}
