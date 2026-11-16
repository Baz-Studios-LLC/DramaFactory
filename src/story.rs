//! Series development and explicit continuity. Structural checks cannot judge
//! literary quality; committed canon additionally requires reviewed final media.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CharacterArc {
    pub character_id: String,
    pub desire: String,
    pub flaw: String,
    pub transformation: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fact {
    pub key: String,
    pub value: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Change {
    pub key: String,
    pub from: Option<String>,
    pub to: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EpisodeOutline {
    pub episode_id: String,
    pub goal: String,
    pub conflict: String,
    pub turn: String,
    pub payoff: String,
    pub hook: String,
    pub ending_resolution: String,
    pub requires: Vec<Fact>,
    pub changes: Vec<Change>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StoryThread {
    pub setup_episode_id: String,
    pub payoff_episode_id: String,
    pub setup: String,
    pub payoff: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeriesPlan {
    pub series_id: String,
    pub revision: u64,
    pub audience: String,
    pub genre: String,
    pub theme: String,
    pub central_question: String,
    pub ending: String,
    pub creative: crate::directing::CreativeProfile,
    pub characters: Vec<CharacterArc>,
    pub initial_facts: Vec<Fact>,
    pub episodes: Vec<EpisodeOutline>,
    pub threads: Vec<StoryThread>,
}
#[derive(Serialize)]
pub struct EpisodeContinuity {
    pub episode_id: String,
    pub status: String,
    pub commit_id: Option<String>,
    pub export_id: Option<String>,
    pub entry_state: BTreeMap<String, String>,
    pub exit_state: BTreeMap<String, String>,
    pub issues: Vec<String>,
}
#[derive(Serialize)]
pub struct StoryReport {
    pub revision: u64,
    pub issues: Vec<String>,
    pub episodes: Vec<EpisodeContinuity>,
    pub canon: BTreeMap<String, String>,
    pub limitation: &'static str,
}
impl Store {
    pub fn series_plan(&self, series: &str) -> Result<SeriesPlan> {
        if !self.series()?.iter().any(|s| s.id == series) {
            bail!("Series not found");
        }
        let data = self
            .conn
            .query_row(
                "SELECT data FROM story_plans WHERE series_id=?1",
                [series],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(match data {
            Some(data) => serde_json::from_str(&data)?,
            None => SeriesPlan {
                series_id: series.into(),
                ..Default::default()
            },
        })
    }
    pub fn save_series_plan(&self, draft: &SeriesPlan) -> Result<SeriesPlan> {
        let tx = self.conn.unchecked_transaction()?;
        let current = self.series_plan(&draft.series_id)?;
        draft.creative.validate()?;
        if current.revision != draft.revision {
            bail!(
                "Series plan changed in another session. Keep your draft and reload before saving."
            );
        }
        let episodes = self.episodes(&draft.series_id)?;
        let cast = self.library(&draft.series_id, "character")?;
        let mut ids = BTreeSet::new();
        for outline in &draft.episodes {
            if !episodes.iter().any(|e| e.id == outline.episode_id)
                || !ids.insert(&outline.episode_id)
            {
                bail!("Outline episodes must belong to this series and appear once");
            }
        }
        ids.clear();
        for arc in &draft.characters {
            if !cast.iter().any(|c| c.id == arc.character_id) || !ids.insert(&arc.character_id) {
                bail!("Character arcs must refer to distinct cast members in this series");
            }
        }
        let mut saved = draft.clone();
        saved.revision += 1;
        let data = serde_json::to_string(&saved)?;
        // Acquire the write transaction and use a compare-and-swap as well.
        // A competing writer can never overwrite a stale development draft.
        if current.revision == 0 {
            self.conn.execute(
                "INSERT INTO story_plans(series_id,revision,data) VALUES(?1,?2,?3)",
                params![saved.series_id, saved.revision, data],
            )?;
        } else if self.conn.execute(
            "UPDATE story_plans SET revision=?2,data=?3 WHERE series_id=?1 AND revision=?4",
            params![saved.series_id, saved.revision, data, current.revision],
        )? != 1
        {
            bail!("Series plan changed; reload before saving");
        }
        self.conn.execute(
            "INSERT INTO story_history(series_id,revision,data) VALUES(?1,?2,?3)",
            params![saved.series_id, saved.revision, data],
        )?;
        tx.commit()?;
        Ok(saved)
    }
    fn episode_fingerprint(&self, series: &str, episode: &str) -> Result<String> {
        let request = self.export_request(episode)?;
        if self.get_episode(episode)?.series_id != series {
            bail!("Episode belongs to another series");
        }
        for clip in &request.clips {
            if crate::worker::hash_file(&clip.path)? != clip.hash {
                bail!("Selected take changed on disk");
            }
            for audio in &clip.audio {
                if crate::worker::hash_file(&audio.path)? != audio.hash {
                    bail!("Dialogue changed on disk");
                }
            }
        }
        for cue in &request.cues {
            if crate::worker::hash_file(&cue.path)? != cue.hash {
                bail!("Audio cue changed on disk");
            }
        }
        let bytes = serde_json::to_vec(&request)?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    pub fn story_report(&self, series: &str) -> Result<StoryReport> {
        if self.conn.is_autocommit() {
            let tx = self.conn.unchecked_transaction()?;
            let report = self.story_report_inner(series)?;
            tx.commit()?;
            Ok(report)
        } else {
            self.story_report_inner(series)
        }
    }
    fn story_report_inner(&self, series: &str) -> Result<StoryReport> {
        let plan = self.series_plan(series)?;
        let mut issues = Vec::new();
        for (label, text) in [
            ("Audience", &plan.audience),
            ("Genre", &plan.genre),
            ("Theme", &plan.theme),
            ("Central question", &plan.central_question),
            ("Planned ending", &plan.ending),
        ] {
            if text.trim().is_empty() {
                issues.push(format!("{label} is missing"));
            }
        }
        if plan.episodes.len() < 2 {
            issues.push("Plan at least two episodes to establish a continuing story".into());
        }
        if plan.characters.is_empty() {
            issues.push("Add character arcs linked to the cast".into());
        }
        for arc in &plan.characters {
            if [&arc.desire, &arc.flaw, &arc.transformation]
                .iter()
                .any(|s| s.trim().is_empty())
            {
                issues.push("Every cast arc needs desire, flaw and transformation".into());
            }
        }
        if plan.threads.is_empty() {
            issues.push("Plan at least one setup and later payoff".into());
        }
        for thread in &plan.threads {
            let setup = plan
                .episodes
                .iter()
                .position(|e| e.episode_id == thread.setup_episode_id);
            let payoff = plan
                .episodes
                .iter()
                .position(|e| e.episode_id == thread.payoff_episode_id);
            if thread.setup.trim().is_empty()
                || thread.payoff.trim().is_empty()
                || setup.is_none()
                || payoff.is_none()
                || setup >= payoff
            {
                issues.push(
                    "A story thread needs a specific setup and payoff in a later planned episode"
                        .into(),
                );
            }
        }
        let mut projected = BTreeMap::new();
        for fact in &plan.initial_facts {
            if fact.key.trim().is_empty()
                || fact.value.trim().is_empty()
                || projected
                    .insert(fact.key.clone(), fact.value.clone())
                    .is_some()
            {
                issues
                    .push("Initial continuity facts need distinct keys and nonempty values".into());
            }
        }
        let mut canon = projected.clone();
        let mut reports = Vec::new();
        let mut prefix = true;
        let mut parent: Option<String> = None;
        for (index, outline) in plan.episodes.iter().enumerate() {
            let entry = projected.clone();
            let mut local = Vec::new();
            for (label, text) in [
                ("Goal", &outline.goal),
                ("Conflict", &outline.conflict),
                ("Turn", &outline.turn),
                ("Payoff", &outline.payoff),
            ] {
                if text.trim().is_empty() {
                    local.push(format!("{label} is missing"));
                }
            }
            if index + 1 < plan.episodes.len() && outline.hook.trim().is_empty() {
                local.push("Continuing episodes need a consequential hook".into());
            }
            if index + 1 == plan.episodes.len() && outline.ending_resolution.trim().is_empty() {
                local.push("Finale must describe how it resolves the central question".into());
            }
            let mut required = BTreeSet::new();
            for fact in &outline.requires {
                if !required.insert(&fact.key)
                    || fact.key.trim().is_empty()
                    || entry.get(&fact.key) != Some(&fact.value)
                {
                    local.push(format!("Entry continuity mismatch: {}", fact.key));
                }
            }
            let mut changed = BTreeSet::new();
            for change in &outline.changes {
                if change.key.trim().is_empty()
                    || change.to.trim().is_empty()
                    || !changed.insert(&change.key)
                    || entry.get(&change.key) != change.from.as_ref()
                {
                    local.push(format!(
                        "Continuity change has the wrong prior value: {}",
                        change.key
                    ));
                }
                projected.insert(change.key.clone(), change.to.clone());
            }
            if outline.changes.is_empty() {
                local.push("Record what changes because of this episode".into());
            }
            let commit=self.conn.query_row("SELECT id,revision,fingerprint,export_id,parent FROM canon_commits WHERE series_id=?1 AND episode_id=?2 ORDER BY rowid DESC LIMIT 1",params![series,outline.episode_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?))).optional()?;
            let mut status = "planned".to_string();
            let mut commit_id = None;
            let mut export_id = None;
            if let Some((id, revision, fingerprint, export, parent_id)) = commit {
                let media_valid = self.assets()?.iter().any(|a| {
                    a.id == export
                        && a.review == "approved"
                        && crate::worker::hash_file(&a.path).is_ok_and(|h| h == a.hash)
                });
                if prefix
                    && local.is_empty()
                    && issues.is_empty()
                    && revision == plan.revision
                    && parent_id == parent
                    && media_valid
                    && self
                        .episode_fingerprint(series, &outline.episode_id)
                        .is_ok_and(|h| h == fingerprint)
                {
                    status = "committed".into();
                    parent = Some(id.clone());
                    canon = projected.clone();
                    commit_id = Some(id);
                    export_id = Some(export);
                } else {
                    status = "stale".into();
                    prefix = false;
                }
            } else {
                prefix = false;
            }
            reports.push(EpisodeContinuity {
                episode_id: outline.episode_id.clone(),
                status,
                commit_id,
                export_id,
                entry_state: entry,
                exit_state: projected.clone(),
                issues: local,
            });
        }
        Ok(StoryReport {
            revision: plan.revision,
            issues,
            episodes: reports,
            canon,
            limitation: "Checks validate declared facts and reviewed media provenance. They do not prove dialogue truth, visual continuity, or narrative quality; those require editorial review.",
        })
    }
    pub fn commit_episode(&self, series: &str, episode: &str, export: &str) -> Result<StoryReport> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let report = self.story_report(series)?;
        if !report.issues.is_empty() || report.episodes.iter().any(|e| !e.issues.is_empty()) {
            bail!(
                "Resolve series/episode development and continuity issues before committing canon"
            );
        }
        let index = report
            .episodes
            .iter()
            .position(|e| e.episode_id == episode)
            .context("Episode is not in the series plan")?;
        if report.episodes[..index]
            .iter()
            .any(|e| e.status != "committed")
        {
            bail!("Commit the preceding episodes first; stale canon must be reviewed again");
        }
        if self.get_episode(episode)?.screenplay.trim().is_empty() {
            bail!("Episode needs a screenplay");
        }
        let asset = self
            .assets()?
            .into_iter()
            .find(|a| a.id == export)
            .context("Final export not found")?;
        if asset.review != "approved"
            || asset.metadata["kind"] != "episode_export"
            || asset.metadata["timeline"] != serde_json::to_value(self.export_request(episode)?)?
            || crate::worker::hash_file(&asset.path)? != asset.hash
        {
            bail!("Approve a final export matching the current episode timeline before committing");
        }
        if report.episodes[index].status == "committed"
            && report.episodes[index].export_id.as_deref() == Some(export)
        {
            tx.commit()?;
            return Ok(report);
        }
        let fingerprint = self.episode_fingerprint(series, episode)?;
        let parent = if index == 0 {
            None
        } else {
            report.episodes[index - 1].commit_id.clone()
        };
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO canon_commits(id,series_id,episode_id,revision,fingerprint,export_id,parent) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,series,episode,report.revision,fingerprint,export,parent])?;
        tx.commit()?;
        self.story_report(series)
    }
}
