use std::path::Path;
use std::io::{IsTerminal, Write};
use glimr::watermark;

/// How the original (embed) dimensions are determined for a suspect.
enum Mode {
    Auto,                // default: blindly recover scale + crop offset, then decode
    Fixed(usize, usize), // exact original dimensions (--size / --ref)
}

fn usage() -> ! {
    eprintln!("Usage: watermark-decode [--size WxH | --ref <original>] <image> [image ...]");
    eprintln!();
    eprintln!("Recovers the watermark payload from an image.  By default it works fully");
    eprintln!("blind — automatically recovering scale and crop offset — so a cropped and/or");
    eprintln!("rescaled suspect decodes with no extra information.  A CRC verdict says whether");
    eprintln!("the recovered payload is genuine.");
    eprintln!();
    eprintln!("Options (mutually exclusive; the fast path when the source size is known):");
    eprintln!("  --size WxH        original (embed) dimensions, e.g. --size 2500x2500");
    eprintln!("  --ref <original>  read original dimensions from a reference image file");
    eprintln!("  -v, --verbose     narrate the blind search (instead of the live progress bar)");
    eprintln!();
    eprintln!("Formats: JPEG, PNG.  A live one-line progress bar shows during blind recovery");
    eprintln!("on an interactive terminal (suppressed when stderr is redirected).");
    std::process::exit(1);
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() { usage(); }

    let mut mode = Mode::Auto;
    let mut verbose = false;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "-v" | "--verbose" => { verbose = true; i += 1; }
            "--size" => {
                let v = raw.get(i + 1).unwrap_or_else(|| usage());
                let (w, h) = parse_size(v).unwrap_or_else(|| {
                    eprintln!("error: --size expects WxH, e.g. 2500x2500 (got '{}')", v);
                    std::process::exit(1);
                });
                mode = Mode::Fixed(w, h);
                i += 2;
            }
            "--ref" => {
                let v = raw.get(i + 1).unwrap_or_else(|| usage());
                let r = image::open(Path::new(v)).unwrap_or_else(|e| {
                    eprintln!("error: cannot read --ref {}: {}", v, e);
                    std::process::exit(1);
                });
                mode = Mode::Fixed(r.width() as usize, r.height() as usize);
                i += 2;
            }
            "--auto" => { i += 1; } // accepted no-op: blind auto is the default
            "-h" | "--help" => usage(),
            _ => { files.push(raw[i].clone()); i += 1; }
        }
    }
    if files.is_empty() { usage(); }

    let mut any_error = false;
    for f in &files {
        if !decode_file(Path::new(f), &mode, verbose) { any_error = true; }
    }
    if any_error { std::process::exit(1); }
}

