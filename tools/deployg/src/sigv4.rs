use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => { out.push('%'); out.push_str(&format!("{:02X}", b)); }
        }
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes { s.push_str(&format!("{:02x}", b)); }
    s
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn epoch_to_ymd(days: u64) -> (u32, u32, u32) {
    // Howard Hinnant's civil_from_days algorithm
    let z   = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y   = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp  = (5 * doy + 2) / 153;
    let d   = doy - (153 * mp + 2) / 5 + 1;
    let mo  = if mp < 10 { mp + 3 } else { mp - 9 };
    let y   = if mo <= 2 { y + 1 } else { y };
    (y as u32, mo as u32, d as u32)
}

/// Returns (YYYYMMDDTHHMMSSZ, YYYYMMDD) in UTC.
pub fn utc_now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let tod  = secs % 86400;
    let days = secs / 86400;
    let (y, mo, d) = epoch_to_ymd(days);
    let h   = tod / 3600;
    let min = (tod % 3600) / 60;
    let sec = tod % 60;
    let date     = format!("{:04}{:02}{:02}", y, mo, d);
    let datetime = format!("{}T{:02}{:02}{:02}Z", date, h, min, sec);
    (datetime, date)
}

/// Sign with a pre-computed payload hash.
/// Use `UNSIGNED_PAYLOAD` for streaming uploads where the body hash is not known upfront.
pub fn sign_with_hash(
    method: &str,
    host: &str,
    path: &str,
    query_params: &[(&str, &str)],
    extra_headers: &[(&str, &str)],
    body_hash: &str,
    access_key: &str,
    secret_key: &str,
) -> Vec<(String, String)> {
    let region  = "auto";
    let service = "s3";

    let (datetime, date) = utc_now();

    // Canonical query string: URI-encode and sort by encoded key
    let mut qpairs: Vec<(String, String)> = query_params.iter()
        .map(|(k, v)| (uri_encode(k), uri_encode(v)))
        .collect();
    qpairs.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query: String = qpairs.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");

    // Canonical headers: lowercase names, sorted alphabetically
    let mut hdrs: Vec<(String, String)> = vec![
        ("host".into(),                  host.to_string()),
        ("x-amz-content-sha256".into(), body_hash.to_string()),
        ("x-amz-date".into(),           datetime.clone()),
    ];
    for (k, v) in extra_headers {
        hdrs.push((k.to_lowercase(), v.to_string()));
    }
    hdrs.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers: String = hdrs.iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();
    let signed_headers: String = hdrs.iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    // Canonical URI: encode each segment, keep '/' separators
    let canonical_uri: String = path.split('/')
        .map(|seg| uri_encode(seg))
        .collect::<Vec<_>>()
        .join("/");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, canonical_uri, canonical_query,
        canonical_headers, signed_headers, body_hash
    );

    let scope          = format!("{}/{}/{}/aws4_request", date, region, service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        datetime, scope, sha256_hex(canonical_request.as_bytes())
    );

    let signing_key = {
        let k_date    = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), date.as_bytes());
        let k_region  = hmac_sha256(&k_date,    region.as_bytes());
        let k_service = hmac_sha256(&k_region,  service.as_bytes());
        hmac_sha256(&k_service, b"aws4_request")
    };
    let signature = hex_encode(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        access_key, scope, signed_headers, signature
    );

    vec![
        ("authorization".to_string(),        authorization),
        ("x-amz-content-sha256".to_string(), body_hash.to_string()),
        ("x-amz-date".to_string(),           datetime),
    ]
}

/// Convenience wrapper: computes SHA-256 of `body` and calls `sign_with_hash`.
pub fn sign(
    method: &str,
    host: &str,
    path: &str,
    query_params: &[(&str, &str)],
    extra_headers: &[(&str, &str)],
    body: &[u8],
    access_key: &str,
    secret_key: &str,
) -> Vec<(String, String)> {
    sign_with_hash(method, host, path, query_params, extra_headers,
                   &sha256_hex(body), access_key, secret_key)
}
