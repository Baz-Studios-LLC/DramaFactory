//! Machine-local credentials, outside project databases, cloud folders and releases.
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
    ElevenLabs,
    Fal,
}
impl Provider {
    #[cfg(target_os = "macos")]
    fn service(self) -> &'static str {
        match self {
            Self::OpenRouter => "studio.bazstudios.dramafactory.openrouter",
            Self::ElevenLabs => "studio.bazstudios.dramafactory.elevenlabs",
            Self::OpenAi => "studio.bazstudios.dramafactory.openai",
            Self::Fal => "studio.bazstudios.dramafactory.fal",
        }
    }
    #[cfg(target_os = "macos")]
    fn local_path(self) -> Result<std::path::PathBuf> {
        let user = directories::UserDirs::new()
            .ok_or_else(|| anyhow::anyhow!("Home directory unavailable"))?;
        // Deliberately independent of DRAMAFACTORY_DATABASE and project paths.
        Ok(user
            .home_dir()
            .join("Library/Application Support/DramaFactory-Credentials")
            .join(format!("{}.credential", self.service())))
    }
    #[cfg(target_os = "macos")]
    fn session(self) -> Result<std::sync::MutexGuard<'static, SessionKey>> {
        static ROUTER: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
        static OPENAI: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
        static MUSIC: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
        static FAL: std::sync::Mutex<SessionKey> = std::sync::Mutex::new(SessionKey(None));
        let session = match self {
            Self::OpenRouter => &ROUTER,
            Self::OpenAi => &OPENAI,
            Self::ElevenLabs => &MUSIC,
            Self::Fal => &FAL,
        };
        session
            .lock()
            .map_err(|_| anyhow::anyhow!("Credential session unavailable; restart the app."))
    }
    pub(crate) fn saved_locally(self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.local_path()
                .ok()
                .and_then(|p| std::fs::symlink_metadata(p).ok())
                .is_some_and(|m| m.is_file() && m.len() > 0)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
    #[cfg(target_os = "macos")]
    pub fn save(self, key: &str) -> Result<()> {
        let mut cache = self.session()?;
        if key.trim().is_empty() || key.chars().any(char::is_whitespace) {
            bail!("Enter an API key without spaces.");
        }
        write_local(&self.local_path()?, key)?;
        cache.0 = Some(key.to_owned());
        Ok(())
    }
    #[cfg(target_os = "macos")]
    pub fn read(self) -> Result<Option<String>> {
        self.session()?.read(|| {
            let path = self.local_path()?;
            if let Some(key) = read_local(&path)? {
                return Ok((!key.is_empty()).then_some(key));
            }
            // One-time migration. Once saved locally, subsequent builds never
            // access Keychain for this provider. Empty files are removal markers.
            use security_framework::passwords::{PasswordOptions, generic_password};
            match generic_password(PasswordOptions::new_generic_password(
                self.service(),
                "api-key",
            )) {
                Ok(bytes) => {
                    let key = String::from_utf8(bytes).map_err(|_| {
                        anyhow::anyhow!("Replace the invalid stored key in Settings.")
                    })?;
                    write_local(&path, &key)?;
                    Ok(Some(key))
                }
                Err(e) if e.code() == -25300 => Ok(None),
                Err(e) => bail!("Keychain could not read the key (code {}).", e.code()),
            }
        })
    }
    #[cfg(target_os = "macos")]
    pub fn remove(self) -> Result<()> {
        let mut cache = self.session()?;
        // Keep an empty marker so a legacy Keychain value cannot reappear.
        write_local(&self.local_path()?, "")?;
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
#[cfg(any(target_os = "macos", all(unix, test)))]
fn credential_directory(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid credential location"))?;
    match std::fs::DirBuilder::new().mode(0o700).create(parent) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => bail!("Could not create private credential directory"),
    }
    let meta = std::fs::symlink_metadata(parent)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        bail!("Unsafe credential directory");
    }
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[cfg(any(target_os = "macos", all(unix, test)))]
fn write_local(path: &std::path::Path, key: &str) -> Result<()> {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    credential_directory(path)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    temp.write_all(key.as_bytes())
        .map_err(|_| anyhow::anyhow!("Could not save credential"))?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|_| anyhow::anyhow!("Could not replace credential file"))?;
    Ok(())
}
#[cfg(any(target_os = "macos", all(unix, test)))]
fn read_local(path: &std::path::Path) -> Result<Option<String>> {
    use std::os::unix::fs::PermissionsExt;
    credential_directory(path)?;
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file() || meta.file_type().is_symlink() {
                bail!("Unsafe credential file");
            }
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            std::fs::read_to_string(path).map(Some).map_err(|_| {
                anyhow::anyhow!("Could not read local credential; replace it in Settings")
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => bail!("Could not inspect local credential"),
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
    #[cfg(unix)]
    #[test]
    fn local_credentials_are_private_atomic_and_removal_stays_removed() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private/provider.credential");
        assert_eq!(read_local(&path).unwrap(), None);
        write_local(&path, "fixture-only").unwrap();
        assert_eq!(read_local(&path).unwrap().as_deref(), Some("fixture-only"));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        write_local(&path, "replacement-fixture").unwrap();
        assert_eq!(
            read_local(&path).unwrap().as_deref(),
            Some("replacement-fixture")
        );
        write_local(&path, "").unwrap();
        assert_eq!(read_local(&path).unwrap().as_deref(), Some(""));
        let linked = temp.path().join("linked");
        std::os::unix::fs::symlink(path.parent().unwrap(), &linked).unwrap();
        assert!(write_local(&linked.join("other.credential"), "fixture").is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn providers_have_independent_credentials() {
        assert_eq!(
            Provider::OpenRouter.service(),
            "studio.bazstudios.dramafactory.openrouter"
        );
        assert_ne!(Provider::OpenRouter.service(), Provider::OpenAi.service());
        assert_ne!(
            Provider::Fal.local_path().unwrap(),
            Provider::OpenRouter.local_path().unwrap()
        );
        assert!(
            Provider::Fal.local_path().unwrap().starts_with(
                directories::UserDirs::new()
                    .unwrap()
                    .home_dir()
                    .join("Library/Application Support/DramaFactory-Credentials")
            )
        );
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
