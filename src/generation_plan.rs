//! Revisioned authoring state. Saving never calls a provider or queues generation.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentDraft {
    #[serde(default)]
    pub id: String,
    pub prompt: String,
    pub mode: u8,
    pub first: String,
    pub last: String,
    pub references: String,
    #[serde(default)]
    pub cast: Vec<crate::cast_reference::CastReference>,
}
impl Default for SegmentDraft {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: String::new(),
            mode: 0,
            first: String::new(),
            last: String::new(),
            references: String::new(),
            cast: Vec::new(),
        }
    }
}
impl SegmentDraft {
    pub fn video_plan(&self) -> Result<crate::video_plan::VideoPlan> {
        use crate::video_plan::{Inputs, VideoPlan};
        if self.mode == 1 && !self.cast.is_empty() {
            bail!("Local character image transport is not connected yet; no request was queued");
        }
        Ok(VideoPlan {
            model: "bytedance/seedance-2.5".into(),
            prompt: self.prompt.clone(),
            duration: 10,
            resolution: "720p".into(),
            aspect_ratio: "9:16".into(),
            generate_audio: true,
            inputs: match self.mode {
                0 => Inputs::Prompt,
                1 => Inputs::References {
                    images: self
                        .references
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned)
                        .collect(),
                },
                2 => Inputs::Frames {
                    first: self.first.trim().into(),
                    last: if self.last.trim().is_empty() {
                        None
                    } else {
                        Some(self.last.trim().into())
                    },
                },
                _ => bail!("Unknown segment input mode"),
            },
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedPlan {
    pub episode_id: String,
    pub revision: u64,
    pub segments: Vec<SegmentDraft>,
}
impl Store {
    pub fn generation_plan(&self, episode: &str) -> Result<SavedPlan> {
        self.get_episode(episode)?;
        let row: Option<(u64, String)> = self
            .conn
            .query_row(
                "SELECT revision,data FROM generation_plans WHERE episode_id=?1",
                [episode],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let loaded: Result<SavedPlan> = match row {
            Some((revision, data)) => Ok(SavedPlan {
                episode_id: episode.into(),
                revision,
                segments: serde_json::from_str(&data)?,
            }),
            None => Ok(SavedPlan {
                episode_id: episode.into(),
                revision: 0,
                segments: vec![SegmentDraft::default()],
            }),
        };
        let mut plan = loaded?;
        for (index, segment) in plan.segments.iter_mut().enumerate() {
            if segment.id.is_empty() {
                segment.id = format!("legacy-{episode}-{index}");
            }
        }
        Ok(plan)
    }
    pub fn save_generation_plan(&self, plan: &SavedPlan) -> Result<SavedPlan> {
        if plan.segments.is_empty()
            || plan.segments.len() > 120
            || plan.segments.iter().any(|segment| {
                segment.mode > 2
                    || segment.prompt.len() > 65536
                    || segment.references.len() > 65536
                    || segment.first.len() > 8192
                    || segment.last.len() > 8192
            })
        {
            bail!("Generation plan exceeds supported authoring bounds");
        }
        let mut segment_ids = std::collections::HashSet::new();
        for segment in &plan.segments {
            if segment.id.is_empty() || segment.id.len() > 100 || !segment_ids.insert(&segment.id) {
                bail!("Segment IDs must be unique and nonempty");
            }
            if segment.cast.len() > 12 {
                bail!("Too many cast image links for this planner");
            }
            let mut ids = std::collections::HashSet::new();
            for reference in &segment.cast {
                if !ids.insert(&reference.character_id) {
                    bail!("Duplicate character reference");
                }
                self.validate_cast_reference(&plan.episode_id, reference)?;
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.query_row(
            "SELECT id FROM episodes WHERE id=?1",
            [&plan.episode_id],
            |row| row.get::<_, String>(0),
        )
        .context("Episode not found")?;
        let current: Option<(u64, String)> = tx
            .query_row(
                "SELECT revision,data FROM generation_plans WHERE episode_id=?1",
                [&plan.episode_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let data = serde_json::to_string(&plan.segments)?;
        if let Some((revision, saved)) = &current {
            if *saved == data {
                return Ok(SavedPlan {
                    revision: *revision,
                    ..plan.clone()
                });
            }
            if *revision != plan.revision {
                bail!(
                    "Generation plan changed elsewhere. Your draft is preserved; reload the saved plan before editing again."
                );
            }
        } else if plan.revision != 0 {
            bail!("Generation plan no longer exists; stale update rejected");
        }
        let revision = plan
            .revision
            .checked_add(1)
            .context("Plan revision limit reached")?;
        tx.execute("INSERT INTO generation_plans(episode_id,revision,data) VALUES(?1,?2,?3) ON CONFLICT(episode_id) DO UPDATE SET revision=excluded.revision,data=excluded.data",params![plan.episode_id,revision,data])?;
        tx.commit()?;
        Ok(SavedPlan {
            revision,
            ..plan.clone()
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_survive_restart_and_reject_stale_cross_process_edits() {
        let dir = std::env::temp_dir().join(format!("df-plans-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("Series", "Idea").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let other = store.create_episode(&series.id, "Two").unwrap();
        let mut first = store.generation_plan(&episode.id).unwrap();
        let mut stale = first.clone();
        first.segments[0].prompt = "First saved".into();
        let saved = store.save_generation_plan(&first).unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(store.save_generation_plan(&first).unwrap().revision, 1);
        stale.segments[0].prompt = "Stale overwrite".into();
        assert!(store.save_generation_plan(&stale).is_err());
        assert!(
            store.generation_plan(&other.id).unwrap().segments[0]
                .prompt
                .is_empty()
        );
        drop(store);
        let reopened = Store::open(path).unwrap();
        assert_eq!(
            reopened.generation_plan(&episode.id).unwrap().segments[0].prompt,
            "First saved"
        );
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
