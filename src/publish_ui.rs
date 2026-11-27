use crate::{
    GOLD, MUTED, Page, Store, Studio,
    publish::{PLATFORMS, ReleaseDraft},
};
use eframe::egui::{self, RichText};
use std::collections::HashMap;
#[derive(Default)]
pub struct Publisher {
    drafts: HashMap<String, Draft>,
    undo: HashMap<
        String,
        (
            crate::sidekick_publish::Snapshot,
            crate::sidekick_publish::Snapshot,
        ),
    >,
    jobs: HashMap<String, std::sync::mpsc::Receiver<crate::buffer_publish::Event>>,
    results: HashMap<String, String>,
}
struct Draft {
    value: ReleaseDraft,
    persisted: ReleaseDraft,
    expected: Option<String>,
    error: String,
}
impl Publisher {
    pub(crate) fn snapshot(
        &mut self,
        store: &Store,
        episode: &str,
    ) -> anyhow::Result<crate::sidekick_publish::Snapshot> {
        let record = store.get_episode(episode)?;
        let (mut value, saved) = store.release_draft(episode)?;
        if !self.drafts.contains_key(episode) {
            if saved.is_none() {
                value.title = record.title.clone();
                value.destinations = [true; 3];
            }
            if value.asset_id.is_none() {
                value.asset_id = store
                    .episode_release_exports(episode)?
                    .first()
                    .map(|a| a.id.clone());
            }
            self.drafts.insert(
                episode.into(),
                Draft {
                    persisted: value.clone(),
                    value,
                    expected: saved.clone(),
                    error: String::new(),
                },
            );
        }
        Ok(crate::sidekick_publish::Snapshot {
            episode_id: episode.into(),
            source: serde_json::to_string(&record)?,
            saved,
            draft: self.drafts[episode].value.clone(),
        })
    }
    pub(crate) fn apply_sidekick(
        &mut self,
        store: &Store,
        expected: &crate::sidekick_publish::Snapshot,
        edit: &crate::sidekick_publish::Edit,
    ) -> anyhow::Result<()> {
        let current = self.snapshot(store, &edit.episode_id)?;
        let value = crate::sidekick_publish::apply(&current, expected, edit)?;
        self.drafts.get_mut(&edit.episode_id).unwrap().value = value;
        let after = self.snapshot(store, &edit.episode_id)?;
        self.undo.insert(edit.episode_id.clone(), (current, after));
        Ok(())
    }
    pub(crate) fn undo_sidekick(&mut self, store: &Store, episode: &str) -> anyhow::Result<()> {
        let (before, after) = self
            .undo
            .get(episode)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No publishing edit to undo"))?;
        anyhow::ensure!(
            self.snapshot(store, episode)? == after,
            "Publishing draft changed after Sidekick's edit. Undo would overwrite newer work."
        );
        self.drafts.get_mut(episode).unwrap().value = before.draft;
        self.undo.remove(episode);
        Ok(())
    }
    pub(crate) fn snapshot_drafts(&self, store: &Store) -> anyhow::Result<()> {
        let directory = store.root().join("drafts");
        std::fs::create_dir_all(&directory)?;
        for (episode, draft) in &self.drafts {
            if draft.value != draft.persisted {
                std::fs::write(
                    directory.join(format!("publish-{episode}-{}.json", uuid::Uuid::new_v4())),
                    serde_json::to_vec_pretty(&draft.value)?,
                )?;
            }
        }
        Ok(())
    }
    pub fn save_for_series(&mut self, store: &Store, series: &str) -> anyhow::Result<()> {
        for episode in store.episodes(series)? {
            if let Some(draft) = self.drafts.get_mut(&episode.id) {
                save(store, &episode.id, draft)?;
            }
        }
        Ok(())
    }
}
fn save(store: &Store, episode: &str, draft: &mut Draft) -> anyhow::Result<()> {
    if draft.expected.is_none() || draft.value != draft.persisted {
        draft.expected =
            Some(store.save_release_draft(episode, &draft.value, draft.expected.as_deref())?);
        draft.persisted = draft.value.clone();
    }
    Ok(())
}
fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(crate::theme::SURFACE)
        .stroke(egui::Stroke::new(1., crate::theme::BORDER))
        .corner_radius(12)
        .inner_margin(16.)
}
impl Studio {
    pub(crate) fn publish_room(&mut self, ui: &mut egui::Ui) {
        ui.heading("Publish");
        self.episode_navigation(ui);
        let Some(episode) = self.episode_id.clone() else {
            return;
        };
        let Ok(record) = self.store.get_episode(&episode) else {
            return;
        };
        if self.selected.as_deref() != Some(&record.series_id) {
            return;
        }
        if let Err(error) = self.publisher.snapshot(&self.store, &episode) {
            ui.label(error.to_string());
            return;
        }
        if self.publisher.undo.contains_key(&episode)
            && ui.button("Undo Sidekick publishing edit").clicked()
            && let Err(error) = self.publisher.undo_sidekick(&self.store, &episode)
        {
            self.publisher.drafts.get_mut(&episode).unwrap().error = error.to_string();
        }
        let exports = match self.store.episode_release_exports(&episode) {
            Ok(exports) => exports,
            Err(error) => {
                ui.label(error.to_string());
                return;
            }
        };
        let Ok(filename) = self.store.episode_video_filename(&episode) else {
            return;
        };
        let draft = self.publisher.drafts.get_mut(&episode).unwrap();
        ui.add_space(8.);
        if draft.value.asset_id.is_none() {
            draft.value.asset_id = exports.first().map(|a| a.id.clone());
        }
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Episode video");
            if exports.is_empty() {
                ui.label("Export this episode from the Editing room first.");
                if ui.button("Open Editing room").clicked() {
                    self.page = Page::EditingRoom;
                }
            } else {
                let selected = exports
                    .iter()
                    .find(|a| Some(&a.id) == draft.value.asset_id.as_ref());
                egui::ComboBox::from_id_salt("publish-export")
                    .selected_text(
                        selected
                            .map(|a| format!("{} · {:.1}s", filename, a.duration))
                            .unwrap_or_else(|| "Choose an export".into()),
                    )
                    .width(ui.available_width().min(500.))
                    .show_ui(ui, |ui| {
                        for asset in &exports {
                            ui.selectable_value(
                                &mut draft.value.asset_id,
                                Some(asset.id.clone()),
                                format!("{} · {:.1}s", filename, asset.duration),
                            );
                        }
                    });
                if let Some(asset) = exports
                    .iter()
                    .find(|a| Some(&a.id) == draft.value.asset_id.as_ref())
                {
                    ui.label(
                        RichText::new(format!(
                            "{} × {} · {:.1}s",
                            asset.width, asset.height, asset.duration
                        ))
                        .color(MUTED),
                    );
                    if !asset.path.is_file() {
                        ui.colored_label(
                            GOLD,
                            "Video unavailable locally. Download or restore the project media.",
                        );
                    }
                    if ui.button("Show video in Finder").clicked() {
                        let result = (|| -> anyhow::Result<()> {
                            let path = self.store.finished_episode_copy(
                                &filename,
                                &asset.path,
                                &asset.hash,
                            )?;
                            std::process::Command::new("open")
                                .arg("-R")
                                .arg(&path)
                                .spawn()?;
                            Ok(())
                        })();
                        if let Err(e) = result {
                            draft.error = format!("Could not reveal episode video: {e}");
                        }
                    }
                }
            }
        });
        ui.add_space(6.);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Release details");
            ui.add(
                egui::TextEdit::singleline(&mut draft.value.title)
                    .hint_text("Episode title")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal_wrapped(|ui| {
                for (index, name) in PLATFORMS.iter().enumerate() {
                    ui.checkbox(&mut draft.value.destinations[index], *name);
                }
            });
            for (index, platform) in PLATFORMS.iter().enumerate() {
                if draft.value.destinations[index] {
                    ui.label(*platform);
                    ui.add(
                        egui::TextEdit::multiline(&mut draft.value.captions[index])
                            .hint_text("Caption, description and hashtags")
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                }
            }
        });
        ui.add_space(6.);
        if draft.value.destinations[0] {
            card().show(ui, |ui| {
                ui.strong("TikTok cover");
                ui.label("Adds 0.2 seconds of artwork before the TikTok video and uses it as the cover. Other platforms use the original episode.");
                if !draft.value.tiktok_cover_png.is_empty() {
                    use std::hash::{Hash, Hasher};
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    draft.value.tiktok_cover_png.hash(&mut hash);
                    ui.add(egui::Image::from_bytes(format!("bytes://tiktok-cover/{episode}/{}", hash.finish()), draft.value.tiktok_cover_png.clone()).max_size(egui::vec2(120., 180.)));
                }
                ui.horizontal(|ui| {
                    if ui.button("Choose cover…").clicked()
                        && let Some(path) = rfd::FileDialog::new().add_filter("Cover image", &["png", "jpg", "jpeg"]).pick_file() {
                        let result = (|| -> anyhow::Result<Vec<u8>> {
                            anyhow::ensure!(std::fs::metadata(&path)?.len() <= 20 * 1024 * 1024, "Choose an image smaller than 20 MB");
                            let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
                            reader.limits(image::Limits::default());
                            let image = reader.decode()?.thumbnail(1080, 1920);
                            let mut bytes = std::io::Cursor::new(Vec::new());
                            image.write_to(&mut bytes, image::ImageFormat::Png)?;
                            Ok(bytes.into_inner())
                        })();
                        match result {
                            Ok(bytes) => { draft.value.tiktok_cover_png = bytes; draft.error.clear(); }
                            Err(error) => draft.error = error.to_string(),
                        }
                    }
                    if !draft.value.tiktok_cover_png.is_empty() && ui.button("Remove cover").clicked() {
                        draft.value.tiktok_cover_png.clear();
                    }
                });
            });
        }
        if ui.button("Save publishing draft").clicked()
            && let Err(error) = save(&self.store, &episode, draft)
        {
            draft.error = error.to_string();
        }
        let credentials_ready = crate::credentials::Provider::Cloudinary.saved_locally()
            && crate::credentials::Provider::Buffer.saved_locally();
        if !credentials_ready {
            ui.colored_label(
                GOLD,
                "Add Cloudinary and Buffer credentials in Settings before publishing.",
            );
        }
        if let Some(rx) = self.publisher.jobs.get(&episode) {
            match rx.try_recv() {
                Ok(crate::buffer_publish::Event::Progress(message)) => {
                    self.publisher.results.insert(episode.clone(), message);
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                Ok(crate::buffer_publish::Event::Finished(message)) => {
                    self.publisher.results.insert(episode.clone(), message);
                    self.publisher.jobs.remove(&episode);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.spinner();
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));
                }
                Err(_) => {
                    self.publisher.jobs.remove(&episode);
                    self.publisher.results.insert(
                        episode.clone(),
                        "Publishing interrupted. Check Buffer.".into(),
                    );
                }
            }
        }
        let asset = exports
            .iter()
            .find(|a| Some(&a.id) == draft.value.asset_id.as_ref());
        let ready = asset.is_some_and(|a| a.path.is_file())
            && draft.value.destinations.contains(&true)
            && draft
                .value
                .destinations
                .iter()
                .enumerate()
                .all(|(i, enabled)| !enabled || !draft.value.captions[i].trim().is_empty())
            && (!draft.value.destinations[1] || !draft.value.title.trim().is_empty())
            && credentials_ready
            && !self.publisher.jobs.contains_key(&episode);
        if ui
            .add_enabled(ready, crate::theme::primary("Publish"))
            .on_hover_text("Publish immediately to each selected platform")
            .clicked()
        {
            if let Err(error) = save(&self.store, &episode, draft) {
                draft.error = error.to_string();
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            self.publisher.jobs.insert(episode.clone(), rx);
            self.publisher.results.remove(&episode);
            let (episode, db, asset, value) = (
                episode.clone(),
                self.store.path.clone(),
                asset.unwrap().clone(),
                draft.value.clone(),
            );
            std::thread::spawn(move || {
                let result = crate::buffer_publish::publish(&db, &episode, &asset, &value, &tx);
                let _ = tx.send(crate::buffer_publish::Event::Finished(
                    result.unwrap_or_else(|e| format!("Publishing stopped: {e}")),
                ));
            });
            ui.ctx().request_repaint();
        }
        ui.weak("Uploads to Cloudinary, then sends every selected platform to Buffer for immediate publishing. Platform processing times may differ.");
        if !self.publisher.results.contains_key(&episode)
            && let Ok(report) = self.store.conn.query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("episode-publish-result:{episode}")],
                |r| r.get::<_, String>(0),
            )
        {
            self.publisher.results.insert(episode.clone(), report);
        }
        if let Some(status) = self.publisher.results.get(&episode) {
            card().show(ui, |ui| {
                ui.strong("Publishing status");
                ui.label(status);
                ui.hyperlink_to("Open Buffer", "https://publish.buffer.com/");
            });
        }
        ui.add_space(8.);
        if draft.error.is_empty() {
            ui.label(
                RichText::new(if draft.value == draft.persisted {
                    "Draft saved locally - Save project to sync"
                } else {
                    "Unsaved publishing changes"
                })
                .small()
                .color(MUTED),
            );
        } else {
            ui.colored_label(GOLD, &draft.error);
            if ui.button("Reload saved release").clicked() {
                self.publisher.drafts.remove(&episode);
            }
        }
    }
}

