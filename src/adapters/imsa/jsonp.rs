use reqwest::blocking::Client;
use serde_json::Value;

pub(super) fn parse_jsonp_body(text: &str, callback: &str) -> Result<Value, String> {
    let trimmed = text.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).map_err(|e| format!("json parse failed: {e}"));
    }

    let prefix = format!("{callback}(");
    if !trimmed.starts_with(&prefix) {
        return Err(format!(
            "response is neither raw JSON nor expected JSONP callback {callback}"
        ));
    }

    let start = prefix.len();
    let end = trimmed
        .rfind(')')
        .ok_or_else(|| "jsonp closing ')' not found".to_string())?;

    let inner = trimmed[start..end].trim();
    serde_json::from_str(inner).map_err(|e| format!("jsonp inner json parse failed: {e}"))
}

pub(super) fn fetch_url_text(client: &Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123 Safari/537.36",
        )
        .header("Accept", "application/javascript, application/json, text/plain, */*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Referer", "https://www.imsa.com/scoring/")
        .header("Origin", "https://www.imsa.com")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("http {status}"));
    }

    response
        .text()
        .map_err(|e| format!("body read failed: {e}"))
}
