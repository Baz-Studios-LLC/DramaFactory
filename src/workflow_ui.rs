//! One shared sequence for the starting guide and navigation. No inferred approvals.
use crate::{GOLD, MUTED, Page, Studio};
use eframe::egui::{self, RichText};
pub struct Stage {
    pub heading: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub outcome: &'static str,
    pub actions: &'static [(Page, &'static str)],
}
pub const STAGES: [Stage; 7] = [
    Stage {
        heading: "01 · SYNOPSIS",
        title: "Define the synopsis",
        description: "Develop the premise, characters' arcs and ending. Keep the full story coherent before producing episodes.",
        outcome: "A series plan that guides every episode.",
        actions: &[(Page::Synopsis, "Synopsis")],
    },
    Stage {
        heading: "02 · CAST & WORLD",
        title: "Create the cast and world",
        description: "Choose the people and places, then create reusable reference images for consistent appearances.",
        outcome: "Cast and location references shared across the series.",
        actions: &[
            (Page::Cast, "Characters"),
            (Page::Locations, "Locations"),
            (Page::Images, "Reference images"),
        ],
    },
    Stage {
        heading: "03 · OVERALL PLOT",
        title: "Develop the overall plot",
        description: "Plan the full series arc, character journeys, episode outlines and ending.",
        outcome: "A coherent series outline that connects every episode.",
        actions: &[(Page::Story, "Overall plot")],
    },
    Stage {
        heading: "04 · EPISODE PLOT",
        title: "Write the episode plot",
        description: "Develop this episode from the series outline, then prepare its screenplay and visual beats.",
        outcome: "A screenplay and visual plan, ready for production.",
        actions: &[
            (Page::Series, "Episode plot"),
            (Page::Scenes, "Visual plan"),
        ],
    },
    Stage {
        heading: "05 · EPISODE GENERATION",
        title: "Generate your footage",
        description: "Generate and compare takes for every segment.",
        outcome: "Clips ready to cut in the Editing room.",
        actions: &[
            (Page::Queue, "Episode generation"),
            (Page::Assets, "Review media"),
        ],
    },
    Stage {
        heading: "06 · EDITING ROOM",
        title: "Shape the final episode",
        description: "Arrange your takes, trim the action and cut the episode together.",
        outcome: "A finished MP4 with picture and sound together.",
        actions: &[(Page::EditingRoom, "Editing room")],
    },
    Stage {
        heading: "07 · PUBLISH",
        title: "Share the episode",
        description: "Prepare your episode for TikTok, YouTube Shorts and Instagram.",
        outcome: "A saved release draft with video, cover and captions.",
        actions: &[(Page::Publish, "Publish")],
    },
];
pub(crate) fn sidebar_toggle(ui: &mut egui::Ui, compact: bool) -> egui::Response {
    let label = if compact {
        "Expand sidebar"
    } else {
        "Collapse sidebar"
    };
    let size = egui::vec2(if compact { ui.available_width() } else { 28. }, 28.);
    let response = ui.add_sized(size, egui::Button::new("").frame(false));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let rect = egui::Rect::from_center_size(response.rect.center(), egui::vec2(18., 14.));
    let stroke = egui::Stroke::new(1.5, ui.style().interact(&response).fg_stroke.color);
    ui.painter()
        .rect_stroke(rect, 2., stroke, egui::StrokeKind::Inside);
    let x = rect.left() + 5.;
    ui.painter().line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        stroke,
    );
    response.on_hover_text(label)
}

