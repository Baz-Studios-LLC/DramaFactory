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
    #[serde(default = "default_duration")]
    pub duration: u32,
    #[serde(default = "default_resolution")]
    pub resolution: String,
    pub mode: u8,
    pub first: String,
    pub last: String,
    pub references: String,
    #[serde(default)]
    pub cast: Vec<crate::cast_reference::CastReference>,
}
fn default_duration() -> u32 {
    10
}
fn default_resolution() -> String {
    "720p".into()
}
impl Default for SegmentDraft {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: String::new(),
            duration: default_duration(),
            resolution: "480p".into(),
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
        Ok(VideoPlan {
            model: "bytedance/seedance-2.5".into(),
            prompt: self.prompt.clone(),
            duration: 30,
            automatic_duration: false,
            resolution: self.resolution.clone(),
            aspect_ratio: "9:16".into(),
            generate_audio: true,
            inputs: match self.mode {
                0 => Inputs::Prompt,
                1 => Inputs::Portraits {
                    portraits: self.cast.clone(),
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
    pub(crate) fn current_plan_cast(&self, plan: &SavedPlan) -> Result<SavedPlan> {
        let mut current = plan.clone();
        let mut references = std::collections::HashMap::new();
        for segment in &mut current.segments {
            for reference in &mut segment.cast {
                if !references.contains_key(&reference.character_id) {
                    let selected = self
                        .prepare_cast_reference(&plan.episode_id, &reference.character_id)
                        .with_context(|| {
                            format!(
                                "{}: select an available portrait on the character card",
                                reference.name
                            )
                        })?;
                    references.insert(reference.character_id.clone(), selected);
                }
                *reference = references[&reference.character_id].clone();
            }
        }
        Ok(current)
    }
    pub fn save_generation_plan(&self, plan: &SavedPlan) -> Result<SavedPlan> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let saved = self.save_generation_plan_in_transaction(plan)?;
        tx.commit()?;
        Ok(saved)
    }
    pub(crate) fn save_generation_plan_in_transaction(
        &self,
        plan: &SavedPlan,
    ) -> Result<SavedPlan> {
        let current_cast = self.current_plan_cast(plan)?;
        let plan = &current_cast;
        let tx = &self.conn;
        if plan.segments.is_empty()
            || plan.segments.len() > 120
            || plan.segments.iter().any(|segment| {
                segment.mode > 2
                    || !(4..=30).contains(&segment.duration)
                    || !["480p", "720p", "1080p"].contains(&segment.resolution.as_str())
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
    fn generation_uses_current_cast_and_preserves_earlier_job_snapshots() {
        let root = std::env::temp_dir().join(format!("df-current-cast-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("db.sqlite")).unwrap();
        let series = store.create_series("Series", "Premise").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let old = root.join("old.png");
        let new = root.join("new.png");
        image::RgbaImage::new(16, 16).save(&old).unwrap();
        image::RgbaImage::new(20, 20).save(&new).unwrap();
        let entry = store
            .add_library(&series.id, "character", "Actor", "Original bio", Some(&old))
            .unwrap();
        let mut draft = store.generation_plan(&episode.id).unwrap();
        draft.segments[0].prompt = "A character walks".into();
        draft.segments[0].mode = 1;
        draft.segments[0].cast.push(
            store
                .prepare_cast_reference(&episode.id, &entry.id)
                .unwrap(),
        );
        let catalog = crate::openrouter::Catalog {
            videos: vec![
                serde_json::json!({"id":"bytedance/seedance-2.5","supported_durations":[10,30],"supported_resolutions":["480p"],"supported_aspect_ratios":["9:16"],"generate_audio":true}),
            ],
            images: vec![],
            fetched_unix_seconds: 0,
        };
        let (saved, first) = store.save_and_enqueue_segment(&draft, 0, &catalog).unwrap();
        let original: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [&first.id], |r| {
                r.get(0)
            })
            .unwrap();
        store.conn.execute("UPDATE library SET reference=?1,name='New name',description='Updated bio' WHERE id=?2",params![new.to_string_lossy(),entry.id]).unwrap();
        let (updated, second) = store.save_and_enqueue_segment(&saved, 0, &catalog).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(updated.segments[0].cast[0].path, new);
        assert_eq!(updated.segments[0].cast[0].brief, "Updated bio");
        assert_eq!(updated.segments[0].cast[0].name, "New name");
        let unchanged: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [&first.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(original, unchanged);
        store
            .conn
            .execute("UPDATE library SET reference=NULL WHERE id=?1", [&entry.id])
            .unwrap();
        assert!(
            store
                .save_and_enqueue_segment(&updated, 0, &catalog)
                .is_err()
        );
        assert_eq!(store.jobs().unwrap().len(), 2);
        assert_eq!(
            store.generation_plan(&episode.id).unwrap().revision,
            updated.revision
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn legacy_profiles_keep_defaults_and_custom_profiles_reach_the_video_request() {
        let new = SegmentDraft::default().video_plan().unwrap();
        assert_eq!(new.resolution, "480p");
        assert_eq!(new.aspect_ratio, "9:16");
        let legacy:SegmentDraft=serde_json::from_value(serde_json::json!({"id":"legacy","prompt":"A person walks and speaks","mode":0,"first":"","last":"","references":""})).unwrap();
        assert_eq!(legacy.duration, 10);
        assert_eq!(legacy.resolution, "720p");
        let mut updated = legacy;
        updated.duration = 24;
        updated.resolution = "480p".into();
        let plan = updated.video_plan().unwrap();
        assert_eq!(plan.duration, 30);
        assert!(!plan.automatic_duration);
        assert_eq!(plan.resolution, "480p");
        assert!(plan.generate_audio);
    }
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
        first.segments[0].duration = 24;
        first.segments[0].resolution = "1080p".into();
        let mut invalid = first.clone();
        invalid.segments[0].duration = 31;
        assert!(store.save_generation_plan(&invalid).is_err());
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
        assert_eq!(
            reopened.generation_plan(&episode.id).unwrap().segments[0].duration,
            24
        );
        assert_eq!(
            reopened.generation_plan(&episode.id).unwrap().segments[0].resolution,
            "1080p"
        );
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
