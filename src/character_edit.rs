use crate::{Studio, library_editing::LibraryEdit, portrait_prompt::PromptDraft};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CharacterDraft {
    pub brief: LibraryEdit,
    pub portrait: PromptDraft,
    pub profile: Option<crate::character_profile::ProfileDraft>,
}
impl CharacterDraft {
    /// Only creative content goes to the model; conflict baselines stay local.
    pub(crate) fn model_context(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.brief.id, "name": self.brief.name,
            "description": self.brief.description,
            "portrait_prompt": self.portrait.prompt,
            "profile": self.profile.as_ref().map(|draft| &draft.profile),
        })
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterEdit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub portrait_prompt: String,
    #[serde(default)]
    pub profile: Option<crate::character_profile::CharacterProfile>,
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
    if let Some(profile) = &mut result.profile {
        if let Some(proposal) = &edit.profile {
            proposal.validate()?;
            profile.profile = proposal.clone();
            profile
                .profile
                .fields
                .retain(|_, value| !value.trim().is_empty());
        } else if edit.description != current.brief.description {
            if !profile.profile.fields.is_empty() {
                bail!(
                    "This character has structured details. The sidekick must edit the profile instead of replacing its full brief."
                );
            }
            profile.profile.summary = edit.description.clone();
        }
        profile.brief.name = edit.name.clone();
        profile.refresh_brief();
        result.brief = profile.brief.clone();
    }
    Ok(result)
}
impl Studio {
    pub(crate) fn character_drafts(&self, series: &str) -> Result<Vec<CharacterDraft>> {
        self.store
            .library(series, "character")?
            .iter()
            .map(|entry| {
                let profile = self
                    .profile_drafts
                    .get(&entry.id)
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| self.store.character_profile(&entry.id))?;
                Ok(CharacterDraft {
                    brief: profile.brief.clone(),
                    profile: Some(profile),
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
        if let Some(profile) = draft.profile {
            self.library_drafts.remove(&draft.brief.id);
            self.profile_drafts.insert(draft.brief.id.clone(), profile);
        } else {
            self.library_drafts
                .insert(draft.brief.id.clone(), draft.brief);
        }
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
    fn sidekick_profile_edits_preserve_other_traits_and_guard_concurrent_form_changes() {
        let mut profile = crate::character_profile::CharacterProfile {
            summary: "Biography".into(),
            ..Default::default()
        };
        profile.fields.insert("height".into(), "170 cm".into());
        profile.fields.insert("hair_color".into(), "Brown".into());
        let brief = LibraryEdit {
            id: "actor".into(),
            name: "Ari".into(),
            description: profile.brief(),
            expected_name: "Ari".into(),
            expected_description: profile.brief(),
        };
        let current = CharacterDraft {
            brief: brief.clone(),
            portrait: PromptDraft {
                id: "actor".into(),
                prompt: "Keep approved identity".into(),
                expected: None,
            },
            profile: Some(crate::character_profile::ProfileDraft {
                brief,
                profile: profile.clone(),
                expected_profile: Some(profile.clone()),
            }),
        };
        let context = current.model_context();
        assert_eq!(context["id"], current.brief.id);
        assert_eq!(context["description"], current.brief.description);
        assert_eq!(context["portrait_prompt"], current.portrait.prompt);
        assert_eq!(context["profile"]["fields"]["height"], "170 cm");
        assert!(!context.to_string().contains("expected"));
        assert!(context.to_string().len() < serde_json::to_string(&current).unwrap().len());
        profile.fields.insert("hair_color".into(), "Auburn".into());
        let proposal = CharacterEdit {
            id: "actor".into(),
            name: "Ari".into(),
            description: current.brief.description.clone(),
            portrait_prompt: current.portrait.prompt.clone(),
            profile: Some(profile),
        };
        let after = apply(&current, &current, &proposal).unwrap();
        assert!(after.brief.description.contains("Hair color: Auburn"));
        assert!(after.brief.description.contains("Height: 170 cm"));
        assert_eq!(
            after.profile.as_ref().unwrap().expected_profile,
            current.profile.as_ref().unwrap().expected_profile
        );
        assert!(apply(&after, &current, &proposal).is_err());
        let mut destructive = proposal.clone();
        destructive.profile = None;
        destructive.description = "Replace everything".into();
        assert!(apply(&current, &current, &destructive).is_err());
    }
    #[test]
    fn character_edit_preserves_saved_baselines_and_rejects_stale_or_wrong_target() {
        let draft = CharacterDraft {
            profile: None,
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
            profile: None,
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
    #[serde(default)]
    pub profile: Option<crate::character_profile::CharacterProfile>,
    #[serde(default)]
    pub style: Option<crate::portrait_style::PortraitStyle>,
}
impl crate::store::Store {
    pub(crate) fn create_sidekick_character(
        &self,
        series: &str,
        id: &str,
        proposal: &NewCharacter,
    ) -> Result<()> {
        use rusqlite::{OptionalExtension, params};
        let profile = proposal.profile.clone().map(|mut profile| {
            profile.fields.retain(|_, value| !value.trim().is_empty());
            profile
        });
        if let Some(profile) = &profile {
            profile.validate()?;
        }
        let description = profile
            .as_ref()
            .map(crate::character_profile::CharacterProfile::brief)
            .unwrap_or_else(|| proposal.description.clone());
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
            self.ensure_character_not_deleted(id)?;
            if existing
                != (
                    series.into(),
                    proposal.name.clone(),
                    description.clone(),
                    Some(proposal.portrait_prompt.clone()),
                )
                || self.saved_character_profile(id)? != profile
                || proposal
                    .style
                    .is_some_and(|style| self.portrait_style(id).ok() != Some(style))
            {
                bail!("Character creation conflicts with an existing record");
            }
            return Ok(());
        }
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM library WHERE series_id=?1 AND kind='character' AND lower(trim(name))=lower(trim(?2)) AND NOT EXISTS(SELECT 1 FROM deleted_characters d WHERE d.library_id=library.id))",params![series,proposal.name],|r|r.get(0))?;
        if duplicate {
            bail!(
                "A character with this name already exists. Ask the sidekick to edit that character instead."
            );
        }
        tx.execute("INSERT INTO library(id,series_id,kind,name,description) VALUES(?1,?2,'character',?3,?4)",params![id,series,proposal.name,description])?;
        if let Some(style) = proposal.style {
            self.save_portrait_style(id, style)?;
        }
        if let Some(profile) = &profile {
            tx.execute(
                "INSERT INTO character_profiles(library_id,document) VALUES(?1,?2)",
                params![id, serde_json::to_string(profile)?],
            )?;
        }
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
            style: Some(crate::portrait_style::PortraitStyle::CelShaded),
            profile: None,
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
        assert!(
            store.portrait_style(&entries[0].id).unwrap()
                == crate::portrait_style::PortraitStyle::CelShaded
        );
        let raw: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [&job.id], |row| {
                row.get(0)
            })
            .unwrap();
        let request: crate::hosted_image::PortraitRequest = serde_json::from_str(&raw).unwrap();
        assert!(request.prompt.contains("SELECTED VISUAL STYLE: Cel-shaded"));
        // Queue construction only: no worker or provider call is started in this test.

        assert_eq!(
            store.portrait_prompt(&entries[0]).unwrap().prompt,
            proposal.portrait_prompt
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
