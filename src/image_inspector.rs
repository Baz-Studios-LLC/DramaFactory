use crate::Studio;
use eframe::egui;
use std::path::{Path, PathBuf};
pub struct Inspector {
    path: PathBuf,
    zoom: Option<f32>,
}
impl Studio {
    pub(crate) fn inspect_portrait(&mut self, path: &Path) {
        self.image_inspector = Some(Inspector {
            path: path.to_owned(),
            zoom: None,
        });
    }

    pub(crate) fn reference_preview(&mut self, ui: &mut egui::Ui, path: &Path, size: egui::Vec2) {
        if ui
            .add(
                egui::Image::from_uri(format!("file://{}", path.display()))
                    .fit_to_exact_size(size)
                    .corner_radius(8)
                    .sense(egui::Sense::click()),
            )
            .on_hover_text("Click to inspect the full reference image")
            .clicked()
        {
            self.image_inspector = Some(Inspector {
                path: path.to_owned(),
                zoom: None,
            });
        }
        if ui.small_button("Inspect image").clicked() {
            self.image_inspector = Some(Inspector {
                path: path.to_owned(),
                zoom: None,
            });
        }
    }
    pub(crate) fn image_inspector_window(&mut self, ctx: &egui::Context) {
        let Some(mut inspector) = self.image_inspector.take() else {
            return;
        };
        let mut open = true;
        egui::Window::new("Reference image · full-size inspection")
            .id(egui::Id::new("reference-inspector"))
            .open(&mut open)
            .default_size(egui::vec2(850., 700.))
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Fit to window").clicked() {
                        inspector.zoom = None;
                    }
                    if ui.button("100% · original pixels").clicked() {
                        inspector.zoom = Some(1.);
                    }
                    if ui.button("-").clicked() {
                        inspector.zoom = Some((inspector.zoom.unwrap_or(1.) / 1.25).max(0.1));
                    }
                    if ui.button("+").clicked() {
                        inspector.zoom = Some((inspector.zoom.unwrap_or(1.) * 1.25).min(4.));
                    }
                    ui.label(
                        inspector
                            .zoom
                            .map(|zoom| format!("{:.0}%", zoom * 100.))
                            .unwrap_or("Fit".into()),
                    );
                });
                ui.separator();
                if !inspector.path.is_file() {
                    ui.label(
                        "The reference file is missing. Restore it or choose another reference.",
                    );
                    return;
                }
                let available = ui.available_size().min(egui::vec2(1600., 1000.));
                egui::ScrollArea::both()
                    .id_salt("reference-image-scroll")
                    .max_height(available.y.max(240.))
                    .show(ui, |ui| {
                        let image =
                            egui::Image::from_uri(format!("file://{}", inspector.path.display()));
                        ui.add(if let Some(zoom) = inspector.zoom {
                            image.fit_to_original_size(zoom)
                        } else {
                            image.fit_to_exact_size(available)
                        });
                    });
            });
        if open {
            self.image_inspector = Some(inspector);
        }
    }
}
