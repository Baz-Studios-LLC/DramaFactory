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
    pub synopsis: Option<SynopsisEdit>,
    #[serde(default)]
    pub character: Option<crate::character_edit::CharacterEdit>,
    #[serde(default)]
    pub new_character: Option<crate::character_edit::NewCharacter>,
}
/// Request provider-enforced output rather than relying on prose instructions alone.
pub fn response_format() -> serde_json::Value {
    use serde_json::json;
    json!({"type":"json_schema","json_schema":{"name":"dramafactory_reply","strict":true,"schema":{
        "type":"object","additionalProperties":false,"required":["message","synopsis","character","new_character"],
        "properties":{
            "message":{"type":"string"},
            "new_character":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["name","description","portrait_prompt","generate_portrait"],"properties":{"generate_portrait":{"type":"boolean"},"name":{"type":"string"},"description":{"type":"string"},"portrait_prompt":{"type":"string"}}}]},
            "synopsis":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["title","premise"],"properties":{"title":{"type":"string"},"premise":{"type":"string"}}}]},
            "character":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["id","name","description","portrait_prompt"],"properties":{"id":{"type":"string"},"name":{"type":"string"},"description":{"type":"string"},"portrait_prompt":{"type":"string"}}}]}
        }
    }}})
}
/// Keep an unusable reply in the conversation without applying any document edits.
pub fn retained_reply(text: &str, truncated: bool) -> Result<Reply> {
    if !truncated && let Ok(reply) = parse_reply(text) {
        return Ok(reply);
    }
    // Leave room for the explanatory prefix within the chat persistence limit.
    let mut end = text.len().min(120 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(Reply {
        message: format!(
            "{} No document edits were applied. The returned text is retained below for review; no automatic retry was made.\n\n{}",
            if truncated {
                "The sidekick reply reached its output limit."
            } else {
                "The sidekick did not return a valid edit response."
            },
            &text[..end]
        ),
        synopsis: None,
        character: None,
        new_character: None,
    })
}
pub fn parse_reply(text: &str) -> Result<Reply> {
    let reply: Reply = serde_json::from_str(text).map_err(|_| {
        anyhow::anyhow!("Sidekick returned an invalid document response. Nothing was applied.")
    })?;
    if reply.message.trim().is_empty() || reply.message.len() > 128 * 1024 {
        bail!("Sidekick reply is empty or too large");
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
