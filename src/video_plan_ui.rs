use crate::Studio;
#[derive(Default)]
pub struct VideoPlanner {
    pub(crate) drafts: std::collections::HashMap<String, crate::generation_plan::SavedPlan>,
    pub(crate) section: u8,
    catalog: Option<crate::openrouter::Catalog>,
    catalog_pending: Option<std::sync::mpsc::Receiver<anyhow::Result<crate::openrouter::Catalog>>>,
    catalog_error: Option<String>,
    preview_source: Option<(String, crate::generation_plan::SegmentDraft)>,
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
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.video_planner.section, 0, "Prepare");
            ui.selectable_value(&mut self.video_planner.section, 1, "Takes");
            ui.selectable_value(&mut self.video_planner.section, 2, "Finished episode");
        });
        ui.add_space(12.);
        self.episode_picker(ui);
        let state = &mut self.video_planner;
        if state.catalog.is_none()
            && state.catalog_pending.is_none()
            && state.catalog_error.is_none()
        {
            let (tx, rx) = std::sync::mpsc::channel();
            state.catalog_pending = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(crate::openrouter::discover());
            });
        }
        if let Some(rx) = &state.catalog_pending {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(catalog) => state.catalog = Some(catalog),
                        Err(e) => state.catalog_error = Some(e.to_string()),
                    }
                    state.catalog_pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    state.catalog_error = Some("Capability check stopped".into());
                    state.catalog_pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        }
        if let Some(error) = state.catalog_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(error);
                if ui.button("Retry capability check").clicked() {
                    state.catalog_error = None;
                }
            });
        }

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
        if let Some((before, after)) = self.sidekick.generation_undo.get(&episode).cloned()
            && ui.small_button("Undo Sidekick plan").clicked()
        {
            match self.generation_snapshot() {
                Ok(current) if crate::sidekick_generation::same(&current, &after) => {
                    self.video_planner
                        .drafts
                        .insert(episode.clone(), before.plan);
                    self.sidekick.generation_undo.remove(&episode);
                    self.message = "Sidekick generation plan undone".into();
                }
                Ok(_) => {
                    self.message =
                        "Plan or source material changed; Undo would overwrite newer work".into()
                }
                Err(error) => self.message = error.to_string(),
            }
        }
        let cast = self
            .selected
            .as_ref()
            .map(|series| self.store.library(series, "character"))
            .transpose()
            .unwrap_or_default()
            .unwrap_or_default();
        if self.video_planner.section == 0 {
            ui.collapsing("Episode screenplay", |ui| {
                match self.store.get_episode(&episode) {
                    Ok(episode) if !episode.screenplay.trim().is_empty() => {
                        eframe::egui::ScrollArea::vertical().id_salt("generation_screenplay").max_height(220.).show(ui, |ui| { ui.label(&episode.screenplay); });
                    }
                    Ok(_) => { ui.label("Save the screenplay on Episode plot to keep it beside your segment direction."); }
                    Err(error) => { ui.label(error.to_string()); }
                }
            });
        }
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
                    state.section == 0 && plan.segments.len() < 120,
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
                let reload = (|| -> anyhow::Result<_> {
                    let directory = self.store.root().join("drafts");
                    std::fs::create_dir_all(&directory)?;
                    std::fs::write(
                        directory.join(format!(
                            "generation-before-reload-{}.json",
                            uuid::Uuid::new_v4()
                        )),
                        serde_json::to_vec_pretty(plan)?,
                    )?;
                    self.store.generation_plan(&episode)
                })();
                match reload {
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
        let saved = self.store.generation_plan(&episode).ok();
        let dirty = saved.as_ref().is_none_or(|saved| {
            saved.revision == 0
                || saved.revision != plan.revision
                || saved.segments != plan.segments
        });
        ui.label(
            eframe::egui::RichText::new(if dirty {
                "Unsaved generation plan"
            } else {
                "Generation plan saved"
            })
            .color(if dirty { crate::GOLD } else { crate::MUTED }),
        );
        if state.section == 0 {
            let validation = state
                .catalog
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Checking Seedance capabilities…"))
                .and_then(|catalog| {
                    for segment in &plan.segments {
                        segment.video_plan()?.preview(catalog)?;
                    }
                    Ok(())
                });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(validation.is_ok(), crate::theme::primary(&format!("Generate episode · {} segments · paid", plan.segments.len())))
                    .on_hover_text("Saves this plan and queues segments without an existing identical request. Existing requests are not retried.").clicked() {
                    match self.store.enqueue_generation_plan(plan, state.catalog.as_ref().expect("validated catalog")) {
                        Ok((saved, count)) => {
                            *plan = saved;
                            self.message = if count == 0 { "Every segment already has a request. Review Takes for results or failures; no new requests were queued.".into() } else {
                                match crate::worker::spawn_detached(&self.store.path) {
                                    Ok(()) => format!("Plan saved; {count} segment requests queued"),
                                    Err(error) => format!("Plan saved and requests queued, but worker did not start: {error}"),
                                }
                            };
                            state.section = 1;
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
            });
            if let Err(error) = validation {
                ui.small(error.to_string());
            }
            ui.small("Generates all missing segments using your paid OpenRouter account. Actual usage determines the cost.");
        }
        if state.section == 2 {
            Self::card(ui, |ui| {
                ui.heading("Episode assembly");
                ui.label("Selected takes play in this order, with their original sound.");
                let mut missing = 0;
                for (number, segment) in plan.segments.iter().enumerate() {
                    let selected = self
                        .store
                        .selected_segment_take(&episode, &segment.id)
                        .ok()
                        .flatten();
                    if selected.is_none() {
                        missing += 1;
                    }
                    ui.label(format!(
                        "{:02} · {} seconds · {}",
                        number + 1,
                        segment.duration,
                        if selected.is_some() {
                            "Take selected"
                        } else {
                            "Choose an approved take"
                        }
                    ));
                }
                if dirty {
                    ui.label("Save the plan before assembling this episode.");
                }
                if ui
                    .add_enabled(
                        !dirty && missing == 0,
                        crate::theme::primary("Assemble episode · MP4"),
                    )
                    .clicked()
                {
                    self.message = match self
                        .store
                        .enqueue_hosted_episode_export(&episode)
                        .and_then(|_| crate::worker::spawn_detached(&self.store.path))
                    {
                        Ok(()) => {
                            "Episode assembly queued with original segment soundtracks".into()
                        }
                        Err(error) => error.to_string(),
                    };
                }
                ui.small("Review dialogue, music and continuity across each cut before approving the finished episode.");
            });
            return;
        }
        if state.section == 0 {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(*index > 0, eframe::egui::Button::new("Move earlier"))
                    .clicked()
                {
                    plan.segments.swap(*index, *index - 1);
                    *index -= 1;
                }
                if ui
                    .add_enabled(
                        *index + 1 < plan.segments.len(),
                        eframe::egui::Button::new("Move later"),
                    )
                    .clicked()
                {
                    plan.segments.swap(*index, *index + 1);
                    *index += 1;
                }
            });
        }
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
            if state.section == 0 {
                ui.heading(format!("Segment {} · Seedance 2.5", *index + 1));
                ui.label("Describe the action, dialogue, camera, music and sound in one prompt.");
                Self::field(ui, "SEGMENT PROMPT", &mut draft.prompt, 4);
                let model = state.catalog.as_ref().and_then(|catalog| {
                    catalog
                        .videos
                        .iter()
                        .find(|model| model["id"] == "bytedance/seedance-2.5")
                });
                ui.add_enabled_ui(model.is_some(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Clip length");
                        eframe::egui::ComboBox::from_id_salt("seedance_duration")
                            .selected_text(format!("{} seconds", draft.duration))
                            .show_ui(ui, |ui| {
                                if let Some(values) =
                                    model.and_then(|m| m["supported_durations"].as_array())
                                {
                                    for duration in values
                                        .iter()
                                        .filter_map(|v| v.as_u64())
                                        .filter(|v| (4..=30).contains(v))
                                    {
                                        ui.selectable_value(
                                            &mut draft.duration,
                                            duration as u32,
                                            format!("{duration} seconds"),
                                        );
                                    }
                                }
                            });
                        ui.label("Resolution");
                        if let Some(values) =
                            model.and_then(|m| m["supported_resolutions"].as_array())
                        {
                            for resolution in values
                                .iter()
                                .filter_map(|v| v.as_str())
                                .filter(|v| ["480p", "720p", "1080p"].contains(v))
                            {
                                ui.selectable_value(
                                    &mut draft.resolution,
                                    resolution.into(),
                                    resolution,
                                );
                            }
                        }
                    });
                });
                if model.is_none() {
                    ui.small("Checking available Seedance settings…");
                }
                ui.small("Vertical 9:16 · picture, dialogue and sound generated together");
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
                            ui.label("Local portraits need a delivery connection before they can be used. For now, unlink these and supply public HTTPS references to generate.");
                        }
                        ui.label("Public HTTPS reference image URLs, one per line (required if no local portraits are linked).");
                        Self::field(ui, "REFERENCES", &mut draft.references, 3);
                    }
                    2 => {
                        ui.label("Only the frame anchors below are sent in this mode. To continue a previous take, use its ending-frame HTTPS image URL. Automatic frame handoff is not connected yet.");
                        ui.label("First frame HTTPS URL");
                        ui.text_edit_singleline(&mut draft.first);
                        ui.label("Last frame HTTPS URL (optional)");
                        ui.text_edit_singleline(&mut draft.last);
                    }
                    _ => {
                        ui.label("No image inputs will be sent.");
                    }
                }
                ui.small(format!(
                    "Seedance 2.5 · {} seconds · {} · 9:16 · sound on",
                    draft.duration, draft.resolution
                ));
                if ui
                    .add_enabled(
                        state.pending.is_none()
                            && !draft.prompt.trim().is_empty()
                            && (draft.mode != 1 || draft.cast.is_empty()),
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
                    state.preview_source = Some((episode.clone(), draft.clone()));
                    state.pending = Some(receiver);
                    state.preview = None;
                    std::thread::spawn(move || {
                        let result = crate::openrouter::discover()
                            .and_then(|catalog| plan.preview(&catalog));
                        let _ = sender.send(result.map_err(|error| error.to_string()));
                    });
                }
                let valid = draft.video_plan().and_then(|request| {
                    request.preview(
                        state
                            .catalog
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("Wait for the capability check"))?,
                    )
                });
                if let Err(error) = &valid {
                    ui.small(error.to_string());
                }
                ui.small("One paid request for this segment. OpenRouter bills actual usage.");
                ui.collapsing("Current provider pricing", |ui| {
                    if let Some(model) = model {
                        ui.code(
                            serde_json::to_string_pretty(&model["pricing_skus"])
                                .unwrap_or_default(),
                        );
                    }
                    ui.small("Token rates do not establish an exact per-segment estimate.");
                });
                if ui
                    .add_enabled(
                        !dirty
                            && saved
                                .as_ref()
                                .is_some_and(|saved| saved.segments.get(*index) == Some(draft))
                            && valid.is_ok(),
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
                            self.message =
                                format!("Segment request {} · {}", &job.id[..8], job.status);
                            state.section = 1;
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
            }
            if state.section == 1 {
                ui.heading(format!("Segment {} · Take review", *index + 1));
                ui.small("Watch at full size. Check words, voices, faces, hands, wardrobe and continuity before approving.");
                match self.store.segment_generation_jobs(&episode, &draft.id) {
                    Ok(jobs) => {
                        if jobs.is_empty() {
                            ui.label("No takes yet. Prepare and generate this segment first.");
                        }
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
                                if let Ok(assets) = self.store.assets()
                                    && let Some(asset) =
                                        assets.iter().find(|asset| asset.id == asset_id)
                                {
                                    ui.label(format!(
                                        "{} × {} · {:.1}s · {}{}",
                                        asset.width,
                                        asset.height,
                                        asset.duration,
                                        asset.review,
                                        if selected.as_deref() == Some(asset_id) {
                                            " · SELECTED"
                                        } else {
                                            ""
                                        }
                                    ));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button("Open take").clicked()
                                        && let Ok(assets) = self.store.assets()
                                        && let Some(asset) =
                                            assets.iter().find(|asset| asset.id == asset_id)
                                    {
                                        #[cfg(target_os = "macos")]
                                        {
                                            let result =
                                                std::process::Command::new("/usr/bin/open")
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
                                        self.message = match self.store.review(asset_id, "approved")
                                        {
                                            Ok(()) => "Take approved after your review".into(),
                                            Err(error) => error.to_string(),
                                        };
                                    }
                                    if ui.button("Reject take").clicked() {
                                        self.message = match self.store.review(asset_id, "rejected")
                                        {
                                            Ok(()) => "Take rejected; original retained".into(),
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
            }
            if state.section == 0
                && state
                    .preview_source
                    .as_ref()
                    .is_some_and(|(id, snapshot)| id == &episode && snapshot == draft)
            {
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
            }
        });
    }
}
