use std::fs;
use std::path::{Path, PathBuf};

mod sigv4;
mod s3;
mod cloudflare;

struct Args {
    archive: Option<String>,
    output:  Option<String>,
    bucket:  Option<String>,
    prefix:  Option<String>,
    force:   bool,
    yes:     bool,
    dryrun:  bool,
}

const HELP: &str = "\
deployg - deploy viewer files to a local directory or R2 bucket

Usage:
  deployg [options]

Options:
  -a, --archive <path>   Archive to deploy (default: Demo.zip in viewer root)
  -o, --output  <path>   Output to local directory (default: ./deploy)
  -b, --bucket  <name>   Upload to R2 bucket (reads %USERPROFILE%\\.r2\\credentials.txt)
  -p, --prefix  <path>   Key prefix in bucket, e.g. 2020/Phoenix  (required with -b)
  -f, --force            Overwrite existing output dir / delete existing prefix contents
  -y, --yes              Skip confirmation prompt when deleting prefix contents
  --dryrun               Simulate without modifying files or uploading to R2
  -?, --help             Show this help

Notes:
  -o and -b are mutually exclusive.
  -p is required when -b is used.
  Without -f, deploying to a non-empty prefix or directory fails immediately.
  With -f and -b, lists files to delete and prompts for confirmation (default N)
  unless -y is also given.
";

fn print_help() {
    print!("{}", HELP);
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut archive: Option<String> = None;
    let mut output:  Option<String> = None;
    let mut bucket:  Option<String> = None;
    let mut prefix:  Option<String> = None;
    let mut force  = false;
    let mut yes    = false;
    let mut dryrun = false;

    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "-?" | "--help" => {
                print_help();
                std::process::exit(0);
            }
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
            "-b" | "--bucket" => {
                i += 1;
                if i >= raw.len() { return Err(format!("'{}' requires a value", raw[i-1])); }
                bucket = Some(raw[i].clone());
            }
            "-p" | "--prefix" => {
                i += 1;
                if i >= raw.len() { return Err(format!("'{}' requires a value", raw[i-1])); }
                prefix = Some(raw[i].clone());
            }
            "-f" | "--force" => { force  = true; }
            "-y" | "--yes"   => { yes    = true; }
            "--dryrun"       => { dryrun = true; }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown flag: {}", arg));
            }
            _ => { return Err(format!("unexpected argument: {}", raw[i])); }
        }
        i += 1;
    }

    // Cross-argument validation
    if output.is_none() && bucket.is_none() {
        return Err("no destination specified; use -o <dir> for local or -b <bucket> -p <prefix> for R2".to_string());
    }
    if output.is_some() && bucket.is_some() {
        return Err("-o/--output and -b/--bucket are mutually exclusive".to_string());
    }
    if bucket.is_some() && prefix.is_none() {
        return Err("-p/--prefix is required when -b/--bucket is used".to_string());
    }
    if prefix.is_some() && bucket.is_none() {
        return Err("-p/--prefix requires -b/--bucket".to_string());
    }

    Ok(Args { archive, output, bucket, prefix, force, yes, dryrun })
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

// --- Credentials ---

#[allow(dead_code)]
struct Credentials {
    auth_token:        String,
    access_key_id:     String,
    secret_access_key: String,
    endpoint:          String,  // e.g. https://<account_id>.r2.cloudflarestorage.com
    domain:            String,  // e.g. https://si-p.jayenh.com
    zone_id:           String,
}

