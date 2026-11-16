//! Public, read-only provider discovery. Never sends credentials or generates media.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub videos: Vec<Value>,
    pub images: Vec<Value>,
    pub fetched_unix_seconds: u64,
}
fn models(value: Value) -> Result<Vec<Value>> {
    let list = value
        .get("data")
        .and_then(Value::as_array)
        .context("Model catalog has no data array")?;
    if list.iter().any(|model| {
        model
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    }) {
        bail!("Model catalog contains an invalid model identifier");
    }
    Ok(list.clone())
}
pub fn discover() -> Result<Catalog> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build();
    let agent: ureq::Agent = config.into();
    let fetch = |kind: &str| -> Result<Vec<Value>> {
        let mut response = agent
            .get(&format!("https://openrouter.ai/api/v1/{kind}/models"))
            .header("User-Agent", "DramaFactory-v002")
            .call()
            .with_context(|| format!("OpenRouter {kind} catalog unavailable"))?;
        let value: Value = response
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_json()?;
        models(value)
    };
    Ok(Catalog {
        videos: fetch("videos")?,
        images: fetch("images")?,
        fetched_unix_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    })
}
// Keep provider diagnostics useful without storing echoed credentials or image bytes.
pub(crate) fn submission_error(status: u16, value: &Value, key: &str) -> String {
    let mut parts = Vec::new();
    if let Some(message) = value["error"]["message"]
        .as_str()
        .or_else(|| value["message"].as_str())
    {
        parts.push(message.to_owned());
    }
    if let Some(raw) = value["error"]["metadata"]["raw"].as_str()
        && let Ok(parsed) = serde_json::from_str::<Value>(raw)
        && let Some(message) = parsed["error"]["message"]
            .as_str()
            .or_else(|| parsed["message"].as_str())
    {
        parts.push(message.to_owned());
    }
    let mut detail = parts.join(" · ");
    if !key.is_empty() {
        detail = detail.replace(key, "[redacted]");
    }
    let sensitive =
        regex::Regex::new(r#"(?i)(?:data:image/[^\s\"]+|bearer\s+\S+|sk-[A-Za-z0-9_-]+)"#)
            .expect("static pattern");
    detail = sensitive
        .replace_all(&detail, "[redacted]")
        .chars()
        .filter(|c| !c.is_control())
        .take(700)
        .collect();
    if detail.is_empty() {
        detail = "No readable error details were returned".into();
    }
    format!(
        "OpenRouter HTTP {status}: {detail}. No automatic resubmission. Check OpenRouter video logs before trying again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_catalog_is_not_mistaken_for_no_available_models() {
        assert!(models(serde_json::json!({"error":"unavailable"})).is_err());
        assert!(models(serde_json::json!({"data":[{"name":"missing id"}]})).is_err());
        let items=models(serde_json::json!({"data":[{"id":"example/video","generate_audio":false,"future_capability":{"mode":"new"}}]})).unwrap();
        assert_eq!(items[0]["generate_audio"], false);
        assert_eq!(items[0]["future_capability"]["mode"], "new");
    }
}
