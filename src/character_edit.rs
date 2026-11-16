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

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewCharacter {
    #[serde(default)]
    pub generate_portrait: bool,
    pub name: String,
    pub description: String,
    pub portrait_prompt: String,
}
impl crate::store::Store {
    pub(crate) fn create_sidekick_character(
        &self,
        series: &str,
        id: &str,
        proposal: &NewCharacter,
    ) -> Result<()> {
        use rusqlite::{OptionalExtension, params};
        if proposal.name.trim().is_empty()
            || proposal.name.len() > 200
            || proposal.description.len() > 12000
            || proposal.portrait_prompt.trim().is_empty()
            || proposal.portrait_prompt.len() > 65536
        {
            bail!("Character proposal is empty or exceeds supported bounds");
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.query_row("SELECT id FROM series WHERE id=?1", [series], |r| {
            r.get::<_, String>(0)
        })?;
        let existing: Option<(String,String,String,Option<String>)>=tx.query_row(
            "SELECT l.series_id,l.name,l.description,p.prompt FROM library l LEFT JOIN portrait_prompts p ON p.library_id=l.id WHERE l.id=?1",[id],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some(existing) = existing {
            if existing
                != (
                    series.into(),
                    proposal.name.clone(),
                    proposal.description.clone(),
                    Some(proposal.portrait_prompt.clone()),
                )
            {
                bail!("Character creation conflicts with an existing record");
            }
            return Ok(());
        }
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM library WHERE series_id=?1 AND kind='character' AND lower(trim(name))=lower(trim(?2)))",params![series,proposal.name],|r|r.get(0))?;
        if duplicate {
            bail!(
                "A character with this name already exists. Ask the sidekick to edit that character instead."
            );
        }
        tx.execute("INSERT INTO library(id,series_id,kind,name,description) VALUES(?1,?2,'character',?3,?4)",params![id,series,proposal.name,proposal.description])?;
        tx.execute(
            "INSERT INTO portrait_prompts(library_id,prompt) VALUES(?1,?2)",
            params![id, proposal.portrait_prompt],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[test]
    fn character_and_prompt_create_atomically_without_images_and_without_duplicate_replay() {
        let root = std::env::temp_dir().join(format!("df-new-character-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("studio.sqlite");
        let store = crate::store::Store::open(path.clone()).unwrap();
        let a = store.create_series("A", "").unwrap();
        let b = store.create_series("B", "").unwrap();
        let proposal = NewCharacter {
            generate_portrait: false,
            name: "Tamsin".into(),
            description: "Clever adult market trader".into(),
            portrait_prompt: "Cinematic portrait, village market".into(),
        };
        store
            .create_sidekick_character(&a.id, "fixture-character", &proposal)
            .unwrap();
        store
            .create_sidekick_character(&a.id, "fixture-character", &proposal)
            .unwrap();
        assert!(
            store
                .create_sidekick_character(&b.id, "fixture-character", &proposal)
                .is_err()
        );
        assert!(
            store
                .create_sidekick_character(&a.id, "duplicate", &proposal)
                .is_err()
        );
        assert!(store.library(&b.id, "character").unwrap().is_empty());
        drop(store);
        let store = crate::store::Store::open(path).unwrap();
        let entries = store.library(&a.id, "character").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].reference.is_none());
        let job = store
            .enqueue_grok_portrait(&entries[0].id, &proposal.portrait_prompt)
            .unwrap();
        assert_eq!(job.status, "queued");
        // Queue construction only: no worker or provider call is started in this test.

        assert_eq!(
            store.portrait_prompt(&entries[0]).unwrap().prompt,
            proposal.portrait_prompt
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
