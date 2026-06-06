use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;
use glimr::watermark;
use rayon::prelude::*;

const SCAN_DEFAULT: (usize, usize) = (1000, 4000); // default long-dimension sweep
const SCAN_TOP_N:   usize = 5;                      // candidates reported per image

/// How the original (embed) dimensions are determined for a suspect.
enum Mode {
    Blind,                       // assume the image is already at native resolution
    Fixed(usize, usize),         // exact original dimensions (--size / --ref)
    Scan(usize, usize),          // brute-force long-dimension sweep [min, max]
}

/// Outcome of a `--scan` sweep, including the noise reference the search collected.
struct ScanReport {
    top: Vec<(f32, usize, usize, [u8; 16])>, // best candidates, score-descending
    noise_median: f32,                       // median score of all candidates (noise floor)
    noise_sigma:  f32,                        // MAD·1.4826 (robust σ estimate)
    processed:    usize,                      // candidates actually evaluated
    total:        usize,                      // candidates planned
    interrupted:  bool,
}

fn usage() -> ! {
    eprintln!("Usage: watermark-decode [MODE] <image> [image ...]");
    eprintln!();
    eprintln!("Modes (pick one; default is blind):");
    eprintln!("  --size WxH         original (embed) dimensions, e.g. --size 2500x2500");
    eprintln!("  --ref <original>   read original dimensions from a reference image file");
    eprintln!("  --scan [MIN:MAX]   brute-force every long-dimension size in MIN..=MAX");
    eprintln!("                     (default {}:{}), resampling the suspect back to each",
        SCAN_DEFAULT.0, SCAN_DEFAULT.1);
    eprintln!("                     candidate grid and keeping the best-scoring fit");
    eprintln!("  --threads N        worker threads for --scan (default 4)");
    eprintln!();
    eprintln!("A rescaled suspect only decodes when resampled back to the exact dimensions");
    eprintln!("it was embedded at.  Give them via --size/--ref, or let --scan find them when");
    eprintln!("they're unknown (assumes an aspect-preserving rescale).  With no mode, a blind");
    eprintln!("decode assumes the image is already at its original resolution.");
    eprintln!("During --scan, Ctrl-C stops the sweep and reports the best candidates so far.");
    eprintln!("Formats: JPEG, PNG.");
    std::process::exit(1);
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() { usage(); }

    let mut mode = Mode::Blind;
    let mut threads = 4usize;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
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
            "--scan" => {
                // Optional MIN:MAX argument; if the next token isn't a range, use default.
                match raw.get(i + 1).and_then(|v| parse_range(v)) {
                    Some((lo, hi)) => { mode = Mode::Scan(lo, hi); i += 2; }
                    None           => { mode = Mode::Scan(SCAN_DEFAULT.0, SCAN_DEFAULT.1); i += 1; }
                }
            }
            "--threads" => {
                let v = raw.get(i + 1).unwrap_or_else(|| usage());
                threads = v.parse().ok().filter(|&n| n >= 1).unwrap_or_else(|| {
                    eprintln!("error: --threads expects a positive integer (got '{}')", v);
                    std::process::exit(1);
                });
                i += 2;
            }
            "-h" | "--help" => usage(),
            _ => { files.push(raw[i].clone()); i += 1; }
        }
    }
    if files.is_empty() { usage(); }

    // Ctrl-C sets a flag the scan workers poll; printing happens in normal code,
    // never in the handler (which must stay async-signal-safe).
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let flag = interrupted.clone();
        let _ = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst));
    }

    let mut any_error = false;
    for f in &files {
        if !decode_file(Path::new(f), &mode, threads, &interrupted) { any_error = true; }
        if interrupted.load(Ordering::SeqCst) {
            eprintln!("interrupted.");
            std::process::exit(130);
        }
    }
    if any_error { std::process::exit(1); }
}

fn parse_size(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(['x', 'X', '*'])?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(':')?;
    let lo: usize = a.trim().parse().ok()?;
    let hi: usize = b.trim().parse().ok()?;
    if lo >= 2 && hi >= lo { Some((lo, hi)) } else { None }
}

