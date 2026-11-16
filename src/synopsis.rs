use crate::{Page, Studio, store::Store};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynopsisDraft {
    pub id: String,
    pub title: String,
    pub premise: String,
    pub expected_title: String,
    pub expected_premise: String,
}
impl Store {
    pub fn save_synopsis(&self, draft: &SynopsisDraft) -> Result<()> {
        if draft.title.trim().is_empty() {
            bail!("Series title is required");
        }
        let changed = self.conn.execute("UPDATE series SET name=?2,premise=?3 WHERE id=?1 AND ((name=?4 AND premise=?5) OR (name=?2 AND premise=?3))",rusqlite::params![draft.id,draft.title,draft.premise,draft.expected_title,draft.expected_premise])?;
        if changed == 0 {
            bail!(
                "Synopsis changed elsewhere or was removed. Your draft is preserved; reload the saved version before editing again."
            );
        }
        Ok(())
    }
}
impl Studio {
    pub(crate) fn synopsis(&mut self, ui: &mut eframe::egui::Ui) {
        Self::title(
            ui,
            "SYNOPSIS",
            "Start with a story worth following.",
            "Define the central idea. This synopsis guides the cast, full series plot, and every episode.",
        );
        if ui.button("+ Start a new series").clicked() {
            self.new_open = true;
        }
        ui.small("Editing this synopsis changes the current series. Start a new series for a different story; its cast and episodes begin empty.");
        ui.add_space(12.);
        let Some(id) = self.selected.clone() else {
            ui.label("Create or choose a series to begin.");
            if ui.button("Create a series").clicked() {
                self.new_open = true;
            }
            return;
        };
        let Some(saved) = self
            .load(self.store.series())
            .into_iter()
            .find(|series| series.id == id)
        else {
            ui.label("This series is no longer available.");
            return;
        };
        let draft = self
            .synopsis_drafts
            .entry(id.clone())
            .or_insert_with(|| SynopsisDraft {
                id: id.clone(),
                title: saved.name.clone(),
                premise: saved.premise.clone(),
                expected_title: saved.name.clone(),
                expected_premise: saved.premise.clone(),
            });
        let dirty = draft.title != draft.expected_title || draft.premise != draft.expected_premise;
        if !dirty && (draft.expected_title != saved.name || draft.expected_premise != saved.premise)
        {
            *draft = SynopsisDraft {
                id: id.clone(),
                title: saved.name.clone(),
                premise: saved.premise.clone(),
                expected_title: saved.name.clone(),
                expected_premise: saved.premise.clone(),
            };
        }
        let mut draft = draft.clone();
        Self::card(ui, |ui| {
            ui.label("SERIES TITLE");
            ui.add(
                eframe::egui::TextEdit::singleline(&mut draft.title)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(12.);
            ui.label("THE IDEA");
            ui.small("Who is at the center? What do they want, what stands in their way, and why will we keep watching?");
            ui.add(
                eframe::egui::TextEdit::multiline(&mut draft.premise)
                    .desired_rows(10)
                    .desired_width(ui.available_width()),
            );
            let dirty =
                draft.title != draft.expected_title || draft.premise != draft.expected_premise;
            ui.small(if dirty { "Unsaved changes" } else { "Saved" });
            if draft.expected_title != saved.name || draft.expected_premise != saved.premise {
                ui.label("The saved synopsis changed elsewhere. Your draft remains here; saving will check for conflicts.");
            }
            if let Some((before, after)) = self.sidekick.synopsis_undo.get(&id).cloned()
                && ui.button("Undo sidekick edit").clicked()
            {
                let result = crate::sidekick_edit::undo(&mut draft, &before, &after);
                if result.is_ok() {
                    self.sidekick.synopsis_undo.remove(&id);
                }
                self.result(result, "Sidekick edit undone; draft not saved");
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        dirty && !draft.title.trim().is_empty(),
                        eframe::egui::Button::new("Save synopsis"),
                    )
                    .clicked()
                {
                    let result = self.store.save_synopsis(&draft);
                    if result.is_ok() {
                        draft.expected_title = draft.title.clone();
                        draft.expected_premise = draft.premise.clone();
                    }
                    self.result(result, "Synopsis saved");
                }
                if ui.button("Reload saved synopsis").clicked() {
                    draft.title = saved.name.clone();
                    draft.premise = saved.premise.clone();
                    draft.expected_title = saved.name.clone();
                    draft.expected_premise = saved.premise.clone();
                }
                if ui
                    .add_enabled(!dirty, eframe::egui::Button::new("Continue to characters"))
                    .clicked()
                {
                    self.page = Page::Cast;
                }
            });
            ui.small("AI development through OpenRouter is being connected. You can write and save your synopsis here now.");
        });
        self.synopsis_drafts.insert(id, draft);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_synopsis_cannot_clobber_newer_saved_story() {
        let root = std::env::temp_dir().join(format!("df-synopsis-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Original", "Premise").unwrap();
        let mut draft = SynopsisDraft {
            id: series.id.clone(),
            title: "First edit".into(),
            premise: "Changed".into(),
            expected_title: series.name,
            expected_premise: series.premise,
        };
        store.save_synopsis(&draft).unwrap();
        store.save_synopsis(&draft).unwrap();
        draft.title = "Stale edit".into();
        assert!(store.save_synopsis(&draft).is_err());
        assert_eq!(store.series().unwrap()[0].name, "First edit");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
