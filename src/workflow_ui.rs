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
pub const STAGES: [Stage; 5] = [
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
        outcome: "An episode script and generation plan. OpenRouter writing is being connected.",
        actions: &[
            (Page::Series, "Episode plot"),
            (Page::Scenes, "Visual plan"),
        ],
    },
    Stage {
        heading: "05 · EPISODE GENERATION",
        title: "Generate, review and finish",
        description: "Watch the results, choose the strongest takes and assemble the episode with its soundtrack.",
        outcome: "Review the finished cut and continuity before starting the next episode.",
        actions: &[
            (Page::Queue, "Episode generation"),
            (Page::Assets, "Review media"),
            (Page::Timeline, "Assemble & export"),
        ],
    },
];
impl Studio {
    fn workflow_link(&mut self, ui: &mut egui::Ui, page: Page, label: &str, sidebar: bool) {
        let active = self.page == page;
        let button = egui::Button::new(RichText::new(label).color(if active || !sidebar {
            GOLD
        } else {
            MUTED
        }))
        .selected(active)
        .frame(!sidebar || active);
        let response = if sidebar {
            ui.add_sized([182., 30.], button)
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
    pub(crate) fn workflow_navigation(&mut self, ui: &mut egui::Ui) {
        self.workflow_link(ui, Page::Studio, "Start / workflow", true);
        for stage in &STAGES {
            ui.add_space(10.);
            ui.label(RichText::new(stage.heading).size(10.).color(GOLD));
            if let Some(&(page, label)) = stage.actions.first() {
                self.workflow_link(ui, page, label, true);
            }
        }
        ui.add_space(12.);
        ui.label(RichText::new("APPLICATION").size(10.).color(GOLD));
        self.workflow_link(ui, Page::Settings, "Settings", true);
    }
    pub(crate) fn workflow_location(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Studio {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(stage) = STAGES
                .iter()
                .find(|stage| stage.actions.iter().any(|(page, _)| *page == self.page))
            {
                ui.label(RichText::new(stage.heading).color(GOLD).size(11.));
            }
            if ui.small_button("Workflow guide").clicked() {
                self.stop_playback();
                self.page = Page::Studio;
            }
        });
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
        }
        ui.add_space(12.);
    }
    pub(crate) fn workflow_home(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "MAKE A SHOW",
            "From idea to finished episode.",
            "Follow these five stages. Return to any stage as the story and performances develop.",
        );
        Self::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(RichText::new("+ Create a series").color(GOLD))
                    .clicked()
                {
                    self.new_open = true;
                }
                ui.label("Or continue an existing project below.");
            });
            ui.add_space(8.);
            ui.small("The Current series selector in the sidebar applies to every workspace.");
            if self.selected.is_none() {
                ui.label(RichText::new("First: create a series or choose one in the sidebar. Then open Series plan to develop its plot and ending.").color(GOLD));
            } else {
                ui.small("These links open the working tools for your selected project. They do not generate or approve anything automatically.");
            }
        });
        ui.add_space(16.);
        let columns = if ui.available_width() >= 800. { 2 } else { 1 };
        for row in STAGES.chunks(columns) {
            ui.columns(row.len(), |cols| {
                for (col, stage) in cols.iter_mut().zip(row) {
                    Self::card(col, |ui| {
                        ui.set_min_height(164.);
                        ui.label(RichText::new(stage.heading).size(10.).color(GOLD));
                        ui.label(RichText::new(stage.title).size(23.));
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
        ui.add(
            egui::Image::new(egui::include_image!("../branding/word_mark.png"))
                .max_size(egui::vec2(270., 90.)),
        );
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
