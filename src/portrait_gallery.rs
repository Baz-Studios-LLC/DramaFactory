use crate::{
    Studio,
    production::{Asset, LibraryEntry},
};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
impl Studio {
    pub(crate) fn portrait_gallery_tile(
        &mut self,
        ui: &mut egui::Ui,
        entry: &LibraryEntry,
        asset: &Asset,
        width: f32,
    ) {
        let selected = entry.reference.as_deref() == Some(asset.path.as_path());
        let size = Vec2::new(width, width * 4. / 3.);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        egui::Image::from_uri(format!("file://{}", asset.path.display()))
            .fit_to_exact_size(size)
            .corner_radius(10)
            .paint_at(ui, rect);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Inspect portrait")
        });
        if response.clicked() {
            self.inspect_portrait(&asset.path);
        }
        let hovered = ui.rect_contains_pointer(rect);
        let mut focused = response.has_focus();
        let actions = [
            (0, "Inspect portrait"),
            (1, "Use this portrait"),
            (2, "Delete portrait"),
        ];
        for (index, label) in actions {
            if selected && index != 0 {
                continue;
            }
            let button = egui::Rect::from_min_size(
                rect.left_bottom() + Vec2::new(8. + index as f32 * 36., -40.),
                Vec2::splat(30.),
            );
            let action = ui.interact(
                button,
                ui.make_persistent_id((&asset.id, "portrait-action", index)),
                egui::Sense::click(),
            );
            action.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            focused |= action.has_focus();
            if hovered || focused || action.hovered() {
                ui.painter().rect_filled(
                    button,
                    7,
                    if action.hovered() {
                        Color32::from_rgb(66, 61, 49)
                    } else {
                        Color32::from_black_alpha(210)
                    },
                );
                draw_icon(
                    ui,
                    button.center(),
                    index,
                    if index == 2 {
                        Color32::from_rgb(255, 169, 149)
                    } else {
                        Color32::WHITE
                    },
                );
            }
            if action
                .on_hover_text(label)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                match index {
                    0 => self.inspect_portrait(&asset.path),
                    1 => {
                        let result = self.store.select_portrait(entry, &asset.id);
                        let error = result.as_ref().err().map(ToString::to_string);
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                ui.make_persistent_id(("gallery-error", &asset.id)),
                                error,
                            )
                        });
                        self.result(result, "Character portrait selected");
                        ui.ctx().request_repaint();
                    }
                    _ => {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                ui.make_persistent_id(("permanent-portrait-delete", &asset.id)),
                                true,
                            )
                        });
                    }
                }
            }
        }
        if selected {
            ui.painter().rect_stroke(
                rect,
                10,
                Stroke::new(2., crate::GOLD),
                egui::StrokeKind::Inside,
            );
            let badge =
                egui::Rect::from_min_size(rect.left_top() + Vec2::splat(8.), Vec2::splat(26.));
            ui.painter().rect_filled(badge, 13, crate::GOLD);
            draw_icon(ui, badge.center(), 1, crate::BG);
            ui.interact(
                badge,
                ui.make_persistent_id((&asset.id, "selected-badge")),
                egui::Sense::hover(),
            )
            .on_hover_text("Selected portrait");
        } else if hovered || focused {
            ui.painter().rect_stroke(
                rect,
                10,
                Stroke::new(1., crate::GOLD),
                egui::StrokeKind::Inside,
            );
        }
        let confirm_id = ui.make_persistent_id(("permanent-portrait-delete", &asset.id));
        if ui
            .ctx()
            .data_mut(|data| data.get_temp::<bool>(confirm_id).unwrap_or(false))
        {
            self.portrait_delete_button(ui, &entry.id, &asset.id);
        }
        if let Some(Some(error)) = ui.ctx().data_mut(|data| {
            data.get_temp::<Option<String>>(ui.make_persistent_id(("gallery-error", &asset.id)))
        }) {
            ui.colored_label(crate::GOLD, error);
        }
    }
}
pub(crate) fn draw_icon(ui: &egui::Ui, center: Pos2, icon: usize, color: Color32) {
    let p = ui.painter();
    let stroke = Stroke::new(1.7, color);
    let at = |x, y| center + Vec2::new(x, y);
    match icon {
        0 => {
            p.circle_stroke(at(-2., -2.), 5., stroke);
            p.line_segment([at(2., 2.), at(7., 7.)], stroke);
        }
        1 => {
            p.line_segment([at(-6., 0.), at(-2., 4.)], stroke);
            p.line_segment([at(-2., 4.), at(7., -5.)], stroke);
        }
        _ => {
            p.line_segment([at(-7., -5.), at(7., -5.)], stroke);
            p.line_segment([at(-3., -8.), at(3., -8.)], stroke);
            p.line_segment([at(-5., -3.), at(-4., 7.)], stroke);
            p.line_segment([at(5., -3.), at(4., 7.)], stroke);
            p.line_segment([at(-4., 7.), at(4., 7.)], stroke);
            p.line_segment([at(-1., -2.), at(-1., 4.)], stroke);
            p.line_segment([at(2., -2.), at(2., 4.)], stroke);
        }
    }
}
