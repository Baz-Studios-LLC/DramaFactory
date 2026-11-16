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
