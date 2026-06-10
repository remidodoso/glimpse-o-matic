use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

mod sigv4;
mod s3;
mod cloudflare;

// An upload source: either a file on disk or in-memory bytes (modified index.html,
// extracted social-preview image).
enum Src { Path(PathBuf), Bytes(Vec<u8>) }

// --- Social preview ---

/// Pull the reserved social-preview entries out of the gallery archive, if present.
/// Returns (image as (ext, bytes), text). Never fatal — a bad/zipless archive just
/// yields no preview.
fn extract_preview(archive: &Path) -> (Option<(String, Vec<u8>)>, Option<String>) {
    let file = match fs::File::open(archive) {
        Ok(f)  => f,
        Err(_) => return (None, None),
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z)  => z,
        Err(e) => { eprintln!("warning: cannot read archive for social preview: {}", e); return (None, None); }
    };
    let mut img: Option<(String, Vec<u8>)> = None;
    let mut txt: Option<String> = None;
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) { Ok(e) => e, Err(_) => continue };
        let base = entry.name().rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        let ext = match base.as_str() {
            "social_preview.jpg" | "social_preview.jpeg" => Some("jpg"),
            "social_preview.png"                         => Some("png"),
            _                                            => None,
        };
        if let Some(ext) = ext {
            if img.is_none() {
                let mut buf = Vec::new();
                if entry.read_to_end(&mut buf).is_ok() { img = Some((ext.to_string(), buf)); }
            }
        } else if base == "social_preview.txt" {
            let mut s = String::new();
            if entry.read_to_string(&mut s).is_ok() { txt = Some(s); }
        }
    }
    (img, txt)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// First line → title, remainder → description.
fn split_title_desc(text: &str) -> (String, String) {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut it = norm.splitn(2, '\n');
    let title = it.next().unwrap_or("").trim().to_string();
    let desc  = it.next().unwrap_or("").trim().to_string();
    (title, desc)
}

// Build the OG/Twitter meta block, emitting only the tags we have data for.
// `base_url` is "" for local (relative image URL) or "{domain}/{prefix}/" for R2 (absolute).
fn build_og_block(img: &Option<(String, Vec<u8>)>, txt: &Option<String>, base_url: &str) -> String {
    let mut s = String::new();
    if let Some(text) = txt {
        let (title, desc) = split_title_desc(text);
        if !title.is_empty() {
            s.push_str(&format!("    <meta property=\"og:title\" content=\"{}\">\n", html_escape(&title)));
            s.push_str(&format!("    <meta name=\"twitter:title\" content=\"{}\">\n", html_escape(&title)));
        }
        if !desc.is_empty() {
            s.push_str(&format!("    <meta property=\"og:description\" content=\"{}\">\n", html_escape(&desc)));
            s.push_str(&format!("    <meta name=\"twitter:description\" content=\"{}\">\n", html_escape(&desc)));
        }
    }
    if let Some((ext, _)) = img {
        let url = if base_url.is_empty() { format!("social_preview.{}", ext) }
                  else { format!("{}social_preview.{}", base_url, ext) };
        s.push_str(&format!("    <meta property=\"og:image\" content=\"{}\">\n", html_escape(&url)));
        s.push_str("    <meta property=\"og:image:width\" content=\"1200\">\n");
        s.push_str("    <meta property=\"og:image:height\" content=\"630\">\n");
        s.push_str(&format!("    <meta name=\"twitter:image\" content=\"{}\">\n", html_escape(&url)));
        s.push_str("    <meta name=\"twitter:card\" content=\"summary_large_image\">\n");
    }
    if (img.is_some() || txt.is_some()) && !base_url.is_empty() {
        s.push_str(&format!("    <meta property=\"og:url\" content=\"{}\">\n", html_escape(base_url)));
        s.push_str("    <meta property=\"og:type\" content=\"website\">\n");
    }
    s
}