impl Studio {
    fn workflow_link(&mut self, ui: &mut egui::Ui, page: Page, label: &str, sidebar: bool) {
        let active = self.page == page;
        let button = egui::Button::new(RichText::new(label).color(if active || !sidebar {
            GOLD
        } else {
            MUTED
        }))
        .selected(active)
        .frame(active);
        let response = if sidebar {
            ui.add_sized([ui.available_width(), 30.], button)
        } else {
            ui.add(button)
        };
        if response.clicked() {
            if self.page != page {
                self.stop_playback();
            }
            self.page = page;
        }
    }
    pub(crate) fn workflow_navigation(&mut self, ui: &mut egui::Ui, compact: bool) {
        let save = if compact {
            ui.horizontal(|ui| {
                let side = ui.available_width().min(36.);
                ui.add_space(((ui.available_width() - side) * 0.5).max(0.));
                let response = ui.add_enabled(
                    self.selected.is_some(),
                    crate::theme::primary("")
                        .min_size(egui::vec2(side, 36.))
                        .corner_radius(9),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        self.selected.is_some(),
                        "Save project",
                    )
                });
                // Draw in button coordinates rather than relying on an emoji's font bearings.
                let rect =
                    egui::Rect::from_center_size(response.rect.center(), egui::vec2(18., 18.));
                let stroke = egui::Stroke::new(
                    1.6,
                    if response.enabled() {
                        egui::Color32::from_rgb(25, 23, 19)
                    } else {
                        MUTED
                    },
                );
                let p = |x, y| rect.min + egui::vec2(x, y);
                ui.painter().add(egui::Shape::closed_line(
                    vec![p(1., 1.), p(13., 1.), p(17., 5.), p(17., 17.), p(1., 17.)],
                    stroke,
                ));
                for (min, max) in [(p(5., 1.), p(11., 6.)), (p(5., 11.), p(13., 17.))] {
                    ui.painter().rect_stroke(
                        egui::Rect::from_min_max(min, max),
                        1.,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                response
            })
            .inner
        } else {
            ui.add_enabled(
                self.selected.is_some(),
                crate::theme::primary("Save project")
                    .min_size(egui::vec2(ui.available_width(), 32.)),
            )
        };
        if save
            .on_hover_text("Save edits across every workspace in the current series")
            .clicked()
        {
            self.save_project();
        }
        if !compact {
            self.project_file_menu(ui);
            ui.add_space(12.);
            ui.label(RichText::new("WORKSPACE").size(10.).color(MUTED));
        } else {
            ui.separator();
        }
        ui.add_space(4.);
        for (index, stage) in STAGES.iter().enumerate() {
            let active = stage.actions.iter().any(|(page, _)| *page == self.page);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.), egui::Sense::click());
            if active || response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    7.,
                    if active {
                        egui::Color32::from_rgb(46, 41, 33)
                    } else {
                        crate::theme::SURFACE
                    },
                );
            }
            if active {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        rect.left_top() + egui::vec2(0., 7.),
                        egui::vec2(3., 20.),
                    ),
                    2.,
                    GOLD,
                );
            }
            let label = if index == 1 {
                "Characters & world"
            } else {
                stage.actions[0].1
            };
            let response = if compact {
                let icon = ["✎", "♟", "⌘", "▤", "▶", "✂", "↑"][index];
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    egui::FontId::proportional(20.),
                    if active { GOLD } else { MUTED },
                );
                response.on_hover_text(label)
            } else {
                ui.painter().text(
                    rect.left_center() + egui::vec2(14., 0.),
                    egui::Align2::LEFT_CENTER,
                    format!("{:02}", index + 1),
                    egui::FontId::proportional(11.),
                    if active { GOLD } else { MUTED },
                );
                ui.painter().text(
                    rect.left_center() + egui::vec2(42., 0.),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(13.),
                    if active { crate::theme::TEXT } else { MUTED },
                );
                response
            };
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label)
            });
            if response.clicked() {
                self.stop_playback();
                self.page = stage.actions[0].0;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            if compact {
                if ui
                    .add_sized(
                        [ui.available_width(), 32.],
                        egui::Button::new("⚙").selected(self.page == Page::Settings),
                    )
                    .on_hover_text("Settings")
                    .clicked()
                {
                    self.stop_playback();
                    self.page = Page::Settings;
                }
            } else {
                self.workflow_link(ui, Page::Settings, "Settings", true);
            }
            ui.separator();
        });
    }
    pub(crate) fn workflow_location(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Queue {
            return;
        }
        if let Some(stage) = STAGES
            .iter()
            .find(|stage| stage.actions.iter().any(|(page, _)| *page == self.page))
            && stage.actions.len() > 1
        {
            ui.horizontal_wrapped(|ui| {
                for &(page, label) in stage.actions {
                    self.workflow_link(ui, page, label, false);
                }
            });
            ui.add_space(18.);
        }
    }
    pub(crate) fn workflow_home(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "MAKE A SHOW",
            "Your next story starts here.",
            "A home for your characters, your stories, and every episode to come.",
        );
        ui.horizontal(|ui| {
            if ui.add(crate::theme::primary("+ Create a series")).clicked() {
                self.start_new_series();
            }
            ui.label(RichText::new("Make something worth watching.").color(MUTED));
        });
        ui.add_space(16.);
        let columns = if ui.available_width() >= 800. { 2 } else { 1 };
        for row in STAGES.chunks(columns) {
            ui.columns(row.len(), |cols| {
                for (col, stage) in cols.iter_mut().zip(row) {
                    Self::card(col, |ui| {
                        ui.set_min_height(160.);
                        ui.label(RichText::new(stage.heading).size(10.).color(GOLD));
                        ui.label(RichText::new(stage.title).size(20.).strong());
                        ui.add_space(6.);
                        ui.label(stage.description);
                        ui.add_space(8.);
                        ui.horizontal_wrapped(|ui| {
                            for &(page, label) in stage.actions {
                                self.workflow_link(ui, page, label, false);
                            }
                        });
                        ui.add_space(8.);
                        ui.label(RichText::new(stage.outcome).size(11.).color(MUTED));
                    });
                }
            });
            ui.add_space(14.);
        }
        ui.add_space(6.);

        ui.heading("Your series");
        for series in self.load(self.store.series()) {
            Self::card(ui, |ui| {
                if ui.button(RichText::new(&series.name).size(20.)).clicked()
                    && self.select_series(&series.id)
                {
                    self.page = Page::Story;
                }
                ui.label(RichText::new(&series.premise).color(MUTED));
            });
            ui.add_space(10.);
        }
    }
}