#[cfg(test)]
mod sidekick_tests {
    use super::*;
    #[test]
    fn sidekick_edits_are_guarded_undoable_and_do_not_publish() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("Series", "").unwrap();
        let episode = store.create_episode(&series.id, "Episode").unwrap();
        let mut publisher = Publisher::default();
        let before = publisher.snapshot(&store, &episode.id).unwrap();
        let edit = crate::sidekick_publish::Edit {
            episode_id: episode.id.clone(),
            title: Some("New title".into()),
            tiktok: Some("TikTok text".into()),
            youtube: Some("YouTube text".into()),
            instagram: None,
        };
        publisher.apply_sidekick(&store, &before, &edit).unwrap();
        assert!(store.release_draft(&episode.id).unwrap().1.is_none());
        assert_eq!(
            publisher.drafts[&episode.id].value.captions[1],
            "YouTube text"
        );
        assert_eq!(
            publisher.drafts[&episode.id].value.destinations,
            before.draft.destinations
        );
        publisher.undo_sidekick(&store, &episode.id).unwrap();
        assert!(publisher.drafts[&episode.id].value == before.draft);
        publisher.drafts.get_mut(&episode.id).unwrap().value.title = "Manual change".into();
        assert!(publisher.apply_sidekick(&store, &before, &edit).is_err());
        let expected = publisher.snapshot(&store, &episode.id).unwrap();
        publisher.apply_sidekick(&store, &expected, &edit).unwrap();
        publisher
            .drafts
            .get_mut(&episode.id)
            .unwrap()
            .value
            .captions[2] = "Newer manual text".into();
        assert!(publisher.undo_sidekick(&store, &episode.id).is_err());
        let expected = publisher.snapshot(&store, &episode.id).unwrap();
        store
            .save_release_draft(&episode.id, &ReleaseDraft::default(), None)
            .unwrap();
        assert!(publisher.apply_sidekick(&store, &expected, &edit).is_err());
        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM buffer_submissions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
