//! Sidekick prepares editable segment plans; this module never queues media.
use crate::{Studio, generation_plan::SavedPlan};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub plan: SavedPlan,
    pub context: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub episode_id: String,
    pub segments: Vec<Segment>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub id: String,
    pub prompt: String,
    pub duration: u32,
    pub resolution: String,
    #[serde(default)]
    pub input_mode: Option<String>,
    #[serde(default)]
    pub character_ids: Option<Vec<String>>,
}
pub fn same(a: &Snapshot, b: &Snapshot) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}
pub fn apply(current: &Snapshot, expected: &Snapshot, edit: &Edit) -> Result<SavedPlan> {
    if !same(current, expected) || edit.episode_id != current.plan.episode_id {
        bail!(
            "Episode, source material or generation plan changed during the reply. Your newer work was preserved."
        );
    }
    if edit.segments.is_empty() || edit.segments.len() > 120 {
        bail!("A generation plan needs 1–120 segments");
    }
    let mut next = current.plan.clone();
    let mut seen = std::collections::HashSet::new();
    next.segments = edit
        .segments
        .iter()
        .map(|proposal| {
            if proposal.prompt.trim().is_empty()
                || proposal.prompt.len() > 65536
                || !(4..=30).contains(&proposal.duration)
                || !["480p", "720p", "1080p"].contains(&proposal.resolution.as_str())
            {
                bail!("Invalid segment prompt, duration or resolution");
            }
            let mut segment = if proposal.id.is_empty() {
                Default::default()
            } else {
                if !seen.insert(proposal.id.clone()) {
                    bail!("Duplicate segment ID");
                }
                current
                    .plan
                    .segments
                    .iter()
                    .find(|s| s.id == proposal.id)
                    .cloned()
                    .context("Unknown segment ID")?
            };
            segment.prompt = proposal.prompt.clone();
            segment.duration = proposal.duration;
            segment.resolution = proposal.resolution.clone();
            if let Some(mode) = &proposal.input_mode {
                segment.mode = match mode.as_str() {
                    "prompt" => 0,
                    "references" => 1,
                    _ => bail!("Unknown reference mode"),
                };
            }
            if let Some(ids) = &proposal.character_ids {
                if ids.len() > 12 { bail!("Select at most 12 character portraits"); }
                if proposal.input_mode.as_deref() == Some("prompt") && !ids.is_empty() {
                    bail!("Portrait selections require reference images mode");
                }
                let available: Vec<crate::cast_reference::CastReference> = serde_json::from_value(
                    current.context.get("available_portraits").cloned().unwrap_or(serde_json::json!([]))
                )?;
                let mut unique = std::collections::HashSet::new();
                segment.cast = ids.iter().map(|id| {
                    if !unique.insert(id) { bail!("Duplicate character selection"); }
                    available.iter().find(|reference| &reference.character_id == id).cloned()
                        .context("Character portrait unavailable. Choose a saved portrait in Characters first.")
                }).collect::<Result<Vec<_>>>()?;
                if !ids.is_empty() { segment.mode = 1; }
            }
            Ok(segment)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(next)
}
pub fn schema() -> serde_json::Value {
    serde_json::json!({"type":"object","additionalProperties":false,"required":["episode_id","segments"],"properties":{"episode_id":{"type":"string"},"segments":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","prompt","duration","resolution","input_mode","character_ids"],"properties":{"id":{"type":"string"},"prompt":{"type":"string"},"duration":{"type":"integer"},"resolution":{"type":"string","enum":["480p","720p","1080p"]},"input_mode":{"type":["string","null"],"enum":["prompt","references",null]},"character_ids":{"type":["array","null"],"items":{"type":"string"}}}}}}})
}
impl Studio {
    pub(crate) fn generation_snapshot(&mut self) -> Result<Snapshot> {
        let series = self.selected.clone().context("Choose a series")?;
        let episode = self.episode_id.clone().context("Choose an episode")?;
        let saved_episode = self.store.get_episode(&episode)?;
        if saved_episode.series_id != series {
            bail!("Episode belongs to another series");
        }
        let saved_plan = self.store.generation_plan(&episode)?;
        let plan = self
            .video_planner
            .drafts
            .entry(episode.clone())
            .or_insert_with(|| saved_plan.clone())
            .clone();
        let available_portraits: Vec<_> = self
            .store
            .library(&series, "character")?
            .iter()
            .filter_map(|entry| self.store.prepare_cast_reference(&episode, &entry.id).ok())
            .collect();
        let context = serde_json::json!({
            "available_portraits":available_portraits,
            "series_id":series,
            "saved_episode":saved_episode,
            "screenplay_draft":self.script,
            "saved_generation_revision":saved_plan.revision,
            "synopsis":self.synopsis_drafts.get(&series),
            "saved_series":self.store.series()?.into_iter().find(|s|s.id==series),
            "plot_draft":self.story_drafts.get(&series),
            "saved_plot":self.store.series_plan(&series)?,
            "characters":self.character_drafts(&series)?.iter().map(crate::character_edit::CharacterDraft::model_context).collect::<Vec<_>>(),
            "locations":self.store.library(&series,"location")?,
            "image_style":self.store.series_portrait_style(&series)?,
        });
        Ok(Snapshot { plan, context })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_preserves_inputs_and_guards_context_ids_and_newer_edits() {
        let mut segment = crate::generation_plan::SegmentDraft {
            prompt: "Old".into(),
            mode: 2,
            first: "https://example.com/frame.png".into(),
            ..Default::default()
        };
        let current = Snapshot {
            plan: SavedPlan {
                episode_id: "episode".into(),
                revision: 3,
                segments: vec![segment.clone()],
            },
            context: serde_json::json!({"screenplay":"INT. SHOP — DAY\nSYLVI: Welcome."}),
        };
        let mut edit = Edit {
            episode_id: "episode".into(),
            segments: vec![Segment {
                id: segment.id.clone(),
                prompt: "Sylvi says: Welcome.".into(),
                duration: 10,
                resolution: "480p".into(),
                input_mode: None,
                character_ids: None,
            }],
        };
        let mut with_portraits = current.clone();
        with_portraits.context["available_portraits"] = serde_json::json!([{
            "character_id":"sylvi", "name":"Sylvi", "brief":"Saved profile", "path":"/portrait.png", "sha256":"hash"
        }]);
        edit.segments[0].input_mode = Some("references".into());
        edit.segments[0].character_ids = Some(vec!["sylvi".into()]);
        let linked = apply(&with_portraits, &with_portraits, &edit).unwrap();
        assert_eq!(linked.segments[0].mode, 1);
        assert_eq!(linked.segments[0].cast[0].character_id, "sylvi");
        assert!(apply(&current, &current, &edit).is_err());
        let mut changed_portrait = with_portraits.clone();
        changed_portrait.context["available_portraits"][0]["sha256"] = "changed".into();
        assert!(apply(&changed_portrait, &with_portraits, &edit).is_err());
        edit.segments[0].character_ids = Some(vec!["sylvi".into(), "sylvi".into()]);
        assert!(apply(&with_portraits, &with_portraits, &edit).is_err());
        edit.segments[0].input_mode = None;
        edit.segments[0].character_ids = None;
        let next = apply(&current, &current, &edit).unwrap();
        assert_eq!(next.segments[0].first, segment.first);
        assert_eq!(next.segments[0].mode, 2);
        let mut changed = current.clone();
        changed.context["screenplay"] = "New dialogue".into();
        assert!(apply(&changed, &current, &edit).is_err());
        changed = current.clone();
        changed.plan.revision += 1;
        assert!(apply(&changed, &current, &edit).is_err());
        edit.segments[0].id = "foreign".into();
        assert!(apply(&current, &current, &edit).is_err());
        edit.segments[0].id.clear();
        let new = apply(&current, &current, &edit).unwrap();
        assert_ne!(new.segments[0].id, segment.id);
        assert_eq!(new.segments[0].resolution, "480p");
        segment.prompt = "New manual draft".into();
        changed = current.clone();
        changed.plan.segments[0] = segment;
        assert!(apply(&changed, &current, &edit).is_err());
        let schema = crate::sidekick_edit::response_format_for("Episode generation");
        assert!(schema["json_schema"]["schema"]["properties"]["generation"].is_object());
        assert!(schema["json_schema"]["schema"]["properties"]["episode"].is_null());
    }
    #[test]
    fn single_generate_saves_atomically_and_reuses_identical_request() {
        let root =
            std::env::temp_dir().join(format!("df-single-generate-{}", uuid::Uuid::new_v4()));
        let store = crate::store::Store::open(root.join("db.sqlite")).unwrap();
        let series = store.create_series("Series", "Premise").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let mut draft = store.generation_plan(&episode.id).unwrap();
        draft.segments[0].prompt = "First".into();
        let catalog = crate::openrouter::Catalog {
            videos: vec![
                serde_json::json!({"id":"bytedance/seedance-2.5","supported_durations":[10,30],"supported_resolutions":["480p"],"supported_aspect_ratios":["9:16"],"generate_audio":true}),
            ],
            images: vec![],
            fetched_unix_seconds: 0,
        };
        store.conn.execute_batch("CREATE TRIGGER fail_queue BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(store.save_and_enqueue_segment(&draft, 0, &catalog).is_err());
        assert_eq!(store.generation_plan(&episode.id).unwrap().revision, 0);
        assert!(store.jobs().unwrap().is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER fail_queue;")
            .unwrap();
        let (saved, job) = store.save_and_enqueue_segment(&draft, 0, &catalog).unwrap();
        assert_eq!(job.status, "queued");
        assert_eq!(job.attempts, 0);
        let mut stale = draft.clone();
        stale.segments[0].prompt = "Stale edit".into();
        assert!(store.save_and_enqueue_segment(&stale, 0, &catalog).is_err());
        let (_, again) = store.save_and_enqueue_segment(&saved, 0, &catalog).unwrap();
        assert_eq!(again.id, job.id);
        assert_eq!(store.jobs().unwrap().len(), 1);
        // A completed take must not swallow an intentional new generation.
        store
            .conn
            .execute("UPDATE jobs SET status='completed' WHERE id=?1", [&job.id])
            .unwrap();
        let (_, fresh) = store.save_and_enqueue_segment(&saved, 0, &catalog).unwrap();
        assert_ne!(fresh.id, job.id);
        let (_, repeated_click) = store.save_and_enqueue_segment(&saved, 0, &catalog).unwrap();
        assert_eq!(repeated_click.id, fresh.id);
        // An older request with automatic duration is not the new fixed request,
        // even when its authored segment snapshot and plan revision are unchanged.
        let mut old_request: serde_json::Value = serde_json::from_str(
            &store
                .conn
                .query_row("SELECT request FROM jobs WHERE id=?1", [&fresh.id], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
        )
        .unwrap();
        old_request["automatic_duration"] = true.into();
        store
            .conn
            .execute(
                "UPDATE jobs SET request=?1 WHERE id=?2",
                rusqlite::params![old_request.to_string(), fresh.id],
            )
            .unwrap();
        let (_, fixed) = store.save_and_enqueue_segment(&saved, 0, &catalog).unwrap();
        assert_ne!(fixed.id, fresh.id);
        let request: serde_json::Value = serde_json::from_str(
            &store
                .conn
                .query_row("SELECT request FROM jobs WHERE id=?1", [&fixed.id], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(request["duration"], 30);
        assert_eq!(request["automatic_duration"], false);

        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn batch_save_and_queue_are_atomic_idempotent_and_do_not_submit() {
        let root = std::env::temp_dir().join(format!("df-generation-{}", uuid::Uuid::new_v4()));
        let store = crate::store::Store::open(root.join("db.sqlite")).unwrap();
        let series = store.create_series("Series", "Premise").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let mut draft = store.generation_plan(&episode.id).unwrap();
        draft.segments[0].prompt = "First".into();
        draft.segments.push(crate::generation_plan::SegmentDraft {
            prompt: "Second".into(),
            ..Default::default()
        });
        let catalog = crate::openrouter::Catalog {
            videos: vec![
                serde_json::json!({"id":"bytedance/seedance-2.5","supported_durations":[10,30],"supported_resolutions":["480p"],"supported_aspect_ratios":["9:16"],"generate_audio":true}),
            ],
            images: vec![],
            fetched_unix_seconds: 0,
        };
        store.conn.execute_batch("CREATE TRIGGER fail_second BEFORE INSERT ON jobs WHEN NEW.prompt='Second' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        assert!(store.enqueue_generation_plan(&draft, &catalog).is_err());
        assert_eq!(store.generation_plan(&episode.id).unwrap().revision, 0);
        assert!(store.jobs().unwrap().is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER fail_second;")
            .unwrap();
        let (saved, count) = store.enqueue_generation_plan(&draft, &catalog).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            store.enqueue_generation_plan(&saved, &catalog).unwrap().1,
            0
        );
        assert_eq!(store.jobs().unwrap().len(), 2);
        assert!(
            store
                .jobs()
                .unwrap()
                .iter()
                .all(|job| job.status == "queued")
        );
        let submitted:i64=store.conn.query_row("SELECT count(*) FROM remote_video_jobs WHERE state!='prepared' OR remote_id IS NOT NULL",[],|r|r.get(0)).unwrap();
        assert_eq!(submitted, 0);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
