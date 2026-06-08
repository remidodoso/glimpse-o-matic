use glimr::watermark;
use std::process::Command;

fn write_y_delta_rgb(pixels: &mut [u8], y_orig: &[f32], y_new: &[f32]) {
    for (chunk, (&yo, &yn)) in pixels.chunks_mut(3).zip(y_orig.iter().zip(y_new.iter())) {
        let d = yn - yo;
        chunk[0] = (chunk[0] as f32 + d).clamp(0.0, 255.0) as u8;
        chunk[1] = (chunk[1] as f32 + d).clamp(0.0, 255.0) as u8;
        chunk[2] = (chunk[2] as f32 + d).clamp(0.0, 255.0) as u8;
    }
}

fn test_image_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests")
        .join("test_a.jpg")
}

#[test]
fn decode_no_crash_on_unwatermarked_image() {
    let img = image::open(test_image_path()).unwrap().into_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let y = watermark::extract_y_rgb(img.as_raw());
    let _payload = watermark::decode_y(&y, w, h);
    // Payload will be noise; just verify no panic and correct length.
}

#[test]
fn roundtrip_known_payload_direct() {
    // Embed a known payload in the Y channel, decode directly (no JPEG), verify match.
    let img = image::open(test_image_path()).unwrap().into_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let orig_y = watermark::extract_y_rgb(img.as_raw());

    let payload: [u8; 16] = [
        0x01, 0x00, 0x00, 0x00,   // unix ts = 1
        0x00, 0x00, 0x00, 0x00,   // ipv4 = 0
        0xef, 0xbe, 0xad, 0xde,   // browser fp = 0xdeadbeef
        0x37, 0x13,               // referrer hash
        0x01,                     // flags: referrer present
        0x01,                     // version = 1
    ];

    let mut y = orig_y.clone();
    watermark::embed_y(&mut y, w, h, &payload);
    let recovered = watermark::decode_y(&y, w, h);
    assert_eq!(recovered, payload, "direct roundtrip: payload mismatch");
}

#[test]
fn roundtrip_known_payload_via_jpeg_q80() {
    use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};

    let img = image::open(test_image_path()).unwrap().into_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels = img.into_raw();
    let orig_y = watermark::extract_y_rgb(&pixels);

    let payload: [u8; 16] = [
        0x78, 0x56, 0x34, 0x12,   // unix ts = 0x12345678
        0x00, 0x00, 0x00, 0x00,   // ipv4 = 0
        0xef, 0xbe, 0xad, 0xde,   // browser fp = 0xdeadbeef
        0xcd, 0xab,               // referrer hash
        0x01,                     // flags: referrer present
        0x01,                     // version = 1
    ];

    let mut y = orig_y.clone();
    watermark::embed_y(&mut y, w, h, &payload);

    // Write delta back (RGB stride, not RGBA) and encode as JPEG q80.
    let mut pixels_wm = pixels.clone();
    write_y_delta_rgb(&mut pixels_wm, &orig_y, &y);
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 80)
        .write_image(&pixels_wm, w as u32, h as u32, ExtendedColorType::Rgb8)
        .unwrap();

    // Reload and decode at native resolution (matched decoder).
    let decoded = image::load_from_memory(&jpeg).unwrap().into_rgb8();
    let dec_y = watermark::extract_y_rgb(decoded.as_raw());
    let recovered = watermark::decode_y(&dec_y, w, h);
    assert_eq!(recovered, payload, "JPEG q80 roundtrip: payload mismatch");
}

#[test]
fn cli_binary_decodes_watermarked_jpeg() {
    use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};

    // Build a watermarked JPEG in memory.
    let img = image::open(test_image_path()).unwrap().into_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels = img.into_raw();
    let orig_y = watermark::extract_y_rgb(&pixels);

    let payload: [u8; 16] = [
        0x78, 0x56, 0x34, 0x12,  // ts = 0x12345678 → 1979-03-15 ...
        0x00, 0x00, 0x00, 0x00,
        0xef, 0xbe, 0xad, 0xde,  // browser fp = 0xdeadbeef
        0x00, 0x00, 0x00, 0x01,  // version = 1
    ];

    let mut y = orig_y.clone();
    watermark::embed_y(&mut y, w, h, &payload);
    let mut pixels_wm = pixels.clone();
    write_y_delta_rgb(&mut pixels_wm, &orig_y, &y);

    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 80)
        .write_image(&pixels_wm, w as u32, h as u32, ExtendedColorType::Rgb8)
        .unwrap();

    // Write to a temp file.
    let tmp = std::env::temp_dir().join("wm_decode_test.jpg");
    std::fs::write(&tmp, &jpeg).unwrap();

    // Run the release binary against it.
    let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("target").join("release").join("watermark-decode")
        .with_extension(std::env::consts::EXE_EXTENSION);

    if !bin.exists() {
        eprintln!("release binary not built yet — skipping CLI test");
        return;
    }

    let out = Command::new(&bin).arg(&tmp).output().expect("failed to run binary");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "binary exited non-zero:\n{}", stdout);
    assert!(stdout.contains("deadbeef"), "browser fp not found in output:\n{}", stdout);
    assert!(stdout.contains("version   : 1"), "version field wrong:\n{}", stdout);

    std::fs::remove_file(&tmp).ok();
}

