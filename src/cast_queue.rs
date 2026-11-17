//! A bounded, sequential cast plan. No network calls or retries occur here.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub name: String,
    pub brief: String,
}
pub struct Queue {
    pub series: String,
    pub context: serde_json::Value,
    pub remaining: std::collections::VecDeque<Task>,
    pub total: usize,
    pub completed: usize,
}
pub fn validate(tasks: &[Task]) -> Result<()> {
    if tasks.is_empty() || tasks.len() > 32 {
        bail!("Cast queue must contain 1–32 characters");
    }
    let mut names = std::collections::HashSet::new();
    for task in tasks {
        if task.name.trim().is_empty()
            || task.name.len() > 200
            || task.brief.trim().is_empty()
            || task.brief.len() > 4000
        {
            bail!("Cast queue contains an invalid name or brief");
        }
        if !names.insert(task.name.trim().to_lowercase()) {
            bail!("Cast queue contains duplicate names");
        }
    }
    Ok(())
}
pub fn validate_reply(reply: &crate::sidekick_edit::Reply, task: &Task) -> Result<()> {
    let creations: Vec<_> = reply
        .new_character
        .iter()
        .chain(reply.new_characters.iter().flatten())
        .collect();
    if creations.len() != 1
        || creations[0].name.trim().to_lowercase() != task.name.trim().to_lowercase()
        || creations[0].generate_portrait
        || reply.character.is_some()
        || reply.cast_queue.is_some()
        || reply.synopsis.is_some()
        || reply.plot.is_some()
        || reply.episode.is_some()
        || reply.new_episodes.is_some()
        || reply.generation.is_some()
    {
        bail!(
            "Cast queue stopped: the reply must create only the requested character, without generating a portrait. Completed characters are retained."
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_reject_duplicates_and_empty_work() {
        assert!(validate(&[]).is_err());
        let task = Task {
            name: "Lead".into(),
            brief: "Village healer".into(),
        };
        assert!(validate(std::slice::from_ref(&task)).is_ok());
        assert!(validate(&[task.clone(), task]).is_err());
    }
    #[test]
    fn accepts_exactly_one_matching_character_and_blocks_media() {
        let task = Task {
            name: "Lead".into(),
            brief: "Healer".into(),
        };
        let mut profile = crate::character_profile::CharacterProfile {
            summary: "Healer".into(),
            fields: Default::default(),
        };
        for (key, _, _) in crate::character_profile::SECTIONS
            .iter()
            .flat_map(|section| section.fields.iter())
        {
            profile
                .fields
                .insert((*key).into(), "Authored detail".into());
        }
        let member = serde_json::json!({"name":"Lead","description":"Healer","portrait_prompt":"Healer portrait","profile":profile,"style":null,"generate_portrait":false});
        let mut value = serde_json::json!({"message":"Profile","new_characters":[member]});
        assert!(
            validate_reply(
                &crate::sidekick_edit::parse_reply(&value.to_string()).unwrap(),
                &task
            )
            .is_ok()
        );
        value["new_characters"][0]["generate_portrait"] = true.into();
        assert!(
            validate_reply(
                &crate::sidekick_edit::parse_reply(&value.to_string()).unwrap(),
                &task
            )
            .is_err()
        );
        value["new_characters"][0]["generate_portrait"] = false.into();
        value["new_characters"][0]["name"] = "Other".into();
        assert!(
            validate_reply(
                &crate::sidekick_edit::parse_reply(&value.to_string()).unwrap(),
                &task
            )
            .is_err()
        );
    }
    #[test]
    fn continuation_cannot_replan_or_edit_other_documents() {
        let task = Task {
            name: "Lead".into(),
            brief: "Healer".into(),
        };
        let reply = crate::sidekick_edit::parse_reply(
            r#"{"message":"Planning","cast_queue":[{"name":"Other","brief":"Rival"}]}"#,
        )
        .unwrap();
        assert!(validate_reply(&reply, &task).is_err());
    }
}
