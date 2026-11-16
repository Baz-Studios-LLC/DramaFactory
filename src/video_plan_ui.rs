use crate::Studio;
#[derive(Default)]
pub struct VideoPlanner {
    drafts: std::collections::HashMap<String, crate::generation_plan::SavedPlan>,
    selected: std::collections::HashMap<String, usize>,
    pending: Option<std::sync::mpsc::Receiver<Result<serde_json::Value, String>>>,
    preview: Option<Result<serde_json::Value, String>>,
}
impl Studio {
    pub(crate) fn snapshot_generation_plans(&self) -> anyhow::Result<()> {
        if !self.video_planner.drafts.is_empty() {
            let directory = self.store.root().join("drafts");
            std::fs::create_dir_all(&directory)?;
            for plan in self.video_planner.drafts.values() {
                std::fs::write(
                    directory.join(format!(
                        "generation-plan-{}-{}.json",
                        plan.episode_id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(plan)?,
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn hosted_video_planner(&mut self, ui: &mut eframe::egui::Ui) {
        self.episode_picker(ui);
        let Some(episode) = self.episode_id.clone() else {
            ui.label("Choose an episode to prepare generation.");
            return;
        };
        if !self.video_planner.drafts.contains_key(&episode) {
            match self.store.generation_plan(&episode) {
                Ok(plan) => {
                    self.video_planner.drafts.insert(episode.clone(), plan);
                }
                Err(error) => {
                    ui.label(error.to_string());
                    return;
                }
            }
        }
        let cast = self
            .selected
            .as_ref()
            .map(|series| self.store.library(series, "character"))
            .transpose()
            .unwrap_or_default()
            .unwrap_or_default();
        let state = &mut self.video_planner;
        let plan = state.drafts.get_mut(&episode).expect("loaded plan");
        let index = state.selected.entry(episode.clone()).or_default();
        *index = (*index).min(plan.segments.len().saturating_sub(1));
        ui.horizontal_wrapped(|ui| {
            for number in 0..plan.segments.len() {
                ui.selectable_value(index, number, format!("Segment {}", number + 1));
            }
            if ui
                .add_enabled(
                    plan.segments.len() < 120,
                    eframe::egui::Button::new("+ Add segment"),
                )
                .clicked()
            {
                plan.segments.push(Default::default());
                *index = plan.segments.len() - 1;
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save episode plan").clicked() {
                match self.store.save_generation_plan(plan) {
                    Ok(saved) => {
                        *plan = saved;
                        self.message = "Generation plan saved; no generation started".into();
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            if ui.button("Reload saved plan").clicked() {
                match self.store.generation_plan(&episode) {
                    Ok(saved) => {
                        *plan = saved;
                        *index = 0;
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            ui.small(format!(
                "Saved revision {} · edits require Save",
                plan.revision
            ));
        });
        let total_seconds: u32 = plan.segments.iter().map(|segment| segment.duration).sum();
        ui.label(format!(
            "Planned episode: {} segments · {}m {:02}s",
            plan.segments.len(),
            total_seconds / 60,
            total_seconds % 60
        ));
        let draft = &mut plan.segments[*index];
        if let Some(receiver) = &state.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    state.preview = Some(result);
                    state.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    state.preview = Some(Err("Preview check stopped. Try again.".into()));
                    state.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        Self::card(ui, |ui| {
            ui.heading("Prepare a Seedance segment");
            ui.label("Describe the action, dialogue, camera, music and sound in one prompt.");
            Self::field(ui, "SEGMENT PROMPT", &mut draft.prompt, 4);
            ui.horizontal_wrapped(|ui| {
                ui.label("Clip length");
                ui.add(
                    eframe::egui::DragValue::new(&mut draft.duration)
                        .range(4..=30)
                        .suffix(" seconds"),
                );
                ui.label("Resolution");
                ui.selectable_value(&mut draft.resolution, "480p".into(), "480p");
                ui.selectable_value(&mut draft.resolution, "720p".into(), "720p");
            });
            ui.small("Vertical 9:16 with generated audio. Each segment is a separate request; save changes before generating. Live capability validation runs before submission.");
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut draft.mode, 0, "Prompt only");
                ui.selectable_value(&mut draft.mode, 1, "Character & scene references");
                ui.selectable_value(&mut draft.mode, 2, "First / last frame");
            });
            match draft.mode {
                1 => {
                    ui.label("Characters in this segment");
                    for entry in &cast {
                        let mut selected = draft
                            .cast
                            .iter()
                            .any(|reference| reference.character_id == entry.id);
                        if ui
                            .add_enabled(
                                entry.reference.is_some(),
                                eframe::egui::Checkbox::new(&mut selected, &entry.name),
                            )
                            .changed()
                        {
                            if selected {
                                match self.store.prepare_cast_reference(&episode, &entry.id) {
                                    Ok(reference) => draft.cast.push(reference),
                                    Err(error) => self.message = error.to_string(),
                                }
                            } else {
                                draft
                                    .cast
                                    .retain(|reference| reference.character_id != entry.id);
                            }
                        }
                    }
                    for reference in &draft.cast {
                        ui.horizontal(|ui| {
                            ui.add(
                                eframe::egui::Image::from_uri(format!(
                                    "file://{}",
                                    reference.path.display()
                                ))
                                .fit_to_exact_size(eframe::egui::vec2(90., 120.)),
                            );
                            ui.vertical(|ui| {
                                ui.label(&reference.name);
                                ui.small("Selected portrait linked");
                                if let Err(error) =
                                    self.store.validate_cast_reference(&episode, reference)
                                {
                                    ui.label(error.to_string());
                                }
                            });
                        });
                    }
                    if !draft.cast.is_empty() {
                        ui.label("These local portrait links are saved, but cannot yet be sent to Seedance. OpenRouter documents public HTTPS image URLs for video references; its Files API does not provide a video-readable upload URL. Generation is blocked while local portraits are linked.");
                    }
                    ui.label("Public HTTPS reference image URLs, one per line (required if no local portraits are linked).");
                    Self::field(ui, "REFERENCES", &mut draft.references, 3);
                }
                2 => {
                    ui.label("Frame-control mode sends frame anchors only. Saved character selections remain in the plan but are inactive in this mode.");
                    ui.label("First frame HTTPS URL");
                    ui.text_edit_singleline(&mut draft.first);
                    ui.label("Last frame HTTPS URL (optional)");
                    ui.text_edit_singleline(&mut draft.last);
                }
                _ => {
                    ui.label("No image inputs will be sent.");
                }
            }
            ui.label(
                "Segment profile: Seedance 2.5 · 10 seconds · 720p · vertical 9:16 · sound enabled",
            );
            if ui
                .add_enabled(
                    state.pending.is_none()
                        && !draft.prompt.trim().is_empty()
                        && !(draft.mode == 1 && !draft.cast.is_empty()),
                    eframe::egui::Button::new("Validate request preview"),
                )
                .clicked()
            {
                let plan = match draft.video_plan() {
                    Ok(plan) => plan,
                    Err(error) => {
                        state.preview = Some(Err(error.to_string()));
                        return;
                    }
                };
                let (sender, receiver) = std::sync::mpsc::channel();
                state.pending = Some(receiver);
                state.preview = None;
                std::thread::spawn(move || {
                    let result =
                        crate::openrouter::discover().and_then(|catalog| plan.preview(&catalog));
                    let _ = sender.send(result.map_err(|error| error.to_string()));
                });
            }
            ui.small("Generating uses the saved segment. Unsaved edits must be saved first. This is a paid OpenRouter request; actual cost comes from the provider.");
            if ui
                .add_enabled(
                    !draft.prompt.trim().is_empty() && !(draft.mode == 1 && !draft.cast.is_empty()),
                    eframe::egui::Button::new("Generate saved segment · paid"),
                )
                .clicked()
            {
                let result = self
                    .store
                    .enqueue_episode_segment(&episode, plan.revision, draft)
                    .and_then(|job| {
                        if job.status == "queued" {
                            crate::worker::spawn_detached(&self.store.path)?;
                        }
                        Ok(job)
                    });
                match result {
                    Ok(job) => {
                        self.message = format!("Segment request {} · {}", &job.id[..8], job.status)
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            match self.store.segment_generation_jobs(&episode, &draft.id) {
                Ok(jobs) => {
                    for job in jobs {
                        ui.small(format!(
                            "Plan revision {} · {} · {}",
                            job["plan_revision"],
                            job["status"].as_str().unwrap_or("unknown"),
                            job["stage"].as_str().unwrap_or("")
                        ));
                        if let Some(asset_id) = job["asset_id"].as_str() {
                            let selected = self
                                .store
                                .selected_segment_take(&episode, &draft.id)
                                .ok()
                                .flatten();
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Open take").clicked()
                                    && let Ok(assets) = self.store.assets()
                                    && let Some(asset) =
                                        assets.iter().find(|asset| asset.id == asset_id)
                                {
                                    #[cfg(target_os = "macos")]
                                    {
                                        let result = std::process::Command::new("/usr/bin/open")
                                            .arg(&asset.path)
                                            .spawn()
                                            .map(|_| ())
                                            .map_err(anyhow::Error::from);
                                        self.message = result
                                            .err()
                                            .map(|error| error.to_string())
                                            .unwrap_or_else(|| {
                                                "Opened original take in system player".into()
                                            });
                                    }
                                }
                                if ui.button("Approve take").clicked() {
                                    self.message = match self.store.review(asset_id, "approved") {
                                        Ok(()) => "Take approved after your review".into(),
                                        Err(error) => error.to_string(),
                                    };
                                }
                                if ui
                                    .button(if selected.as_deref() == Some(asset_id) {
                                        "Selected take"
                                    } else {
                                        "Use this take"
                                    })
                                    .clicked()
                                {
                                    self.message = match self.store.select_segment_take(
                                        &episode,
                                        &draft.id,
                                        asset_id,
                                        selected.as_deref(),
                                    ) {
                                        Ok(()) => "Segment take selected".into(),
                                        Err(error) => error.to_string(),
                                    };
                                }
                            });
                        }
                    }
                }
                Err(error) => {
                    ui.label(error.to_string());
                }
            }
            if ui.button("Assemble saved episode").clicked() {
                self.message = match self
                    .store
                    .enqueue_hosted_episode_export(&episode)
                    .and_then(|_| crate::worker::spawn_detached(&self.store.path))
                {
                    Ok(()) => "Episode assembly queued with original segment soundtracks".into(),
                    Err(error) => error.to_string(),
                };
            }
            if state.pending.is_some() {
                ui.label("Checking current model capabilities…");
            }
            if let Some(result) = &state.preview {
                match result {
                    Ok(value) => {
                        ui.label("Last checked request snapshot — changes above require another check. Not submitted.");
                        ui.collapsing("Inspect payload", |ui| {
                            ui.code(serde_json::to_string_pretty(value).unwrap_or_default());
                        });
                    }
                    Err(error) => {
                        ui.label(error);
                    }
                }
            }
            ui.small("Validation preview is free. Generate is a separate paid action. Automatic local-portrait transport is unresolved. Public HTTPS image references and first/last frame URLs are supported; endpoint-specific media validation remains incomplete.");
        });
    }
}
