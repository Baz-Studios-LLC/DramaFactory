//! Local-only reference selection. A drop never creates a character or starts generation.
use crate::{GOLD, MUTED, Studio};
use anyhow::{Result, bail};
use eframe::egui::{self, RichText};
use std::path::Path;

fn validate_reference(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 24 * 1024 * 1024 {
        bail!("Choose an image file up to 24 MiB.");
    }
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
    ) {
        bail!("Use a PNG, JPEG, or WebP image.");
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| {
        anyhow::anyhow!("This image cannot be decoded or exceeds 8192 pixels per side.")
    })?;
    Ok(())
}
impl Studio {
    pub(crate) fn reference_drop_zone(&mut self, ui: &mut egui::Ui, scope: (&str, &str)) {
        let id = ui.make_persistent_id(("reference-drop", scope));
        let previous_hover = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
        let dragging = ui.input(|input| !input.raw.hovered_files.is_empty());
        let frame = egui::Frame::new()
            .fill(crate::theme::SURFACE)
            .stroke(egui::Stroke::new(
                1_f32,
                if dragging && previous_hover {
                    GOLD
                } else {
                    crate::theme::BORDER
                },
            ))
            .corner_radius(8)
            .inner_margin(14.)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if let Some(path) = &self.library_reference {
                        ui.add(
                            egui::Image::from_uri(format!("file://{}", path.display()))
                                .max_size(egui::vec2(88., 88.))
                                .corner_radius(6),
                        );
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if self.library_reference.is_some() {
                                "Drop an image to replace this reference"
                            } else {
                                "Drag a reference image here"
                            })
                            .strong(),
                        );
                        ui.label(
                            RichText::new("PNG, JPEG or WebP · up to 24 MiB")
                                .small()
                                .color(MUTED),
                        );
                        if let Some(path) = &self.library_reference {
                            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Choose reference image").clicked()
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                                    .pick_file()
                            {
                                match validate_reference(&path) {
                                    Ok(()) => {
                                        self.library_reference = Some(path);
                                        ui.data_mut(|data| data.remove::<String>(id.with("error")));
                                    }
                                    Err(error) => ui.data_mut(|data| {
                                        data.insert_temp(id.with("error"), error.to_string())
                                    }),
                                }
                            }
                            if self.library_reference.is_some()
                                && ui.small_button("Remove").clicked()
                            {
                                self.library_reference = None;
                            }
                        });
                    });
                });
            });
        let position = ui.input(|input| input.pointer.hover_pos());
        let over = position.map(|pos| frame.response.rect.intersect(ui.clip_rect()).contains(pos));
        let dropped = ui.input(|input| input.raw.dropped_files.clone());
        if !dropped.is_empty() && over.unwrap_or(previous_hover) {
            let result = if dropped.len() != 1 {
                Err(anyhow::anyhow!("Drop one reference image at a time."))
            } else if let Some(path) = dropped[0].path.as_ref() {
                validate_reference(path).map(|()| path.clone())
            } else {
                Err(anyhow::anyhow!(
                    "Drop an image file from Finder, or use Choose reference image."
                ))
            };
            match result {
                Ok(path) => {
                    self.library_reference = Some(path);
                    ui.data_mut(|data| data.remove::<String>(id.with("error")));
                }
                Err(error) => {
                    ui.data_mut(|data| data.insert_temp(id.with("error"), error.to_string()))
                }
            }
        }
        ui.data_mut(|data| data.insert_temp(id, dragging && over.unwrap_or(false)));
        if let Some(error) = ui.data(|data| data.get_temp::<String>(id.with("error"))) {
            ui.colored_label(egui::Color32::from_rgb(239, 149, 126), error);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_validation_decodes_content_and_preserves_originals() {
        let root = std::env::temp_dir().join(format!("df-drop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let valid = root.join("reference.png");
        image::RgbaImage::new(8, 8).save(&valid).unwrap();
        let original = std::fs::read(&valid).unwrap();
        validate_reference(&valid).unwrap();
        assert_eq!(std::fs::read(&valid).unwrap(), original);
        let bad = root.join("fake.png");
        std::fs::write(&bad, b"not an image").unwrap();
        assert!(validate_reference(&bad).is_err());
        assert!(validate_reference(&root).is_err());
        std::fs::write(&bad, &original[..original.len() / 2]).unwrap();
        assert!(validate_reference(&bad).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
