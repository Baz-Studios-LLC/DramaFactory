//! Local release drafts. No network requests or publishing credentials.
use crate::{Store, production::Asset};
use anyhow::{Result, ensure};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

pub const PLATFORMS: [&str; 3] = ["TikTok", "YouTube Shorts", "Instagram"];
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ReleaseDraft {
    pub asset_id: Option<String>,
    pub title: String,
    pub captions: [String; 3],
    pub destinations: [bool; 3],
    // Embedded artwork travels with portable projects, without machine-specific paths.
    pub cover_png: Vec<u8>,
}
impl Store {
    pub fn finished_episode_copy(
        &self,
        filename: &str,
        source: &std::path::Path,
        hash: &str,
    ) -> Result<std::path::PathBuf> {
        let studio = directories::ProjectDirs::from("studio", "DramaFactory", "DramaFactory");
        let root = if studio
            .as_ref()
            .is_some_and(|dirs| dirs.data_dir() == self.root())
        {
            directories::UserDirs::new()
                .and_then(|dirs| dirs.video_dir().map(std::path::Path::to_owned))
                .ok_or_else(|| anyhow::anyhow!("Movies folder unavailable"))?
                .join("DramaFactory")
        } else {
            self.root().to_owned()
        };
        let folder = root.join("Finished Episodes");
        std::fs::create_dir_all(&folder)?;
        let stem = filename.strip_suffix(".mp4").unwrap_or(filename);
        let mut temp = None;
        for version in 1.. {
            let path = folder.join(if version == 1 {
                filename.to_owned()
            } else {
                format!("{stem}_v{version}.mp4")
            });
            if path.exists() {
                if path.is_file() && crate::worker::hash_file(&path)? == hash {
                    return Ok(path);
                }
                continue;
            }
            if temp.is_none() {
                let copy = tempfile::NamedTempFile::new_in(&folder)?;
                std::fs::copy(source, copy.path())?;
                ensure!(
                    crate::worker::hash_file(copy.path())? == hash,
                    "Export copy failed verification"
                );
                temp = Some(copy);
            }
            match temp.take().unwrap().persist_noclobber(&path) {
                Ok(_) => return Ok(path),
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    temp = Some(error.file)
                }
                Err(error) => return Err(error.error.into()),
            }
        }
        unreachable!()
    }
    pub fn episode_video_filename(&self, episode: &str) -> Result<String> {
        let record = self.get_episode(episode)?;
        let name: String = self.conn.query_row(
            "SELECT name FROM series WHERE id=?1",
            [&record.series_id],
            |row| row.get(0),
        )?;
        let index = self
            .episodes(&record.series_id)?
            .iter()
            .position(|item| item.id == episode)
            .ok_or_else(|| anyhow::anyhow!("Episode missing from series"))?;
        let stem = name
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        let mut stem = stem.as_str();
        while stem.len() > 200 {
            stem = &stem[..stem.char_indices().next_back().unwrap().0];
        }
        Ok(format!(
            "{}_Episode_{}.mp4",
            if stem.is_empty() { "Series" } else { stem },
            index + 1
        ))
    }
    pub fn release_draft(&self, episode: &str) -> Result<(ReleaseDraft, Option<String>)> {
        self.get_episode(episode)?;
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("episode-publish:{episode}")],
                |r| r.get(0),
            )
            .optional()?;
        let draft = raw
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        Ok((draft, raw))
    }
    pub fn save_release_draft(
        &self,
        episode: &str,
        draft: &ReleaseDraft,
        expected: Option<&str>,
    ) -> Result<String> {
        self.get_episode(episode)?;
        let value = serde_json::to_string(draft)?;
        let key = format!("episode-publish:{episode}");
        let changed = if let Some(expected) = expected {
            self.conn.execute(
                "UPDATE settings SET value=?1 WHERE key=?2 AND value=?3",
                rusqlite::params![value, key, expected],
            )?
        } else {
            self.conn.execute(
                "INSERT OR IGNORE INTO settings(key,value) VALUES(?1,?2)",
                rusqlite::params![key, value],
            )?
        };
        ensure!(
            changed == 1,
            "Release draft changed elsewhere. Reload before editing again."
        );
        Ok(value)
    }
    pub fn episode_release_exports(&self, episode: &str) -> Result<Vec<Asset>> {
        self.get_episode(episode)?;
        Ok(self.conn.prepare(
            "SELECT a.id,a.job_id,a.path,a.hash,a.width,a.height,a.duration,a.metadata,a.review FROM assets a JOIN jobs j ON j.id=a.job_id WHERE j.kind='export' AND j.prompt='Editing room export' AND j.status='completed' AND json_extract(j.request,'$.episode_id')=?1 ORDER BY a.rowid DESC"
        )?.query_map([episode], crate::production::asset_row)?.collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finished_copies_preserve_versions_and_verify_content() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(dir.path().join("studio.sqlite"))?;
        let source = dir.path().join("export.mp4");
        std::fs::write(&source, b"first export")?;
        let hash = crate::worker::hash_file(&source)?;
        let first = store.finished_episode_copy("Test_Episode_1.mp4", &source, &hash)?;
        assert_eq!(
            store.finished_episode_copy("Test_Episode_1.mp4", &source, &hash)?,
            first
        );
        std::fs::write(&source, b"revised export")?;
        let revised_hash = crate::worker::hash_file(&source)?;
        let revised = store.finished_episode_copy("Test_Episode_1.mp4", &source, &revised_hash)?;
        assert_eq!(revised.file_name().unwrap(), "Test_Episode_1_v2.mp4");
        assert_eq!(std::fs::read(first)?, b"first export");
        assert_eq!(std::fs::read(revised)?, b"revised export");
        assert!(
            store
                .finished_episode_copy("Bad.mp4", &source, &hash)
                .is_err()
        );
        assert!(!store.root().join("Finished Episodes/Bad.mp4").exists());
        Ok(())
    }

    #[test]
    fn release_survives_reopen_and_rejects_stale_saves() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("df-release-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("studio.sqlite");
        let store = Store::open(path.clone())?;
        let series = store.create_series("Test", "")?;
        let episode = store.create_episode(&series.id, "Episode")?;
        let mut draft = ReleaseDraft {
            title: "Release".into(),
            ..Default::default()
        };
        let first = store.save_release_draft(&episode.id, &draft, None)?;
        draft.captions[0] = "TikTok caption".into();
        store.save_release_draft(&episode.id, &draft, Some(&first))?;
        assert!(
            store
                .save_release_draft(&episode.id, &draft, Some(&first))
                .is_err()
        );
        drop(store);
        let reopened = Store::open(path)?;
        assert_eq!(
            reopened.release_draft(&episode.id)?.0.captions[0],
            "TikTok caption"
        );
        drop(reopened);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