// Replace the content between <!--OG--> and <!--/OG--> with `block`.
fn splice_og(html: &str, block: &str) -> String {
    let (open, close) = ("<!--OG-->", "<!--/OG-->");
    if let (Some(a), Some(b)) = (html.find(open), html.find(close)) {
        if b >= a + open.len() {
            return format!("{}\n{}{}", &html[..a + open.len()], block, &html[b..]);
        }
    }
    eprintln!("warning: <!--OG--> markers not found in index.html; social meta not injected");
    html.to_string()
}

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

    // Social-preview assets carried in the archive (optional). Hoisted out so the image
    // is a directly-fetchable object and the meta lands in index.html's <head>.
    let (preview_img, preview_txt) = extract_preview(&archive_src);
    let have_preview = preview_img.is_some() || preview_txt.is_some();

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

        // index.html with social meta spliced in (absolute URLs on R2).
        let index_html = match fs::read_to_string(viewer_root.join("index.html")) {
            Ok(s)  => s,
            Err(e) => { eprintln!("error reading index.html: {}", e); std::process::exit(2); }
        };
        let index_bytes = if have_preview {
            let base = format!("{}/{}/", creds.domain.trim_end_matches('/'), prefix_clean);
            splice_og(&index_html, &build_og_block(&preview_img, &preview_txt, &base)).into_bytes()
        } else {
            index_html.into_bytes()
        };

        let mut uploads: Vec<(Src, String, String)> = Vec::new();
        for f in &viewer_files {
            let key = format!("{}/{}", prefix_clean, f);
            if *f == "index.html" {
                uploads.push((Src::Bytes(index_bytes.clone()), key, f.to_string()));
            } else {
                uploads.push((Src::Path(viewer_root.join(f)), key, f.to_string()));
            }
        }
        uploads.push((Src::Path(archive_src.clone()), format!("{}/Demo.zip", prefix_clean), "Demo.zip".to_string()));
        let about_src = viewer_root.join("about.html");
        if about_src.exists() {
            uploads.push((Src::Path(about_src), format!("{}/about.html", prefix_clean), "about.html".to_string()));
        }
        if let Some((ext, bytes)) = &preview_img {
            uploads.push((
                Src::Bytes(bytes.clone()),
                format!("{}/social_preview.{}", prefix_clean, ext),
                format!("social_preview.{}", ext),
            ));
        }

        println!("Uploading to {}/{}/:", bucket, prefix_clean);
        for (src, key, label) in &uploads {
            if args.dryrun {
                let size = match src {
                    Src::Path(p)  => std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                    Src::Bytes(b) => b.len() as u64,
                };
                println!("  {:<22}  {}  done (dryrun)", label, s3::fmt_size(size));
            } else {
                let res = match src {
                    Src::Path(p)  => s3::put_object(
                        &creds.endpoint, bucket, key, p, label,
                        &creds.access_key_id, &creds.secret_access_key),
                    Src::Bytes(b) => s3::put_object_bytes(
                        &creds.endpoint, bucket, key, b, label,
                        &creds.access_key_id, &creds.secret_access_key),
                };
                if let Err(e) = res { eprintln!("error: {}", e); std::process::exit(1); }
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

        // index.html with social meta spliced in (relative image URL for local).
        let index_html = match fs::read_to_string(viewer_root.join("index.html")) {
            Ok(s)  => s,
            Err(e) => { eprintln!("error reading index.html: {}", e); std::process::exit(2); }
        };
        let index_bytes = if have_preview {
            splice_og(&index_html, &build_og_block(&preview_img, &preview_txt, "")).into_bytes()
        } else {
            index_html.into_bytes()
        };

        for f in &viewer_files {
            if *f == "index.html" {
                if let Err(e) = fs::write(output_dir.join(f), &index_bytes) {
                    eprintln!("error writing index.html: {}", e);
                    std::process::exit(2);
                }
            } else {
                copy_file(&viewer_root.join(f), &output_dir.join(f));
            }
        }
        copy_file(&archive_src, &output_dir.join("Demo.zip"));
        if has_about {
            copy_file(&about_src, &output_dir.join("about.html"));
        }
        if let Some((ext, bytes)) = &preview_img {
            if let Err(e) = fs::write(output_dir.join(format!("social_preview.{}", ext)), bytes) {
                eprintln!("error writing social_preview: {}", e);
                std::process::exit(2);
            }
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
    if let Some((ext, _)) = &preview_img {
        println!("  social_preview.{}  (+ og:image meta)", ext);
    } else if preview_txt.is_some() {
        println!("  (social text meta injected into index.html)");
    }
}
