use crate::{Studio, library_editing::LibraryEdit, portrait_prompt::PromptDraft};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CharacterDraft {
    pub brief: LibraryEdit,
    pub portrait: PromptDraft,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterEdit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub portrait_prompt: String,
}
pub fn apply(
    current: &CharacterDraft,
    expected: &CharacterDraft,
    edit: &CharacterEdit,
) -> Result<CharacterDraft> {
    if current != expected || current.brief.id != edit.id || current.portrait.id != edit.id {
        bail!(
            "Character or portrait prompt changed while the sidekick was replying. Proposal retained; newer edits were not overwritten."
        );
    }
    if edit.name.trim().is_empty()
        || edit.name.len() > 200
        || edit.description.len() > 12000
        || edit.portrait_prompt.trim().is_empty()
        || edit.portrait_prompt.len() > 65536
    {
        bail!("Character proposal exceeds supported bounds");
    }
    let mut result = current.clone();
    result.brief.name = edit.name.clone();
    result.brief.description = edit.description.clone();
    result.portrait.prompt = edit.portrait_prompt.clone();
    Ok(result)
}
impl Studio {
    pub(crate) fn character_drafts(&self, series: &str) -> Result<Vec<CharacterDraft>> {
        self.store
            .library(series, "character")?
            .iter()
            .map(|entry| {
                Ok(CharacterDraft {
                    brief: self
                        .library_drafts
                        .get(&entry.id)
                        .cloned()
                        .unwrap_or_else(|| LibraryEdit::from_entry(entry)),
                    portrait: self
                        .portrait_prompts
                        .get(&entry.id)
                        .cloned()
                        .map(Ok)
                        .unwrap_or_else(|| self.store.portrait_prompt(entry))?,
                })
            })
            .collect()
    }
    pub(crate) fn put_character_draft(&mut self, draft: CharacterDraft) {
        self.library_drafts
            .insert(draft.brief.id.clone(), draft.brief);
        self.portrait_prompts
            .insert(draft.portrait.id.clone(), draft.portrait);
    }
    pub(crate) fn undo_character_sidekick(&mut self, series: &str, id: &str) -> Result<()> {
        let (before, after) = self
            .sidekick
            .character_undo
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No character edit to undo"))?;
        let current = self
            .character_drafts(series)?
            .into_iter()
            .find(|draft| draft.brief.id == id)
            .ok_or_else(|| anyhow::anyhow!("Character no longer exists"))?;
        if current != after {
            bail!("Character changed after this sidekick edit; Undo would overwrite newer work.");
        }
        self.put_character_draft(before);
        self.sidekick.character_undo.remove(id);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn character_edit_preserves_saved_baselines_and_rejects_stale_or_wrong_target() {
        let draft = CharacterDraft {
            brief: LibraryEdit {
                id: "actor".into(),
                name: "Old".into(),
                description: "Adult".into(),
                expected_name: "Saved".into(),
                expected_description: "Saved brief".into(),
            },
            portrait: PromptDraft {
                id: "actor".into(),
                prompt: "Draft portrait".into(),
                expected: Some("Saved prompt".into()),
            },
        };
        let mut edit = CharacterEdit {
            id: "actor".into(),
            name: "New".into(),
            description: "Revised adult role".into(),
            portrait_prompt: "New portrait".into(),
        };
        let applied = apply(&draft, &draft, &edit).unwrap();
        assert_eq!(applied.brief.expected_name, "Saved");
        assert_eq!(applied.portrait.expected, Some("Saved prompt".into()));
        assert!(apply(&applied, &draft, &edit).is_err());
        edit.id = "another".into();
        assert!(apply(&draft, &draft, &edit).is_err());
    }
}
