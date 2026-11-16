//! Read-only credit checks using the existing Keychain credential.
use anyhow::{Context, Result, bail};
use serde_json::Value;
#[derive(Debug)]
pub(crate) struct Credits {
    pub balance: Option<f64>,
    pub allowance: Option<f64>,
    pub balance_note: Option<String>,
}
fn remaining(value: &Value) -> Result<f64> {
    let data = &value["data"];
    let total = data["total_credits"]
        .as_f64()
        .context("Credit total unavailable")?;
    let used = data["total_usage"]
        .as_f64()
        .context("Credit usage unavailable")?;
    let balance = total - used;
    if !balance.is_finite() {
        bail!("Invalid credit balance");
    }
    Ok(balance)
}
fn allowance(value: &Value) -> Option<f64> {
    [
        value["data"]["limit_remaining"].as_f64(),
        value["data"]["effective_budget"]["remaining_usd"].as_f64(),
    ]
    .into_iter()
    .flatten()
    .filter(|n| n.is_finite())
    .reduce(f64::min)
}
pub(crate) fn fetch() -> Result<Credits> {
    let key = crate::credentials::read()?.context("Save an OpenRouter key in Settings")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let get = |path: &str| -> Result<(u16, Value)> {
        let mut response = agent
            .get(&format!("https://openrouter.ai/api/v1/{path}"))
            .header("Authorization", &format!("Bearer {key}"))
            .call()
            .map_err(|_| anyhow::anyhow!("Could not connect to OpenRouter for credit status"))?;
        let status = response.status().as_u16();
        let value = response
            .body_mut()
            .with_config()
            .limit(128 * 1024)
            .read_json::<Value>()
            .unwrap_or(Value::Null);
        Ok((status, value))
    };
    let (status, value) = get("key")?;
    if status != 200 {
        bail!("OpenRouter credit check returned HTTP {status}. Check your key in Settings.");
    }
    let allowance = allowance(&value);
    let (balance, balance_note) = match get("credits") {
        Ok((200, value)) => match remaining(&value) {
            Ok(balance) => (Some(balance), None),
            Err(_) => (None, Some("Account balance unavailable".into())),
        },
        Ok((403, _)) => (
            None,
            Some("Account balance isn’t available to this key".into()),
        ),
        Ok((status, _)) => (
            None,
            Some(format!("Account balance unavailable (HTTP {status})")),
        ),
        Err(_) => (None, Some("Account balance check couldn’t connect".into())),
    };
    Ok(Credits {
        balance,
        allowance,
        balance_note,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn balances_and_limits_are_distinct_and_missing_is_not_zero() {
        assert_eq!(
            remaining(&json!({"data":{"total_credits":20,"total_usage":6.5}})).unwrap(),
            13.5
        );
        assert!(remaining(&json!({"data":{}})).is_err());
        assert_eq!(
            allowance(
                &json!({"data":{"limit_remaining":74.5,"effective_budget":{"remaining_usd":17.5}}})
            ),
            Some(17.5)
        );
        assert_eq!(allowance(&json!({"data":{"limit_remaining":null}})), None);
        assert_eq!(
            remaining(&json!({"data":{"total_credits":0,"total_usage":1}})).unwrap(),
            -1.
        );
    }
}
