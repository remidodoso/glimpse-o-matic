#[allow(dead_code)]
/// Purge a list of URLs from Cloudflare's cache for the given zone.
/// Accepts up to 30 URLs per call (CF API limit); batches automatically if needed.
pub fn purge_cache(zone_id: &str, auth_token: &str, urls: &[String]) -> Result<(), String> {
    let cf_url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/purge_cache",
        zone_id
    );

    for chunk in urls.chunks(30) {
        let file_list: String = chunk.iter()
            .map(|u| format!("\"{}\"", u.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        let body = format!("{{\"files\":[{}]}}", file_list);

        let resp_body = ureq::post(&cf_url)
            .set("Authorization", &format!("Bearer {}", auth_token))
            .set("Content-Type", "application/json")
            .send_string(&body)
            .map_err(|e| match e {
                ureq::Error::Status(code, resp) => {
                    let b = resp.into_string().unwrap_or_default();
                    format!("cache purge failed (HTTP {}): {}", code, b.trim())
                }
                other => format!("cache purge failed: {}", other),
            })?
            .into_string()
            .map_err(|e| format!("reading purge response: {}", e))?;

        if !resp_body.contains("\"success\":true") {
            return Err(format!("cache purge non-success: {}", resp_body.trim()));
        }
    }

    Ok(())
}
