use crate::{Studio, production::LibraryEntry};
impl Studio {
    pub(crate) fn grok_portrait_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
    ) {
        let mut style = match self.store.portrait_style(&entry.id) {
            Ok(style) => style,
            Err(error) => {
                ui.label(format!("Could not load image style: {error}"));
                return;
            }
        };
        ui.label("IMAGE STYLE");
        ui.horizontal_wrapped(|ui| {
            for option in crate::portrait_style::PortraitStyle::ALL {
                if ui
                    .selectable_label(style == option, option.label())
                    .clicked()
                {
                    match self.store.save_portrait_style(&entry.id, option) {
                        Ok(()) => style = option,
                        Err(error) => self.message = error.to_string(),
                    }
                }
            }
        });
        ui.small("Applies to your next image. Existing portraits stay unchanged; style guidance does not guarantee a particular result.");
        let jobs = self.load(self.store.portrait_jobs(&entry.id));
        let pending = jobs
            .iter()
            .any(|job| matches!(job.status.as_str(), "queued" | "running" | "blocked"));
        if let Some(job) = jobs.first() {
            ui.horizontal(|ui| {
                if matches!(job.status.as_str(), "queued" | "running") {
                    ui.spinner();
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                }
                ui.label(match job.status.as_str() {
                    "queued" => "Portrait queued",
                    "running" => "Generating portrait…",
                    "completed" => "Portrait ready · review the image below",
                    "failed" => "Portrait generation failed",
                    "canceled" => "Portrait generation canceled",
                    _ => "Portrait job needs attention",
                });
            });
            if let Ok(timing) = self.store.job_timing(&job.id) {
                let elapsed = timing
                    .attempt_elapsed_seconds
                    .map(crate::job_timing::duration)
                    .unwrap_or_else(|| "not started".into());
                ui.small(format!("{} · elapsed {elapsed}", timing.stage));
            }
            if !job.error.is_empty() {
                ui.colored_label(eframe::egui::Color32::from_rgb(239, 149, 126), &job.error);
            }
            if job.status == "running" {
                ui.small(
                    "Time remaining is unavailable. A separate worker may need Keychain approval.",
                );
            }
        }
        if ui
            .add_enabled(
                !pending,
                eframe::egui::Button::new(if pending {
                    "Image generation in progress"
                } else {
                    "Generate image · paid"
                }),
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
                    self.store.enqueue_grok_portrait(&entry.id, prompt)
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
            })
            .collect();
        if !portraits.is_empty() {
            ui.add_space(10.);
            ui.label(eframe::egui::RichText::new("Generated portraits").strong());
            ui.small("Review at full size before choosing the face for your series.");
            ui.horizontal_wrapped(|ui| {
                for asset in portraits {
                    ui.push_id(&asset.id, |ui| {
                        ui.vertical(|ui| {
                            self.reference_preview(ui, &asset.path, eframe::egui::vec2(260., 340.));
                            let selected = entry.reference.as_ref() == Some(&asset.path);
                            ui.small(if selected {
                                "Selected character portrait"
                            } else {
                                "New portrait option"
                            });
                            if ui
                                .add_enabled(
                                    !selected,
                                    eframe::egui::Button::new("Use this portrait"),
                                )
                                .clicked()
                            {
                                let result = self.store.review(&asset.id, "approved");
                                self.result(result, "Character portrait selected");
                            }
                        });
                    });
                }
            });
        }
        eframe::egui::CollapsingHeader::new("Portrait prompt & settings").id_salt((&entry.id,"grok-portrait")).show(ui,|ui|{
            if !self.portrait_prompts.contains_key(&entry.id) {
                match self.store.portrait_prompt(entry){Ok(draft)=>{self.portrait_prompts.insert(entry.id.clone(),draft);},Err(error)=>{ui.label(error.to_string());return;}}
            }
            if self.sidekick.character_undo.contains_key(&entry.id) && ui.button("Undo sidekick character edit").clicked() {
                let result=self.undo_character_sidekick(&entry.series_id,&entry.id);self.result(result,"Character and portrait drafts restored");
            }
            let mut draft=self.portrait_prompts[&entry.id].clone();
            Self::field(ui,"PORTRAIT PROMPT",&mut draft.prompt,4);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save portrait prompt").clicked(){match self.store.save_portrait_prompt(&draft){Ok(saved)=>{draft=saved;self.message="Portrait prompt saved".into();},Err(error)=>self.message=error.to_string()}}
                if ui.button("Reload portrait prompt").clicked(){match self.store.portrait_prompt(entry){Ok(saved)=>draft=saved,Err(error)=>self.message=error.to_string()}}
            });
            let prompt=draft.prompt.clone();
            self.portrait_prompts.insert(entry.id.clone(),draft);
            ui.small("OpenRouter · 1K · 3:4 · medium quality. Creates a new option, not a replacement. Uses your paid account; cost is reported by the provider. Text-to-image only in this first pass.");
            if ui.add_enabled(!pending && !prompt.trim().is_empty(),eframe::egui::Button::new("Generate portrait · paid")).clicked(){
                let result=self.store.enqueue_grok_portrait(&entry.id,&prompt).and_then(|_|crate::worker::spawn_detached(&self.store.path));
                self.result(result,"Grok portrait queued; review the result before choosing it");
            }

        });
    }
}
