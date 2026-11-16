//! Recoverable cast deletion. Original records, media and history are retained.
use crate::{production::LibraryEntry, store::Store};
use anyhow::{Context, Result, bail};

impl Store {
    pub(crate) fn ensure_character_not_deleted(&self, id: &str) -> Result<()> {
        let deleted: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM deleted_characters WHERE library_id=?1)",
            [id],
            |row| row.get(0),
        )?;
        if deleted {
            bail!("This character is deleted. Restore it from Deleted characters before using it.");
        }
        Ok(())
    }
    pub fn character_deletion_blockers(&self, id: &str) -> Result<Vec<String>> {
        let mut blockers = Vec::new();
        // Inspect stable IDs in current saved documents, never guess from prose names.
        for (table, label) in [
            ("story_plans", "the overall story plan"),
            ("shot_directions", "shot direction"),
            ("generation_plans", "an episode generation plan"),
        ] {
            let used:bool=self.conn.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table}, json_tree({table}.data) j WHERE j.type='text' AND j.value=?1)"),
                [id],|row|row.get(0)
            )?;
            if used {
                blockers.push(format!(
                    "Remove this character's link from {label} before deleting."
                ));
            }
        }
        let active:bool=self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs, json_tree(CASE WHEN json_valid(jobs.request) THEN jobs.request ELSE '{}' END) j WHERE jobs.status IN ('queued','running','blocked') AND j.type='text' AND j.value=?1)",
            [id],|row|row.get(0)
        )?;
        if active {
            blockers.push(
                "Wait for or cancel this character's active generation jobs before deleting."
                    .into(),
            );
        }
        Ok(blockers)
    }
    pub fn delete_character(&self, expected: &LibraryEntry) -> Result<()> {
        if expected.kind != "character" {
            bail!("Only characters can be deleted here");
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let active = self.library(&expected.series_id, "character")?;
        let deleted = self.deleted_characters(&expected.series_id)?;
        let current = active
            .iter()
            .chain(deleted.iter())
            .find(|entry| entry.id == expected.id)
            .context("Character not found in this series")?;
        if current != expected {
            bail!(
                "Character changed since deletion was requested. Review the updated card and try again."
            );
        }
        if deleted.iter().any(|entry| entry.id == expected.id) {
            tx.commit()?;
            return Ok(());
        }
        let blockers = self.character_deletion_blockers(&expected.id)?;
        if !blockers.is_empty() {
            bail!("{}", blockers.join("\n"));
        }
        tx.execute(
            "INSERT INTO deleted_characters(library_id) VALUES(?1)",
            [&expected.id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn restore_character(&self, series: &str, id: &str) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        tx.query_row(
            "SELECT id FROM library WHERE id=?1 AND series_id=?2 AND kind='character'",
            rusqlite::params![id, series],
            |row| row.get::<_, String>(0),
        )
        .context("Character not found in this series")?;
        tx.execute("DELETE FROM deleted_characters WHERE library_id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}
impl crate::Studio {
    pub(crate) fn character_delete_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
    ) {
        ui.push_id((&entry.id,"delete-character"),|ui| {
            let confirming=self.delete_character.as_ref().filter(|expected|expected.id==entry.id).cloned();
            if let Some(expected)=confirming {
                ui.separator();
                ui.label(format!("Delete {}?",expected.name));
                ui.small("Moves this character to Deleted characters. Portraits, saved details and history are retained; you can restore it later.");
                let blockers=match self.store.character_deletion_blockers(&entry.id) {
                    Ok(blockers)=>blockers,Err(error)=>vec![error.to_string()],
                };
                for blocker in &blockers {ui.colored_label(crate::GOLD,blocker);}
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Cancel").clicked() {self.delete_character=None;}
                    if ui.add_enabled(blockers.is_empty(),eframe::egui::Button::new("Delete character")).clicked() {
                        let result=self.store.delete_character(&expected);
                        if result.is_ok() {self.delete_character=None;}
                        self.result(result,"Character moved to Deleted characters; portraits and history retained");
                    }
                });
            } else if ui.small_button("Delete character…").clicked() {
                self.delete_character=Some(entry.clone());
            }
        });
    }
    pub(crate) fn deleted_character_list(&mut self, ui: &mut eframe::egui::Ui, series: &str) {
        let deleted = self.load(self.store.deleted_characters(series));
        if deleted.is_empty() {
            return;
        }
        eframe::egui::CollapsingHeader::new(format!("Deleted characters ({})",deleted.len()))
            .id_salt((series,"deleted-characters")).show(ui,|ui| {
                ui.small("Restoring returns the same character, profile and selected portrait to the cast.");
                for entry in deleted {
                    ui.push_id(&entry.id,|ui|ui.horizontal(|ui| {
                        ui.label(&entry.name);
                        if ui.button("Restore").clicked() {
                            let result=self.store.restore_character(series,&entry.id);
                            self.result(result,"Character restored");
                        }
                    }));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delete_restore_retains_media_profile_style_and_identity_across_restart() {
        let root =
            std::env::temp_dir().join(format!("df-delete-character-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let image = root.join("original.png");
        std::fs::write(&image, b"retained original bytes").unwrap();
        let profile = crate::character_profile::CharacterProfile {
            summary: "Biography".into(),
            ..Default::default()
        };
        let entry = store
            .create_profile_character(
                &series.id,
                "Ari",
                &profile,
                Some(&image),
                crate::portrait_style::PortraitStyle::Cartoon,
            )
            .unwrap();
        let prompt = store.portrait_prompt(&entry).unwrap();
        store.save_portrait_prompt(&prompt).unwrap();
        store.delete_character(&entry).unwrap();
        store.delete_character(&entry).unwrap();
        assert!(store.library(&series.id, "character").unwrap().is_empty());
        assert!(store.deleted_characters(&series.id).unwrap()[0] == entry);
        assert!(store.enqueue_grok_portrait(&entry.id, "Portrait").is_err());
        assert_eq!(
            std::fs::read(entry.reference.as_ref().unwrap()).unwrap(),
            b"retained original bytes"
        );
        assert_eq!(std::fs::read(&image).unwrap(), b"retained original bytes");
        let other = store.create_series("Other", "").unwrap();
        assert!(store.restore_character(&other.id, &entry.id).is_err());
        drop(store);
        let store = Store::open(path).unwrap();
        assert!(store.library(&series.id, "character").unwrap().is_empty());
        store.restore_character(&series.id, &entry.id).unwrap();
        store.restore_character(&series.id, &entry.id).unwrap();
        assert!(store.library(&series.id, "character").unwrap()[0] == entry);
        assert!(store.deleted_characters(&series.id).unwrap().is_empty());
        assert_eq!(store.character_profile(&entry.id).unwrap().profile, profile);
        assert_eq!(store.portrait_prompt(&entry).unwrap().prompt, prompt.prompt);
        assert!(
            store.portrait_style(&entry.id).unwrap()
                == crate::portrait_style::PortraitStyle::Cartoon
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_deletion_saved_links_and_pending_jobs_are_rejected() {
        let root =
            std::env::temp_dir().join(format!("df-delete-conflicts-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let entry = store
            .add_library(&series.id, "character", "Ari", "Original", None)
            .unwrap();
        let mut edit = crate::library_editing::LibraryEdit::from_entry(&entry);
        edit.name = "New name".into();
        let current = store.update_library_entry(&edit).unwrap();
        assert!(store.delete_character(&entry).is_err());
        let mut plan = store.series_plan(&series.id).unwrap();
        plan.characters.push(crate::story::CharacterArc {
            character_id: entry.id.clone(),
            ..Default::default()
        });
        let mut plan = store.save_series_plan(&plan).unwrap();
        assert!(
            store
                .delete_character(&current)
                .unwrap_err()
                .to_string()
                .contains("story plan")
        );
        plan.characters.clear();
        store.save_series_plan(&plan).unwrap();
        let episode = store.create_episode(&series.id, "Pilot").unwrap();
        // A durable generation link is enough to block removal, even before media exists.
        store
            .conn
            .execute(
                "INSERT INTO generation_plans(episode_id,revision,data) VALUES(?1,1,?2)",
                rusqlite::params![
                    episode.id,
                    serde_json::json!([{"cast":[{"character_id":entry.id}]}]).to_string()
                ],
            )
            .unwrap();
        assert!(
            store
                .delete_character(&current)
                .unwrap_err()
                .to_string()
                .contains("generation plan")
        );
        store
            .conn
            .execute(
                "DELETE FROM generation_plans WHERE episode_id=?1",
                [&episode.id],
            )
            .unwrap();
        let job = store
            .enqueue_grok_portrait(&entry.id, "Fixture only; never executed")
            .unwrap();
        assert!(
            store
                .delete_character(&current)
                .unwrap_err()
                .to_string()
                .contains("active generation")
        );
        store
            .conn
            .execute(
                "UPDATE jobs SET status='canceled',cancel_requested=1 WHERE id=?1",
                [&job.id],
            )
            .unwrap();
        store.delete_character(&current).unwrap();
        assert!(store.job(&job.id).is_ok());
        let location = store
            .add_library(&series.id, "location", "Market", "", None)
            .unwrap();
        assert!(store.delete_character(&location).is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
