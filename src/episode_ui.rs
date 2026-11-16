use crate::{GOLD, MUTED, Page, Studio};
use eframe::egui::{self, RichText};
#[derive(Default, PartialEq)]
pub(crate) enum EpisodeSection {
    #[default]
    Beats,
    Screenplay,
    Continuity,
}
impl Studio {
    pub(crate) fn episode_navigation(&mut self, ui: &mut egui::Ui) {
        let Some(series) = self.selected.clone() else {
            ui.label("Choose the current series in the sidebar.");
            return;
        };
        let episodes = self.load(self.store.ordered_gui_episodes(&series));
        let index = episodes
            .iter()
            .position(|episode| Some(&episode.id) == self.episode_id.as_ref());
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(index.is_some_and(|i| i > 0), egui::Button::new("Previous"))
                .clicked()
                && let Some(index) = index
            {
                self.select_episode(&episodes[index - 1].id);
            }
            ui.label(
                index
                    .map(|i| {
                        format!(
                            "Episode {:02} of {} · {}",
                            i + 1,
                            episodes.len(),
                            episodes[i].title
                        )
                    })
                    .unwrap_or("No episode selected".into()),
            );
            if ui
                .add_enabled(
                    index.is_some_and(|i| i + 1 < episodes.len()),
                    egui::Button::new("Next"),
                )
                .clicked()
                && let Some(index) = index
            {
                self.select_episode(&episodes[index + 1].id);
            }
            if ui.button("Browse episodes").clicked() {
                self.page = Page::Series;
            }
        });
        ui.add_space(12.);
    }
    pub(crate) fn episode_snapshot(&self) -> anyhow::Result<crate::sidekick_episode::EpisodeDraft> {
        let id = self
            .episode_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Choose an episode"))?;
        let episode = self.store.get_episode(id)?;
        if Some(&episode.series_id) != self.selected.as_ref() {
            anyhow::bail!("Episode belongs to another series");
        }
        Ok(crate::sidekick_episode::EpisodeDraft {
            series_id: episode.series_id.clone(),
            episode_id: id.clone(),
            screenplay: self.script.clone(),
            expected_screenplay: self.script_baseline.clone(),
            plan: self
                .story_drafts
                .get(&episode.series_id)
                .cloned()
                .unwrap_or(self.store.series_plan(&episode.series_id)?),
        })
    }
    pub(crate) fn put_episode_draft(&mut self, draft: crate::sidekick_episode::EpisodeDraft) {
        self.script = draft.screenplay;
        self.script_baseline = draft.expected_screenplay;
        self.story_drafts.insert(draft.series_id, draft.plan);
    }
    fn reload_episode_work(&mut self) {
        let result = (|| -> anyhow::Result<()> {
            let current = self.episode_snapshot()?;
            let saved = self.store.get_episode(&current.episode_id)?;
            let plan = self.store.series_plan(&current.series_id)?;
            let directory = self.store.root().join("drafts");
            std::fs::create_dir_all(&directory)?;
            std::fs::write(
                directory.join(format!(
                    "episode-{}-{}.json",
                    current.episode_id,
                    uuid::Uuid::new_v4()
                )),
                serde_json::to_vec_pretty(&current)?,
            )?;
            self.sidekick.episode_undo.remove(&current.episode_id);
            self.put_episode_draft(crate::sidekick_episode::EpisodeDraft {
                screenplay: saved.screenplay.clone(),
                expected_screenplay: saved.screenplay,
                plan,
                ..current
            });
            Ok(())
        })();
        self.result(
            result,
            "Saved story and screenplay loaded; previous draft preserved in recovery files",
        );
    }
    fn save_episode_work(&mut self) -> bool {
        let result = (|| -> anyhow::Result<()> {
            let mut draft = self.episode_snapshot()?;
            let revision = self.store.save_episode_draft(&draft)?;
            draft.plan.revision = revision;
            draft.expected_screenplay = draft.screenplay.clone();
            self.story_reports.remove(&draft.series_id);
            self.sidekick.episode_undo.remove(&draft.episode_id);
            self.put_episode_draft(draft);
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.message = "Episode outline and screenplay saved".into();
                true
            }
            Err(error) => {
                self.message = error.to_string();
                false
            }
        }
    }
    pub(crate) fn episode_workspace(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "EPISODE PLOT",
            "Make every episode matter.",
            "Shape the story beats, write the scene, and carry its consequences forward.",
        );
        let Some(series) = self.selected.clone() else {
            ui.label("Choose a series to begin.");
            return;
        };
        let episodes = self.load(self.store.ordered_gui_episodes(&series));
        if ui.available_width() < 760. {
            ui.collapsing("Browse & add episodes", |ui| {
                self.episode_browser(ui, &series, &episodes)
            });
            self.episode_editor(ui, &series, &episodes);
        } else {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(220., 0.),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.set_width(220.);
                        self.episode_browser(ui, &series, &episodes);
                    },
                );
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.episode_editor(ui, &series, &episodes);
                });
            });
        }
    }
    fn episode_browser(
        &mut self,
        ui: &mut egui::Ui,
        series: &str,
        episodes: &[crate::store::Episode],
    ) {
        Self::card(ui, |ui| {
            ui.label(RichText::new("EPISODES").color(GOLD));
            ui.add(
                egui::TextEdit::singleline(&mut self.episode_search)
                    .desired_width(ui.available_width())
                    .hint_text(crate::theme::placeholder("Search title or number")),
            );
            let term = self.episode_search.trim().to_lowercase();
            let planned = self
                .story_drafts
                .get(series)
                .cloned()
                .or_else(|| self.store.series_plan(series).ok());
            egui::ScrollArea::vertical()
                .id_salt(("episode-browser", series))
                .max_height(480.)
                .show(ui, |ui| {
                    for (index, episode) in episodes.iter().enumerate() {
                        if !term.is_empty()
                            && !format!("{} {:02} {}", index + 1, index + 1, episode.title)
                                .to_lowercase()
                                .contains(&term)
                        {
                            continue;
                        }
                        let outlined = planned
                            .as_ref()
                            .is_some_and(|p| p.episodes.iter().any(|o| o.episode_id == episode.id));
                        let state = if !episode.screenplay.trim().is_empty() {
                            "Screenplay saved"
                        } else if outlined {
                            "Outline planned"
                        } else {
                            "Not outlined"
                        };
                        let active = self.episode_id.as_deref() == Some(&episode.id);
                        if ui
                            .add_sized(
                                [ui.available_width(), 58.],
                                egui::Button::new(format!(
                                    "{:02}  {}\n{state}",
                                    index + 1,
                                    episode.title
                                ))
                                .selected(active)
                                .wrap(),
                            )
                            .clicked()
                        {
                            self.select_episode(&episode.id);
                        }
                    }
                    if episodes.is_empty() {
                        ui.label("Add your first episode below.");
                    }
                });
            ui.collapsing("+ Add episode", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.episode_title)
                        .desired_width(ui.available_width())
                        .hint_text(crate::theme::placeholder("Episode title")),
                );
                if ui
                    .add_enabled(
                        !self.episode_title.trim().is_empty(),
                        egui::Button::new("Create episode"),
                    )
                    .clicked()
                    && self.save_script()
                {
                    match self.store.create_episode(series, &self.episode_title) {
                        Ok(episode) => {
                            self.select_episode(&episode.id);
                            self.episode_title.clear();
                            self.episode_search.clear();
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
            });
        });
    }
    fn episode_editor(
        &mut self,
        ui: &mut egui::Ui,
        series: &str,
        episodes: &[crate::store::Episode],
    ) {
        let Some(index) = episodes
            .iter()
            .position(|e| Some(&e.id) == self.episode_id.as_ref())
        else {
            ui.heading(if episodes.is_empty() {
                "Start your first episode"
            } else {
                "Choose an episode"
            });
            if episodes.is_empty()
                && ui
                    .add(crate::theme::primary("Create first episode with sidekick"))
                    .clicked()
            {
                self.prepare_episode_discussion();
            }
            return;
        };
        let episode = &episodes[index];
        let mut plan = match self
            .story_drafts
            .remove(series)
            .map(Ok)
            .unwrap_or_else(|| self.store.series_plan(series))
        {
            Ok(plan) => plan,
            Err(error) => {
                ui.label(error.to_string());
                return;
            }
        };
        ui.label(
            RichText::new(format!(
                "EPISODE {:02} · {:.0} sec target",
                index + 1,
                plan.creative.episode_seconds
            ))
            .color(GOLD),
        );
        ui.heading(&episode.title);
        // Restore the shared draft before actions that snapshot or save it.
        self.story_drafts.insert(series.into(), plan.clone());
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(index > 0, egui::Button::new("Previous"))
                .clicked()
            {
                self.select_episode(&episodes[index - 1].id);
            }
            if ui
                .add_enabled(index + 1 < episodes.len(), egui::Button::new("Next"))
                .clicked()
            {
                self.select_episode(&episodes[index + 1].id);
            }
            if ui.add(crate::theme::primary("Save episode")).clicked() {
                self.save_episode_work();
            }
            if ui.button("Reload saved").on_hover_text("Preserve this draft in a recovery file, then reload the saved story plan and screenplay").clicked() {self.reload_episode_work();}
            if ui.button("Develop with sidekick").clicked() {
                self.prepare_episode_discussion();
            }
            if ui
                .add_enabled(
                    !self.script.trim().is_empty(),
                    egui::Button::new("Prepare generation →"),
                )
                .clicked()
                && self.save_episode_work()
            {
                self.page = Page::Queue;
            }
        });
        if self.episode_id.as_deref() != Some(&episode.id) {
            return;
        }
        plan = self.story_drafts.remove(series).unwrap_or(plan);
        let saved = self.store.series_plan(series).ok();
        let saved_outline = saved
            .as_ref()
            .and_then(|p| p.episodes.iter().find(|o| o.episode_id == episode.id));
        let draft_outline = plan.episodes.iter().find(|o| o.episode_id == episode.id);
        let dirty = self.script != self.script_baseline
            || serde_json::to_value(saved_outline).ok() != serde_json::to_value(draft_outline).ok();
        ui.label(
            RichText::new(if dirty {
                "Unsaved episode changes"
            } else {
                "Saved"
            })
            .small()
            .color(if dirty { GOLD } else { MUTED }),
        );
        if let Some((before, after)) = self.sidekick.episode_undo.get(&episode.id).cloned() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(GOLD, "Sidekick draft · review before saving");
                if ui.button("Undo sidekick edit").clicked() {
                    let current = crate::sidekick_episode::EpisodeDraft {
                        series_id: series.into(),
                        episode_id: episode.id.clone(),
                        screenplay: self.script.clone(),
                        expected_screenplay: self.script_baseline.clone(),
                        plan: plan.clone(),
                    };
                    match crate::sidekick_episode::undo(&current, &before, &after) {
                        Ok(restored) => {
                            plan = restored.plan;
                            self.script = restored.screenplay;
                            self.sidekick.episode_undo.remove(&episode.id);
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
            });
            ui.collapsing("Review episode changes", |ui| {
                for (label, draft) in [
                    ("Story beats before", &before),
                    ("Story beats after", &after),
                ] {
                    ui.collapsing(label, |ui| {
                        if let Some(outline) = draft
                            .plan
                            .episodes
                            .iter()
                            .find(|o| o.episode_id == episode.id)
                        {
                            for (name, text) in [
                                ("Purpose", &outline.goal),
                                ("Conflict", &outline.conflict),
                                ("Turn", &outline.turn),
                                ("Payoff", &outline.payoff),
                                ("Hook", &outline.hook),
                                ("Resolution", &outline.ending_resolution),
                            ] {
                                ui.label(format!("{name}: {text}"));
                            }
                            for fact in &outline.requires {
                                ui.label(format!("Requires {}: {}", fact.key, fact.value));
                            }
                            for change in &outline.changes {
                                ui.label(format!(
                                    "{}: {} → {}",
                                    change.key,
                                    change.from.as_deref().unwrap_or("New fact"),
                                    change.to
                                ));
                            }
                        } else {
                            ui.label("No outline");
                        }
                    });
                }
                ui.collapsing("Screenplay before", |ui| {
                    ui.label(&before.screenplay);
                });
                ui.collapsing("Screenplay after", |ui| {
                    ui.label(&after.screenplay);
                });
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.episode_section,
                EpisodeSection::Beats,
                "Story beats",
            );
            ui.selectable_value(
                &mut self.episode_section,
                EpisodeSection::Screenplay,
                "Screenplay",
            );
            ui.selectable_value(
                &mut self.episode_section,
                EpisodeSection::Continuity,
                "Continuity",
            );
        });
        ui.collapsing("Story context", |ui| {
            ui.label(RichText::new("SERIES QUESTION").small().color(MUTED));
            ui.label(&plan.central_question);
            if let Some(act) = plan
                .acts
                .iter()
                .find(|a| a.start_episode <= index as u32 + 1 && a.end_episode > index as u32)
            {
                ui.label(format!("Act: {}", act.title));
                ui.label(&act.escalation);
            }
            if index == 0 {
                ui.small("Opening episode · establish the people, stakes and central conflict.");
            } else if let Some(previous) = plan
                .episodes
                .iter()
                .find(|o| o.episode_id == episodes[index - 1].id)
            {
                ui.label(
                    RichText::new("PREVIOUS EPISODE · PLANNED HANDOFF")
                        .small()
                        .color(MUTED),
                );
                ui.label(format!(
                    "Turn: {}\nPayoff: {}\nHook: {}",
                    previous.turn, previous.payoff, previous.hook
                ));
                for change in &previous.changes {
                    ui.label(format!("{}: {}", change.key, change.to));
                }
            } else {
                ui.small("Previous episode has no outline yet.");
            }
            if let Some(next) = episodes
                .get(index + 1)
                .and_then(|e| plan.episodes.iter().find(|o| o.episode_id == e.id))
            {
                ui.label(format!("Next episode's purpose: {}", next.goal));
            }
        });
        if self.episode_section == EpisodeSection::Screenplay {
            ui.small(format!("{} words", self.script.split_whitespace().count()));
            egui::ScrollArea::vertical()
                .id_salt(("episode-script", &episode.id))
                .max_height(600.)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.script)
                            .font(egui::FontId::monospace(14.))
                            .desired_rows(22)
                            .desired_width(ui.available_width())
                            .margin(egui::vec2(12., 12.))
                            .hint_text(crate::theme::placeholder(
                                "Scene headings, action, character names and dialogue…",
                            )),
                    );
                });
        } else {
            let existing = plan
                .episodes
                .iter()
                .position(|o| o.episode_id == episode.id);
            let mut outline = existing
                .map(|i| plan.episodes[i].clone())
                .unwrap_or_else(|| crate::story::EpisodeOutline {
                    episode_id: episode.id.clone(),
                    ..Default::default()
                });
            let before = serde_json::to_value(&outline).ok();
            Self::card(ui, |ui| {
                if self.episode_section == EpisodeSection::Beats {
                    for (label, value) in [
                        ("PURPOSE — WHY THIS EPISODE EXISTS", &mut outline.goal),
                        ("CONFLICT & STAKES", &mut outline.conflict),
                        ("EMOTIONAL TURN — WHAT CHANGES", &mut outline.turn),
                        ("PAYOFF — WHAT THE AUDIENCE GETS", &mut outline.payoff),
                        ("EXIT HOOK — WHY WATCH THE NEXT EPISODE", &mut outline.hook),
                        (
                            "RESOLUTION — IF THIS CLOSES THE STORY",
                            &mut outline.ending_resolution,
                        ),
                    ] {
                        Self::field(ui, label, value, 3);
                    }
                } else {
                    ui.heading("Facts required on entry");
                    crate::story_ui::facts(ui, &mut outline.requires);
                    ui.heading("Consequences carried forward");
                    let mut remove = None;
                    for (i, change) in outline.changes.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            Self::field(ui, "FACT / RELATIONSHIP", &mut change.key, 1);
                            let mut introduced = change.from.is_none();
                            if ui
                                .checkbox(&mut introduced, "Introduced in this episode")
                                .changed()
                            {
                                change.from = if introduced {
                                    None
                                } else {
                                    Some(String::new())
                                };
                            }
                            if let Some(from) = &mut change.from {
                                Self::field(ui, "BEFORE", from, 1);
                            }
                            Self::field(ui, "AFTER", &mut change.to, 1);
                            if ui.small_button("Remove consequence").clicked() {
                                remove = Some(i);
                            }
                            ui.separator();
                        });
                    }
                    if let Some(i) = remove {
                        outline.changes.remove(i);
                    }
                    if ui.button("+ Consequence").clicked() {
                        outline.changes.push(Default::default());
                    }
                    ui.collapsing("Setups & payoffs linked to this episode", |ui| {
                        for thread in plan.threads.iter().filter(|t| {
                            t.setup_episode_id == episode.id || t.payoff_episode_id == episode.id
                        }) {
                            ui.label(format!("{} → {}", thread.setup, thread.payoff));
                        }
                    });
                }
            });
            if before != serde_json::to_value(&outline).ok() {
                crate::sidekick_episode::replace_outline(&mut plan, &episode.id, Some(outline));
            }
        }
        self.story_drafts.insert(series.into(), plan);
    }
}
