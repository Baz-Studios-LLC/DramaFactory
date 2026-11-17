//! The desktop has one current series, persisted independently of episode drafts.
use crate::{Page, Studio, store::Store, synopsis::SynopsisDraft};
use anyhow::{Result, bail};
use rusqlite::OptionalExtension;
impl Store {
    pub(crate) fn current_gui_series(&self) -> Result<Option<String>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key='gui:current_series'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let series = self.series()?;
        Ok(id.filter(|id| series.iter().any(|item| &item.id == id)))
    }
    pub(crate) fn current_gui_episode(&self, series: &str) -> Result<Option<String>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("gui:episode:{series}")],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id.filter(|id| {
            self.get_episode(id)
                .is_ok_and(|episode| episode.series_id == series)
        }))
    }
    pub(crate) fn remember_gui_episode(&self, id: &str) -> Result<()> {
        let episode = self.get_episode(id)?;
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![format!("gui:episode:{}",episode.series_id),id])?;
        Ok(())
    }
    pub(crate) fn ordered_gui_episodes(&self, series: &str) -> Result<Vec<crate::store::Episode>> {
        let mut episodes = self.episodes(series)?;
        let plan = self.series_plan(series)?;
        let order: std::collections::HashMap<_, _> = plan
            .episodes
            .iter()
            .enumerate()
            .map(|(index, outline)| (outline.episode_id.as_str(), index))
            .collect();
        episodes.sort_by_key(|episode| {
            order
                .get(episode.id.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        Ok(episodes)
    }
    pub(crate) fn remember_gui_series(&self, id: &str) -> Result<()> {
        if !self.series()?.iter().any(|series| series.id == id) {
            bail!("Series no longer exists");
        }
        self.conn.execute("INSERT INTO settings(key,value) VALUES('gui:current_series',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[id])?;
        Ok(())
    }
}
impl Studio {
    fn preserve_series_work(&mut self) -> bool {
        if !self.save_script() {
            return false;
        }
        if let Err(error) = self
            .snapshot_new_dialogue()
            .and_then(|_| self.snapshot_shot_form())
        {
            self.message = format!("Could not preserve unfinished work: {error}");
            return false;
        }
        true
    }
    pub(crate) fn start_new_series(&mut self) {
        if !self.preserve_series_work() {
            return;
        }
        // Give the conversation a durable series identity immediately, while
        // leaving the actual synopsis blank for explicit authoring and saving.
        let series = match self.store.create_series("Untitled series", "") {
            Ok(series) => series,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        if !self.activate_series(&series.id) {
            return;
        }
        self.synopsis_drafts.insert(
            series.id.clone(),
            SynopsisDraft {
                id: series.id,
                title: String::new(),
                premise: String::new(),
                expected_title: series.name,
                expected_premise: series.premise,
            },
        );
        self.page = Page::Synopsis;
        self.message.clear();
    }
    pub(crate) fn select_series(&mut self, id: &str) -> bool {
        if self.selected.as_deref() == Some(id) {
            return true;
        }
        if !self.preserve_series_work() {
            return false;
        }
        self.activate_series(id)
    }
    fn activate_series(&mut self, id: &str) -> bool {
        if let Err(error) = self.store.remember_gui_series(id) {
            self.message = error.to_string();
            return false;
        }
        self.remember_new_line();
        self.stop_playback();
        self.image_inspector = None;
        self.selected = Some(id.into());
        self.delete_character = None;
        self.sidekick.reset_context();
        self.episode_id = None;
        self.edit_shot = None;
        self.shot_workspace = Default::default();
        self.dialogue_workspace = Default::default();
        self.dialogue_shot = None;
        self.episode_search.clear();
        self.script.clear();
        self.script_baseline.clear();
        if let Ok(Some(episode)) = self
            .store
            .current_gui_episode(id)
            .and_then(|id| id.map(|id| self.store.get_episode(&id)).transpose())
        {
            self.episode_id = Some(episode.id);
            self.script = episode.screenplay;
            self.script_baseline = self.script.clone();
        }
        self.restore_new_line();
        self.request.shot_id = None;
        self.request.image = None;
        self.request.image_hash = None;
        self.request.selected_take_before = None;
        self.request.direction_context = serde_json::Value::Null;
        self.image_request.library_id = None;
        self.image_request.library_reference_before = None;
        self.image_request.library_context_before = serde_json::Value::Null;
        self.image_request.reference = None;
        self.image_request.reference_hash = None;
        self.library_reference = None;
        true
    }
    pub(crate) fn select_episode(&mut self, id: &str) -> bool {
        if self.episode_id.as_deref() == Some(id) {
            return true;
        }
        let episode = match self.store.get_episode(id) {
            Ok(episode) if Some(&episode.series_id) == self.selected.as_ref() => episode,
            _ => {
                self.message = "Choose an episode from the current series".into();
                return false;
            }
        };
        if !self.save_script() {
            return false;
        }
        if let Err(error) = self
            .snapshot_new_dialogue()
            .and_then(|_| self.snapshot_shot_form())
        {
            self.message = format!("Could not preserve unfinished work: {error}");
            return false;
        }
        if let Err(error) = self.store.remember_gui_episode(id) {
            self.message = error.to_string();
            return false;
        }
        self.remember_new_line();
        self.stop_playback();
        self.image_inspector = None;
        self.episode_id = Some(episode.id);
        self.script = episode.screenplay;
        self.script_baseline = self.script.clone();
        self.edit_shot = None;
        self.shot_workspace = Default::default();
        self.dialogue_workspace = Default::default();
        self.dialogue_shot = None;
        self.restore_new_line();
        self.request.shot_id = None;
        self.request.selected_take_before = None;
        self.request.direction_context = serde_json::Value::Null;
        true
    }
    pub(crate) fn current_series_picker(&mut self, ui: &mut eframe::egui::Ui) {
        ui.label(
            eframe::egui::RichText::new("CURRENT SERIES")
                .size(10.)
                .color(crate::GOLD),
        );
        let series = self.load(self.store.series());
        let name = series
            .iter()
            .find(|series| Some(&series.id) == self.selected.as_ref())
            .map(|series| series.name.as_str())
            .unwrap_or("Choose a series");
        eframe::egui::ComboBox::from_id_salt("global-series")
            .selected_text(name)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for series in series {
                    if ui
                        .selectable_label(self.selected.as_ref() == Some(&series.id), &series.name)
                        .clicked()
                        && self.select_series(&series.id)
                    {
                        ui.close();
                    }
                }
            });
        ui.add_space(6.);
        if ui
            .add_sized(
                [ui.available_width(), 30.],
                eframe::egui::Button::new("+ New series"),
            )
            .clicked()
        {
            self.start_new_series();
        }
    }
}

impl Store {
    pub(crate) fn video_aspect_ratio(&self) -> &'static str {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key='video:aspect_ratio'",
                [],
                |r| r.get(0),
            )
            .ok();
        if value.as_deref() == Some("16:9") {
            "16:9"
        } else {
            "9:16"
        }
    }
    pub(crate) fn video_dimensions(&self, short: u32, long: u32) -> (u32, u32) {
        if self.video_aspect_ratio() == "16:9" {
            (long, short)
        } else {
            (short, long)
        }
    }
    pub(crate) fn set_video_aspect_ratio(&self, ratio: &str) -> Result<()> {
        if !["9:16", "16:9"].contains(&ratio) {
            bail!("Choose portrait or landscape");
        }
        self.conn.execute("INSERT INTO settings(key,value) VALUES('video:aspect_ratio',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [ratio])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_series_survives_restart_and_rejects_unknown_ids() {
        let root = std::env::temp_dir().join(format!("df-context-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("Current project", "").unwrap();
        assert!(store.current_gui_series().unwrap().is_none());
        store.remember_gui_series(&series.id).unwrap();
        assert!(store.remember_gui_series("missing").is_err());
        let first = store.create_episode(&series.id, "First created").unwrap();
        let second = store.create_episode(&series.id, "Second created").unwrap();
        let mut plan = store.series_plan(&series.id).unwrap();
        plan.episodes = vec![
            crate::story::EpisodeOutline {
                episode_id: second.id.clone(),
                ..Default::default()
            },
            crate::story::EpisodeOutline {
                episode_id: first.id.clone(),
                ..Default::default()
            },
        ];
        store.save_series_plan(&plan).unwrap();
        assert_eq!(
            store.ordered_gui_episodes(&series.id).unwrap()[0].id,
            second.id
        );
        store.remember_gui_episode(&second.id).unwrap();
        assert!(store.remember_gui_episode("missing").is_err());
        drop(store);
        let reopened = Store::open(path).unwrap();
        assert_eq!(
            reopened.current_gui_series().unwrap().as_deref(),
            Some(series.id.as_str())
        );
        assert_eq!(
            reopened.current_gui_episode(&series.id).unwrap().as_deref(),
            Some(second.id.as_str())
        );
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
