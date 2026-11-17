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
#[derive(Clone, Copy)]
pub enum Provider {
    OpenRouter,
    OpenAi,
}
impl Provider {
    #[cfg(target_os = "macos")]
    fn service(self) -> &'static str {
        match self {
            Self::OpenRouter => "studio.bazstudios.dramafactory.openrouter",
            Self::OpenAi => "studio.bazstudios.dramafactory.openai",
        }
    }
    #[cfg(target_os = "macos")]
    fn session(self) -> Result<std::sync::MutexGuard<'static, SessionKey>> {
        static ROUTER: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
        static OPENAI: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
        let session = match self {
            Self::OpenRouter => &ROUTER,
            Self::OpenAi => &OPENAI,
        };
        session
            .lock()
            .map_err(|_| anyhow::anyhow!("Credential session unavailable; restart the app."))
    }
    #[cfg(target_os = "macos")]
    pub fn save(self, key: &str) -> Result<()> {
        let mut cache = self.session()?;
        security_framework::passwords::set_generic_password(
            self.service(),
            "api-key",
            key.as_bytes(),
        )
        .map_err(|e| anyhow::anyhow!("Keychain could not save the key (code {}).", e.code()))?;
        cache.0 = Some(key.to_owned());
        Ok(())
    }
    #[cfg(target_os = "macos")]
    pub fn read(self) -> Result<Option<String>> {
        self.session()?.read(|| {
            use security_framework::passwords::{PasswordOptions, generic_password};
            match generic_password(PasswordOptions::new_generic_password(
                self.service(),
                "api-key",
            )) {
                Ok(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|_| {
                    anyhow::anyhow!("Replace the invalid stored key in Settings.")
                })?)),
                Err(e) if e.code() == -25300 => Ok(None),
                Err(e) => bail!("Keychain could not read the key (code {}).", e.code()),
            }
        })
    }
    #[cfg(target_os = "macos")]
    pub fn remove(self) -> Result<()> {
        let mut cache = self.session()?;
        match security_framework::passwords::delete_generic_password(self.service(), "api-key") {
            Ok(()) => {}
            Err(e) if e.code() == -25300 => {}
            Err(e) => bail!("Keychain could not remove the key (code {}).", e.code()),
        }
        cache.0 = None;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    pub fn save(self, _: &str) -> Result<()> {
        bail!("Secure credential storage is unavailable on this platform.")
    }
    #[cfg(not(target_os = "macos"))]
    pub fn read(self) -> Result<Option<String>> {
        bail!("Secure credential storage is unavailable on this platform.")
    }
    #[cfg(not(target_os = "macos"))]
    pub fn remove(self) -> Result<()> {
        bail!("Secure credential storage is unavailable on this platform.")
    }
}
pub fn save(key: &str) -> Result<()> {
    Provider::OpenRouter.save(key)
}
pub fn read() -> Result<Option<String>> {
    Provider::OpenRouter.read()
}
pub fn remove() -> Result<()> {
    Provider::OpenRouter.remove()
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
    #[cfg(target_os = "macos")]
    #[test]
    fn providers_have_independent_credentials() {
        assert_eq!(
            Provider::OpenRouter.service(),
            "studio.bazstudios.dramafactory.openrouter"
        );
        assert_ne!(Provider::OpenRouter.service(), Provider::OpenAi.service());
        let router = Provider::OpenRouter.session().unwrap();
        let openai = Provider::OpenAi.session().unwrap();
        assert!(!std::ptr::eq(&*router, &*openai));
    }
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
