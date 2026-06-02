use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;
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
        })
    }

    pub fn load_zip(&mut self, zip_bytes: &[u8]) -> Result<(), JsValue> {
        glog("load_zip", &format!("start ({} bytes)", zip_bytes.len()));
        let cursor = Cursor::new(zip_bytes);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if file.is_dir() { continue; }
            let name = file.name().to_string();
            if !is_image_ext(&name) { continue; }
            let mut raw = Vec::new();
            file.read_to_end(&mut raw)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let decoded = if name.to_lowercase().ends_with(".dat") {
                xor_bytes(&raw)
            } else {
                raw
            };
            entries.push((name, decoded));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        self.pixel_cache.clear();
        self.names       = entries.iter().map(|(n, _)| n.clone()).collect();
        self.image_bytes = entries.into_iter().map(|(_, b)| b).collect();

        glog("load_zip", &format!("done — {} images loaded", self.names.len()));
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

        self.ensure_decoded(index)?;

        let (img_w, img_h) = {
            let e = self.pixel_cache.get(&index).unwrap();
            (e.0, e.1)
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

    /// Draws a thumbnail for image `index` into a caller-supplied canvas element.
    /// Sets canvas width/height to match the scaled dimensions, then blits.
    /// `carousel_size` — target size in CSS px on the constrained axis.
    /// `fit_to_width`  — true in landscape (vertical strip); false in portrait (horizontal strip).
    pub fn draw_thumbnail(
        &mut self,
        canvas: HtmlCanvasElement,
        index: usize,
        carousel_size: f64,
        fit_to_width: bool,
    ) -> Result<(), JsValue> {
        self.ensure_decoded(index)?;

        let (img_w, img_h) = {
            let e = self.pixel_cache.get(&index).unwrap();
            (e.0, e.1)
        };

        let iw = img_w as f64;
        let ih = img_h as f64;
        let scale = if fit_to_width { carousel_size / iw } else { carousel_size / ih };
        let thumb_w = (iw * scale).round() as u32;
        let thumb_h = (ih * scale).round() as u32;

        canvas.set_width(thumb_w);
        canvas.set_height(thumb_h);
        let ctx = get_2d_context(&canvas)?;

        glog("draw_thumbnail", &format!("resize start image {} {}×{} → {}×{}", index, img_w, img_h, thumb_w, thumb_h));
        let thumb_pixels = {
            let pixels = &self.pixel_cache[&index].2;
            let src = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(img_w, img_h, pixels.as_slice())
                .ok_or_else(|| JsValue::from_str("thumbnail: bad pixel cache"))?;
            image::imageops::resize(&src, thumb_w, thumb_h, image::imageops::FilterType::Triangle)
        };
        glog("draw_thumbnail", &format!("resize done  image {}", index));

        let img_data = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(thumb_pixels.as_raw().as_slice()), thumb_w, thumb_h
        )?;
        ctx.put_image_data(&img_data, 0.0, 0.0)?;
        glog("draw_thumbnail", &format!("put_image_data done image {}", index));
        Ok(())
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
    fn draw_image_in_column(
        &mut self,
        ctx: &CanvasRenderingContext2d,
        index: usize,
        col_x: f64,
        col_w: f64,
        col_h: f64,
    ) -> Result<(), JsValue> {
        self.ensure_decoded(index)?;

        // Extract dimensions without holding a borrow into pixel_cache across the canvas calls.
        let (img_w, img_h) = {
            let e = self.pixel_cache.get(&index).unwrap();
            (e.0, e.1)
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

    fn ensure_decoded(&mut self, index: usize) -> Result<(), JsValue> {
        if self.pixel_cache.contains_key(&index) {
            return Ok(());
        }
        glog("ensure_decoded", &format!("start image {} ({} bytes)", index, self.image_bytes[index].len()));
        let img = image::load_from_memory(&self.image_bytes[index])
            .map_err(|e| JsValue::from_str(&format!("decode error: {e}")))?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        glog("ensure_decoded", &format!("done  image {} → {}×{}", index, w, h));
        self.pixel_cache.insert(index, (w, h, rgba.into_raw()));
        Ok(())
    }
}
