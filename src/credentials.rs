//! Native credential boundary. Never serialize or expose key bytes in diagnostics.
use anyhow::{Result, bail};
// Process-local only: a successful authorization is reused until quit or key replacement.
#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct SessionKey(Option<String>);
#[cfg(any(target_os = "macos", test))]
impl SessionKey {
    fn read(&mut self, load: impl FnOnce() -> Result<Option<String>>) -> Result<Option<String>> {
        if let Some(key) = &self.0 {
            return Ok(Some(key.clone()));
        }
        let key = load()?;
        self.0 = key.clone();
        Ok(key)
    }
}
#[cfg(target_os = "macos")]
static SESSION: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
#[cfg(target_os = "macos")]
fn session() -> Result<std::sync::MutexGuard<'static, SessionKey>> {
    SESSION
        .lock()
        .map_err(|_| anyhow::anyhow!("Credential session unavailable; restart the app."))
}
#[cfg(target_os = "macos")]
const SERVICE: &str = "studio.bazstudios.dramafactory.openrouter";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "api-key";
#[cfg(target_os = "macos")]
pub fn save(key: &str) -> Result<()> {
    let mut cache = session()?;
    security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, key.as_bytes()).map_err(
        |error| anyhow::anyhow!("Keychain could not save the key (code {}).", error.code()),
    )?;
    cache.0 = Some(key.to_owned());
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn read() -> Result<Option<String>> {
    session()?.read(read_keychain)
}
#[cfg(target_os = "macos")]
fn read_keychain() -> Result<Option<String>> {
    use security_framework::passwords::{PasswordOptions, generic_password};
    match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT)) {
        Ok(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|_| {
            anyhow::anyhow!("Stored key has an invalid format. Replace it in Settings.")
        })?)),
        Err(error) if error.code() == -25300 => Ok(None),
        Err(error) => bail!("Keychain could not read the key (code {}).", error.code()),
    }
}
#[cfg(target_os = "macos")]
pub fn remove() -> Result<()> {
    let mut cache = session()?;
    let result = match security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == -25300 => Ok(()),
        Err(error) => bail!("Keychain could not remove the key (code {}).", error.code()),
    };
    if result.is_ok() {
        cache.0 = None;
    }
    result
}
#[cfg(not(target_os = "macos"))]
pub fn save(_: &str) -> Result<()> {
    bail!("Secure credential storage is not implemented on this platform yet.")
}
#[cfg(not(target_os = "macos"))]
pub fn read() -> Result<Option<String>> {
    bail!("Secure credential storage is not implemented on this platform yet.")
}
#[cfg(not(target_os = "macos"))]
pub fn remove() -> Result<()> {
    bail!("Secure credential storage is not implemented on this platform yet.")
}
/// GET only, fixed origin, redirects disabled; response content is never echoed.
pub fn test_connection() -> Result<String> {
    let key =
        read()?.ok_or_else(|| anyhow::anyhow!("No OpenRouter key saved. Save a key first."))?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .max_redirects(0)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = match agent
        .get("https://openrouter.ai/api/v1/key")
        .header("Authorization", &format!("Bearer {key}"))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(401 | 403)) => {
            bail!("OpenRouter did not accept this key. Check or replace it.")
        }
        Err(_) => bail!("Could not check OpenRouter. Check your connection and try again."),
    };
    let value: serde_json::Value = response
        .body_mut()
        .with_config()
        .limit(65536)
        .read_json()
        .map_err(|_| anyhow::anyhow!("OpenRouter returned an unreadable key status."))?;
    if !value.get("data").is_some_and(serde_json::Value::is_object) {
        bail!("OpenRouter returned an unexpected key status.");
    }
    Ok("OpenRouter accepted the saved key. No generation was started; model access and spending limits are checked separately.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_reuses_authorization_and_does_not_cache_failure() {
        let mut cache = SessionKey::default();
        assert!(cache.read(|| bail!("denied")).is_err());
        assert!(cache.read(|| Ok(None)).unwrap().is_none());
        assert_eq!(
            cache
                .read(|| Ok(Some("fixture".into())))
                .unwrap()
                .as_deref(),
            Some("fixture")
        );
        assert_eq!(
            cache
                .read(|| panic!("must not access Keychain again"))
                .unwrap()
                .as_deref(),
            Some("fixture")
        );
        cache.0 = Some("replacement".into());
        assert_eq!(
            cache
                .read(|| panic!("must reuse saved key"))
                .unwrap()
                .as_deref(),
            Some("replacement")
        );
        cache.0 = None;
        assert!(cache.read(|| Ok(None)).unwrap().is_none());
    }
}
