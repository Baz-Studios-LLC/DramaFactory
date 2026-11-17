use crate::{Studio, production::LibraryEntry};
use eframe::egui;
impl Studio {
    pub(crate) fn character_grid(
        &mut self,
        ui: &mut egui::Ui,
        series: &str,
        entries: &[LibraryEntry],
    ) {
        let assets = self.load(self.store.assets());
        let deleted = self.store.deleted_portraits().unwrap_or_default();
        let gap = 14.;
        let columns = ((ui.available_width() + gap) / 190.).floor().clamp(1., 6.) as usize;
        let width =
            ((ui.available_width() - gap * (columns - 1) as f32) / columns as f32).min(230.);
        egui::Grid::new(("character-gallery", series))
            .num_columns(columns)
            .spacing(egui::vec2(gap, gap))
            .show(ui, |ui| {
                for (index, entry) in entries.iter().enumerate() {
                    let candidate = assets.iter().find(|asset| {
                        !deleted.contains(&asset.id)
                            && asset.metadata["provider"] == "openrouter"
                            && asset.metadata["request"]["library_id"] == entry.id
                    });
                    let portrait = entry
                        .reference
                        .as_deref()
                        .or_else(|| candidate.map(|asset| asset.path.as_path()));
                    let name = self
                        .profile_drafts
                        .get(&entry.id)
                        .map(|draft| draft.brief.name.as_str())
                        .or_else(|| {
                            self.library_drafts
                                .get(&entry.id)
                                .map(|draft| draft.name.as_str())
                        })
                        .unwrap_or(&entry.name)
                        .to_owned();
                    let frame = egui::Frame::new()
                        .fill(crate::theme::SURFACE)
                        .stroke(egui::Stroke::new(1., crate::theme::BORDER))
                        .corner_radius(12)
                        .inner_margin(8);
                    let tile = frame.show(ui, |ui| {
                        ui.set_width((width - 16.).max(40.));
                        ui.vertical(|ui| {
                            let size = egui::vec2(
                                (width - 16.).max(40.),
                                (width - 16.).max(40.) * 4. / 3.,
                            );
                            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            if let Some(path) = portrait {
                                paint_portrait(ui, path, rect, 8);
                            } else {
                                ui.painter().rect_filled(rect, 8, crate::BG);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "No portrait yet",
                                    egui::FontId::proportional(14.),
                                    crate::MUTED,
                                );
                            }
                            ui.add_space(6.);
                            ui.add(
                                egui::Label::new(egui::RichText::new(&name).strong()).truncate(),
                            );
                        });
                    });
                    let response = ui.interact(
                        tile.response.rect,
                        ui.make_persistent_id(("open-character", &entry.id)),
                        egui::Sense::click(),
                    );
                    self.portrait_context_menu(&response, entry, portrait);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                    });
                    if response.hovered() || response.has_focus() {
                        ui.painter().rect_stroke(
                            tile.response.rect,
                            12,
                            egui::Stroke::new(1.5, crate::GOLD),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response
                        .on_hover_text(format!("Open {name}"))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.open_character = Some((series.to_owned(), entry.id.clone()));
                    }
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }
}

/// Compact, selectable portrait used by the generation cast picker.
pub(crate) fn cast_choice(
    ui: &mut egui::Ui,
    entry: &LibraryEntry,
    selected: bool,
    width: f32,
) -> bool {
    let enabled = entry.reference.is_some();
    let size = egui::vec2(width, width * 4. / 3. + 38.);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, selected, &entry.name)
    });
    let border = if selected || response.hovered() || response.has_focus() {
        crate::GOLD
    } else {
        crate::theme::BORDER
    };
    ui.painter().rect_filled(rect, 8, crate::theme::SURFACE);
    let picture = egui::Rect::from_min_size(
        rect.min + egui::vec2(5., 5.),
        egui::vec2(width - 10., width * 4. / 3. - 10.),
    );
    if let Some(path) = &entry.reference {
        paint_portrait(ui, path, picture, 5);
    } else {
        ui.painter().text(
            picture.center(),
            egui::Align2::CENTER_CENTER,
            "No portrait",
            egui::FontId::proportional(12.),
            crate::MUTED,
        );
    }
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 5., picture.bottom() + 5.),
        rect.max - egui::vec2(5., 3.),
    );
    // Paint inside the existing cell: adding a label widget here advances Grid
    // to another column and pushes later portraits outside the viewport.
    let name = ui.painter().layout(
        entry.name.clone(),
        egui::FontId::proportional(12.),
        ui.visuals().text_color(),
        name_rect.width(),
    );
    let position = egui::pos2(name_rect.center().x - name.size().x / 2., name_rect.top());
    ui.painter()
        .with_clip_rect(name_rect)
        .galley(position, name, ui.visuals().text_color());
    ui.painter().rect_stroke(
        rect,
        8,
        egui::Stroke::new(if selected { 2. } else { 1. }, border),
        egui::StrokeKind::Inside,
    );
    if selected {
        let center = picture.right_top() + egui::vec2(-11., 11.);
        ui.painter().circle_filled(center, 10., crate::GOLD);
        ui.painter().line_segment(
            [center + egui::vec2(-4., 0.), center + egui::vec2(-1., 3.)],
            egui::Stroke::new(2., crate::BG),
        );
        ui.painter().line_segment(
            [center + egui::vec2(-1., 3.), center + egui::vec2(5., -4.)],
            egui::Stroke::new(2., crate::BG),
        );
    }
    let clicked = enabled && response.clicked();
    response.on_hover_text(if enabled {
        entry.name.clone()
    } else {
        format!("{}: choose a portrait in Characters first", entry.name)
    });
    clicked
}

/// Keep the whole portrait visible inside a uniform card without distorting it.
fn paint_portrait(ui: &egui::Ui, path: &std::path::Path, bounds: egui::Rect, radius: u8) {
    ui.painter().rect_filled(bounds, radius, crate::BG);
    let image = egui::Image::from_uri(format!("file://{}", path.display()))
        .fit_to_exact_size(bounds.size())
        .maintain_aspect_ratio(true)
        .corner_radius(radius);
    let source_size = image
        .load_for_size(ui.ctx(), bounds.size())
        .ok()
        .and_then(|texture| texture.size());
    let fitted = image.calc_size(bounds.size(), source_size);
    image.paint_at(ui, egui::Rect::from_center_size(bounds.center(), fitted));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portrait_cards_stay_in_their_grid_cells() {
        let context = egui::Context::default();
        for columns in [1, 3, 5] {
            for _ in 0..3 {
                let _ = context.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let grid = egui::Grid::new(("test-cast", columns))
                            .num_columns(columns)
                            .spacing([10., 10.])
                            .show(ui, |ui| {
                                for index in 0..9 {
                                    let entry = LibraryEntry {
                                        id: index.to_string(),
                                        series_id: "series".into(),
                                        kind: "character".into(),
                                        name: "Bailiff Garret Crowe".into(),
                                        description: String::new(),
                                        reference: None,
                                        voice: String::new(),
                                    };
                                    cast_choice(ui, &entry, index == 0, 120.);
                                    if (index + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        assert!(
                            grid.response.rect.width() <= columns as f32 * 130.,
                            "extra columns: {:?}",
                            grid.response.rect
                        );
                    });
                });
            }
        }
    }
}