#[test]
fn cli_decodes_resized_suspect_with_size_flag() {
    use image::{codecs::jpeg::JpegEncoder, imageops, ExtendedColorType, ImageEncoder};

    // Embed at native, then simulate a non-power-of-2 downscale (70%) — the kind
    // of resize that defeats blind decoding — and recover it via --size.
    let img = image::open(test_image_path()).unwrap().into_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels = img.into_raw();
    let orig_y = watermark::extract_y_rgb(&pixels);

    let payload: [u8; 16] = [
        0x78, 0x56, 0x34, 0x12,
        0x00, 0x00, 0x00, 0x00,
        0xef, 0xbe, 0xad, 0xde,  // browser fp = 0xdeadbeef
        0x00, 0x00, 0x00, 0x01,  // version = 1
    ];

    let mut y = orig_y.clone();
    watermark::embed_y(&mut y, w, h, &payload);
    let mut pixels_wm = pixels.clone();
    write_y_delta_rgb(&mut pixels_wm, &orig_y, &y);

    let wm_img = image::RgbImage::from_raw(w as u32, h as u32, pixels_wm).unwrap();
    let (nw, nh) = ((w * 7 / 10) as u32, (h * 7 / 10) as u32);
    let scaled = imageops::resize(&wm_img, nw, nh, imageops::FilterType::Lanczos3);

    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 85)
        .write_image(scaled.as_raw(), nw, nh, ExtendedColorType::Rgb8)
        .unwrap();
    let tmp = std::env::temp_dir().join("wm_decode_resized_test.jpg");
    std::fs::write(&tmp, &jpeg).unwrap();

    let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("target").join("release").join("watermark-decode")
        .with_extension(std::env::consts::EXE_EXTENSION);
    if !bin.exists() {
        eprintln!("release binary not built yet — skipping CLI test");
        return;
    }

    let size_arg = format!("{}x{}", w, h);
    let out = Command::new(&bin)
        .arg("--size").arg(&size_arg).arg(&tmp)
        .output().expect("failed to run binary");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "binary exited non-zero:\n{}", stdout);
    assert!(stdout.contains("deadbeef"), "fp not recovered from resized suspect:\n{}", stdout);
    assert!(stdout.contains("version   : 1"), "version field wrong:\n{}", stdout);
    assert!(!stdout.contains("not detected"), "verdict says not detected:\n{}", stdout);

    std::fs::remove_file(&tmp).ok();
}

#[test]
fn cli_auto_default_decodes() {
    use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};

    // Default (no flags) = fully-blind --auto.  On a native-resolution watermarked
    // JPEG it takes the fast path (one matched decode) and CRC-verifies.  (Blind
    // recovery on cropped/rescaled inputs is measured by the lib `blind_auto_sweep`;
    // end-to-end rescale via the binary is covered by the --size test above.)
    let img = image::open(test_image_path()).unwrap().into_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels = img.into_raw();
    let orig_y = watermark::extract_y_rgb(&pixels);

    let payload: [u8; 16] = [
        0x78, 0x56, 0x34, 0x12,
        0x00, 0x00, 0x00, 0x00,
        0xef, 0xbe, 0xad, 0xde,  // browser fp = 0xdeadbeef
        0x00, 0x00, 0x00, 0x01,  // version = 1
    ];

    let mut y = orig_y.clone();
    watermark::embed_y(&mut y, w, h, &payload);
    let mut pixels_wm = pixels.clone();
    write_y_delta_rgb(&mut pixels_wm, &orig_y, &y);

    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90)
        .write_image(&pixels_wm, w as u32, h as u32, ExtendedColorType::Rgb8)
        .unwrap();
    let tmp = std::env::temp_dir().join("wm_decode_auto_test.jpg");
    std::fs::write(&tmp, &jpeg).unwrap();

    let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("target").join("release").join("watermark-decode")
        .with_extension(std::env::consts::EXE_EXTENSION);
    if !bin.exists() {
        eprintln!("release binary not built yet — skipping CLI test");
        return;
    }

    let out = Command::new(&bin).arg(&tmp).output().expect("failed to run binary");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "binary exited non-zero:\n{}", stdout);
    assert!(stdout.contains("deadbeef"), "fp not recovered by --auto:\n{}", stdout);
    assert!(stdout.contains("version   : 1"), "payload version wrong:\n{}", stdout);
    assert!(stdout.contains("verified (CRC ok)"), "default decode not CRC-verified:\n{}", stdout);

    std::fs::remove_file(&tmp).ok();
}
