//! Tab-independent, atomic episode-range authoring. Original state is retained for recovery.
use crate::{Studio, sidekick_episode::NewEpisode, story::EpisodeOutline};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

impl Studio {
    pub(crate) fn episode_batch_snapshot(&self, series: &str) -> Result<Value> {
        let episodes = self.store.ordered_gui_episodes(series)?;
        let saved_plan = self.store.series_plan(series)?;
        let plan = self.story_drafts.get(series).unwrap_or(&saved_plan);
        let episodes: Vec<_> = episodes.into_iter().enumerate().map(|(i,e)| {
            let selected = self.episode_id.as_deref() == Some(&e.id);
            json!({"number":i+1,"id":e.id,"title":e.title,"screenplay":if selected {&self.script} else {&e.screenplay},"saved_screenplay":e.screenplay})
        }).collect();
        Ok(
            json!({"series_id":series,"episodes":episodes,"plan":plan,"saved_plan":saved_plan,"episode_fields":crate::sidekick_episode::creation_schema()["items"]}),
        )
    }
    pub(crate) fn apply_episode_batch(&mut self, payload: &Value) -> Result<Value> {
        let series = payload["series_id"]
            .as_str()
            .context("series_id required")?;
        let current = self.episode_batch_snapshot(series)?;
        ensure!(
            current == payload["expected"],
            "Episodes or plot changed while authoring. Read the batch again; newer work was preserved."
        );
        let start = payload["start_episode"]
            .as_u64()
            .context("start_episode must be a one-based number")? as usize;
        let proposals: Vec<NewEpisode> = serde_json::from_value(payload["episodes"].clone())?;
        let existing = current["episodes"].as_array().unwrap();
        ensure!(
            start > 0
                && start <= existing.len() + 1
                && !proposals.is_empty()
                && proposals.len() <= 25,
            "Provide 1-25 consecutive episodes; do not skip missing episode numbers"
        );
        for p in &proposals {
            ensure!(
                !p.title.trim().is_empty()
                    && !p.screenplay.trim().is_empty()
                    && p.screenplay.len() <= 128 * 1024,
                "Each episode needs a title and a screenplay of at most 128 KiB"
            );
        }
        let directory = self.store.root().join("drafts");
        std::fs::create_dir_all(&directory)?;
        let backup = directory.join(format!(
            "episode-batch-before-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&backup, serde_json::to_vec_pretty(&current)?)?;
        let tx = self.store.conn.unchecked_transaction()?;
        // Recheck inside the transaction so another MCP writer cannot slip between reads and writes.
        ensure!(
            self.episode_batch_snapshot(series)? == current,
            "Episode state changed; batch not saved"
        );
        let mut plan = self.store.series_plan(series)?;
        // Establish explicit order for existing episodes that have no outline yet.
        for episode in existing {
            let id = episode["id"].as_str().unwrap();
            if !plan.episodes.iter().any(|o| o.episode_id == id) {
                plan.episodes.push(EpisodeOutline {
                    episode_id: id.into(),
                    ..Default::default()
                });
            }
        }
        let mut changed = Vec::new();
        for (offset, proposal) in proposals.iter().enumerate() {
            let number = start + offset;
            let id = if let Some(episode) = existing.get(number - 1) {
                let id = episode["id"].as_str().unwrap();
                self.store.conn.execute(
                    "UPDATE episodes SET title=?2 WHERE id=?1",
                    rusqlite::params![id, proposal.title],
                )?;
                self.store.save_screenplay_if_unchanged(
                    id,
                    episode["saved_screenplay"].as_str().unwrap(),
                    &proposal.screenplay,
                )?;
                id.to_string()
            } else {
                let episode = self.store.create_episode(series, &proposal.title)?;
                self.store
                    .save_screenplay(&episode.id, &proposal.screenplay)?;
                episode.id
            };
            let outline = proposal.outline(&id);
            crate::sidekick_episode::replace_outline(&mut plan, &id, Some(outline.clone()));
            changed.push((id, outline, proposal.screenplay.clone()));
        }
        let saved_plan = self.store.save_series_plan_in_transaction(&plan)?;
        tx.commit()?;
        // Merge only touched outlines into the GUI draft, retaining unrelated unsaved story edits.
        if let Some(draft) = self.story_drafts.get_mut(series) {
            for (id, outline, _) in &changed {
                crate::sidekick_episode::replace_outline(draft, id, Some(outline.clone()));
            }
            for outline in &saved_plan.episodes {
                if !draft
                    .episodes
                    .iter()
                    .any(|o| o.episode_id == outline.episode_id)
                {
                    draft.episodes.push(outline.clone());
                }
            }
            draft.episodes.sort_by_key(|o| {
                saved_plan
                    .episodes
                    .iter()
                    .position(|saved| saved.episode_id == o.episode_id)
                    .unwrap_or(usize::MAX)
            });
            draft.revision = saved_plan.revision;
        }
        for (id, _, screenplay) in &changed {
            if self.episode_id.as_deref() == Some(id) {
                self.script = screenplay.clone();
                self.script_baseline = screenplay.clone();
            }
        }
        Ok(
            json!({"saved":true,"start_episode":start,"end_episode":start+proposals.len()-1,"episode_ids":changed.iter().map(|(id,_,_)|id).collect::<Vec<_>>(),"backup":backup,"page_changed":false,"selected_episode_changed":false}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn proposal(n: usize) -> NewEpisode {
        NewEpisode {
            title: format!("Episode {n}"),
            screenplay: format!("INT. ROOM - DAY\nEpisode {n} screenplay."),
            goal: format!("Goal {n}"),
            conflict: "A problem".into(),
            turn: "A turn".into(),
            payoff: "A payoff".into(),
            hook: "A hook".into(),
            ending_resolution: "Continues".into(),
            requires: vec![],
            changes: vec![],
        }
    }
    #[test]
    fn fills_five_through_fifteen_from_synopsis_without_navigation() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::Store::open(temp.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("BlockSide", "Premise").unwrap();
        let mut ids = Vec::new();
        for n in 1..=5 {
            let e = store
                .create_episode(&series.id, &format!("Episode {n}"))
                .unwrap();
            store
                .save_screenplay(&e.id, &format!("Original {n}"))
                .unwrap();
            ids.push(e.id);
        }
        let mut app = Studio::from_store(store);
        app.selected = Some(series.id.clone());
        app.episode_id = Some(ids[0].clone());
        app.script = "Unsaved episode one".into();
        app.script_baseline = "Original 1".into();
        app.page = crate::Page::Synopsis;
        let mut plan = app.store.series_plan(&series.id).unwrap();
        plan.theme = "Unsaved theme".into();
        app.story_drafts.insert(series.id.clone(), plan);
        let expected = app.episode_batch_snapshot(&series.id).unwrap();
        let result=app.apply_episode_batch(&json!({"series_id":series.id,"expected":expected,"start_episode":5,"episodes":(5..=15).map(proposal).collect::<Vec<_>>()})).unwrap();
        assert_eq!(result["end_episode"], 15);
        let all = app.store.ordered_gui_episodes(&series.id).unwrap();
        assert_eq!(all.len(), 15);
        assert_eq!(all[4].id, ids[4]);
        assert_eq!(all[14].title, "Episode 15");
        assert_eq!(app.script, "Unsaved episode one");
        assert_eq!(app.episode_id, Some(ids[0].clone()));
        assert!(matches!(app.page, crate::Page::Synopsis));
        assert_eq!(app.story_drafts[&series.id].theme, "Unsaved theme");
        assert!(app.store.series_plan(&series.id).unwrap().theme.is_empty());
        assert_eq!(all[0].screenplay, "Original 1");
        assert!(std::path::Path::new(result["backup"].as_str().unwrap()).exists());
        let stale=app.apply_episode_batch(&json!({"series_id":series.id,"expected":expected,"start_episode":5,"episodes":[proposal(5)]}));
        assert!(stale.is_err());
        assert_eq!(app.store.episodes(&series.id).unwrap().len(), 15);
    }
    #[test]
    fn late_gui_edits_reject_batch_and_mid_batch_failure_rolls_back() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::Store::open(temp.path().join("db.sqlite")).unwrap();
        let series = store.create_series("Test", "").unwrap();
        let ep = store.create_episode(&series.id, "One").unwrap();
        let mut app = Studio::from_store(store);
        app.selected = Some(series.id.clone());
        app.episode_id = Some(ep.id.clone());
        let expected = app.episode_batch_snapshot(&series.id).unwrap();
        app.script = "Newer work".into();
        assert!(app.apply_episode_batch(&json!({"series_id":series.id,"expected":expected,"start_episode":1,"episodes":[proposal(1)]})).is_err());
        app.store.conn.execute_batch("CREATE TRIGGER fail_batch BEFORE UPDATE OF screenplay ON episodes WHEN NEW.screenplay='FAIL' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        let expected = app.episode_batch_snapshot(&series.id).unwrap();
        let mut bad = proposal(2);
        bad.screenplay = "FAIL".into();
        assert!(app.apply_episode_batch(&json!({"series_id":series.id,"expected":expected,"start_episode":1,"episodes":[proposal(1),bad]})).is_err());
        assert_eq!(app.store.episodes(&series.id).unwrap().len(), 1);
        assert!(app.store.get_episode(&ep.id).unwrap().screenplay.is_empty());
        assert_eq!(app.script, "Newer work");
    }
}
