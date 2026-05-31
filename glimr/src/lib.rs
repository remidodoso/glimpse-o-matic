use wasm_bindgen::prelude::*;
use std::io::{Cursor, Read};
use zip::ZipArchive;

const XOR_KEY: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

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

#[wasm_bindgen]
pub struct GlimrZip {
    entries: Vec<(String, Vec<u8>)>,
}

#[wasm_bindgen]
impl GlimrZip {
    #[wasm_bindgen(constructor)]
    pub fn new(zip_bytes: &[u8]) -> Result<GlimrZip, JsValue> {
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

        Ok(GlimrZip { entries })
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn entry_name(&self, i: usize) -> String {
        self.entries[i].0.clone()
    }

    pub fn entry_data(&self, i: usize) -> Vec<u8> {
        self.entries[i].1.clone()
    }
}
