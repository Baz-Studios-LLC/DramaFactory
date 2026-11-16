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