fn decode_file(path: &Path, mode: &Mode, threads: usize, interrupted: &Arc<AtomicBool>) -> bool {
    let img = match image::open(path) {
        Ok(img) => img.into_rgb8(),
        Err(e)  => { eprintln!("error: {}: {}", path.display(), e); return false; }
    };
    let (w, h) = (img.width() as usize, img.height() as usize);

    // Report the source the moment it's read, before any (possibly long) decode.
    print_header(path, w, h);

    let y = watermark::extract_y_rgb(img.as_raw());

    match mode {
        Mode::Blind => {
            let (payload, score) = watermark::decode_y_at_size_verbose(&y, w, h, w, h);
            print_detection_fixed(score, &payload, true);
            print_fields(&payload);
        }
        Mode::Fixed(ow, oh) => {
            let (payload, score) = watermark::decode_y_at_size_verbose(&y, w, h, *ow, *oh);
            if w != *ow || h != *oh {
                println!("  regridded : → {}×{} (embed grid)", ow, oh);
            }
            print_detection_fixed(score, &payload, false);
            print_fields(&payload);
        }
        Mode::Scan(min, max) => {
            let report = scan_search(&y, w, h, *min, *max, threads, interrupted);
            print_scan_report(&report, w, h);
        }
    }
    true
}

/// Brute-force size recovery: sweep every long-dimension size in `min..=max`,
/// derive the short dimension from the suspect's aspect ratio (±1 for rounding),
/// resample the suspect back to each candidate grid and score it.
///
/// The alignment peak is razor-sharp (a few px wide), so we step by 1 px — that
/// guarantees we test the exact original integer size.  Candidates are
/// independent → bounded `rayon` pool of `threads` workers.  The candidate scores
/// double as a noise reference (median/MAD) for the confidence estimate, and a
/// running best is surfaced live.  Ctrl-C (`interrupted`) makes workers bail so
/// whatever was found so far can still be reported.
fn scan_search(
    y: &[f32], w: usize, h: usize, min: usize, max: usize, threads: usize,
    interrupted: &Arc<AtomicBool>,
) -> ScanReport {
    let landscape = w >= h;
    let (long, short) = if landscape { (w, h) } else { (h, w) };

    // One candidate per long size, with short derived from aspect (±1 rounding slop).
    let mut cands: Vec<(usize, usize)> = Vec::new();
    for l in min..=max {
        let base = ((l as u64 * short as u64 + long as u64 / 2) / long as u64) as i64;
        for d in [0i64, -1, 1] {
            let s = base + d;
            if s < 16 { continue; }
            let (cw, ch) = if landscape { (l, s as usize) } else { (s as usize, l) };
            cands.push((cw, ch));
        }
    }

    let total = cands.len();
    eprintln!("  scanning {} candidate sizes ({}..{} long dim, {} threads)…",
        total, min, max, threads);
    let done = AtomicUsize::new(0);
    let best_bits = AtomicU32::new(0f32.to_bits());
    let print_lock = Mutex::new(());
    let tick = (total / 50).max(1);
    let floor = watermark::detection_floor(); // a candidate above this is worth surfacing live

    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()
        .expect("failed to build thread pool");

    let opt: Vec<Option<(f32, usize, usize, [u8; 16])>> = pool.install(|| {
        cands.par_iter().map(|&(cw, ch)| {
            if interrupted.load(Ordering::Relaxed) { return None; }
            let (payload, score) = watermark::decode_y_at_size_verbose(y, w, h, cw, ch);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;

            // New running best? (lock-free gate; only improvements touch the lock)
            let mut cur = best_bits.load(Ordering::Relaxed);
            while score > f32::from_bits(cur) {
                match best_bits.compare_exchange(
                    cur, score.to_bits(), Ordering::Relaxed, Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        if score > floor {
                            let _g = print_lock.lock().unwrap();
                            eprintln!("\r  ★ {}×{}  score {:.0}                    ", cw, ch, score);
                        }
                        break;
                    }
                    Err(observed) => cur = observed,
                }
            }

            if n % tick == 0 {
                let _g = print_lock.lock().unwrap();
                eprint!("\r  …{:>3}%  best so far {:.0}    ",
                    n * 100 / total, f32::from_bits(best_bits.load(Ordering::Relaxed)));
            }
            Some((score, cw, ch, payload))
        }).collect()
    });
    eprintln!("\r  scan complete.                          ");

    let mut results: Vec<(f32, usize, usize, [u8; 16])> = opt.into_iter().flatten().collect();
    let processed = results.len();
    let scores: Vec<f32> = results.iter().map(|r| r.0).collect();
    let (noise_median, noise_sigma) = noise_stats(&scores);

    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(SCAN_TOP_N);

    ScanReport {
        top: results,
        noise_median,
        noise_sigma,
        processed,
        total,
        interrupted: interrupted.load(Ordering::Relaxed),
    }
}

