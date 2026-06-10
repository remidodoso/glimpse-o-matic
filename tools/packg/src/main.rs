use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use rand::Rng;

const XOR_KEY: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

struct Args {
    input_dir: String,
    output:    Option<String>,
    force:     bool,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut input_dir: Option<String> = None;
    let mut output:    Option<String> = None;
    let mut force = false;

    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= raw.len() {
                    return Err(format!("'{}' requires a value", raw[i - 1]));
                }
                output = Some(raw[i].clone());
            }
            "-f" | "--force" => {
                force = true;
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown flag: {}", arg));
            }
            _ => {
                if input_dir.is_none() {
                    input_dir = Some(raw[i].clone());
                } else {
                    return Err(format!("unexpected argument: {}", raw[i]));
                }
            }
        }
        i += 1;
    }

    let input_dir = input_dir.ok_or("missing required argument: <input-dir>")?;
    Ok(Args { input_dir, output, force })
}

fn generate_unique_names(count: usize) -> Vec<String> {
    let mut rng  = rand::thread_rng();
    let mut seen = HashSet::new();
    let mut names: Vec<String> = Vec::with_capacity(count);

    while names.len() < count {
        let first = (b'a' + rng.gen_range(0u8..26)) as char;
        let rest: String = (0..7).map(|_| {
            let n = rng.gen_range(0u8..36);
            if n < 10 { (b'0' + n) as char } else { (b'a' + n - 10) as char }
        }).collect();
        let name = format!("{}{}", first, rest);
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }

    names.sort();
    names
}

fn xor_encode(data: &[u8]) -> Vec<u8> {
    data.iter().enumerate().map(|(i, &b)| b ^ XOR_KEY[i % 4]).collect()
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a)  => a,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("usage: packg <input-dir> [-o|--output <path>] [-f|--force]");
            std::process::exit(1);
        }
    };

    let input_path = Path::new(&args.input_dir);
    if !input_path.is_dir() {
        eprintln!("error: '{}' is not a directory", args.input_dir);
        std::process::exit(1);
    }

    let output_path: PathBuf = match &args.output {
        Some(p) => PathBuf::from(p),
        None => {
            let stem = input_path
                .file_name()
                .expect("input path has no file name")
                .to_string_lossy();
            PathBuf::from(format!("{}.zip", stem))
        }
    };

    if output_path.exists() && !args.force {
        eprintln!(
            "Archive already exists: {} (use -f to overwrite)",
            output_path.display()
        );
        std::process::exit(1);
    }

    let entries = match std::fs::read_dir(input_path) {
        Ok(e)  => e,
        Err(e) => {
            eprintln!("error reading directory: {}", e);
            std::process::exit(2);
        }
    };

    let mut jpg_files: Vec<PathBuf> = Vec::new();
    let mut preview_img: Option<(String, PathBuf)> = None; // (zip entry name, source path)
    let mut preview_txt: Option<PathBuf> = None;
    let mut skipped = 0usize;

    for entry in entries {
        let entry = match entry {
            Ok(e)  => e,
            Err(e) => {
                eprintln!("error reading directory entry: {}", e);
                std::process::exit(2);
            }
        };
        let path = entry.path();
        if path.is_dir() {
            skipped += 1;
            continue;
        }
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Reserved social-preview files: carried through un-encoded, not gallery images.
        if fname == "social_preview.jpg" || fname == "social_preview.jpeg" {
            if preview_img.is_none() { preview_img = Some(("social_preview.jpg".to_string(), path)); }
            else { skipped += 1; }
            continue;
        }
        if fname == "social_preview.png" {
            if preview_img.is_none() { preview_img = Some(("social_preview.png".to_string(), path)); }
            else { skipped += 1; }
            continue;
        }
        if fname == "social_preview.txt" {
            preview_txt = Some(path);
            continue;
        }

        if ext == "jpg" || ext == "jpeg" {
            jpg_files.push(path);
        } else {
            skipped += 1;
        }
    }

    if jpg_files.is_empty() {
        eprintln!("warning: no gallery .jpg files found in '{}'", args.input_dir);
        std::process::exit(0);
    }

    jpg_files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let count = jpg_files.len();
    let hash_names = generate_unique_names(count);

    let zip_file = match std::fs::File::create(&output_path) {
        Ok(f)  => f,
        Err(e) => {
            eprintln!("error creating output file: {}", e);
            std::process::exit(2);
        }
    };

    let mut zip = zip::ZipWriter::new(zip_file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);

    // Social-preview assets first, plain (un-encoded), so a future splash can render as
    // the stream opens. The glimr stream parser skips them; they are not gallery images.
    for (name, src) in preview_img.iter().map(|(n, s)| (n.as_str(), s))
        .chain(preview_txt.iter().map(|s| ("social_preview.txt", s)))
    {
        let data = match std::fs::read(src) {
            Ok(d)  => d,
            Err(e) => { eprintln!("error reading '{}': {}", src.display(), e); std::process::exit(2); }
        };
        if let Err(e) = zip.start_file(name, options) {
            eprintln!("error writing zip entry: {}", e); std::process::exit(2);
        }
        if let Err(e) = zip.write_all(&data) {
            eprintln!("error writing zip data: {}", e); std::process::exit(2);
        }
    }

    for (hash_name, src_path) in hash_names.iter().zip(jpg_files.iter()) {
        let data = match std::fs::read(src_path) {
            Ok(d)  => d,
            Err(e) => {
                eprintln!("error reading '{}': {}", src_path.display(), e);
                std::process::exit(2);
            }
        };
        let encoded    = xor_encode(&data);
        let entry_name = format!("{}.dat", hash_name);
        if let Err(e) = zip.start_file(&entry_name, options) {
            eprintln!("error writing zip entry: {}", e);
            std::process::exit(2);
        }
        if let Err(e) = zip.write_all(&encoded) {
            eprintln!("error writing zip data: {}", e);
            std::process::exit(2);
        }
    }

    if let Err(e) = zip.finish() {
        eprintln!("error finalizing zip: {}", e);
        std::process::exit(2);
    }

    let zip_size = std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);

    println!(
        "{} image{} packed from {}/",
        count,
        if count == 1 { "" } else { "s" },
        args.input_dir
    );
    if skipped > 0 {
        println!("{} item{} skipped", skipped, if skipped == 1 { "" } else { "s" });
    }
    if preview_img.is_some() || preview_txt.is_some() {
        let mut parts: Vec<&str> = Vec::new();
        if let Some((n, _)) = &preview_img { parts.push(n); }
        if preview_txt.is_some() { parts.push("social_preview.txt"); }
        println!("Social preview: {} (un-encoded)", parts.join(", "));
    }
    println!("Output: {} ({})", output_path.display(), format_size(zip_size));
}
