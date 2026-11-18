use crate::{
    GOLD, MUTED, Page, Store, Studio,
    publish::{PLATFORMS, ReleaseDraft},
};
use eframe::egui::{self, RichText};
use std::collections::HashMap;
#[derive(Default)]
pub struct Publisher {
    drafts: HashMap<String, Draft>,
    hosting: Option<[String; 2]>,
    jobs: HashMap<String, std::sync::mpsc::Receiver<String>>,
    results: HashMap<String, String>,
}
struct Draft {
    value: ReleaseDraft,
    persisted: ReleaseDraft,
    expected: Option<String>,
    error: String,
}
impl Publisher {
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
        if !self.publisher.drafts.contains_key(&episode) {
            match self.store.release_draft(&episode) {
                Ok((mut value, expected)) => {
                    if expected.is_none() {
                        value.title = record.title;
                        value.destinations = [true; 3];
                    }
                    self.publisher.drafts.insert(
                        episode.clone(),
                        Draft {
                            persisted: value.clone(),
                            value,
                            expected,
                            error: String::new(),
                        },
                    );
                }
                Err(error) => {
                    ui.label(error.to_string());
                    return;
                }
            }
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
            ui.add(
                egui::TextEdit::multiline(&mut draft.value.captions[0])
                    .hint_text("Caption for all selected channels")
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(6.);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Cover artwork");
            ui.horizontal(|ui| {
                if !draft.value.cover_png.is_empty() {
                    let uri = format!("bytes://release-cover/{}/{}", episode, { use std::hash::{Hash, Hasher}; let mut hash = std::hash::DefaultHasher::new(); draft.value.cover_png.hash(&mut hash); hash.finish() });
                    ui.add(egui::Image::from_bytes(uri, draft.value.cover_png.clone()).max_size(egui::vec2(100., 150.)));
                }
                ui.vertical(|ui| {
                    if ui.button(if draft.value.cover_png.is_empty() { "Choose image…" } else { "Replace image…" }).clicked()
                        && let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                        let result = (|| -> anyhow::Result<Vec<u8>> {
                            anyhow::ensure!(std::fs::metadata(&path)?.len() <= 20 * 1024 * 1024, "Choose an image smaller than 20 MB.");
                            let img = image::open(path)?.thumbnail(1080, 1920);
                            let mut bytes = std::io::Cursor::new(Vec::new());
                            img.write_to(&mut bytes, image::ImageFormat::Png)?;
                            Ok(bytes.into_inner())
                        })();
                        match result { Ok(bytes) => { draft.value.cover_png = bytes; draft.error.clear(); }, Err(error) => draft.error = error.to_string() }
                    }
                    if !draft.value.cover_png.is_empty() && ui.small_button("Remove cover").clicked() { draft.value.cover_png.clear(); }
                    ui.label(RichText::new("Artwork is saved with the project. Video-frame cover selection will be added with publishing.").small().color(MUTED));
                });
            });
        });
        if let Err(error) = save(&self.store, &episode, draft) {
            draft.error = error.to_string();
        }
        let hosting = self.publisher.hosting.get_or_insert_with(|| {
            self.store
                .conn
                .query_row(
                    "SELECT value FROM settings WHERE key='buffer-hosting'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        });
        egui::CollapsingHeader::new("Video hosting")
            .default_open(hosting[0].is_empty())
            .show(ui, |ui| {
                ui.label("Set up Cloudinary once to upload episode videos directly from this Mac.");
                let cloud =
                    ui.add(egui::TextEdit::singleline(&mut hosting[0]).hint_text("Cloud name"));
                let preset = ui.add(
                    egui::TextEdit::singleline(&mut hosting[1]).hint_text("Unsigned upload preset"),
                );
                if (cloud.changed() || preset.changed())
                    && let Err(e) = self.store.conn.execute(
                        "INSERT OR REPLACE INTO settings(key,value) VALUES('buffer-hosting',?1)",
                        [serde_json::to_string(hosting).unwrap()],
                    )
                {
                    draft.error = e.to_string();
                }
            });
        if let Some(rx) = self.publisher.jobs.get(&episode) {
            match rx.try_recv() {
                Ok(message) => {
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
            && !draft.value.captions[0].trim().is_empty()
            && hosting.iter().all(|s| !s.trim().is_empty())
            && !self.publisher.jobs.contains_key(&episode)
            && draft.value == draft.persisted;
        if ui
            .add_enabled(ready, egui::Button::new("Publish to Buffer"))
            .on_hover_text("Add to the next queue slot for each selected channel")
            .clicked()
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.publisher.jobs.insert(episode.clone(), rx);
            self.publisher.results.remove(&episode);
            let (episode, db, asset, value, hosting) = (
                episode.clone(),
                self.store.path.clone(),
                asset.unwrap().clone(),
                draft.value.clone(),
                hosting.clone(),
            );
            std::thread::spawn(move || {
                let result = crate::buffer_publish::queue(
                    &db,
                    &episode,
                    &asset,
                    &value,
                    &hosting[0],
                    &hosting[1],
                );
                let _ = tx.send(result.unwrap_or_else(|e| e.to_string()));
            });
            ui.ctx().request_repaint();
        }
        if let Some(status) = self.publisher.results.get(&episode) {
            ui.label(status);
        }
        ui.add_space(8.);
        if draft.error.is_empty() {
            ui.label(
                RichText::new(
                    "Draft saved locally · Save project to sync it to your project folder",
                )
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
