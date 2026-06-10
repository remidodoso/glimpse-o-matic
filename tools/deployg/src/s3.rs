use std::path::Path;
use crate::sigv4;

// --- XML helpers ---

fn xml_values(xml: &str, tag: &str) -> Vec<String> {
    let open  = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut vals = Vec::new();
    let mut rest = xml;
    while let Some(s) = rest.find(&open) {
        rest = &rest[s + open.len()..];
        if let Some(e) = rest.find(&close) {
            vals.push(rest[..e].to_string());
            rest = &rest[e + close.len()..];
        } else {
            break;
        }
    }
    vals
}

fn xml_value(xml: &str, tag: &str) -> Option<String> {
    xml_values(xml, tag).into_iter().next()
}

fn s3_error_msg(xml: &str) -> String {
    xml_value(xml, "Message").unwrap_or_else(|| xml.trim().to_string())
}

// --- URL building ---

pub fn host_of(endpoint: &str) -> &str {
    endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}

/// Build an S3 URL. Empty-valued params appear as `?key` (no `=`) per S3 convention.
fn build_url(endpoint: &str, path: &str, params: &[(&str, &str)]) -> String {
    let mut url = format!("{}{}", endpoint, path);
    if !params.is_empty() {
        url.push('?');
        let qs: String = params.iter()
            .map(|(k, v)| {
                let ek = sigv4::uri_encode(k);
                if v.is_empty() { ek } else { format!("{}={}", ek, sigv4::uri_encode(v)) }
            })
            .collect::<Vec<_>>()
            .join("&");
        url.push_str(&qs);
    }
    url
}

fn map_err(e: ureq::Error, op: &str) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            format!("{} failed (HTTP {}): {}", op, code, s3_error_msg(&body))
        }
        other => format!("{} failed: {}", op, other),
    }
}

// --- Public operations ---

/// List all object keys under `prefix` in `bucket`, paginating as needed.
pub fn list_prefix(
    endpoint: &str,
    bucket: &str,
    prefix: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<Vec<String>, String> {
    let host      = host_of(endpoint);
    let s3_prefix = format!("{}/", prefix.trim_end_matches('/'));
    let path      = format!("/{}", bucket);
    let mut keys: Vec<String> = Vec::new();
    let mut continuation: Option<String> = None;

    loop {
        let cont = continuation.clone().unwrap_or_default();
        let mut params: Vec<(&str, &str)> = vec![
            ("list-type", "2"),
            ("max-keys",  "1000"),
            ("prefix",    &s3_prefix),
        ];
        if continuation.is_some() {
            params.push(("continuation-token", &cont));
        }

        let url    = build_url(endpoint, &path, &params);
        let signed = sigv4::sign("GET", host, &path, &params, &[], b"", access_key, secret_key);
        let mut req = ureq::request("GET", &url);
        for (k, v) in &signed { req = req.set(k, v); }

        let xml = req.call()
            .map_err(|e| map_err(e, "list"))?
            .into_string()
            .map_err(|e| format!("reading list response: {}", e))?;

        keys.extend(xml_values(&xml, "Key"));

        let truncated = xml_value(&xml, "IsTruncated")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if truncated {
            continuation = xml_value(&xml, "NextContinuationToken");
            if continuation.is_none() { break; }
        } else {
            break;
        }
    }

    Ok(keys)
}

/// Delete a batch of object keys from `bucket` in one S3 DeleteObjects call.
pub fn delete_objects(
    endpoint: &str,
    bucket: &str,
    keys: &[String],
    access_key: &str,
    secret_key: &str,
) -> Result<(), String> {
    if keys.is_empty() { return Ok(()); }

    let host   = host_of(endpoint);
    let path   = format!("/{}", bucket);
    let params = [("delete", "")];

    let mut xml_body = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Delete><Quiet>true</Quiet>"
    );
    for key in keys {
        let safe = key.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        xml_body.push_str("<Object><Key>");
        xml_body.push_str(&safe);
        xml_body.push_str("</Key></Object>");
    }
    xml_body.push_str("</Delete>");

    let body  = xml_body.as_bytes();
    let extra = [("content-type", "application/xml")];
    let url    = build_url(endpoint, &path, &params);
    let signed = sigv4::sign("POST", host, &path, &params, &extra, body, access_key, secret_key);

    let mut req = ureq::request("POST", &url);
    for (k, v) in &signed { req = req.set(k, v); }
    for (k, v) in &extra  { req = req.set(k, v); }

    let xml = req.send_bytes(body)
        .map_err(|e| map_err(e, "delete"))?
        .into_string()
        .map_err(|e| format!("reading delete response: {}", e))?;

    // Report any per-key errors from <DeleteResult>
    let errors = xml_values(&xml, "Error");
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter()
            .map(|e| {
                let key = xml_value(e, "Key").unwrap_or_default();
                let msg = xml_value(e, "Message").unwrap_or_default();
                format!("  {}: {}", key, msg)
            })
            .collect();
        return Err(format!("some deletes failed:\n{}", msgs.join("\n")));
    }

    Ok(())
}

