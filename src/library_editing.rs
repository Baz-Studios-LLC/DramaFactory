//! Brief revisions keep stable cast/location IDs, voices and reference images.
use crate::{production::LibraryEntry, store::Store};
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryEdit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub expected_name: String,
    pub expected_description: String,
}
impl LibraryEdit {
    pub(crate) fn from_entry(entry: &LibraryEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            expected_name: entry.name.clone(),
            expected_description: entry.description.clone(),
        }
    }
}
impl Store {
    pub fn update_library_entry(&self, edit: &LibraryEdit) -> Result<LibraryEntry> {
        if edit.name.trim().is_empty() || edit.name.len() > 200 || edit.description.len() > 12000 {
            bail!("Library entry needs a name up to 200 bytes and a brief up to 12000 bytes");
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let (series, kind): (String, String) = self
            .conn
            .query_row(
                "SELECT series_id,kind FROM library WHERE id=?1",
                [&edit.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("Library entry not found")?;
        let mut entry = self
            .library(&series, &kind)?
            .into_iter()
            .find(|e| e.id == edit.id)
            .context("Library entry not found")?;
        if entry.name == edit.name && entry.description == edit.description {
            tx.commit()?;
            return Ok(entry);
        }
        if entry.name != edit.expected_name || entry.description != edit.expected_description {
            bail!(
                "This brief changed in another session; your draft is preserved. Reload and reconcile before saving"
            );
        }
        self.conn.execute(
            "UPDATE library SET name=?2,description=?3 WHERE id=?1",
            params![edit.id, edit.name, edit.description],
        )?;
        entry.name = edit.name.clone();
        entry.description = edit.description.clone();
        tx.commit()?;
        Ok(entry)
    }
}
impl crate::Studio {
    pub(crate) fn library_brief_editor(&mut self, ui: &mut eframe::egui::Ui, entry: &LibraryEntry) {
        ui.push_id((&entry.id,"brief"),|ui|ui.collapsing("Edit name and brief",|ui| {
            let mut draft=self.library_drafts.get(&entry.id).cloned().unwrap_or_else(||LibraryEdit::from_entry(entry));
            let stale=draft.expected_name!=entry.name || draft.expected_description!=entry.description;
            if stale {ui.label(eframe::egui::RichText::new("A newer brief is saved. Your draft is preserved; reload and reconcile.").color(crate::GOLD));}
            let mut changed=ui.text_edit_singleline(&mut draft.name).changed();
            changed|=ui.add(eframe::egui::TextEdit::multiline(&mut draft.description).desired_width(f32::INFINITY).desired_rows(4)).changed();
            if changed {self.library_drafts.insert(entry.id.clone(),draft.clone());}
            ui.horizontal(|ui| {
                if ui.button("Save brief").clicked() {
                    let result=self.store.update_library_entry(&draft);
                    if result.is_ok() {self.library_drafts.remove(&entry.id);}
                    self.result(result.map(|_|()),"Brief saved; reference image and voice preserved");
                }
                if ui.button("Reload saved brief").clicked(){self.library_drafts.remove(&entry.id);}
            });
            ui.small("Changing a brief affects future direction and final-cut continuity checks. Review existing coverage after saving.");
        }));
    }
}
