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