fn parse_size(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(['x', 'X', '*'])?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn decode_file(path: &Path, mode: &Mode, verbose: bool) -> bool {
    let img = match image::open(path) {
        Ok(img) => img.into_rgb8(),
        Err(e)  => { eprintln!("error: {}: {}", path.display(), e); return false; }
    };
    let (w, h) = (img.width() as usize, img.height() as usize);
    print_header(path, w, h);
    let y = watermark::extract_y_rgb(img.as_raw());

    match mode {
        Mode::Fixed(ow, oh) => {
            let (dec, score) = watermark::decode_y_at_size_verbose(&y, w, h, *ow, *oh);
            if w != *ow || h != *oh {
                println!("  regridded : → {}×{} (embed grid)", ow, oh);
            }
            print_detection_fixed(score, &dec);
            print_fields(&dec.data);
        }
        Mode::Auto => {
            // Fast path: an unmodified / native-resolution image decodes with a single
            // matched pass — if its CRC verifies, we're done instantly.
            let (dec, score) = watermark::decode_y_at_size_verbose(&y, w, h, w, h);
            if dec.verified {
                if verbose { eprintln!("  · native matched decode → CRC verified"); }
                println!("  detection : verified (CRC ok)  (native, score {:.0})", score);
                print_fields(&dec.data);
            } else {
                if verbose {
                    eprintln!("  · native decode unverified (score {:.0}) → recovering blind…", score);
                }
                let r = blind_with_progress(&y, w, h, verbose);
                let verdict = if r.verified { "verified (CRC ok)" }
                              else if r.confidence >= 3.0 { "likely — CRC failed" }
                              else { "not detected" };
                println!("  recovered : scale {:.3}, tile offset ({},{})", r.scale, r.offset.0, r.offset.1);
                println!("  detection : {}  (confidence {:.1})", verdict, r.confidence);
                print_fields(&r.data);
            }
        }
    }
    true
}

/// Run the blind decode, rendering progress: a live one-line bar on an interactive
/// terminal (stderr), or `--verbose` prose, or silent (redirected / non-TTY).
fn blind_with_progress(y: &[f32], w: usize, h: usize, verbose: bool) -> watermark::registration::BlindResult {
    use watermark::registration::Progress::*;
    let bar = !verbose && std::io::stderr().is_terminal();
    let mut best_prom = 0.0f32;
    let mut cb = |ev: watermark::registration::Progress| {
        match ev {
            Phase(p) => {
                if verbose { eprintln!("  · {}…", p); }
                else if bar { eprint!("\r  {:<38}", format!("{}…", p)); let _ = std::io::stderr().flush(); }
            }
            Scale(s) => {
                if verbose { eprintln!("  · coarse scale ≈ {:.3}", s); }
            }
            Refine { step, total, scale, prominence, verified } => {
                if prominence > best_prom { best_prom = prominence; }
                if verbose {
                    eprintln!("  · refine {}/{}: scale {:.3}, prominence {:.1}{}",
                        step, total, scale, prominence, if verified { ", CRC ✓" } else { "" });
                } else if bar {
                    eprint!("\r  recovering… refine {}/{}  best {:.1}{:<8}",
                        step, total, best_prom, "");
                    let _ = std::io::stderr().flush();
                }
            }
        }
    };
    let r = watermark::registration::decode_blind_auto_cb(y, w, h, &mut cb);
    if bar { eprint!("\r{:<60}\r", ""); let _ = std::io::stderr().flush(); } // erase the bar
    r
}

fn print_header(path: &Path, w: usize, h: usize) {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    println!("{}", path.display());
    println!("  source    : {}×{}  ({:.2} MP, {})",
        w, h, (w * h) as f64 / 1_000_000.0, fmt_bytes(bytes));
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1_048_576 { format!("{:.1} MB", n as f64 / 1_048_576.0) }
    else if n >= 1024 { format!("{:.0} KB", n as f64 / 1024.0) }
    else { format!("{} B", n) }
}

/// Detection line for the size-known modes (--size / --ref).  CRC is the definitive
/// verdict; the alignment score is the fallback signal when the CRC fails.
fn print_detection_fixed(score: f32, dec: &watermark::Decoded) {
    let band = if dec.verified { "verified (CRC ok)" }
               else if score >= watermark::detection_strong() { "strong but CRC failed" }
               else if score >= watermark::detection_floor() { "weak — wrong size?" }
               else { "not detected" };
    println!("  detection : {}  (score {:.0})", band, score);
}

/// Print the decoded payload fields (no header / detection line).
fn print_fields(p: &[u8; 16]) {
    let ts       = u32::from_le_bytes(p[0..4].try_into().unwrap());
    let fp       = u32::from_le_bytes(p[8..12].try_into().unwrap());
    let ref_hash = u16::from_le_bytes(p[12..14].try_into().unwrap());
    let flags    = p[14];
    let version  = p[15];

    let ip_str = if p[4] == 0 && p[5] == 0 && p[6] == 0 && p[7] == 0 {
        "n/a".to_string()
    } else {
        format!("{}.{}.{}.{}", p[4], p[5], p[6], p[7])
    };

    let ref_str = if flags & 1 != 0 {
        format!("{:04x}  (present)", ref_hash)
    } else {
        "—".to_string()
    };

    let ts_str = if ts == 0 {
        "n/a".to_string()
    } else {
        let (yr, mo, dy, hh, mm, ss) = unix_to_datetime(ts);
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC  (unix {})", yr, mo, dy, hh, mm, ss, ts)
    };

    let hex: String = p.chunks(4)
        .map(|c| c.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("  ");

    println!("  version   : {}", version);
    println!("  timestamp : {}", ts_str);
    println!("  ipv4      : {}", ip_str);
    println!("  browser   : {:08x}", fp);
    println!("  referrer  : {}", ref_str);
    println!("  payload   : {}", hex);
}

// Gregorian calendar (Howard Hinnant's algorithm).
fn unix_to_datetime(ts: u32) -> (i32, u32, u32, u32, u32, u32) {
    let h  = (ts / 3600) % 24;
    let mi = (ts / 60)   % 60;
    let s  =  ts         % 60;

    let z   = (ts / 86400) as i64 + 719468;
    let era = if z >= 0 { z / 146097 } else { (z - 146096) / 146097 };
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp  = (5 * doy + 2) / 153;
    let d   = doy - (153 * mp + 2) / 5 + 1;
    let mo  = if mp < 10 { mp + 3 } else { mp - 9 };
    let y   = yoe as i64 + era * 400 + if mo <= 2 { 1 } else { 0 };

    (y as i32, mo, d, h, mi, s)
}
