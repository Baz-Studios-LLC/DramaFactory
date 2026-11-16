use crate::{Studio, production::LibraryEntry};
impl Studio {
    pub(crate) fn grok_portrait_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
        displayed_portrait: Option<&std::path::Path>,
    ) {
        let jobs = self.load(self.store.portrait_jobs(&entry.id));
        if let Some(reference) = displayed_portrait {
            let label = match jobs.first() {
                Some(job) if job.status == "queued" => "Manage portrait · queued",
                Some(job) if job.status == "running" => "Manage portrait · generating…",
                Some(job) if job.status == "failed" || job.status == "blocked" => {
                    "Manage portrait · needs attention"
                }
                Some(job)
                    if job.status == "completed"
                        && self.load(self.store.assets()).iter().any(|asset| {
                            Some(&asset.id) == job.output_id.as_ref()
                                && asset.review == "unreviewed"
                                && asset.path != reference
                        }) =>
                {
                    "Manage portrait · new image to review"
                }
                _ => "Manage portrait",
            };
            eframe::egui::CollapsingHeader::new(label)
                // Keep candidates compact too; approval remains an explicit action.
                .id_salt((
                    &entry.id,
                    reference,
                    entry.reference.is_some(),
                    "manage-portrait",
                ))
                .default_open(false)
                .show(ui, |ui| {
                    self.portrait_editor(ui, entry, &jobs, displayed_portrait)
                });
        } else {
            self.portrait_editor(ui, entry, &jobs, displayed_portrait);
        }
    }
    fn portrait_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
        jobs: &[crate::store::Job],
        displayed_portrait: Option<&std::path::Path>,
    ) {
        let mut style = match self.store.portrait_style(&entry.id) {
            Ok(style) => style,
            Err(error) => {
                ui.label(format!("Could not load image style: {error}"));
                return;
            }
        };
        if entry.kind == "character" {
            ui.small(format!("Style · {}", style.label()))
                .on_hover_text("Change the series image style on Synopsis");
        } else if crate::portrait_style::picker(ui, &mut style) {
            let result = self.store.save_portrait_style(&entry.id, style);
            self.result(result, "Image style saved for the next generation");
        }
        let pending = jobs
            .iter()
            .any(|job| matches!(job.status.as_str(), "queued" | "running" | "blocked"));
        let profile_ready = self.profile_drafts.get(&entry.id).is_none_or(|draft| {
            !draft.dirty()
                && self
                    .store
                    .character_profile(&entry.id)
                    .is_ok_and(|saved| saved == *draft)
        });
        if !profile_ready {
            ui.colored_label(
                crate::GOLD,
                "Save or reload Character profile before generating an image.",
            );
        }
        if let Some(job) = jobs.first() {
            ui.horizontal(|ui| {
                if matches!(job.status.as_str(), "queued" | "running") {
                    ui.add(eframe::egui::Spinner::new().size(14.));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                }
                ui.label(match job.status.as_str() {
                    "queued" => "Portrait queued",
                    "running" => "Generating portrait…",
                    "completed" if displayed_portrait.is_some() => "Portrait generation completed",
                    "completed" => "Portrait ready · review the image below",
                    "failed" => "Portrait generation failed",
                    "canceled" => "Portrait generation canceled",
                    _ => "Portrait job needs attention",
                });
                if let Ok(timing) = self.store.job_timing(&job.id)
                    && let Some(seconds) = timing.attempt_elapsed_seconds
                {
                    ui.label(
                        eframe::egui::RichText::new(crate::job_timing::duration(seconds))
                            .small()
                            .color(crate::MUTED),
                    );
                }
            });
            if !job.error.is_empty() {
                ui.colored_label(eframe::egui::Color32::from_rgb(239, 149, 126), &job.error);
            }
        }
        if !pending
            && ui
                .add_enabled(
                    profile_ready,
                    eframe::egui::Button::new("Generate image · paid"),
                )
                .clicked()
        {
            let result = self
                .store
                .portrait_prompt(entry)
                .and_then(|saved| {
                    let prompt = self
                        .portrait_prompts
                        .get(&entry.id)
                        .map(|draft| draft.prompt.as_str())
                        .unwrap_or(&saved.prompt);
                    self.store.enqueue_grok_portrait_with_profile(
                        &entry.id,
                        prompt,
                        self.profile_drafts.get(&entry.id),
                    )
                })
                .and_then(|_| crate::worker::spawn_detached(&self.store.path));
            self.result(result, "Portrait queued; review it before choosing it");
        }
        let assets = self.load(self.store.assets());
        let portraits: Vec<_> = assets
            .iter()
            .filter(|asset| {
                asset.metadata["provider"] == "openrouter"
                    && asset.metadata["request"]["library_id"] == entry.id
                    && displayed_portrait != Some(asset.path.as_path())
            })
            .collect();
        if !portraits.is_empty() {
            ui.add_space(10.);
            ui.label(
                eframe::egui::RichText::new(if displayed_portrait.is_some() {
                    "Other portraits"
                } else {
                    "Generated portraits"
                })
                .strong(),
            );
            ui.horizontal_wrapped(|ui| {
                for asset in portraits {
                    ui.push_id(&asset.id, |ui| {
                        ui.vertical(|ui| {
                            self.reference_preview(ui, &asset.path, eframe::egui::vec2(260., 340.));
                            if ui.button("Use this portrait").clicked() {
                                let result = self.store.review(&asset.id, "approved");
                                self.result(result, "Character portrait selected");
                            }
                        });
                    });
                }
            });
        }
        eframe::egui::CollapsingHeader::new("Portrait prompt & settings")
            .id_salt((&entry.id, "grok-portrait"))
            .show(ui, |ui| {
                if !self.portrait_prompts.contains_key(&entry.id) {
                    match self.store.portrait_prompt(entry) {
                        Ok(draft) => {
                            self.portrait_prompts.insert(entry.id.clone(), draft);
                        }
                        Err(error) => {
                            ui.label(error.to_string());
                            return;
                        }
                    }
                }
                if self.sidekick.character_undo.contains_key(&entry.id)
                    && ui.button("Undo sidekick character edit").clicked()
                {
                    let result = self.undo_character_sidekick(&entry.series_id, &entry.id);
                    self.result(result, "Character and portrait drafts restored");
                }
                let mut draft = self.portrait_prompts[&entry.id].clone();
                Self::field(ui, "ADDITIONAL PORTRAIT DIRECTION", &mut draft.prompt, 4);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Save portrait prompt").clicked() {
                        match self.store.save_portrait_prompt(&draft) {
                            Ok(saved) => {
                                draft = saved;
                                self.message = "Portrait prompt saved".into();
                            }
                            Err(error) => self.message = error.to_string(),
                        }
                    }
                    if ui.button("Reload portrait prompt").clicked() {
                        match self.store.portrait_prompt(entry) {
                            Ok(saved) => draft = saved,
                            Err(error) => self.message = error.to_string(),
                        }
                    }
                });
                let prompt = draft.prompt.clone();
                self.portrait_prompts.insert(entry.id.clone(), draft);
                ui.collapsing("Full prompt for next image", |ui| {
                    match self.store.grok_portrait_request(&entry.id, &prompt) {
                        Ok(request) => {
                            ui.add(
                                eframe::egui::Label::new(&request.prompt)
                                    .wrap()
                                    .selectable(true),
                            );
                            if ui.button("Copy full prompt").clicked() {
                                ui.ctx().copy_text(request.prompt);
                            }
                        }
                        Err(error) => {
                            ui.colored_label(crate::GOLD, error.to_string());
                        }
                    }
                });
                ui.small("1K · 3:4 · medium quality");
            });
    }
}
