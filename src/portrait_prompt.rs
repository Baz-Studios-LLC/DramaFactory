use crate::{production::LibraryEntry, store::Store};
use anyhow::{Result, bail};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptDraft {
    pub id: String,
    pub prompt: String,
    pub expected: Option<String>,
}
impl Store {
    pub fn portrait_prompt(&self, entry: &LibraryEntry) -> Result<PromptDraft> {
        let saved: Option<String> = self
            .conn
            .query_row(
                "SELECT prompt FROM portrait_prompts WHERE library_id=?1",
                [&entry.id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(PromptDraft{id:entry.id.clone(),prompt:saved.clone().unwrap_or_else(||format!("Photorealistic cinematic casting portrait of {}. {}. Natural lighting, realistic skin, expressive face, no text.",entry.name,entry.description.trim_end_matches('.'))),expected:saved})
    }
    pub fn save_portrait_prompt(&self, draft: &PromptDraft) -> Result<PromptDraft> {
        if draft.prompt.trim().is_empty() || draft.prompt.len() > 65536 {
            bail!("Portrait prompt needs 1–65536 bytes");
        }
        let tx = self.conn.unchecked_transaction()?;
        let current: Option<String> = tx
            .query_row(
                "SELECT prompt FROM portrait_prompts WHERE library_id=?1",
                [&draft.id],
                |row| row.get(0),
            )
            .optional()?;
        if current.as_deref() != Some(&draft.prompt) && current != draft.expected {
            bail!(
                "Portrait prompt changed elsewhere. Your draft is preserved; reload before saving."
            );
        }
        tx.execute("INSERT INTO portrait_prompts(library_id,prompt) VALUES(?1,?2) ON CONFLICT(library_id) DO UPDATE SET prompt=excluded.prompt",rusqlite::params![draft.id,draft.prompt])?;
        tx.commit()?;
        Ok(PromptDraft {
            expected: Some(draft.prompt.clone()),
            ..draft.clone()
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prompts_persist_and_stale_edits_are_rejected() {
        let root = std::env::temp_dir().join(format!("df-prompts-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("S", "P").unwrap();
        let entry = store
            .add_library(&series.id, "character", "A", "Adult", None)
            .unwrap();
        let mut draft = store.portrait_prompt(&entry).unwrap();
        let mut stale = draft.clone();
        draft.prompt = "Saved prompt".into();
        store.save_portrait_prompt(&draft).unwrap();
        store.save_portrait_prompt(&draft).unwrap();
        stale.prompt = "Stale prompt".into();
        assert!(store.save_portrait_prompt(&stale).is_err());
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(
            store.portrait_prompt(&entry).unwrap().prompt,
            "Saved prompt"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
