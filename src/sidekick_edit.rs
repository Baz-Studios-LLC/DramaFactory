//! Validated document proposals affect drafts only, never saved canon.
use crate::synopsis::SynopsisDraft;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynopsisEdit {
    pub title: String,
    pub premise: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub message: String,
    #[serde(default)]
    pub music: Option<crate::episode_music::MusicDraft>,
    #[serde(default)]
    pub generation: Option<crate::sidekick_generation::Edit>,
    #[serde(default)]
    pub publishing: Option<crate::sidekick_publish::Edit>,
    #[serde(default)]
    pub plot: Option<crate::sidekick_plot::PlotEdit>,
    #[serde(default)]
    pub episode: Option<crate::sidekick_episode::EpisodeEdit>,
    #[serde(default)]
    pub new_episodes: Option<Vec<crate::sidekick_episode::NewEpisode>>,
    pub synopsis: Option<SynopsisEdit>,
    #[serde(default)]
    pub character: Option<crate::character_edit::CharacterEdit>,
    #[serde(default)]
    pub new_character: Option<crate::character_edit::NewCharacter>,
    #[serde(default)]
    pub new_characters: Option<Vec<crate::character_edit::NewCharacter>>,
    #[serde(default)]
    pub cast_queue: Option<Vec<crate::cast_queue::Task>>,
}
/// Request provider-enforced output rather than relying on prose instructions alone.
pub fn response_format() -> serde_json::Value {
    use serde_json::json;
    let profile_schema = crate::character_profile::response_schema();
    json!({"type":"json_schema","json_schema":{"name":"dramafactory_reply","strict":true,"schema":{
        "type":"object","additionalProperties":false,"required":["message","synopsis","character","new_character","plot","episode","new_episodes","generation"],
        "properties":{
            "message":{"type":"string"},
            "music":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["episode_id","direction","start","duration"],"properties":{"episode_id":{"type":"string"},"direction":{"type":"string"},"start":{"type":"number"},"duration":{"type":"number"}}}]},
            "publishing":{"anyOf":[{"type":"null"},crate::sidekick_publish::schema()]},
            "generation":{"anyOf":[{"type":"null"},crate::sidekick_generation::schema()]},
            "new_episodes":{"anyOf":[{"type":"null"},crate::sidekick_episode::creation_schema()]},
            "episode":{"anyOf":[{"type":"null"},crate::sidekick_episode::response_schema()]},
            "plot":{"anyOf":[{"type":"null"},crate::sidekick_plot::response_schema()]},
            "new_character":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["name","description","portrait_prompt","generate_portrait","profile","style"],"properties":{"generate_portrait":{"type":"boolean"},"name":{"type":"string"},"description":{"type":"string"},"portrait_prompt":{"type":"string"},"profile":profile_schema.clone(),"style":{"anyOf":[{"type":"null"},{"type":"string","enum":crate::portrait_style::PortraitStyle::ALL}]}}}]},
            "synopsis":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["title","premise"],"properties":{"title":{"type":"string"},"premise":{"type":"string"}}}]},
            "character":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["id","name","description","portrait_prompt","profile"],"properties":{"id":{"type":"string"},"name":{"type":"string"},"description":{"type":"string"},"portrait_prompt":{"type":"string"},"profile":{"anyOf":[{"type":"null"},profile_schema]}}}]}
        }
    }}})
}
/// Keep the provider grammar small: only this workspace's edit actions are active.
pub fn response_format_for(workspace: &str) -> serde_json::Value {
    let active: &[&str] = match workspace {
        "All workspaces" => &[
            "synopsis",
            "character",
            "new_characters",
            "cast_queue",
            "plot",
            "episode",
            "new_episodes",
            "generation",
            "music",
            "publishing",
        ],
        "Synopsis" => &["synopsis"],
        "Characters" | "Locations" => &["character", "new_characters", "cast_queue"],
        "Overall plot" => &["plot", "new_episodes"],
        "Episode plot" => &["episode", "new_episodes"],
        "Episode generation" => &["generation"],
        "Editing room" => &["music"],
        "Publish" => &["publishing"],
        _ => &[],
    };
    let mut format = response_format();
    let character_schema =
        format["json_schema"]["schema"]["properties"]["new_character"]["anyOf"][1].clone();
    format["json_schema"]["schema"]["properties"]["new_characters"] = serde_json::json!({"anyOf":[{"type":"null"},{"type":"array","minItems":1,"maxItems":12,"items":character_schema}]});
    format["json_schema"]["schema"]["properties"]["cast_queue"] = serde_json::json!({"anyOf":[{"type":"null"},{"type":"array","items":{"type":"object","additionalProperties":false,"required":["name","brief"],"properties":{"name":{"type":"string"},"brief":{"type":"string"}}}}]});
    if let Some(properties) = format["json_schema"]["schema"]["properties"].as_object_mut() {
        properties.retain(|key, _| key == "message" || active.contains(&key.as_str()));
        let required: Vec<_> = properties.keys().cloned().collect();
        format["json_schema"]["schema"]["required"] = serde_json::json!(required);
    }
    format
}
/// Character profiles exceed some providers' grammar-state budgets. Keep the
/// shape in the prompt and apply the same strict local validation before edits.
pub fn provider_format_for(workspace: &str) -> serde_json::Value {
    if matches!(workspace, "Characters" | "Locations" | "All workspaces") {
        serde_json::json!({"type":"json_object"})
    } else {
        response_format_for(workspace)
    }
}
/// Keep an unusable reply in the conversation without applying any document edits.
pub fn retained_reply(text: &str, truncated: bool) -> Result<Reply> {
    if !truncated && let Ok(reply) = parse_reply(text) {
        return Ok(reply);
    }
    // A provider can finish a valid JSON envelope while truncating its message.
    // Retain the readable message, but never apply edits from limited output.
    let parsed = if truncated {
        parse_reply(text).ok()
    } else {
        None
    };
    let text = parsed.as_ref().map_or(text, |reply| reply.message.as_str());
    // Leave room for the explanatory prefix within the chat persistence limit.
    let mut end = text.len().min(120 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let ordinary_text = !truncated && !text.trim_start().starts_with(['{', '[', '`']);
    Ok(Reply {
        music: None,
        message: if ordinary_text {
            text[..end].to_owned()
        } else {
            format!(
                "{} No document edits were applied. The returned text is retained below for review; no automatic retry was made.\n\n{}",
                if truncated {
                    "The sidekick reply reached its output limit. Use Continue reply below to prepare a continuation."
                } else {
                    "The sidekick did not return a valid edit response."
                },
                &text[..end]
            )
        },
        generation: None,
        publishing: None,
        synopsis: None,
        plot: None,
        episode: None,
        new_episodes: None,
        character: None,
        new_character: None,
        new_characters: None,
        cast_queue: None,
    })
}
pub fn parse_reply(text: &str) -> Result<Reply> {
    let reply: Reply = serde_json::from_str(text).map_err(|_| {
        anyhow::anyhow!("Sidekick returned an invalid document response. Nothing was applied.")
    })?;
    if let Some(edit) = &reply.publishing {
        edit.validate()?;
    }
    if let Some(music) = &reply.music {
        music.validate()?;
    }
    if let Some(tasks) = &reply.cast_queue {
        crate::cast_queue::validate(tasks)?;
        if reply.music.is_some()
            || reply.publishing.is_some()
            || reply.new_character.is_some()
            || reply.new_characters.is_some()
            || reply.character.is_some()
            || reply.synopsis.is_some()
            || reply.plot.is_some()
            || reply.episode.is_some()
            || reply.new_episodes.is_some()
            || reply.generation.is_some()
        {
            bail!("Cast planning cannot be combined with document changes");
        }
    }
    if reply.generation.is_some()
        && (reply.synopsis.is_some()
            || reply.character.is_some()
            || reply.new_character.is_some()
            || reply.new_characters.is_some()
            || reply.plot.is_some()
            || reply.new_episodes.is_some())
    {
        bail!("Generation preparation cannot be combined with other document edits");
    }
    if let (Some(plan), Some(episode)) = (&reply.generation, &reply.episode)
        && plan.episode_id != episode.episode_id
    {
        bail!("Segment and screenplay revisions must refer to the same episode");
    }
    if let Some(edit) = &reply.generation
        && serde_json::to_vec(edit)?.len() > 256 * 1024
    {
        bail!("Generation plan proposal too large");
    }
    if reply.message.trim().is_empty() || reply.message.len() > 128 * 1024 {
        bail!("Sidekick reply is empty or too large");
    }
    if let Some(profile) = reply
        .character
        .as_ref()
        .and_then(|edit| edit.profile.as_ref())
    {
        profile.validate_complete()?;
    }
    if reply.new_character.is_some() && reply.new_characters.is_some() {
        bail!("Use one character creation action, not both formats");
    }
    if let Some(characters) = &reply.new_characters
        && (characters.is_empty() || characters.len() > 12)
    {
        bail!("Create between one and twelve characters per response");
    }
    let mut names = std::collections::HashSet::new();
    for character in reply
        .new_character
        .iter()
        .chain(reply.new_characters.iter().flatten())
    {
        if !names.insert(character.name.trim().to_lowercase()) {
            bail!("Character batch contains duplicate names");
        }
        character
            .profile
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("New characters require a complete profile"))?
            .validate_complete()?;
    }
    if let Some(plot) = &reply.plot
        && serde_json::to_vec(plot)?.len() > 512 * 1024
    {
        bail!("Plot proposal too large");
    }
    if reply.new_episodes.is_some() && (reply.episode.is_some() || reply.plot.is_some()) {
        bail!("Create episodes or edit existing plot content in separate responses");
    }
    if reply.episode.is_some() && reply.plot.is_some() {
        bail!("Use one episode or overall plot edit per response");
    }
    if let Some(edit) = &reply.episode
        && serde_json::to_vec(edit)?.len() > 192 * 1024
    {
        bail!("Episode proposal too large");
    }
    if let Some(edit) = &reply.synopsis {
        validate(edit)?;
    }
    Ok(reply)
}
fn validate(edit: &SynopsisEdit) -> Result<()> {
    if edit.title.trim().is_empty() || edit.title.len() > 1024 || edit.premise.len() > 128 * 1024 {
        bail!("Sidekick synopsis edit is empty or too large");
    }
    Ok(())
}
pub fn apply(
    current: &mut SynopsisDraft,
    expected: &SynopsisDraft,
    edit: &SynopsisEdit,
) -> Result<SynopsisDraft> {
    validate(edit)?;
    if current != expected {
        bail!(
            "Your synopsis changed while the sidekick was replying. Its proposal is retained in chat; your newer draft was not overwritten."
        );
    }
    let before = current.clone();
    current.title = edit.title.clone();
    current.premise = edit.premise.clone();
    Ok(before)
}
pub fn undo(
    current: &mut SynopsisDraft,
    before: &SynopsisDraft,
    after: &SynopsisDraft,
) -> Result<()> {
    if current != after {
        bail!(
            "The synopsis changed after this sidekick edit. Undo would overwrite newer work, so it was not applied."
        );
    }
    *current = before.clone();
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn publishing_contract_is_available_and_truncation_cannot_apply_it() {
        for workspace in ["All workspaces", "Publish"] {
            assert!(
                response_format_for(workspace)["json_schema"]["schema"]["properties"]
                    .get("publishing")
                    .is_some()
            );
        }
        let text=serde_json::json!({"message":"Prepared draft","publishing":{"episode_id":"episode","title":"Title","tiktok":"TikTok","youtube":"YouTube","instagram":null}}).to_string();
        assert!(parse_reply(&text).unwrap().publishing.is_some());
        assert!(retained_reply(&text, true).unwrap().publishing.is_none());
    }
    #[test]
    fn segment_and_screenplay_edits_can_target_the_same_episode() {
        let mut response = serde_json::json!({"message":"Updated drafts","episode":{"episode_id":"a","screenplay":"Updated scene"},"generation":{"episode_id":"a","segments":[{"id":"s","prompt":"Closing shot","duration":22,"resolution":"480p"}]}});
        assert!(super::parse_reply(&response.to_string()).is_ok());
        response["generation"]["episode_id"] = "b".into();
        assert!(super::parse_reply(&response.to_string()).is_err());
    }

    use super::*;
    #[test]
    fn proposal_and_undo_never_clobber_intervening_draft_or_save() {
        let expected = SynopsisDraft {
            id: "s".into(),
            title: "Before".into(),
            premise: "Idea".into(),
            expected_title: "Saved".into(),
            expected_premise: "Saved premise".into(),
        };
        let mut current = expected.clone();
        let edit = SynopsisEdit {
            title: "After".into(),
            premise: "Better idea".into(),
        };
        let before = apply(&mut current, &expected, &edit).unwrap();
        let after = current.clone();
        assert_eq!(current.expected_title, "Saved");
        undo(&mut current, &before, &after).unwrap();
        assert_eq!(current, expected);
        current.title = "Manual change".into();
        assert!(apply(&mut current, &expected, &edit).is_err());
        assert_eq!(current.title, "Manual change");
        assert!(undo(&mut current, &before, &after).is_err());
        assert!(parse_reply(r#"{"message":"Hi","synopsis":null,"execute":"arbitrary"}"#).is_err());
        assert!(
            parse_reply(r#"{"message":"Hi","synopsis":{"title":"Title","premise":"Idea"}}"#)
                .is_ok()
        );
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[test]
    fn conversational_text_has_no_edit_error_and_music_is_validated() {
        let text = "ElevenLabs supports instrumental scoring in the Editing room.";
        let reply = retained_reply(text, false).unwrap();
        assert_eq!(reply.message, text);
        assert!(reply.music.is_none() && reply.synopsis.is_none());
        assert!(
            retained_reply("{broken JSON", false)
                .unwrap()
                .message
                .contains("valid edit response")
        );
        assert!(
            retained_reply(text, true)
                .unwrap()
                .message
                .contains("output limit")
        );
        let mut data = serde_json::json!({"message":"Prepared", "music":{"episode_id":"episode","direction":"Soft piano, instrumental, space for dialogue","start":0,"duration":30}});
        assert!(parse_reply(&data.to_string()).unwrap().music.is_some());
        data["music"]["duration"] = 601.into();
        assert!(parse_reply(&data.to_string()).is_err());
        data["music"]["duration"] = 30.into();
        data["music"]["start"] = (-1).into();
        assert!(parse_reply(&data.to_string()).is_err());
        assert!(
            response_format_for("All workspaces")["json_schema"]["schema"]["properties"]
                .get("music")
                .is_some()
        );
    }

    #[test]
    fn cross_tab_contract_exposes_guarded_actions_without_provider_grammar() {
        let schema = response_format_for("All workspaces");
        let properties = &schema["json_schema"]["schema"]["properties"];
        for action in [
            "synopsis",
            "character",
            "cast_queue",
            "new_characters",
            "plot",
            "episode",
            "new_episodes",
            "generation",
        ] {
            assert!(properties.get(action).is_some(), "missing {action}");
        }
        assert_eq!(
            provider_format_for("All workspaces"),
            serde_json::json!({"type":"json_object"})
        );
        assert!(properties.get("new_character").is_none());
    }
    #[test]
    fn episode_requests_do_not_compile_unrelated_profile_and_plot_grammars() {
        let full = response_format();
        let episode = response_format_for("Episode plot");
        let properties = &episode["json_schema"]["schema"]["properties"];
        assert!(properties.get("character").is_none());
        assert!(properties.get("plot").is_none());
        assert!(properties["episode"]["anyOf"].is_array());
        assert!(properties["new_episodes"]["anyOf"].is_array());
        assert!(episode.to_string().len() < full.to_string().len() / 2);
        assert!(parse_reply(r#"{"message":"Done","episode":null,"new_episodes":null}"#).is_ok());
        assert!(response_format_for("Characters")["json_schema"]["schema"]["properties"]["character"]["anyOf"].is_array());
    }
    #[test]
    fn batch_characters_validate_profiles_duplicates_and_truncation() {
        let mut profile = crate::character_profile::CharacterProfile {
            summary: "Fictional cast".into(),
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
        let member = |name| serde_json::json!({"name":name,"description":"Cast member","portrait_prompt":"Portrait","profile":profile,"style":null,"generate_portrait":false});
        let reply =
            serde_json::json!({"message":"Cast ready","new_characters":[member("A"),member("B")]});
        assert_eq!(
            parse_reply(&reply.to_string())
                .unwrap()
                .new_characters
                .unwrap()
                .len(),
            2
        );
        assert!(
            retained_reply(&reply.to_string(), true)
                .unwrap()
                .new_characters
                .is_none()
        );
        let duplicate =
            serde_json::json!({"message":"Cast","new_characters":[member("A"),member(" a ")]});
        assert!(parse_reply(&duplicate.to_string()).is_err());
        assert_eq!(
            provider_format_for("Characters"),
            serde_json::json!({"type":"json_object"})
        );
        let schema = response_format_for("Characters");
        assert!(
            schema["json_schema"]["schema"]["properties"]
                .get("new_characters")
                .is_some()
        );
    }
    #[test]
    fn sidekick_profiles_require_every_field_but_manual_drafts_do_not() {
        use crate::character_profile::{CharacterProfile, SECTIONS};
        let mut profile = CharacterProfile {
            summary: "Fictional artisan".into(),
            fields: Default::default(),
        };
        assert!(profile.validate().is_ok());
        assert!(profile.validate_complete().is_err());
        for (key, _, _) in SECTIONS.iter().flat_map(|section| section.fields.iter()) {
            profile
                .fields
                .insert((*key).into(), "Authored detail".into());
        }
        let reply = |profile: &CharacterProfile| {
            serde_json::json!({"message":"Profile ready", "synopsis":null, "character":null, "new_character":{"name":"Artisan", "description":"Artisan", "portrait_prompt":"Artisan", "generate_portrait":false, "profile":profile, "style":null}}).to_string()
        };
        assert!(parse_reply(&reply(&profile)).is_ok());
        for key in ["height", "weight"] {
            let previous = profile.fields.insert(key.into(), "  ".into()).unwrap();
            assert!(parse_reply(&reply(&profile)).is_err());
            assert!(
                retained_reply(&reply(&profile), false)
                    .unwrap()
                    .new_character
                    .is_none()
            );
            profile.fields.insert(key.into(), previous);
        }
    }
    #[test]
    fn malformed_and_truncated_outputs_are_retained_without_edits() {
        let reply = retained_reply("A useful plain-text character concept", false).unwrap();
        assert!(reply.message.contains("useful plain-text"));
        assert!(reply.synopsis.is_none() && reply.character.is_none());
        let valid =
            r#"{"message":"Drafted","synopsis":{"title":"New","premise":"Idea"},"character":null}"#;
        assert!(retained_reply(valid, false).unwrap().synopsis.is_some());
        assert!(retained_reply(valid, true).unwrap().synopsis.is_none());
        assert!(
            retained_reply(&"é".repeat(100000), false)
                .unwrap()
                .message
                .len()
                < 128 * 1024
        );
        assert_eq!(response_format()["json_schema"]["strict"], true);
    }
}
