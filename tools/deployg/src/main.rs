use std::fs;
use std::path::{Path, PathBuf};

struct Args {
    archive: Option<String>,
    output:  Option<String>,
    force:   bool,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut archive: Option<String> = None;
    let mut output:  Option<String> = None;
    let mut force = false;

    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "-a" | "--archive" => {
                i += 1;
                if i >= raw.len() { return Err(format!("'{}' requires a value", raw[i-1])); }
                archive = Some(raw[i].clone());
            }
            "-o" | "--output" => {
                i += 1;
                if i >= raw.len() { return Err(format!("'{}' requires a value", raw[i-1])); }
                output = Some(raw[i].clone());
            }
            "-f" | "--force" => { force = true; }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown flag: {}", arg));
            }
            _ => { return Err(format!("unexpected argument: {}", raw[i])); }
        }
        i += 1;
    }

    Ok(Args { archive, output, force })
}

fn find_viewer_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..3 {
        if dir.join("index.html").exists() {
            return Some(dir);
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None    => break,
        }
    }
    None
}

fn is_dir_empty(path: &Path) -> bool {
    fs::read_dir(path).map(|mut e| e.next().is_none()).unwrap_or(false)
}

fn copy_file(src: &Path, dst: &Path) {
    if let Err(e) = fs::copy(src, dst) {
        eprintln!("error copying {} -> {}: {}", src.display(), dst.display(), e);
        std::process::exit(2);
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a)  => a,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("usage: deployg [-a|--archive <path>] [-o|--output <path>] [-f|--force]");
            std::process::exit(1);
        }
    };

    let viewer_root = match find_viewer_root() {
        Some(r) => r,
        None => {
            eprintln!("error: could not find viewer root (index.html not found in current, parent, or grandparent directory)");
            std::process::exit(1);
        }
    };

    let archive_src: PathBuf = match &args.archive {
        Some(p) => PathBuf::from(p),
        None    => viewer_root.join("Demo.zip"),
    };

    let output_dir: PathBuf = match &args.output {
        Some(p) => PathBuf::from(p),
        None    => std::env::current_dir().unwrap().join("deploy"),
    };

    let viewer_files = [
        "index.html",
        "main.js",
        "main.css",
        "pkg/glimr.js",
        "pkg/glimr_bg.wasm",
    ];

    // Verify all source files exist before touching the output directory
    for f in &viewer_files {
        let src = viewer_root.join(f);
        if !src.exists() {
            eprintln!("error: missing viewer file: {}", src.display());
            if f.starts_with("pkg/") {
                eprintln!("hint: run build.ps1 to generate pkg/ files");
            }
            std::process::exit(1);
        }
    }
    if !archive_src.exists() {
        eprintln!("error: archive not found: {}", archive_src.display());
        std::process::exit(1);
    }

    // Handle existing output directory
    if output_dir.exists() && !is_dir_empty(&output_dir) {
        if !args.force {
            eprintln!(
                "Output directory already exists: {} (use -f to overwrite)",
                output_dir.display()
            );
            std::process::exit(1);
        }
        println!("Overwriting: {}", output_dir.display());
        if let Err(e) = fs::remove_dir_all(&output_dir) {
            eprintln!("error removing directory: {}", e);
            std::process::exit(2);
        }
    }

    if let Err(e) = fs::create_dir_all(output_dir.join("pkg")) {
        eprintln!("error creating output directory: {}", e);
        std::process::exit(2);
    }

    // Copy viewer files
    for f in &viewer_files {
        copy_file(&viewer_root.join(f), &output_dir.join(f));
    }

    // Copy archive as Demo.zip
    copy_file(&archive_src, &output_dir.join("Demo.zip"));

    // Summary
    let archive_name = archive_src.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");
    let archive_label = if archive_name == "Demo.zip" {
        "Demo.zip".to_string()
    } else {
        format!("Demo.zip  (from {})", archive_name)
    };

    println!("Deployed to: {}", output_dir.display());
    for f in &viewer_files {
        println!("  {}", f);
    }
    println!("  {}", archive_label);
}
