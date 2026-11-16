use crate::{GOLD, MUTED, Studio, story::*};
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum StorySection {
    #[default]
    Bible,
    Episodes,
    Continuity,
}

fn facts(ui: &mut egui::Ui, values: &mut Vec<Fact>) {
    let mut remove = None;
    for (index, fact) in values.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut fact.key)
                        .hint_text("Fact, e.g. Mara knows the code"),
                );
                ui.add(egui::TextEdit::singleline(&mut fact.value).hint_text("Value, e.g. no"));
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        values.remove(index);
    }
    if ui.small_button("+ Fact").clicked() {
        values.push(Fact::default());
    }
}
impl Studio {
    pub(crate) fn story_development(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "THE WRITERS’ ROOM",
            "A story worth continuing.",
            "Plan the ending. Develop the characters. Carry every consequence forward.",
        );
        let Some(series) = self.selected.clone() else {
            ui.label("Create or select a series to develop its story.");
            return;
        };
        if let Some(receiver) = &self.story_check
            && let Ok((id, result)) = receiver.try_recv()
        {
            self.story_reports.insert(id, result);
            self.story_check = None;
        }
        if !self.story_reports.contains_key(&series) && self.story_check.is_none() {
            let path = self.store.path.clone();
            let id = series.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = crate::store::Store::open(path)
                    .and_then(|store| store.story_report(&id))
                    .map_err(|e| e.to_string());
                let _ = tx.send((id, result));
            });
            self.story_check = Some(rx);
        }
        let Some(mut plan) = self
            .story_drafts
            .remove(&series)
            .or_else(|| self.store.series_plan(&series).ok())
        else {
            return;
        };
        let episodes = self.load(self.store.episodes(&series));
        let cast = self.load(self.store.library(&series, "character"));
        let episode_name = |id: &str| {
            episodes
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.title.as_str())
                .unwrap_or("Choose episode")
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("DEVELOPMENT REVISION {}", plan.revision)).color(GOLD));
            if ui.button("Save series plan").clicked() {
                match self.store.save_series_plan(&plan) {
                    Ok(saved) => {
                        plan = saved;
                        self.story_reports.remove(&series);
                        self.message =
                            "Series plan saved. Previous continuity approvals require review."
                                .into();
                    }
                    Err(e) => self.message = e.to_string(),
                }
            }
            if ui
                .button("Reload saved plan")
                .on_hover_text(
                    "Preserve this draft as a recovery file, then load the current saved revision",
                )
                .clicked()
            {
                let dir = self.store.root().join("drafts");
                let result = (|| -> anyhow::Result<SeriesPlan> {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(
                        dir.join(format!("story-{}-{}.json", series, uuid::Uuid::new_v4())),
                        serde_json::to_vec_pretty(&plan)?,
                    )?;
                    self.store.series_plan(&series)
                })();
                match result {
                    Ok(saved) => {
                        plan = saved;
                        self.message =
                            "Saved plan loaded; previous draft preserved in recovery files".into();
                    }
                    Err(e) => self.message = e.to_string(),
                }
            }
        });
        ui.label(RichText::new("Drafts stay separate from canon. Save before checking continuity or committing an episode.").color(MUTED));
        ui.add_space(12.);
        ui.horizontal(|ui| {
            for (section, label) in [
                (StorySection::Bible, "Series bible"),
                (StorySection::Episodes, "Episode arc"),
                (StorySection::Continuity, "Continuity & canon"),
            ] {
                ui.selectable_value(&mut self.story_section, section, label);
            }
        });
        ui.add_space(14.);
        if self.story_section == StorySection::Bible {
            Self::card(ui, |ui| {
                ui.heading("Series bible");
                ui.collapsing("Casting, genre & cut rhythm", |ui| {
                egui::ComboBox::from_id_salt("creative-style").selected_text(plan.creative.style.replace('_'," ")).show_ui(ui, |ui| {
                    for style in ["character_drama","slice_of_life","romance","thriller","melodrama","custom"] { ui.selectable_value(&mut plan.creative.style,style.into(),style.replace('_'," ")); }
                });
                Self::field(ui,"CASTING DIRECTION",&mut plan.creative.casting_brief,3);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Cut guide"); ui.add(egui::DragValue::new(&mut plan.creative.min_shot_seconds).range(0.5..=30.).suffix(" s"));
                    ui.label("to"); ui.add(egui::DragValue::new(&mut plan.creative.max_shot_seconds).range(0.5..=30.).suffix(" s"));
                    ui.label("Episode target"); ui.add(egui::DragValue::new(&mut plan.creative.episode_seconds).range(15. ..=600.).suffix(" s"));
                });
                ui.label(RichText::new("The cut range is a guide. Hold longer for an emotional beat when needed; a camera change must serve the story.").color(MUTED));
            });
                for (label, value, rows) in [
                    ("AUDIENCE", &mut plan.audience, 1),
                    ("GENRE & TONE", &mut plan.genre, 1),
                    ("THEME", &mut plan.theme, 2),
                    ("CENTRAL DRAMATIC QUESTION", &mut plan.central_question, 2),
                    (
                        "THE ENDING — WHAT THE STORY ULTIMATELY RESOLVES",
                        &mut plan.ending,
                        3,
                    ),
                ] {
                    Self::field(ui, label, value, rows);
                }
                ui.collapsing("Facts true before episode one", |ui| {
                    facts(ui, &mut plan.initial_facts);
                });
            });
            ui.add_space(14.);
            Self::card(ui, |ui| {
                ui.heading("Character journeys");
                ui.label(
                    RichText::new(
                        "Link each arc to the same cast identity and voice used in production.",
                    )
                    .color(MUTED),
                );
                let mut remove = None;
                for (index, arc) in plan.characters.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.separator();
                        ui.label(
                            RichText::new(
                                cast.iter()
                                    .find(|c| c.id == arc.character_id)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("Missing cast member"),
                            )
                            .color(GOLD),
                        );
                        Self::field(ui, "WHAT THEY WANT", &mut arc.desire, 2);
                        Self::field(ui, "WHAT HOLDS THEM BACK", &mut arc.flaw, 2);
                        Self::field(ui, "HOW THEY CHANGE BY THE END", &mut arc.transformation, 2);
                        if ui.small_button("Remove arc").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    plan.characters.remove(index);
                }
                for character in &cast {
                    if !plan
                        .characters
                        .iter()
                        .any(|a| a.character_id == character.id)
                        && ui.button(format!("+ {} journey", character.name)).clicked()
                    {
                        plan.characters.push(CharacterArc {
                            character_id: character.id.clone(),
                            ..Default::default()
                        });
                    }
                }
                if cast.is_empty() {
                    ui.label("Add characters in Cast, then develop their journeys here.");
                }
            });
            ui.add_space(14.);
        }
        if self.story_section == StorySection::Episodes {
            Self::card(ui, |ui| {
                ui.heading("The episode arc");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Series target");
                    ui.add(
                        egui::DragValue::new(&mut plan.creative.target_episodes)
                            .range(1..=150)
                            .suffix(" episodes"),
                    );
                    ui.label(format!("{} outlines planned", plan.episodes.len()));
                    ui.label(format!(
                        "{:.0} minutes at the target length",
                        plan.creative.episode_seconds * plan.creative.target_episodes as f64 / 60.
                    ));
                });
                ui.label(RichText::new("Working format: around 55–75 episodes, usually 1–2 minutes each. Targets are editable for each series.").color(MUTED));
                ui.label(RichText::new("Each episode must change the situation, deliver a payoff, and create a reason to continue. The last episode resolves the central question.").color(MUTED));
                let mut action = None;
                let count = plan.episodes.len();
                for (index, outline) in plan.episodes.iter_mut().enumerate() {
                    egui::CollapsingHeader::new(format!(
                        "{:02}  {}",
                        index + 1,
                        episode_name(&outline.episode_id)
                    ))
                    .id_salt(&outline.episode_id)
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(index > 0, egui::Button::new("Earlier"))
                                .clicked()
                            {
                                action = Some((index, -1));
                            }
                            if ui
                                .add_enabled(index + 1 < count, egui::Button::new("Later"))
                                .clicked()
                            {
                                action = Some((index, 1));
                            }
                            if ui.button("Remove from plan").clicked() {
                                action = Some((index, 0));
                            }
                        });
                        for (label, value) in [
                            ("GOAL", &mut outline.goal),
                            ("CONFLICT", &mut outline.conflict),
                            ("TURN — WHAT CHANGES THE SITUATION", &mut outline.turn),
                            ("PAYOFF — WHAT THIS EPISODE DELIVERS", &mut outline.payoff),
                        ] {
                            Self::field(ui, label, value, 2);
                        }
                        if index + 1 < count {
                            Self::field(ui, "HOOK INTO THE NEXT EPISODE", &mut outline.hook, 2);
                        } else {
                            Self::field(
                                ui,
                                "HOW THE FINALE RESOLVES THE CENTRAL QUESTION",
                                &mut outline.ending_resolution,
                                3,
                            );
                        }
                        ui.collapsing("Facts required on entry", |ui| {
                            facts(ui, &mut outline.requires);
                        });
                        ui.collapsing("Consequences carried into the next episode", |ui| {
                            let mut remove = None;
                            for (i, change) in outline.changes.iter_mut().enumerate() {
                                ui.push_id(i, |ui| {
                                    ui.separator();
                                    ui.add(
                                        egui::TextEdit::singleline(&mut change.key)
                                            .hint_text("Fact that changes"),
                                    );
                                    let mut new = change.from.is_none();
                                    if ui.checkbox(&mut new, "New fact introduced here").changed() {
                                        change.from = if new { None } else { Some(String::new()) };
                                    }
                                    if let Some(value) = &mut change.from {
                                        Self::field(ui, "BEFORE", value, 1);
                                    }
                                    Self::field(ui, "AFTER", &mut change.to, 1);
                                    if ui.small_button("Remove change").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            }
                            if let Some(i) = remove {
                                outline.changes.remove(i);
                            }
                            if ui.small_button("+ Consequence").clicked() {
                                outline.changes.push(Change::default());
                            }
                        });
                    });
                }
                if let Some((index, direction)) = action {
                    match direction {
                        -1 => plan.episodes.swap(index, index - 1),
                        1 => plan.episodes.swap(index, index + 1),
                        _ => {
                            plan.episodes.remove(index);
                        }
                    }
                }
                for episode in &episodes {
                    if !plan.episodes.iter().any(|e| e.episode_id == episode.id)
                        && ui.button(format!("+ Plan {}", episode.title)).clicked()
                    {
                        plan.episodes.push(EpisodeOutline {
                            episode_id: episode.id.clone(),
                            ..Default::default()
                        });
                    }
                }
                if episodes.is_empty() {
                    ui.label(
                        "Create episodes in Series & episodes, then arrange the complete arc here.",
                    );
                }
            });
            ui.add_space(14.);
            Self::card(ui, |ui| {
                ui.heading("Setups & payoffs");
                let mut remove = None;
                for (index, thread) in plan.threads.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.separator();
                        for (label, id) in [
                            ("Plant in", &mut thread.setup_episode_id),
                            ("Pay off in", &mut thread.payoff_episode_id),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(label);
                                egui::ComboBox::from_id_salt(label)
                                    .selected_text(episode_name(id))
                                    .show_ui(ui, |ui| {
                                        for episode in &episodes {
                                            ui.selectable_value(
                                                id,
                                                episode.id.clone(),
                                                &episode.title,
                                            );
                                        }
                                    });
                            });
                        }
                        Self::field(ui, "WHAT IS PLANTED", &mut thread.setup, 2);
                        Self::field(ui, "HOW IT PAYS OFF", &mut thread.payoff, 2);
                        if ui.small_button("Remove thread").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    plan.threads.remove(index);
                }
                if ui.button("+ Story thread").clicked() {
                    plan.threads.push(StoryThread::default());
                }
            });
            ui.add_space(14.);
        }
        if self.story_section == StorySection::Continuity {
            Self::card(ui, |ui| {
                ui.heading("Saved continuity & delivered canon");
                ui.label(RichText::new("This is the latest completed check. Refresh after production edits; committing always rechecks the current state.").color(MUTED));
                if ui
                    .add_enabled(
                        self.story_check.is_none(),
                        egui::Button::new("Check saved continuity"),
                    )
                    .clicked()
                {
                    self.story_reports.remove(&series);
                }
                let checked = self.story_reports.remove(&series);
                let mut refresh = false;
                match checked.as_ref() {
                    None => {
                        ui.spinner();
                        ui.label("Checking saved continuity and media…");
                    }
                    Some(result) => match result {
                        Err(e) => {
                            ui.label(e.to_string());
                        }
                        Ok(report) => {
                            ui.label(format!("Checking saved revision {}", report.revision));
                            for issue in &report.issues {
                                ui.label(RichText::new(issue).color(GOLD));
                            }
                            let assets = self.load(self.store.assets());
                            for item in &report.episodes {
                                ui.push_id(&item.episode_id,|ui|{
                        ui.separator();ui.label(RichText::new(format!("{} · {}",episode_name(&item.episode_id),item.status)).color(GOLD));
                        for issue in &item.issues{ui.label(issue);}
                        ui.collapsing("Planned entry and exit facts",|ui| {for (key,value) in &item.exit_state{ui.label(format!("{}: {} to {}",key,item.entry_state.get(key).map(String::as_str).unwrap_or("not yet established"),value));}});
                        let choice=self.story_exports.entry(item.episode_id.clone()).or_default();
                        egui::ComboBox::from_id_salt("export").selected_text(if choice.is_empty(){"Choose reviewed final cut"}else{"Final cut selected"}).show_ui(ui,|ui| {
                            for asset in &assets {if asset.metadata["kind"]=="episode_export" && asset.metadata["timeline"]["episode_id"]==item.episode_id && asset.review=="approved" {ui.selectable_value(choice,asset.id.clone(),format!("{} · {:.2}s · {}",self.store.asset_label(asset),asset.duration,&asset.id[..8]));}}
                        });
                        let choice=choice.clone();
                        if ui.add_enabled(!choice.is_empty(),egui::Button::new("Approve story continuity & commit episode")).on_hover_text("Only after reviewing the story and finished cut. Preceding canon must be current.").clicked(){let r=self.store.commit_episode(&series,&item.episode_id,&choice);self.result(r,"Episode committed to canon");refresh=true;}
                    });
                            }
                            ui.collapsing("Currently committed facts", |ui| {
                                for (key, value) in &report.canon {
                                    ui.label(format!("{key}: {value}"));
                                }
                            });
                            ui.label(RichText::new(report.limitation).color(MUTED));
                        }
                    },
                }
                if !refresh && let Some(result) = checked {
                    self.story_reports.insert(series.clone(), result);
                }
            });
        }
        self.story_drafts.insert(series, plan);
    }
}
