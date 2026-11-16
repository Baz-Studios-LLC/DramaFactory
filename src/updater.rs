//! Signed executable updates. The trust root is compiled into the app. Release
//! metadata supplies filenames, never arbitrary commands or installation paths.
use anyhow::{Context, Result, bail};
use ed25519_dalek::{Signature, VerifyingKey};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub target: String,
    pub binary: String,
    pub sha256: String,
    pub size: u64,
    pub signature: String,
}
pub const TARGET: &str = "aarch64-apple-darwin";
fn decode<const N: usize>(s: &str) -> Result<[u8; N]> {
    if !s.is_ascii() || s.len() != N * 2 {
        bail!("Invalid signature or key length");
    }
    let mut a = [0; N];
    for (i, b) in a.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).context("Invalid signature encoding")?;
    }
    Ok(a)
}
impl Manifest {
    fn signed_bytes(&self) -> Vec<u8> {
        format!(
            "{}\n{}\n{}\n{}\n{}\n",
            self.version, self.target, self.binary, self.sha256, self.size
        )
        .into_bytes()
    }
    pub fn verify(&self) -> Result<()> {
        self.verify_with(&decode::<32>(
            include_str!("../branding/update-public-key.hex").trim(),
        )?)
    }
    fn verify_with(&self, key: &[u8; 32]) -> Result<()> {
        let version = crate::updates::version(&self.version).context("Invalid release version")?;
        if version <= crate::updates::CURRENT {
            bail!("Update must be newer than this application");
        }
        if self.target != TARGET || !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            bail!("Update is for another platform");
        }
        if self.binary != format!("DramaFactory-{}-{TARGET}", self.version) {
            bail!("Unexpected release asset filename");
        }
        decode::<32>(&self.sha256)?;
        if self.size == 0 || self.size > 250 * 1024 * 1024 {
            bail!("Update exceeds the executable size limit");
        }
        VerifyingKey::from_bytes(key)?.verify_strict(
            &self.signed_bytes(),
            &Signature::from_bytes(&decode::<64>(&self.signature)?),
        )?;
        Ok(())
    }
    pub fn verify_file(&self, path: &Path) -> Result<()> {
        self.verify()?;
        if path.metadata()?.len() != self.size || crate::worker::hash_file(path)? != self.sha256 {
            bail!("Update content does not match its signed manifest");
        }
        Ok(())
    }
}
pub fn stage(version: &str, root: &Path) -> Result<PathBuf> {
    if crate::updates::version(version)
        .filter(|v| *v > crate::updates::CURRENT)
        .is_none()
    {
        bail!("No newer valid release");
    }
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(180)))
        .build();
    let agent: ureq::Agent = config.into();
    let base = format!(
        "https://github.com/{}/releases/download/{version}",
        crate::updates::REPOSITORY
    );
    let manifest: Manifest = agent
        .get(&format!("{base}/update-{TARGET}.json"))
        .call()?
        .body_mut()
        .read_json()?;
    manifest.verify()?;
    if manifest.version != version {
        bail!("Release manifest version mismatch");
    }
    let dir = root.join("updates").join(version);
    std::fs::create_dir_all(&dir)?;
    let temp = dir.join("download.partial");
    let mut response = agent.get(&format!("{base}/{}", manifest.binary)).call()?;
    let mut source = response.body_mut().as_reader().take(manifest.size + 1);
    let mut dest = File::create(&temp)?;
    std::io::copy(&mut source, &mut dest)?;
    dest.sync_all()?;
    manifest.verify_file(&temp)?;
    let staged = dir.join(&manifest.binary);
    std::fs::rename(&temp, &staged)?;
    let manifest_path = dir.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(manifest_path)
}
pub fn schedule_install(manifest: &Path, database: &Path) -> Result<()> {
    let parsed: Manifest = serde_json::from_slice(&std::fs::read(manifest)?)?;
    parsed.verify_file(
        &manifest
            .parent()
            .context("Missing staging directory")?
            .join(&parsed.binary),
    )?;
    let exe = std::env::current_exe()?;
    if exe.file_name().and_then(|x| x.to_str()) != Some("DramaFactory")
        || exe
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            != Some("MacOS")
    {
        bail!("Automatic installation is available in the Mac app bundle");
    }
    let directory = manifest.parent().context("Missing update directory")?;
    let helper = directory.join(format!("installer-{}", uuid::Uuid::new_v4()));
    std::fs::copy(&exe, &helper)?;
    adhoc_sign(&helper)?;
    let mut command = Command::new(helper);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .arg("--apply-update")
        .arg(manifest)
        .arg(&exe)
        .arg(database)
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(File::create(directory.join("install.log"))?))
        .spawn()?;
    Ok(())
}
#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    // kill(pid, 0) checks existence without sending a signal.
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}
fn adhoc_sign(path: &Path) -> Result<()> {
    let result = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(path)
        .output()?;
    if !result.status.success() {
        bail!(
            "Local code signing failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let result = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(path)
        .output()?;
    if !result.status.success() {
        bail!(
            "Local signature verification failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}
#[cfg(unix)]
pub fn apply(args: &[String]) -> Result<()> {
    if args.len() != 4 {
        bail!("Installer arguments invalid");
    }
    let manifest_path = Path::new(&args[0]);
    let target = Path::new(&args[1]);
    let database = Path::new(&args[2]);
    let pid: i32 = args[3].parse()?;
    if pid <= 1 {
        bail!("Invalid parent process");
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
    let directory = manifest_path
        .parent()
        .context("Missing staging directory")?;
    let staged = directory.join(&manifest.binary);
    manifest.verify_file(&staged)?;
    if target.file_name().and_then(|x| x.to_str()) != Some("DramaFactory")
        || target
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            != Some("MacOS")
    {
        bail!("Invalid app installation path");
    }
    let contents = target
        .parent()
        .and_then(|p| p.parent())
        .context("Missing app Contents directory")?;
    let bundle = contents.parent().context("Missing app bundle")?;
    let info = contents.join("Info.plist");
    let signing = Command::new("/usr/bin/codesign")
        .args(["--display", "--verbose=2"])
        .arg(bundle)
        .output()?;
    if !signing.status.success()
        || !String::from_utf8_lossy(&signing.stderr).contains("Signature=adhoc")
    {
        bail!(
            "This installer supports local ad-hoc development bundles only. Developer ID distribution requires complete signed bundle updates."
        );
    }
    let start = std::time::Instant::now();
    while process_exists(pid) {
        if start.elapsed() > Duration::from_secs(86400) {
            bail!("Update expired while waiting for app exit");
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(
            database
                .parent()
                .context("Database directory missing")?
                .join("worker.lock"),
        )?;
    while lock.try_lock_exclusive().is_err() {
        if start.elapsed() > Duration::from_secs(86400) {
            bail!("Update expired while waiting for render completion");
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    let pending = target.with_file_name("DramaFactory.pending");
    // Backups live outside the sealed app bundle. Keeping extra executable or
    // plist backups inside Contents would invalidate or complicate its signature.
    let backup = directory.join(format!("previous-v{:03}", crate::updates::CURRENT));
    let info_backup = directory.join(format!(
        "previous-Info-v{:03}.plist",
        crate::updates::CURRENT
    ));
    if backup.exists() || info_backup.exists() {
        bail!("Previous update backup exists; retaining it for review");
    }
    std::fs::copy(target, &backup)?;
    std::fs::copy(&info, &info_backup)?;
    File::open(&backup)?.sync_all()?;
    std::fs::copy(&staged, &pending)?;
    std::fs::set_permissions(&pending, std::fs::metadata(target)?.permissions())?;
    manifest.verify_file(&pending)?;
    File::open(&pending)?.sync_all()?;
    std::fs::rename(&pending, target)?;
    let installation = (|| -> Result<()> {
        let mut document = plist::Value::from_file(&info)?;
        let dict = document
            .as_dictionary_mut()
            .context("Invalid app property list")?;
        let build = crate::updates::version(&manifest.version)
            .context("Invalid version")?
            .to_string();
        dict.insert(
            "CFBundleVersion".into(),
            plist::Value::String(build.clone()),
        );
        dict.insert(
            "CFBundleShortVersionString".into(),
            plist::Value::String(build),
        );
        let temporary = directory.join("new-Info.plist");
        document.to_file_xml(&temporary)?;
        std::fs::copy(temporary, &info)?;
        adhoc_sign(bundle)?;
        Ok(())
    })();
    if let Err(error) = installation {
        std::fs::copy(&backup, &pending)?;
        std::fs::rename(&pending, target)?;
        std::fs::copy(&info_backup, &info)?;
        adhoc_sign(bundle).context("Could not restore the previous bundle signature")?;
        return Err(error);
    }
    // Local ad-hoc signing changes the embedded signature bytes, so preserve both
    // the authenticated download digest and final installed digest in the receipt.
    let receipt = serde_json::json!({"release":manifest,"installed_executable_sha256":crate::worker::hash_file(target)?});
    std::fs::write(
        directory.join("installed.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}

#[cfg(not(unix))]
pub fn apply(_args: &[String]) -> Result<()> {
    bail!("Automatic installation is currently supported on macOS only")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn rejects_tampering_and_downgrades() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut m = Manifest {
            version: format!("v{:03}", crate::updates::CURRENT + 1),
            target: TARGET.into(),
            binary: format!("DramaFactory-v{:03}-{TARGET}", crate::updates::CURRENT + 1),
            sha256: "ab".repeat(32),
            size: 123,
            signature: String::new(),
        };
        m.signature = key
            .sign(&m.signed_bytes())
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(m.verify_with(&key.verifying_key().to_bytes()).is_ok());
        m.size = 124;
        assert!(m.verify_with(&key.verifying_key().to_bytes()).is_err());
        m.version = "v001".into();
        assert!(m.verify_with(&key.verifying_key().to_bytes()).is_err());
    }
    #[test]
    fn malformed_hex_never_panics() {
        assert!(decode::<32>(&"é".repeat(32)).is_err());
    }
}
