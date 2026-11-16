//! Sidekick plot patches edit drafts, never saved plans or episode scripts.
use crate::{store::Store, story::*};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlotEdit {
    pub audience: Option<String>,
    pub genre: Option<String>,
    pub theme: Option<String>,
    pub central_question: Option<String>,
    pub ending: Option<String>,
    pub series_arc: Option<String>,
    pub acts: Option<Vec<PlotAct>>,
    pub storylines: Option<Vec<Storyline>>,
    pub characters: Option<Vec<CharacterArc>>,
    pub initial_facts: Option<Vec<Fact>>,
    pub episodes: Option<Vec<EpisodeOutline>>,
    pub threads: Option<Vec<StoryThread>>,
    pub target_episodes: Option<u32>,
    pub episode_seconds: Option<f64>,
}
pub fn same(a: &SeriesPlan, b: &SeriesPlan) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}
pub fn apply(
    store: &Store,
    current: &SeriesPlan,
    expected: &SeriesPlan,
    edit: &PlotEdit,
) -> Result<SeriesPlan> {
    if !same(current, expected)
        || store.series_plan(&expected.series_id)?.revision != expected.revision
    {
        bail!(
            "Overall plot changed while the sidekick was replying. Your newer work was preserved; the proposal remains in chat."
        );
    }
    let mut next = current.clone();
    macro_rules! patch { ($($field:ident),*) => { $(if let Some(value) = &edit.$field { next.$field = value.clone(); })* }; }
    patch!(
        audience,
        genre,
        theme,
        central_question,
        ending,
        series_arc,
        acts,
        storylines,
        characters,
        initial_facts,
        episodes,
        threads
    );
    if let Some(value) = edit.target_episodes {
        next.creative.target_episodes = value;
    }
    if let Some(value) = edit.episode_seconds {
        next.creative.episode_seconds = value;
    }
    next.creative.validate()?;
    if serde_json::to_vec(&next)?.len() > 512 * 1024 {
        bail!("Plot proposal is too large");
    }
    let cast = store.library(&next.series_id, "character")?;
    let episodes = store.episodes(&next.series_id)?;
    let mut ids = std::collections::HashSet::new();
    for arc in &next.characters {
        if !cast.iter().any(|c| c.id == arc.character_id) || !ids.insert(&arc.character_id) {
            bail!("Character journeys must use distinct cast IDs from this series");
        }
    }
    ids.clear();
    for outline in &next.episodes {
        if !episodes.iter().any(|e| e.id == outline.episode_id) || !ids.insert(&outline.episode_id)
        {
            bail!("Episode outlines must use distinct existing episode IDs from this series");
        }
    }
    for thread in &next.threads {
        if !next
            .episodes
            .iter()
            .any(|e| e.episode_id == thread.setup_episode_id)
            || !next
                .episodes
                .iter()
                .any(|e| e.episode_id == thread.payoff_episode_id)
        {
            bail!("Setups and payoffs must link to planned episodes");
        }
    }
    for act in &next.acts {
        if act.start_episode == 0
            || act.end_episode < act.start_episode
            || act.end_episode > next.creative.target_episodes
        {
            bail!("Act episode ranges must fit the series target");
        }
    }
    Ok(next)
}
pub fn undo(current: &SeriesPlan, before: &SeriesPlan, after: &SeriesPlan) -> Result<SeriesPlan> {
    if !same(current, after) {
        bail!("The plot changed after this edit; Undo would overwrite newer work");
    }
    Ok(before.clone())
}
pub fn response_schema() -> serde_json::Value {
    use serde_json::{Value, json};
    fn object(fields: Vec<(&str, Value)>) -> Value {
        let required: Vec<_> = fields.iter().map(|(k, _)| *k).collect();
        let properties: serde_json::Map<String, Value> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        json!({"type":"object","additionalProperties":false,"required":required,"properties":properties})
    }
    fn texts(keys: &[&'static str]) -> Vec<(&'static str, Value)> {
        keys.iter()
            .map(|k| (*k, json!({"type":"string"})))
            .collect()
    }
    fn array(value: Value) -> Value {
        json!({"type":"array","items":value})
    }
    fn nullable(value: Value) -> Value {
        json!({"anyOf":[{"type":"null"},value]})
    }
    let fact = object(texts(&["key", "value"]));
    let mut change = texts(&["key", "to"]);
    change.push(("from", nullable(json!({"type":"string"}))));
    let mut episode = texts(&[
        "episode_id",
        "goal",
        "conflict",
        "turn",
        "payoff",
        "hook",
        "ending_resolution",
    ]);
    episode.push(("requires", array(fact.clone())));
    episode.push(("changes", array(object(change))));
    let mut act = texts(&[
        "title",
        "opening",
        "escalation",
        "turning_point",
        "consequence",
    ]);
    for key in ["start_episode", "end_episode"] {
        act.push((key, json!({"type":"integer","minimum":1,"maximum":150})));
    }
    let mut fields = texts(&[
        "audience",
        "genre",
        "theme",
        "central_question",
        "ending",
        "series_arc",
    ]);
    fields.extend([
        ("acts", array(object(act))),
        (
            "storylines",
            array(object(texts(&["title", "throughline", "resolution"]))),
        ),
        (
            "characters",
            array(object(texts(&[
                "character_id",
                "desire",
                "flaw",
                "transformation",
            ]))),
        ),
        ("initial_facts", array(fact)),
        ("episodes", array(object(episode))),
        (
            "threads",
            array(object(texts(&[
                "setup_episode_id",
                "payoff_episode_id",
                "setup",
                "payoff",
            ]))),
        ),
        (
            "target_episodes",
            json!({"type":"integer","minimum":1,"maximum":150}),
        ),
        (
            "episode_seconds",
            json!({"type":"number","minimum":15,"maximum":600}),
        ),
    ]);
    object(fields.into_iter().map(|(k, v)| (k, nullable(v))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plot_patches_preserve_drafts_and_conflict_on_manual_or_external_edits() {
        let root = std::env::temp_dir().join(format!("df-plot-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("test.sqlite")).unwrap();
        let series = store.create_series("Test", "A story").unwrap();
        let expected = store.series_plan(&series.id).unwrap();
        let edit = PlotEdit {
            ending: Some("Earned resolution".into()),
            acts: Some(vec![PlotAct {
                title: "First act".into(),
                start_episode: 1,
                end_episode: 15,
                ..Default::default()
            }]),
            storylines: Some(vec![Storyline {
                title: "Family".into(),
                throughline: "Trust grows".into(),
                resolution: "Reconciliation".into(),
            }]),
            ..Default::default()
        };
        let after = apply(&store, &expected, &expected, &edit).unwrap();
        assert_eq!(after.ending, "Earned resolution");
        assert_eq!(after.creative.target_episodes, 67);
        assert!(store.series_plan(&series.id).unwrap().ending.is_empty());
        assert!(same(&undo(&after, &expected, &after).unwrap(), &expected));
        let mut manual = after.clone();
        manual.theme = "Manual text".into();
        assert!(apply(&store, &manual, &expected, &edit).is_err());
        assert!(undo(&manual, &expected, &after).is_err());
        let mut bad = edit.clone();
        bad.characters = Some(vec![CharacterArc {
            character_id: "foreign".into(),
            ..Default::default()
        }]);
        assert!(apply(&store, &expected, &expected, &bad).is_err());
        bad = edit.clone();
        bad.episodes = Some(vec![EpisodeOutline {
            episode_id: "invented".into(),
            ..Default::default()
        }]);
        assert!(apply(&store, &expected, &expected, &bad).is_err());
        let saved = store.save_series_plan(&after).unwrap();
        assert!(apply(&store, &expected, &expected, &edit).is_err());
        assert_eq!(saved.acts[0].title, "First act");
        drop(store);
        let store = Store::open(root.join("test.sqlite")).unwrap();
        let reopened = store.series_plan(&series.id).unwrap();
        assert_eq!(reopened.storylines[0].resolution, "Reconciliation");
        assert_eq!(reopened.acts.len(), 1);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_plans_load_and_plot_payload_roundtrips() {
        let old: SeriesPlan =
            serde_json::from_str(r#"{"series_id":"s","revision":2,"ending":"Keep this ending"}"#)
                .unwrap();
        assert!(old.acts.is_empty() && old.storylines.is_empty());
        assert_eq!(old.ending, "Keep this ending");
        let reply=crate::sidekick_edit::parse_reply(r#"{"message":"Updated","synopsis":null,"plot":{"series_arc":"Start, escalate, resolve"}}"#).unwrap();
        assert_eq!(
            reply.plot.unwrap().series_arc.as_deref(),
            Some("Start, escalate, resolve")
        );
        assert!(
            crate::sidekick_edit::retained_reply(
                r#"{"message":"Updated","plot":{"ending":"New"}}"#,
                true
            )
            .unwrap()
            .plot
            .is_none()
        );
        assert!(response_schema()["properties"]["acts"]["anyOf"][1]["items"]["properties"]["turning_point"].is_object());
    }
}