/// Robust noise floor of the candidate-score distribution: median and a σ
/// estimate from the MAD (median absolute deviation).  Robust to the handful of
/// real-signal points clustered near the true size.
fn noise_stats(scores: &[f32]) -> (f32, f32) {
    if scores.is_empty() { return (0.0, 1.0); }
    let mut v = scores.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = v[v.len() / 2];
    let mut dev: Vec<f32> = v.iter().map(|x| (x - median).abs()).collect();
    dev.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = dev[dev.len() / 2];
    (median, (mad * 1.4826).max(1e-3))
}

/// Statistical confidence band for a peak `sigma_above` σ over the noise floor.
fn band_scan(sigma_above: f32) -> &'static str {
    if !sigma_above.is_finite() || sigma_above < 3.0 { "not detected" }
    else if sigma_above < 6.0  { "weak" }
    else if sigma_above < 12.0 { "likely" }
    else { "almost certain" }
}

/// `true` if the payload's structural fields are self-consistent — a weak
/// built-in checksum (version must be 1).  Corroborates a statistical detection.
/// (A real CRC/ECC in the payload, deferred, would make this rigorous.)
fn version_note(p: &[u8; 16]) -> &'static str {
    if p[15] == 1 { "version field valid" } else { "version field INVALID" }
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

/// Detection line for the size-known modes (Fixed / Blind): no noise distribution,
/// so band by the ALPHA-derived absolute thresholds, corroborated by the version field.
fn print_detection_fixed(score: f32, p: &[u8; 16], blind: bool) {
    let band = if score >= watermark::detection_strong() { "detected" }
               else if score >= watermark::detection_floor() { "weak — wrong size?" }
               else { "not detected" };
    let mut detail = format!("score {:.0}; {}", score, version_note(p));
    if blind { detail.push_str("; assumed native resolution"); }
    println!("  detection : {}  ({})", band, detail);
}

fn print_scan_report(report: &ScanReport, w: usize, h: usize) {
    if report.top.is_empty() {
        println!("  detection : not detected (no candidates evaluated)");
        if report.interrupted {
            println!("  ** interrupted before any candidate completed **");
        }
        return;
    }

    let (best, cw, ch, payload) = report.top[0];
    let sigma_above = (best - report.noise_median) / report.noise_sigma;

    println!("  best fit  : {}×{}  (resampled from {}×{} suspect)", cw, ch, w, h);
    println!("  detection : {}  (score {:.0}, {:.0}σ above noise median {:.0}; {})",
        band_scan(sigma_above), best, sigma_above, report.noise_median, version_note(&payload));
    print_fields(&payload);

    if report.top.len() > 1 {
        println!("  candidates:");
        for &(s, cw, ch, _) in &report.top[1..] {
            let sa = (s - report.noise_median) / report.noise_sigma;
            println!("     {}×{}  score {:.0} ({:.0}σ)", cw, ch, s, sa);
        }
    }
    if report.interrupted {
        println!("  ** interrupted: {}/{} candidates scanned — best so far shown **",
            report.processed, report.total);
    }
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
