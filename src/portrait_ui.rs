use crate::{Studio, production::LibraryEntry};
impl Studio {
    pub(crate) fn portrait_select_button(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
        asset: &str,
    ) {
        let id = ui.make_persistent_id(("portrait-selection-error", asset));
        if ui.button("Use this portrait").clicked() {
            let result = self.store.select_portrait(entry, asset);
            match &result {
                Ok(()) => {
                    ui.ctx().data_mut(|data| data.remove::<String>(id));
                    ui.ctx().request_repaint();
                }
                Err(error) => {
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(id, error.to_string()));
                }
            }
            self.result(result, "Character portrait selected");
        }
        if let Some(error) = ui.ctx().data_mut(|data| data.get_temp::<String>(id)) {
            ui.colored_label(crate::GOLD, error);
        }
    }
    pub(crate) fn portrait_delete_button(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &str,
        asset: &str,
    ) {
        let id = ui.make_persistent_id(("permanent-portrait-delete", asset));
        let confirming = ui
            .ctx()
            .data_mut(|data| data.get_temp::<bool>(id).unwrap_or(false));
        if confirming {
            ui.group(|ui| {
                ui.label("Permanently delete this portrait?");
                ui.small("The image file will be erased. This cannot be undone.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        ui.ctx().data_mut(|data| data.remove::<bool>(id));
                    }
                    if ui
                        .button(
                            eframe::egui::RichText::new("Delete permanently")
                                .color(eframe::egui::Color32::from_rgb(239, 149, 126)),
                        )
                        .clicked()
                    {
                        let result = self.store.permanently_delete_portrait(entry, asset);
                        if result.is_ok() {
                            ui.ctx().data_mut(|data| data.remove::<bool>(id));
                        }
                        self.result(result, "Portrait permanently deleted");
                    }
                });
            });
        } else if ui.small_button("Delete portrait…").clicked() {
            ui.ctx().data_mut(|data| data.insert_temp(id, true));
        }
    }

    pub(crate) fn grok_portrait_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
        displayed_portrait: Option<&std::path::Path>,
    ) {
        let import_error = ui.make_persistent_id(("portrait-import-error", &entry.id));
        let hover_id = import_error.with("hover");
        let previous_hover = ui
            .data(|data| data.get_temp::<bool>(hover_id))
            .unwrap_or(false);
        let dragging = ui.input(|input| !input.raw.hovered_files.is_empty());
        let mut chosen = None;
        let zone = eframe::egui::Frame::group(ui.style())
            .stroke(eframe::egui::Stroke::new(
                1_f32,
                if dragging && previous_hover {
                    crate::GOLD
                } else {
                    crate::theme::BORDER
                },
            ))
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 4.).max(0.));
                ui.label("Drop your portrait image here");
                ui.small("PNG, JPEG or WebP · up to 24 MiB · no generation");
                if ui.button("Use my image…").clicked() {
                    chosen = rfd::FileDialog::new()
                        .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                        .pick_file();
                }
            });
        let over = ui
            .input(|input| input.pointer.hover_pos())
            .map(|pos| zone.response.rect.intersect(ui.clip_rect()).contains(pos));
        let dropped = ui.input(|input| input.raw.dropped_files.clone());
        if !dropped.is_empty() && over.unwrap_or(previous_hover) {
            if dropped.len() == 1 && dropped[0].path.is_some() {
                chosen = dropped[0].path.clone();
            } else {
                ui.data_mut(|data| {
                    data.insert_temp(
                        import_error,
                        "Drop one image file from Finder, or use Use my image.".to_string(),
                    )
                });
            }
        }
        ui.data_mut(|data| data.insert_temp(hover_id, dragging && over.unwrap_or(false)));
        if let Some(path) = chosen {
            match self.store.import_character_portrait(entry, &path) {
                Ok(()) => {
                    ui.data_mut(|data| data.remove::<String>(import_error));
                    ui.ctx().request_repaint();
                }
                Err(error) => {
                    ui.data_mut(|data| data.insert_temp(import_error, error.to_string()));
                }
            }
        }
        if let Some(error) = ui.data(|data| data.get_temp::<String>(import_error)) {
            ui.colored_label(crate::GOLD, error);
        }
        let jobs = self.load(self.store.portrait_jobs(&entry.id));
        let deleted = self.store.deleted_portraits().unwrap_or_default();
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
                                && !deleted.contains(&asset.id)
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
        if let Some(job) = jobs.first().filter(|job| job.status != "completed") {
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
                    eframe::egui::Button::new("Generate portrait"),
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
        let deleted = self.store.deleted_portraits().unwrap_or_default();
        let portraits: Vec<_> = assets
            .iter()
            .filter(|asset| {
                !deleted.contains(&asset.id)
                    && asset.metadata["provider"] == "openrouter"
                    && asset.metadata["request"]["library_id"] == entry.id
            })
            .collect();
        if !portraits.is_empty() {
            ui.add_space(10.);
            ui.label(
                eframe::egui::RichText::new(format!("Generated portraits ({})", portraits.len()))
                    .strong(),
            );
            let columns = ((ui.available_width() + 12.) / 192.).floor().clamp(1., 4.) as usize;
            let width =
                ((ui.available_width() - 12. * (columns - 1) as f32) / columns as f32).min(210.);
            eframe::egui::Grid::new((&entry.id, "portrait-choices"))
                .num_columns(columns)
                .spacing(eframe::egui::vec2(12., 16.))
                .show(ui, |ui| {
                    for (index, asset) in portraits.iter().enumerate() {
                        ui.push_id(&asset.id, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                self.portrait_gallery_tile(ui, entry, asset, width);
                            });
                        });
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        }

        let trashed: Vec<_> = assets
            .iter()
            .filter(|asset| {
                deleted.contains(&asset.id) && asset.metadata["request"]["library_id"] == entry.id
            })
            .collect();
        if !trashed.is_empty() {
            eframe::egui::CollapsingHeader::new(format!("Deleted portraits ({})", trashed.len()))
                .id_salt((&entry.id, "deleted-portraits"))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for asset in trashed {
                            ui.push_id(&asset.id, |ui| {
                                ui.vertical(|ui| {
                                    self.reference_preview(
                                        ui,
                                        &asset.path,
                                        eframe::egui::vec2(140., 180.),
                                    );
                                    if ui.small_button("Restore portrait").clicked() {
                                        let result = self.store.restore_portrait(&asset.id);
                                        self.result(result, "Portrait restored");
                                    }
                                    self.portrait_delete_button(ui, &entry.id, &asset.id);
                                });
                            });
                        }
                    });
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
