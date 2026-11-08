//! Release discovery runs away from the UI thread. Installation is deliberately
//! gated on a future signed-artifact verifier; release metadata is not trusted code.
use serde::Deserialize;
use std::sync::mpsc::{self, Receiver};
pub const CURRENT: u32 = 1;
pub const REPOSITORY: &str = "Baz-Studios-LLC/DramaFactory";
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}
pub fn version(tag: &str) -> Option<u32> {
    let digits = tag.strip_prefix('v')?;
    if digits.len() < 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|v| *v > 0)
}
pub fn check() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<String> {
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(15)))
                .build();
            let agent: ureq::Agent = config.into();
            let response = agent
                .get(&format!(
                    "https://api.github.com/repos/{REPOSITORY}/releases/latest"
                ))
                .header("User-Agent", "DramaFactory-v001")
                .header("Accept", "application/vnd.github+json")
                .call();
            let mut response = match response {
                Ok(response) => response,
                Err(ureq::Error::StatusCode(404)) => {
                    return Ok(
                        "No published releases are available yet. Development build: v001.".into(),
                    );
                }
                Err(error) => return Err(error.into()),
            };
            let release: Release = response.body_mut().read_json()?;
            let latest = version(&release.tag_name)
                .ok_or_else(|| anyhow::anyhow!("Release has an invalid version label"))?;
            if release.draft || release.prerelease {
                anyhow::bail!("No stable release available");
            }
            Ok(if latest > CURRENT {
                format!(
                    "{} is available. Automatic installation awaits signed release packages.",
                    release.tag_name
                )
            } else {
                "You are running the latest published version.".into()
            })
        })();
        let _ = tx.send(match result {
            Ok(s) => s,
            Err(e) => format!("Update check unavailable: {e}"),
        });
    });
    rx
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_number_versions() {
        assert_eq!(version("v001"), Some(1));
        assert_eq!(version("v010"), Some(10));
        assert_eq!(version("v1000"), Some(1000));
        for invalid in ["v000", "v1", "1.0.0", "v001-beta", "v001.1", "v-01"] {
            assert_eq!(version(invalid), None);
        }
    }
}
