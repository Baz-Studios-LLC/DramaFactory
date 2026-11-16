//! Guarded episode draft edits and atomic outline/screenplay saves.
use crate::{
    store::Store,
    story::{EpisodeOutline, SeriesPlan},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize)]
pub struct EpisodeDraft {
    pub series_id: String,
    pub episode_id: String,
    pub screenplay: String,
    pub expected_screenplay: String,
    pub plan: SeriesPlan,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeEdit {
    pub episode_id: String,
    pub screenplay: Option<String>,
    pub outline: Option<EpisodeOutline>,
}
pub fn same(a: &EpisodeDraft, b: &EpisodeDraft) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}
pub fn apply(
    store: &Store,
    current: &EpisodeDraft,
    expected: &EpisodeDraft,
    edit: &EpisodeEdit,
) -> Result<EpisodeDraft> {
    if !same(current, expected) || edit.episode_id != expected.episode_id {
        bail!(
            "Episode changed while the sidekick was replying. Your newer work was preserved; proposal retained in chat."
        );
    }
    let saved = store.get_episode(&expected.episode_id)?;
    if saved.series_id != expected.series_id
        || saved.screenplay != expected.expected_screenplay
        || store.series_plan(&expected.series_id)?.revision != expected.plan.revision
    {
        bail!("Saved episode or overall plot changed; reload before applying this proposal");
    }
    let mut next = current.clone();
    if let Some(script) = &edit.screenplay {
        if script.len() > 128 * 1024 {
            bail!("Screenplay proposal exceeds 128 KiB");
        }
        next.screenplay = script.clone();
    }
    if let Some(outline) = &edit.outline {
        if outline.episode_id != expected.episode_id
            || serde_json::to_vec(outline)?.len() > 64 * 1024
        {
            bail!("Outline must target this episode and fit within 64 KiB");
        }
        replace_outline(&mut next.plan, &next.episode_id, Some(outline.clone()));
    }
    Ok(next)
}
pub fn replace_outline(plan: &mut SeriesPlan, id: &str, outline: Option<EpisodeOutline>) {
    if let Some(index) = plan.episodes.iter().position(|o| o.episode_id == id) {
        if let Some(outline) = outline {
            plan.episodes[index] = outline;
        } else {
            plan.episodes.remove(index);
        }
    } else if let Some(outline) = outline {
        plan.episodes.push(outline);
    }
}
pub fn undo(
    current: &EpisodeDraft,
    before: &EpisodeDraft,
    after: &EpisodeDraft,
) -> Result<EpisodeDraft> {
    if !same(current, after) {
        bail!("Episode changed after this edit; Undo would overwrite newer work");
    }
    Ok(before.clone())
}
impl Store {
    pub fn save_episode_draft(&self, draft: &EpisodeDraft) -> Result<u64> {
        if draft.plan.series_id != draft.series_id {
            bail!("Episode draft has a mismatched series plan");
        }
        let tx = self.conn.unchecked_transaction()?;
        let saved = self.get_episode(&draft.episode_id)?;
        let mut plan = self.series_plan(&draft.series_id)?;
        if saved.series_id != draft.series_id
            || saved.screenplay != draft.expected_screenplay
            || plan.revision != draft.plan.revision
        {
            bail!(
                "Episode or overall plot changed elsewhere. Your draft is preserved; nothing was saved."
            );
        }
        let outline = draft
            .plan
            .episodes
            .iter()
            .find(|o| o.episode_id == draft.episode_id)
            .cloned();
        let old = plan
            .episodes
            .iter()
            .find(|o| o.episode_id == draft.episode_id);
        let changed = serde_json::to_value(old)? != serde_json::to_value(&outline)?;
        if changed {
            replace_outline(&mut plan, &draft.episode_id, outline);
            plan = self.save_series_plan_in_transaction(&plan)?;
        }
        self.save_screenplay_if_unchanged(
            &draft.episode_id,
            &draft.expected_screenplay,
            &draft.screenplay,
        )?;
        tx.commit()?;
        Ok(plan.revision)
    }
}
pub fn response_schema() -> serde_json::Value {
    use serde_json::json;
    let mut fields = serde_json::Map::new();
    for key in [
        "episode_id",
        "goal",
        "conflict",
        "turn",
        "payoff",
        "hook",
        "ending_resolution",
    ] {
        fields.insert(key.into(), json!({"type":"string"}));
    }
    fields.insert("requires".into(),json!({"type":"array","items":{"type":"object","additionalProperties":false,"required":["key","value"],"properties":{"key":{"type":"string"},"value":{"type":"string"}}}}));
    fields.insert("changes".into(),json!({"type":"array","items":{"type":"object","additionalProperties":false,"required":["key","from","to"],"properties":{"key":{"type":"string"},"from":{"anyOf":[{"type":"null"},{"type":"string"}]},"to":{"type":"string"}}}}));
    let required: Vec<_> = fields.keys().cloned().collect();
    json!({"type":"object","additionalProperties":false,"required":["episode_id","screenplay","outline"],"properties":{
        "episode_id":{"type":"string"},"screenplay":{"anyOf":[{"type":"null"},{"type":"string"}]},
        "outline":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":required,"properties":fields}]}
    }})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn episode_edit_save_undo_and_conflicts_preserve_other_story_work() {
        let root = std::env::temp_dir().join(format!("df-episode-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("test.sqlite")).unwrap();
        let series = store.create_series("Series", "Premise").unwrap();
        let episode = store.create_episode(&series.id, "Pilot").unwrap();
        let other = store.create_episode(&series.id, "Second").unwrap();
        let mut plan = store.series_plan(&series.id).unwrap();
        plan.theme = "Unsaved overall theme".into();
        let before = EpisodeDraft {
            series_id: series.id.clone(),
            episode_id: episode.id.clone(),
            screenplay: String::new(),
            expected_screenplay: String::new(),
            plan,
        };
        let edit = EpisodeEdit {
            episode_id: episode.id.clone(),
            screenplay: Some("INT. SHOP — DAY\nSYLVI\nWe open today.".into()),
            outline: Some(EpisodeOutline {
                episode_id: episode.id.clone(),
                goal: "Open the shop".into(),
                turn: "First customer arrives".into(),
                ..Default::default()
            }),
        };
        let after = apply(&store, &before, &before, &edit).unwrap();
        assert!(
            store
                .get_episode(&episode.id)
                .unwrap()
                .screenplay
                .is_empty()
        );
        assert!(same(&undo(&after, &before, &after).unwrap(), &before));
        let mut manual = after.clone();
        manual.screenplay.push_str("\nA manual edit.");
        assert!(undo(&manual, &before, &after).is_err());
        assert!(apply(&store, &manual, &before, &edit).is_err());
        let mut wrong = edit.clone();
        wrong.episode_id = other.id.clone();
        assert!(apply(&store, &before, &before, &wrong).is_err());
        wrong = edit.clone();
        wrong.outline.as_mut().unwrap().episode_id = other.id;
        assert!(apply(&store, &before, &before, &wrong).is_err());
        let revision = store.save_episode_draft(&after).unwrap();
        let saved = store.series_plan(&series.id).unwrap();
        assert_eq!(revision, 1);
        assert!(
            saved.theme.is_empty(),
            "Episode save must not save unrelated overall plot changes"
        );
        assert_eq!(saved.episodes[0].goal, "Open the shop");
        assert_eq!(
            store.get_episode(&episode.id).unwrap().screenplay,
            after.screenplay
        );
        assert!(store.save_episode_draft(&before).is_err());
        let mut current = after.clone();
        current.plan.revision = revision;
        current.expected_screenplay = current.screenplay.clone();
        current.plan.episodes[0].goal = "Another goal".into();
        store.save_screenplay(&episode.id, "External edit").unwrap();
        assert!(store.save_episode_draft(&current).is_err());
        assert_eq!(
            store.series_plan(&series.id).unwrap().episodes[0].goal,
            "Open the shop"
        );
        assert!(apply(&store, &current, &current, &edit).is_err());
        current.expected_screenplay = "External edit".into();
        store.conn.execute_batch("CREATE TEMP TRIGGER reject_episode_write BEFORE UPDATE ON episodes BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;").unwrap();
        assert!(store.save_episode_draft(&current).is_err());
        assert_eq!(store.series_plan(&series.id).unwrap().revision, 1);
        assert_eq!(
            store.series_plan(&series.id).unwrap().episodes[0].goal,
            "Open the shop"
        );
        store
            .conn
            .execute_batch("DROP TRIGGER reject_episode_write;")
            .unwrap();
        drop(store);
        let reopened = Store::open(root.join("test.sqlite")).unwrap();
        assert_eq!(
            reopened.get_episode(&episode.id).unwrap().screenplay,
            "External edit"
        );
        assert_eq!(reopened.series_plan(&series.id).unwrap().episodes.len(), 1);
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn response_preserves_null_fields_and_rejects_two_plot_actions() {
        let reply=crate::sidekick_edit::parse_reply(r#"{"message":"Ready","synopsis":null,"episode":{"episode_id":"ep","screenplay":"Scene","outline":null}}"#).unwrap();
        assert_eq!(reply.episode.unwrap().screenplay.as_deref(), Some("Scene"));
        assert!(crate::sidekick_edit::parse_reply(r#"{"message":"Ready","episode":{"episode_id":"ep","screenplay":"Scene","outline":null},"plot":{}}"#).is_err());
        assert!(crate::sidekick_edit::retained_reply(r#"{"message":"Ready","episode":{"episode_id":"ep","screenplay":"Scene","outline":null}}"#,true).unwrap().episode.is_none());
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewEpisode {
    pub title: String,
    pub screenplay: String,
    pub goal: String,
    pub conflict: String,
    pub turn: String,
    pub payoff: String,
    pub hook: String,
    pub ending_resolution: String,
    pub requires: Vec<crate::story::Fact>,
    pub changes: Vec<crate::story::Change>,
}
impl NewEpisode {
    fn outline(&self, id: &str) -> EpisodeOutline {
        EpisodeOutline {
            episode_id: id.into(),
            goal: self.goal.clone(),
            conflict: self.conflict.clone(),
            turn: self.turn.clone(),
            payoff: self.payoff.clone(),
            hook: self.hook.clone(),
            ending_resolution: self.ending_resolution.clone(),
            requires: self.requires.clone(),
            changes: self.changes.clone(),
        }
    }
}
impl Store {
    /// Create authored episodes together; never queue media or overwrite existing episodes.
    pub fn create_sidekick_episodes(
        &self,
        series: &str,
        request_id: &str,
        expected_revision: u64,
        proposals: &[NewEpisode],
    ) -> Result<Vec<String>> {
        use rusqlite::OptionalExtension;
        if proposals.is_empty() || proposals.len() > 5 {
            bail!("Create one to five episodes per request");
        }
        let payload = serde_json::to_string(proposals)?;
        if payload.len() > 512 * 1024 {
            bail!("Episode creation payload is too large");
        }
        let key = format!("sidekick:episodes:{request_id}");
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let prior: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key=?1", [&key], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(prior) = prior {
            let prior: serde_json::Value = serde_json::from_str(&prior)?;
            if prior["series"] != series || prior["payload"] != payload {
                bail!("Episode request identity was reused with different content");
            }
            return Ok(serde_json::from_value(prior["ids"].clone())?);
        }
        let mut plan = self.series_plan(series)?;
        if plan.revision != expected_revision {
            bail!(
                "Overall plot changed while episodes were being developed; proposal retained for review"
            );
        }
        let existing = self.episodes(series)?;
        let mut titles: std::collections::HashSet<_> = existing
            .iter()
            .map(|e| e.title.trim().to_lowercase())
            .collect();
        for proposal in proposals {
            if proposal.title.trim().is_empty()
                || proposal.title.len() > 200
                || proposal.screenplay.trim().is_empty()
                || proposal.screenplay.len() > 128 * 1024
                || [
                    &proposal.goal,
                    &proposal.conflict,
                    &proposal.turn,
                    &proposal.payoff,
                ]
                .iter()
                .any(|text| text.trim().is_empty())
            {
                bail!("New episodes need a title, complete story beats and a screenplay");
            }
            if !titles.insert(proposal.title.trim().to_lowercase()) {
                bail!(
                    "An episode with this title already exists; edit it instead of creating a duplicate"
                );
            }
        }
        let mut ids = Vec::new();
        for proposal in proposals {
            let episode = self.create_episode(series, &proposal.title)?;
            self.save_screenplay_if_unchanged(&episode.id, "", &proposal.screenplay)?;
            plan.episodes.push(proposal.outline(&episode.id));
            ids.push(episode.id);
        }
        self.save_series_plan_in_transaction(&plan)?;
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)",
            rusqlite::params![
                key,
                serde_json::json!({"series":series,"payload":payload,"ids":ids}).to_string()
            ],
        )?;
        tx.commit()?;
        Ok(ids)
    }
}
pub fn creation_schema() -> serde_json::Value {
    use serde_json::json;
    let mut outline = response_schema()["properties"]["outline"]["anyOf"][1].clone();
    let fields = outline["properties"]
        .as_object_mut()
        .expect("outline schema");
    fields.remove("episode_id");
    fields.insert("title".into(), json!({"type":"string","minLength":1}));
    fields.insert("screenplay".into(), json!({"type":"string","minLength":1}));
    outline["required"] = json!(fields.keys().cloned().collect::<Vec<_>>());
    json!({"type":"array","minItems":1,"maxItems":5,"items":outline})
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    fn proposal(title: &str) -> NewEpisode {
        NewEpisode {
            title: title.into(),
            screenplay: "INT. SHOP — DAY\nThe door opens.".into(),
            goal: "Open the shop".into(),
            conflict: "The permit is missing".into(),
            turn: "A rival arrives".into(),
            payoff: "The first sale".into(),
            hook: "A summons".into(),
            ending_resolution: String::new(),
            requires: vec![],
            changes: vec![],
        }
    }
    #[test]
    fn episode_creation_is_atomic_idempotent_and_does_not_queue_media() {
        let root = std::env::temp_dir().join(format!("df-episode-create-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("test.sqlite")).unwrap();
        let series = store.create_series("Series", "Premise").unwrap();
        let batch = vec![proposal("First"), proposal("Second")];
        let ids = store
            .create_sidekick_episodes(&series.id, "request", 0, &batch)
            .unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(
            store
                .create_sidekick_episodes(&series.id, "request", 0, &batch)
                .unwrap(),
            ids
        );
        assert_eq!(store.episodes(&series.id).unwrap().len(), 2);
        assert_eq!(
            store.series_plan(&series.id).unwrap().episodes[0].episode_id,
            ids[0]
        );
        assert!(!store.get_episode(&ids[0]).unwrap().screenplay.is_empty());
        assert!(store.jobs().unwrap().is_empty());
        assert!(
            store
                .create_sidekick_episodes(&series.id, "stale", 0, &[proposal("Third")])
                .is_err()
        );
        assert!(
            store
                .create_sidekick_episodes(&series.id, "duplicate", 1, &[proposal("FIRST")])
                .is_err()
        );
        let mut invalid = proposal("Fourth");
        invalid.screenplay.clear();
        assert!(
            store
                .create_sidekick_episodes(&series.id, "partial", 1, &[proposal("Third"), invalid])
                .is_err()
        );
        assert_eq!(store.episodes(&series.id).unwrap().len(), 2);
        let other = store.create_series("Other", "").unwrap();
        assert!(
            store
                .create_sidekick_episodes(&other.id, "request", 0, &batch)
                .is_err()
        );
        assert!(
            creation_schema()["items"]["properties"]
                .get("episode_id")
                .is_none()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
