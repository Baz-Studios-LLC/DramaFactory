use crate::{
    GOLD, MUTED, Page, Store, Studio,
    publish::{PLATFORMS, ReleaseDraft},
};
use eframe::egui::{self, RichText};
use std::collections::HashMap;
#[derive(Default)]
pub struct Publisher {
    drafts: HashMap<String, Draft>,
    platform: usize,
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
        let draft = self.publisher.drafts.get_mut(&episode).unwrap();
        ui.add_space(8.);
        ui.label(
            RichText::new(
                "Prepare your release here. Account connections and posting are coming next.",
            )
            .color(MUTED),
        );
        ui.add_space(8.);
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
                            .map(|a| {
                                format!(
                                    "{} · {:.1}s",
                                    a.path.file_name().unwrap_or_default().to_string_lossy(),
                                    a.duration
                                )
                            })
                            .unwrap_or_else(|| "Choose an export".into()),
                    )
                    .width(ui.available_width().min(500.))
                    .show_ui(ui, |ui| {
                        for asset in &exports {
                            ui.selectable_value(
                                &mut draft.value.asset_id,
                                Some(asset.id.clone()),
                                format!(
                                    "{} · {:.1}s",
                                    asset.path.file_name().unwrap_or_default().to_string_lossy(),
                                    asset.duration
                                ),
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
                        let _ = std::process::Command::new("open")
                            .arg("-R")
                            .arg(&asset.path)
                            .spawn();
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
            ui.horizontal_wrapped(|ui| {
                for (index, name) in PLATFORMS.iter().enumerate() {
                    ui.selectable_value(&mut self.publisher.platform, index, *name);
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut draft.value.captions[self.publisher.platform])
                    .hint_text("Caption, description and hashtags…")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.small(format!(
                    "{} characters",
                    draft.value.captions[self.publisher.platform]
                        .chars()
                        .count()
                ));
                if ui.button("Copy caption").clicked() {
                    ui.ctx()
                        .copy_text(draft.value.captions[self.publisher.platform].clone());
                }
                if ui.button("Use for all platforms").clicked() {
                    draft.value.captions = std::array::from_fn(|_| {
                        draft.value.captions[self.publisher.platform].clone()
                    });
                }
            });
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
