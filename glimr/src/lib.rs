use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use std::collections::HashMap;
use std::io::Read;
use flate2::read::DeflateDecoder;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

const XOR_KEY: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

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

// Exported for direct use (e.g. future streaming path)
#[wasm_bindgen]
pub fn xor_decode(input: &[u8]) -> Vec<u8> {
    xor_bytes(input)
}

// ---------------------------------------------------------------------------
// Streaming zip parser — reads local file headers sequentially from byte 0.
// Errors out on data descriptors (bit 3), zip64, unknown compression, or a
// bad signature. Windows Explorer and packg zips are always compatible.
// ---------------------------------------------------------------------------

fn parse_zip_streaming(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, JsValue> {
    let mut pos = 0;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    loop {
        if pos + 4 > data.len() { break; }

        let sig = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        match sig {
            0x04034b50 => {
                if pos + 30 > data.len() {
                    return Err(JsValue::from_str("truncated local file header"));
                }

                let flags       = u16::from_le_bytes(data[pos +  6..pos +  8].try_into().unwrap());
                let compression = u16::from_le_bytes(data[pos +  8..pos + 10].try_into().unwrap());
                let comp_size   = u32::from_le_bytes(data[pos + 18..pos + 22].try_into().unwrap());
                let fname_len   = u16::from_le_bytes(data[pos + 26..pos + 28].try_into().unwrap()) as usize;
                let extra_len   = u16::from_le_bytes(data[pos + 28..pos + 30].try_into().unwrap()) as usize;

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

                let data_start = pos + 30 + fname_len + extra_len;
                let data_end   = data_start + comp_size as usize;
                if data_end > data.len() {
                    return Err(JsValue::from_str("truncated entry data"));
                }

                let name = String::from_utf8_lossy(&data[pos + 30..pos + 30 + fname_len]).into_owned();
                let compressed = &data[data_start..data_end];
                pos = data_end;

                if name.ends_with('/') || !is_image_ext(&name) { continue; }

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

                entries.push((name, decoded));
            }
            0x02014b50 | 0x06054b50 => break, // central directory or EOCD — done
            _ => return Err(JsValue::from_str(&format!(
                "unrecognised zip signature 0x{sig:08x} at offset {pos} — file may be corrupted or an unsupported format."
            ))),
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
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

#[wasm_bindgen]
pub struct GlimrRenderer {
    names: Vec<String>,
    image_bytes: Vec<Vec<u8>>,                          // XOR-decoded JPEG/PNG bytes
    pixel_cache: HashMap<usize, (u32, u32, Vec<u8>)>,  // lazily decoded RGBA
    canvas: HtmlCanvasElement,   // #photo
    backing: HtmlCanvasElement,  // #backing
    decode: HtmlCanvasElement,   // hidden, used to scale-blit decoded images
    // Incremental load state — active between begin_zip_load and finish_zip_load
    pending_zip:     Vec<u8>,
    pending_pos:     usize,
    pending_entries: Vec<(String, Vec<u8>)>,
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
            names: Vec::new(),
            image_bytes: Vec::new(),
            pixel_cache: HashMap::new(),
            canvas,
            backing,
            decode,
            pending_zip:     Vec::new(),
            pending_pos:     0,
            pending_entries: Vec::new(),
        })
    }

    pub fn load_zip(&mut self, zip_bytes: &[u8]) -> Result<(), JsValue> {
        glog("load_zip", &format!("start ({} bytes)", zip_bytes.len()));
        let entries = parse_zip_streaming(zip_bytes)?;
        self.pixel_cache.clear();
        self.names       = entries.iter().map(|(n, _)| n.clone()).collect();
        self.image_bytes = entries.into_iter().map(|(_, b)| b).collect();
        glog("load_zip", &format!("done — {} images loaded", self.names.len()));
        Ok(())
    }

    // --- Incremental load API (used by JS rAF loop for progress reporting) ---

    /// Stores the zip bytes and resets parse state. Returns total byte count
    /// so JS can compute progress as load_bytes_done() / total.
    pub fn begin_zip_load(&mut self, zip_bytes: &[u8]) -> u32 {
        glog("begin_zip_load", &format!("start ({} bytes)", zip_bytes.len()));
        self.pending_zip = zip_bytes.to_vec();
        self.pending_pos = 0;
        self.pending_entries.clear();
        zip_bytes.len() as u32
    }

    /// Parses one local file header. Returns Ok(false) while entries remain,
    /// Ok(true) when the central directory is reached or the buffer is exhausted,
    /// Err if the zip is malformed or unsupported.
    pub fn load_next_entry(&mut self) -> Result<bool, JsValue> {
        let pos = self.pending_pos;

        if pos + 4 > self.pending_zip.len() {
            return Ok(true);
        }

        let sig = u32::from_le_bytes(self.pending_zip[pos..pos + 4].try_into().unwrap());
        match sig {
            0x04034b50 => {
                // Parse the header and decompress the entry inside a block so the
                // immutable borrow of self.pending_zip ends before we mutate self.
                let (new_pos, maybe_entry): (usize, Option<(String, Vec<u8>)>) = {
                    let data = &self.pending_zip;

                    if pos + 30 > data.len() {
                        return Err(JsValue::from_str("truncated local file header"));
                    }

                    let flags       = u16::from_le_bytes(data[pos +  6..pos +  8].try_into().unwrap());
                    let compression = u16::from_le_bytes(data[pos +  8..pos + 10].try_into().unwrap());
                    let comp_size   = u32::from_le_bytes(data[pos + 18..pos + 22].try_into().unwrap());
                    let fname_len   = u16::from_le_bytes(data[pos + 26..pos + 28].try_into().unwrap()) as usize;
                    let extra_len   = u16::from_le_bytes(data[pos + 28..pos + 30].try_into().unwrap()) as usize;

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

                    let data_start = pos + 30 + fname_len + extra_len;
                    let data_end   = data_start + comp_size as usize;
                    if data_end > data.len() {
                        return Err(JsValue::from_str("truncated entry data"));
                    }

                    let name = String::from_utf8_lossy(&data[pos + 30..pos + 30 + fname_len]).into_owned();

                    let entry = if !name.ends_with('/') && is_image_ext(&name) {
                        let compressed = &data[data_start..data_end];
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
                        Some((name, decoded))
                    } else {
                        None
                    };

                    (data_end, entry)
                };

                self.pending_pos = new_pos;
                if let Some(e) = maybe_entry {
                    self.pending_entries.push(e);
                }
                Ok(false)
            }
            0x02014b50 | 0x06054b50 => {
                self.pending_pos = self.pending_zip.len();
                glog("load_next_entry", &format!("done — {} image entries", self.pending_entries.len()));
                Ok(true)
            }
            _ => Err(JsValue::from_str(&format!(
                "unrecognised zip signature 0x{sig:08x} at offset {pos} — file may be corrupted or unsupported."
            ))),
        }
    }

    /// Current byte position in the pending zip; divide by begin_zip_load's
    /// return value to get extraction progress (0.0–1.0).
    pub fn load_bytes_done(&self) -> u32 {
        self.pending_pos as u32
    }

    /// Sorts accumulated entries, populates names/image_bytes, frees the
    /// buffered zip bytes. Call once load_next_entry returns Ok(true).
    pub fn finish_zip_load(&mut self) -> Result<(), JsValue> {
        let mut entries = std::mem::take(&mut self.pending_entries);
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        self.pixel_cache.clear();
        self.names       = entries.iter().map(|(n, _)| n.clone()).collect();
        self.image_bytes = entries.into_iter().map(|(_, b)| b).collect();
        self.pending_zip = Vec::new();
        glog("finish_zip_load", &format!("done — {} images", self.names.len()));
        Ok(())
    }

    pub fn image_count(&self) -> usize {
        self.names.len()
    }

    pub fn image_name(&self, i: usize) -> String {
        self.names[i].clone()
    }

    /// Returns the XOR-decoded JPEG/PNG bytes for image i.
    /// JS uses this for the download button (one-shot blob URL, revoked immediately after click).
    pub fn raw_bytes(&self, i: usize) -> Vec<u8> {
        self.image_bytes[i].clone()
    }

    /// Decoded pixel width; 0 if image i has not been drawn yet.
    pub fn image_width(&self, i: usize) -> u32 {
        self.pixel_cache.get(&i).map(|e| e.0).unwrap_or(0)
    }

    /// Decoded pixel height; 0 if image i has not been drawn yet.
    pub fn image_height(&self, i: usize) -> u32 {
        self.pixel_cache.get(&i).map(|e| e.1).unwrap_or(0)
    }

    /// Size of the stored (XOR-decoded) JPEG/PNG bytes for image i.
    pub fn image_file_size(&self, i: usize) -> usize {
        self.image_bytes[i].len()
    }

    /// Returns the raw (XOR-decoded) bytes for image i as a Uint8Array.
    /// JS passes these to createImageBitmap; the Blob is transient and never
    /// stored as an accessible object.
    pub fn get_image_bytes(&self, i: usize) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.image_bytes[i].as_slice())
    }

    /// Stores watermarked RGBA pixels for image i. Called by JS after
    /// createImageBitmap → OffscreenCanvas → getImageData. Watermark
    /// is applied here before caching.
    pub fn receive_pixels(&mut self, i: usize, width: u32, height: u32, data: &[u8]) -> Result<(), JsValue> {
        // TODO: apply spread-spectrum watermark here
        glog("receive_pixels", &format!("image {} {}×{}", i, width, height));
        self.pixel_cache.insert(i, (width, height, data.to_vec()));
        Ok(())
    }

    /// Returns true if image i has been decoded and cached.
    pub fn is_decoded(&self, i: usize) -> bool {
        self.pixel_cache.contains_key(&i)
    }

    /// Draw image at `index` onto the photo canvas.
    /// `offset` is the slide drag offset in CSS pixels:
    ///   > 0 → dragging right (prev image enters from left)
    ///   < 0 → dragging left  (next image enters from right)
    pub fn draw(&mut self, index: usize, offset: f64) -> Result<(), JsValue> {
        let w = self.canvas.client_width() as u32;
        let h = self.canvas.client_height() as u32;
        if w == 0 || h == 0 { return Ok(()); }

        // Size and clear the backing canvas.
        self.backing.set_width(w);
        self.backing.set_height(h);
        let back_ctx = get_2d_context(&self.backing)?;
        back_ctx.set_fill_style_str("#777777");
        back_ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

        let wf = w as f64;
        let hf = h as f64;

        // Prev image enters from the left when dragging right.
        if offset > 0.0 && index > 0 {
            self.draw_image_in_column(&back_ctx, index - 1, offset - wf, wf, hf)?;
        }

        // Current image, shifted by the drag offset.
        self.draw_image_in_column(&back_ctx, index, offset, wf, hf)?;

        // Next image enters from the right when dragging left.
        if offset < 0.0 && index + 1 < self.names.len() {
            self.draw_image_in_column(&back_ctx, index + 1, offset + wf, wf, hf)?;
        }

        // Blit backing → photo canvas.
        self.canvas.set_width(w);
        self.canvas.set_height(h);
        get_2d_context(&self.canvas)?
            .draw_image_with_html_canvas_element(&self.backing, 0.0, 0.0)
            .map_err(|e| e)
    }

    /// Renders image `index` in zoom/pan mode.
    /// `scale`  — zoom factor (1.0 = 1:1 pixels, fit_scale = fully zoomed out)
    /// `pan_x/y` — top-left corner of the viewport window in image-space pixels
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

        // Source rect: the portion of the image visible at this pan/zoom.
        let src_w = f64::min(img_w as f64 - pan_x, wf / scale);
        let src_h = f64::min(img_h as f64 - pan_y, hf / scale);
        let dst_w = src_w * scale;
        let dst_h = src_h * scale;
        let dst_x = (wf - dst_w) / 2.0;
        let dst_y = (hf - dst_h) / 2.0;

        // Size and clear backing canvas.
        self.backing.set_width(w);
        self.backing.set_height(h);
        let back_ctx = get_2d_context(&self.backing)?;
        back_ctx.set_fill_style_str("#777777");
        back_ctx.fill_rect(0.0, 0.0, wf, hf);

        // Write full RGBA into decode canvas at native resolution.
        self.decode.set_width(img_w);
        self.decode.set_height(img_h);
        let decode_ctx = get_2d_context(&self.decode)?;
        let img_data = {
            let pixels = &self.pixel_cache[&index].2;
            ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels.as_slice()), img_w, img_h)?
        };
        decode_ctx.put_image_data(&img_data, 0.0, 0.0)?;

        // Crop + scale in one 9-arg drawImage call.
        back_ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            &self.decode,
            pan_x, pan_y, src_w, src_h,
            dst_x, dst_y, dst_w, dst_h,
        ).map_err(|e| e)?;

        // Blit backing → photo canvas.
        self.canvas.set_width(w);
        self.canvas.set_height(h);
        get_2d_context(&self.canvas)?
            .draw_image_with_html_canvas_element(&self.backing, 0.0, 0.0)
            .map_err(|e| e)
    }

    /// Draws the `<` / `>` hover arrow directly onto `self.canvas` (on top of the blitted image).
    /// `zone`    — "left", "right", or "" (no-op).
    /// `opacity` — current animation opacity (0.0–1.0); no-op if ≤ 0.
    /// `index`   — current image index; used to show `>>` / `<<` at gallery boundaries.
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

    // Fit-scales image `index` into the column starting at `col_x` with dimensions `col_w × col_h`.
    // If the image has not yet been decoded, draws a grey placeholder and returns.
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

        let iw = img_w as f64;
        let ih = img_h as f64;
        let scale = f64::min(col_w / iw, col_h / ih);
        let dst_w = iw * scale;
        let dst_h = ih * scale;
        let h_pad = (col_w - dst_w) / 2.0;
        let v_pad = (col_h - dst_h) / 2.0;

        // Write RGBA into the hidden decode canvas at native resolution.
        self.decode.set_width(img_w);
        self.decode.set_height(img_h);
        let decode_ctx = get_2d_context(&self.decode)?;

        // Block scope ends the pixel_cache borrow before we borrow self.decode below.
        let img_data = {
            let pixels = &self.pixel_cache[&index].2;
            ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels.as_slice()), img_w, img_h)?
        };
        decode_ctx.put_image_data(&img_data, 0.0, 0.0)?;

        // Scale-blit decode canvas → backing canvas column.
        ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
            &self.decode,
            col_x + h_pad,
            v_pad,
            dst_w,
            dst_h,
        )
        .map_err(|e| e)
    }

}
