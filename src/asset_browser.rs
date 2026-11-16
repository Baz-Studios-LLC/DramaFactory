//! Screening-room navigation keeps failed takes discoverable without loading every card.
use crate::asset_catalog::{AssetPage, AssetQuery};
use crate::production::Asset;
pub struct AssetBrowser {
    pub query: AssetQuery,
    page: Option<AssetPage>,
    error: Option<String>,
    dirty: bool,
    last_refresh: std::time::Instant,
}
impl Default for AssetBrowser {
    fn default() -> Self {
        Self {
            query: AssetQuery::default(),
            page: None,
            error: None,
            dirty: true,
            last_refresh: std::time::Instant::now(),
        }
    }
}
impl AssetBrowser {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn show_asset(&mut self, id: &str) {
        self.query = AssetQuery {
            text: id.into(),
            ..Default::default()
        };
        self.invalidate();
    }
}
impl crate::Studio {
    pub(crate) fn catalog_assets(&mut self, ui: &mut eframe::egui::Ui) -> Option<Vec<Asset>> {
        let before = self.asset_browser.query.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Search");
            ui.add(
                eframe::egui::TextEdit::singleline(&mut self.asset_browser.query.text)
                    .hint_text("Labels, prompts or review notes")
                    .desired_width(280.),
            );
            eframe::egui::ComboBox::from_id_salt("media-filter")
                .selected_text(match self.asset_browser.query.media.as_str() {
                    "video" => "Videos",
                    "image" => "Images",
                    "audio" => "Audio",
                    _ => "All media",
                })
                .show_ui(ui, |ui| {
                    for (key, label) in [
                        ("all", "All media"),
                        ("video", "Videos"),
                        ("image", "Images"),
                        ("audio", "Audio"),
                    ] {
                        ui.selectable_value(&mut self.asset_browser.query.media, key.into(), label);
                    }
                });
            eframe::egui::ComboBox::from_id_salt("review-filter")
                .selected_text(match self.asset_browser.query.review.as_str() {
                    "approved" => "Approved",
                    "rejected" => "Rejected",
                    "unreviewed" => "Unreviewed",
                    _ => "All reviews",
                })
                .show_ui(ui, |ui| {
                    for (key, label) in [
                        ("all", "All reviews"),
                        ("unreviewed", "Unreviewed"),
                        ("approved", "Approved"),
                        ("rejected", "Rejected"),
                    ] {
                        ui.selectable_value(
                            &mut self.asset_browser.query.review,
                            key.into(),
                            label,
                        );
                    }
                });
            if ui.button("Clear filters").clicked() {
                self.asset_browser.query = AssetQuery::default();
            }
            if ui.button("Refresh").clicked() {
                self.asset_browser.invalidate();
            }
        });
        if self.asset_browser.query != before {
            self.asset_browser.query.offset = 0;
            self.asset_browser.invalidate();
        }
        if self.asset_browser.dirty
            || self.asset_browser.last_refresh.elapsed() > std::time::Duration::from_secs(2)
        {
            self.asset_browser.dirty = false;
            self.asset_browser.last_refresh = std::time::Instant::now();
            match self.store.find_assets(&self.asset_browser.query) {
                Ok(page) => {
                    self.asset_browser.page = Some(page);
                    self.asset_browser.error = None;
                }
                Err(error) => {
                    self.asset_browser.error = Some(error.to_string());
                    self.asset_browser.page = None;
                }
            }
        }
        if let Some(error) = &self.asset_browser.error {
            ui.label(eframe::egui::RichText::new(error).color(crate::GOLD));
            ui.small(
                "Your media and filters are preserved. Refresh to try loading this view again.",
            );
            return None;
        }
        let page = self.asset_browser.page.as_ref()?;
        let assets = page.assets.clone();
        let total = page.total;
        let offset = page.offset;
        let limit = page.limit;
        ui.horizontal(|ui| {
            ui.label(if assets.is_empty() {
                format!("{total} matching assets")
            } else {
                format!(
                    "{}–{} of {total} · newest first",
                    offset as u64 + 1,
                    offset as u64 + assets.len() as u64
                )
            });
            if ui
                .add_enabled(offset > 0, eframe::egui::Button::new("Previous"))
                .clicked()
            {
                self.asset_browser.query.offset = offset.saturating_sub(limit);
                self.asset_browser.invalidate();
            }
            if ui
                .add_enabled(
                    (offset as u64 + limit as u64) < total,
                    eframe::egui::Button::new("Next"),
                )
                .clicked()
            {
                self.asset_browser.query.offset = offset.saturating_add(limit);
                self.asset_browser.invalidate();
            }
        });
        ui.add_space(12.);
        Some(assets)
    }
}