fn load_credentials(bucket: &str) -> Result<Credentials, String> {
    let home = std::env::var("USERPROFILE")
        .map_err(|_| "USERPROFILE environment variable not set".to_string())?;
    let cred_path = PathBuf::from(&home).join(".r2").join("credentials.txt");

    let content = fs::read_to_string(&cred_path)
        .map_err(|e| format!("cannot read {}: {}", cred_path.display(), e))?;

    let mut found = false;
    let mut in_stanza = false;
    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if in_stanza { break; } // past our stanza, stop
            let name = &line[1..line.len() - 1];
            if name == bucket {
                found = true;
                in_stanza = true;
            }
            continue;
        }
        if in_stanza {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_string();
                let val = line[eq + 1..].trim().to_string();
                fields.insert(key, val);
            }
        }
    }

    if !found {
        return Err(format!(
            "stanza [{}] not found in {}",
            bucket,
            cred_path.display()
        ));
    }

    let require = |key: &str| -> Result<String, String> {
        fields.get(key)
            .cloned()
            .ok_or_else(|| format!("missing '{}' in [{}] stanza of {}", key, bucket, cred_path.display()))
    };

    Ok(Credentials {
        auth_token:        require("auth_token")?,
        access_key_id:     require("access_key_id")?,
        secret_access_key: require("secret_access_key")?,
        endpoint:          require("endpoint")?,
        domain:            require("domain")?,
        zone_id:           require("zone_id")?,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(a)  => a,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("Run 'deployg --help' for usage.");
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

    let viewer_files = [
        "index.html",
        "main.js",
        "main.css",
        "sip.png",
        "pkg/glimr.js",
        "pkg/glimr_bg.wasm",
    ];

    // Verify all source files exist before touching anything
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

    if let Some(ref bucket) = args.bucket {
        let prefix = args.prefix.as_deref().unwrap_or("");
        let creds = match load_credentials(bucket) {
            Ok(c)  => c,
            Err(e) => { eprintln!("error: {}", e); std::process::exit(1); }
        };

        // List existing objects under the prefix
        use std::io::Write;
        print!("Checking {}/{}/ ... ", bucket, prefix);
        std::io::stdout().flush().ok();
        let existing = match s3::list_prefix(
            &creds.endpoint, bucket, prefix,
            &creds.access_key_id, &creds.secret_access_key,
        ) {
            Ok(v)  => v,
            Err(e) => { eprintln!("\nerror: {}", e); std::process::exit(1); }
        };
        println!("{} file(s) found", existing.len());

        if !existing.is_empty() {
            if !args.force {
                eprintln!(
                    "Prefix {}/{} already contains {} file(s). Use -f to overwrite.\nNo action taken.",
                    bucket, prefix, existing.len()
                );
                std::process::exit(1);
            }

            println!("\nWill delete ({} file(s)):", existing.len());
            for key in &existing {
                println!("  {}", key);
            }

            if args.dryrun {
                println!("\nProceed? [y/N] y  (dryrun)\n");
                println!("Deleted {} file(s). (dryrun)\n", existing.len());
            } else {
                if !args.yes {
                    print!("\nProceed? [y/N] ");
                    std::io::stdout().flush().ok();
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).unwrap_or(0);
                    if input.trim().to_lowercase() != "y" {
                        println!("Aborted. No action taken.");
                        std::process::exit(0);
                    }
                }
                println!();

                match s3::delete_objects(
                    &creds.endpoint, bucket, &existing,
                    &creds.access_key_id, &creds.secret_access_key,
                ) {
                    Ok(())  => println!("Deleted {} file(s).\n", existing.len()),
                    Err(e)  => { eprintln!("error: {}", e); std::process::exit(1); }
                }
            }
        }

        // --- Upload ---
        let prefix_clean = prefix.trim_matches('/');
        let mut uploads: Vec<(PathBuf, String, String)> = viewer_files.iter()
            .map(|f| (
                viewer_root.join(f),
                format!("{}/{}", prefix_clean, f),
                f.to_string(),
            ))
            .collect();
        uploads.push((
            archive_src.clone(),
            format!("{}/Demo.zip", prefix_clean),
            "Demo.zip".to_string(),
        ));
        let about_src = viewer_root.join("about.html");
        if about_src.exists() {
            uploads.push((
                about_src,
                format!("{}/about.html", prefix_clean),
                "about.html".to_string(),
            ));
        }

        println!("Uploading to {}/{}/:", bucket, prefix_clean);
        for (local, key, label) in &uploads {
            if args.dryrun {
                print!("  {:<22}  uploading...\r", label);
                std::io::stdout().flush().ok();
                let data = match std::fs::read(local) {
                    Ok(d)  => d,
                    Err(e) => { eprintln!("error: cannot read {}: {}", local.display(), e); std::process::exit(1); }
                };
                let _hash = sigv4::sha256_hex(&data);
                let done = format!("  {:<22}  {}  done (dryrun)", label, s3::fmt_size(data.len() as u64));
                println!("\r{:<55}", done);
            } else if let Err(e) = s3::put_object(
                &creds.endpoint, bucket, key, local, label,
                &creds.access_key_id, &creds.secret_access_key,
            ) {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }

        // --- Cloudflare cache purge (disabled until API token permissions verified) ---
        // let domain = creds.domain.trim_end_matches('/');
        // let purge_urls: Vec<String> = uploads.iter()
        //     .map(|(_, key, _)| format!("{}/{}", domain, key))
        //     .collect();
        // print!("\nPurging Cloudflare cache ... ");
        // std::io::stdout().flush().ok();
        // if let Err(e) = cloudflare::purge_cache(&creds.zone_id, &creds.auth_token, &purge_urls) {
        //     eprintln!("\nerror: {}", e);
        //     std::process::exit(1);
        // }
        // println!("done ({} URLs)", purge_urls.len());
        return;
    }

    // --- Local directory output path ---

    let output_dir: PathBuf = PathBuf::from(args.output.as_deref().unwrap());
    let about_src = viewer_root.join("about.html");
    let has_about = about_src.exists();

    if !args.dryrun {
        if output_dir.exists() && !is_dir_empty(&output_dir) {
            if !args.force {
                eprintln!(
                    "Output directory already exists and is non-empty: {}\nUse -f to overwrite.",
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

        for f in &viewer_files {
            copy_file(&viewer_root.join(f), &output_dir.join(f));
        }
        copy_file(&archive_src, &output_dir.join("Demo.zip"));
        if has_about {
            copy_file(&about_src, &output_dir.join("about.html"));
        }
    }

    let archive_name = archive_src.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");
    let archive_label = if archive_name == "Demo.zip" {
        "Demo.zip".to_string()
    } else {
        format!("Demo.zip  (from {})", archive_name)
    };

    if args.dryrun {
        println!("Would deploy to: {}", output_dir.display());
    } else {
        println!("Deployed to: {}", output_dir.display());
    }
    for f in &viewer_files {
        println!("  {}", f);
    }
    println!("  {}", archive_label);
    if has_about {
        println!("  about.html");
    }
}