// --- Upload ---

pub fn fmt_size(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.1} KB", bytes as f64 / 1_000.0) }
    else                   { format!("{} B", bytes) }
}

fn mime_type(key: &str) -> &'static str {
    match key.rsplit('.').next().unwrap_or("") {
        "html"          => "text/html; charset=utf-8",
        "js"            => "application/javascript",
        "css"           => "text/css",
        "png"           => "image/png",
        "jpg" | "jpeg"  => "image/jpeg",
        "txt"           => "text/plain; charset=utf-8",
        "wasm"          => "application/wasm",
        "zip"           => "application/zip",
        _               => "application/octet-stream",
    }
}

/// Upload a local file to `bucket` under `key` (which includes the prefix).
/// R2 requires Content-Length, so the file is loaded into memory before sending.
/// Prints a progress label then overwrites it with size + "done" on completion.
/// Upload a local file to `bucket` under `key`. Reads the file, then delegates to
/// `put_object_bytes`.
pub fn put_object(
    endpoint:   &str,
    bucket:     &str,
    key:        &str,   // e.g. "2020/Phoenix/index.html"
    src_path:   &Path,
    label:      &str,   // display name for progress output
    access_key: &str,
    secret_key: &str,
) -> Result<(), String> {
    let data = std::fs::read(src_path)
        .map_err(|e| format!("cannot read {}: {}", src_path.display(), e))?;
    put_object_bytes(endpoint, bucket, key, &data, label, access_key, secret_key)
}

/// Upload in-memory bytes to `bucket` under `key`. R2 requires Content-Length, so the
/// whole body is sent at once. Content-type is derived from the key's extension.
pub fn put_object_bytes(
    endpoint:   &str,
    bucket:     &str,
    key:        &str,
    data:       &[u8],
    label:      &str,
    access_key: &str,
    secret_key: &str,
) -> Result<(), String> {
    use std::io::Write as _;

    let host    = host_of(endpoint);
    let s3_path = format!("/{}/{}", bucket, key);
    let mime    = mime_type(key);

    print!("  {:<22}  uploading...\r", label);
    std::io::stdout().flush().ok();

    let body_hash = sigv4::sha256_hex(data);
    let extra     = [("content-type", mime)];
    let signed    = sigv4::sign_with_hash(
        "PUT", host, &s3_path, &[], &extra, &body_hash, access_key, secret_key,
    );

    let url = format!("{}{}", endpoint, s3_path);
    let mut req = ureq::request("PUT", &url);
    for (k, v) in &signed { req = req.set(k, v); }
    for (k, v) in &extra  { req = req.set(k, v); }

    req.send_bytes(data)
        .map_err(|e| map_err(e, &format!("upload {}", label)))?;

    let done = format!("  {:<22}  {}  done", label, fmt_size(data.len() as u64));
    println!("\r{:<55}", done);
    std::io::stdout().flush().ok();

    Ok(())
}
