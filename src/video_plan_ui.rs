use crate::Studio;
#[derive(Default)]
pub struct VideoPlanner {
    pub(crate) drafts: std::collections::HashMap<String, crate::generation_plan::SavedPlan>,
    pub(crate) section: u8,
    pub(crate) show_old_attempts: bool,
    pub(crate) player: Option<crate::media::Preview>,
    pub(crate) player_visible: bool,
    player_asset: Option<String>,
    player_error: Option<String>,
    thumbnails: crate::take_gallery::Thumbnails,
    open_take: Option<String>,
    delete_take: Option<String>,
    credits: Option<Result<crate::credit_status::Credits, String>>,
    credits_pending:
        Option<std::sync::mpsc::Receiver<Result<crate::credit_status::Credits, String>>>,
    credits_checked: Option<std::time::Instant>,
    catalog: Option<crate::openrouter::Catalog>,
    catalog_pending: Option<std::sync::mpsc::Receiver<anyhow::Result<crate::openrouter::Catalog>>>,
    catalog_error: Option<String>,
    generation_error: Option<String>,
    selected: std::collections::HashMap<String, usize>,
}
impl Studio {
    fn video_credit_status(&mut self, ui: &mut eframe::egui::Ui) {
        let state = &mut self.video_planner;
        if let Some(rx) = &state.credits_pending {
            match rx.try_recv() {
                Ok(result) => {
                    state.credits = Some(result);
                    state.credits_checked = Some(std::time::Instant::now());
                    state.credits_pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    state.credits = Some(Err("Credit check stopped".into()));
                    state.credits_checked = Some(std::time::Instant::now());
                    state.credits_pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut refresh = state
            .credits_checked
            .is_none_or(|time| time.elapsed().as_secs() >= 60);
        ui.horizontal_wrapped(|ui| {
            ui.strong("Credits");
            if state.credits_pending.is_some() {
                ui.spinner();
            }
            if let Some(result) = &state.credits {
                match result {
                    Ok(credits) => {
                        if let Some(balance) = credits.balance {
                            ui.label(format!("${balance:.2} available"));
                        }
                        if let Some(allowance) = credits.allowance {
                            ui.label(format!("Key limit: ${allowance:.2}"));
                        }
                        if let Some(note) = &credits.balance_note {
                            ui.weak(note);
                        }
                        if credits.balance.is_some_and(|n| n <= 0.)
                            || credits.allowance.is_some_and(|n| n <= 0.)
                        {
                            ui.colored_label(
                                eframe::egui::Color32::from_rgb(255, 150, 120),
                                "Credits or key allowance exhausted",
                            );
                        }
                    }
                    Err(error) => {
                        ui.label(error);
                    }
                }
            }
            if ui
                .add_enabled(
                    state.credits_pending.is_none(),
                    eframe::egui::Button::new("Refresh").small(),
                )
                .clicked()
            {
                refresh = true;
            }
            ui.hyperlink_to("Manage credits", "https://openrouter.ai/settings/credits");
        });
        if refresh && state.credits_pending.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            state.credits_pending = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(crate::credit_status::fetch().map_err(|error| error.to_string()));
            });
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
        ui.add_space(8.);
    }

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
        ui.spacing_mut().item_spacing.y = 8.;
        self.video_credit_status(ui);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.video_planner.section, 0, "Prepare");
            ui.selectable_value(&mut self.video_planner.section, 1, "Takes");
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
        let total_seconds: u32 = plan.segments.iter().map(|segment| segment.duration).sum();
        let saved = self.store.generation_plan(&episode).ok();
        let dirty = saved.as_ref().is_none_or(|saved| {
            saved.revision == 0
                || saved.revision != plan.revision
                || saved.segments != plan.segments
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save plan").clicked() {
                match self.store.save_generation_plan(plan) {
                    Ok(saved) => {
                        *plan = saved;
                        self.message = "Generation plan saved; no generation started".into();
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            ui.small(format!(
                "{} segments · ~{}m {:02}s planned · {}",
                plan.segments.len(),
                total_seconds / 60,
                total_seconds % 60,
                if dirty { "Unsaved changes" } else { "Saved" }
            ));
            if ui.small_button("Reload").clicked() {
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
        });
        let cast_error = match self.store.current_plan_cast(plan) {
            Ok(current) => {
                *plan = current;
                None
            }
            Err(error) => Some(format!("{error:#}")),
        };
        if let Some(error) = &cast_error {
            ui.colored_label(crate::GOLD, error);
        }
        if state.section == 0 {
            ui.collapsing("Generate all segments", |ui| {
            let validation = state
                .catalog
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Checking model capabilities…"))
                .and_then(|catalog| {
                    for (number, segment) in plan.segments.iter().enumerate() {
                        segment.video_plan().and_then(|request| request.preview(catalog))
                            .map_err(|error| anyhow::anyhow!("Segment {}: {error}", number + 1))?;
                    }
                    Ok(())
                });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(validation.is_ok(), crate::theme::primary(&format!("Generate {} segments", plan.segments.len())))
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
            });
        }
        if state.section == 0 {
            ui.collapsing("Reorder segment", |ui| {
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
            });
        }
        let mut generate_segment = false;
        let draft = &mut plan.segments[*index];
        Self::card(ui, |ui| {
            if state.section == 0 {
                ui.heading(format!("Segment {}", *index + 1));

                Self::field(ui, "SEGMENT PROMPT", &mut draft.prompt, 4);
                let model = state.catalog.as_ref().and_then(|catalog| {
                    catalog
                        .videos
                        .iter()
                        .find(|model| model["id"] == draft.model)
                });
                ui.add_enabled_ui(model.is_some(), |ui| {
                    ui.horizontal_wrapped(|ui| {
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
                    ui.small("Checking available model settings…");
                }
                ui.small("9:16 · Sound on");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut draft.mode, 0, "Prompt only");
                    ui.selectable_value(&mut draft.mode, 1, "Reference images");
                    ui.selectable_value(&mut draft.mode, 2, "First / last frame");
                });
                match draft.mode {
                    1 => {
                        ui.label("Characters in this segment");
                        let columns = ((ui.available_width() / 124.).floor() as usize).clamp(1, 8);
                        let width = ((ui.available_width() - 10. * (columns - 1) as f32)
                            / columns as f32)
                            .min(120.);
                        eframe::egui::Grid::new(("segment-cast", &draft.id))
                            .num_columns(columns)
                            .spacing([10., 10.])
                            .show(ui, |ui| {
                                for (number, entry) in cast.iter().enumerate() {
                                    let selected = draft
                                        .cast
                                        .iter()
                                        .any(|reference| reference.character_id == entry.id);
                                    if crate::character_grid::cast_choice(
                                        ui, entry, selected, width,
                                    ) {
                                        if selected {
                                            draft.cast.retain(|reference| {
                                                reference.character_id != entry.id
                                            });
                                        } else {
                                            match self
                                                .store
                                                .prepare_cast_reference(&episode, &entry.id)
                                            {
                                                Ok(reference) => draft.cast.push(reference),
                                                Err(error) => self.message = error.to_string(),
                                            }
                                        }
                                    }
                                    if (number + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                    2 => {
                        ui.small(
                            "Add frame image URLs. Previous takes are not linked automatically.",
                        );
                        ui.label("First frame HTTPS URL");
                        ui.text_edit_singleline(&mut draft.first);
                        ui.label("Last frame HTTPS URL (optional)");
                        ui.text_edit_singleline(&mut draft.last);
                    }
                    _ => {}
                }

                ui.horizontal_wrapped(|ui| {
                    ui.label("Model");
                    let choice = crate::video_plan::model_choice(&draft.model);
                    eframe::egui::ComboBox::from_id_salt(("video-model", &draft.id))
                        .selected_text(choice.map_or("Choose model", |c| c.name))
                        .show_ui(ui, |ui| {
                            for choice in crate::video_plan::MODEL_CHOICES {
                                let available = state.catalog.as_ref().is_some_and(|catalog| {
                                    catalog.videos.iter().any(|m| m["id"] == choice.id)
                                });
                                ui.add_enabled_ui(available, |ui| {
                                    if ui
                                        .selectable_value(
                                            &mut draft.model,
                                            choice.id.into(),
                                            format!("{} · {}s", choice.name, choice.duration),
                                        )
                                        .changed()
                                    {
                                        if draft.resolution == "480p"
                                            && choice.minimum_resolution == "720p"
                                        {
                                            draft.resolution = "720p".into();
                                        }
                                        state.generation_error = None;
                                    }
                                });
                            }
                        });
                    if let Some(choice) = crate::video_plan::model_choice(&draft.model) {
                        ui.small(format!("{} seconds", choice.duration));
                    }
                });
                if draft.model == "google/veo-3.1" {
                    ui.small("8 seconds · 720p minimum · up to 3 character references");
                }
                if draft.model == "bytedance/seedance-2.0" {
                    ui.small("15-second maximum — use a shorter scene for natural dialogue.");
                }

                if let Ok(request) = draft.video_plan() {
                    let count = request.assembled_prompt().chars().count();
                    let label = match request.prompt_limit() {
                        Some(limit) => format!(
                            "Prompt: {count} / {limit} characters · includes character profiles"
                        ),
                        None => format!(
                            "Prompt: {count} characters · includes character profiles · provider limit unverified"
                        ),
                    };
                    ui.small(label);
                }
                let valid = draft.video_plan().and_then(|request| {
                    if let Some(error) = &cast_error {
                        anyhow::bail!("{error}");
                    }
                    request.preview(
                        state
                            .catalog
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("Wait for the capability check"))?,
                    )
                });
                if let Err(error) = &valid {
                    ui.colored_label(crate::GOLD, error.to_string());
                }
                if ui
                    .add_enabled(valid.is_ok(), crate::theme::primary("Generate video"))
                    .clicked()
                {
                    state.generation_error = None;
                    generate_segment = true;
                }

                if let Some(error) = &state.generation_error {
                    ui.colored_label(crate::GOLD, error);
                }
                ui.collapsing("Technical details", |ui| {
                    if let Ok(value) = &valid {
                        ui.code(serde_json::to_string_pretty(value).unwrap_or_default());
                    }
                });
            }
            if state.section == 1 {
                ui.heading(format!("Segment {} · Take review", *index + 1));

                match self.store.segment_generation_jobs(&episode, &draft.id) {
                    Ok(jobs) => {
                        if jobs.is_empty() {
                            ui.label("No takes yet. Prepare and generate this segment first.");
                        }
                        let episode_label = self
                            .selected
                            .as_deref()
                            .and_then(|series| self.store.ordered_gui_episodes(series).ok())
                            .and_then(|episodes| {
                                episodes.iter().position(|item| item.id == episode)
                            })
                            .map(|number| format!("Episode {}", number + 1))
                            .unwrap_or_else(|| "Episode".into());
                        let assets = self.store.assets().unwrap_or_default();
                        let selected_clip = self
                            .store
                            .selected_segment_take(&episode, &draft.id)
                            .ok()
                            .flatten();
                        let take_numbers: std::collections::HashMap<String, usize> = jobs
                            .iter()
                            .rev()
                            .filter(|job| job["status"] == "completed")
                            .enumerate()
                            .filter_map(|(number, job)| {
                                job["asset_id"]
                                    .as_str()
                                    .map(|id| (id.to_owned(), number + 1))
                            })
                            .collect();
                        let clips: Vec<_> = jobs
                            .iter()
                            .filter_map(|job| job["asset_id"].as_str())
                            .filter_map(|id| assets.iter().find(|asset| asset.id == id))
                            .collect();
                        let columns =
                            ((ui.available_width() + 12.) / 180.).floor().clamp(1., 6.) as usize;
                        let width = ((ui.available_width() - 12. * (columns - 1) as f32)
                            / columns as f32)
                            .min(200.);
                        let ffmpeg = self
                            .store
                            .config()
                            .map(|config| config.ffmpeg)
                            .unwrap_or_default();
                        let mut clicked_clip = None;
                        eframe::egui::Grid::new(("take-gallery", &episode, &draft.id))
                            .num_columns(columns)
                            .spacing([12., 12.])
                            .show(ui, |ui| {
                                for (position, asset) in clips.iter().enumerate() {
                                    let title = format!(
                                        "{episode_label} · Segment {} · Take {}",
                                        *index + 1,
                                        take_numbers[&asset.id]
                                    );
                                    let thumbnail = state.thumbnails.get(
                                        ui,
                                        asset,
                                        &self.store.root(),
                                        &ffmpeg,
                                    );
                                    let (play, delete) = crate::take_gallery::card(
                                        ui,
                                        asset,
                                        &title,
                                        thumbnail,
                                        width,
                                        selected_clip.as_deref() == Some(&asset.id),
                                        state.open_take.as_deref() == Some(&asset.id),
                                    );
                                    if delete {
                                        state.open_take = Some(asset.id.clone());
                                        state.delete_take = Some(asset.id.clone());
                                    }
                                    if play {
                                        state.open_take = Some(asset.id.clone());
                                        clicked_clip = Some(asset.id.clone());
                                        state.delete_take = None;
                                    }
                                    if (position + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        for (number, job) in jobs.into_iter().enumerate() {
                            if number > 0
                                && !job["asset_id"].is_string()
                                && !state.show_old_attempts
                                && !matches!(job["status"].as_str(), Some("queued" | "running"))
                            {
                                continue;
                            }
                            if let Some(id) = job["asset_id"].as_str()
                                && state.open_take.as_deref() != Some(id)
                            {
                                continue;
                            }
                            if let Some(id) = job["asset_id"].as_str() {
                                let take_number = take_numbers[id];
                                ui.separator();
                                ui.strong(format!(
                                    "{episode_label} · Segment {} · Take {take_number}",
                                    *index + 1
                                ));
                            }
                            if !job["asset_id"].is_string() {
                                ui.small(job["stage"].as_str().unwrap_or(""));
                            }
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
                                        "{} × {} · {:.1}s{}",
                                        asset.width,
                                        asset.height,
                                        asset.duration,
                                        if selected.as_deref() == Some(asset_id) {
                                            " · SELECTED"
                                        } else {
                                            ""
                                        }
                                    ));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if (state.player.is_none()
                                        && ui.button("Play in app").clicked())
                                        || clicked_clip.as_deref() == Some(asset_id)
                                    {
                                        state.player = None;
                                        state.player_asset = Some(asset_id.to_owned());
                                        state.player_error = None;
                                        let result =
                                            (|| -> anyhow::Result<crate::media::Preview> {
                                                let asset = self
                                                    .store
                                                    .assets()?
                                                    .into_iter()
                                                    .find(|asset| asset.id == asset_id)
                                                    .ok_or_else(|| {
                                                        anyhow::anyhow!(
                                                            "Take is no longer available"
                                                        )
                                                    })?;
                                                crate::media::Preview::open(
                                                    self.store.config()?.ffmpeg,
                                                    asset.path,
                                                    asset.width as f32 / asset.height.max(1) as f32,
                                                    true,
                                                )
                                            })();
                                        match result {
                                            Ok(player) => state.player = Some(player),
                                            Err(error) => {
                                                state.player_error = Some(error.to_string())
                                            }
                                        }
                                    }
                                    if ui.button("Delete clip…").clicked() {
                                        state.delete_take = Some(asset_id.to_owned());
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
                                if state.delete_take.as_deref() == Some(asset_id) {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Permanently delete this clip?");
                                        if ui.button("Cancel").clicked() {
                                            state.delete_take = None;
                                        }
                                        if ui.button("Delete permanently").clicked() {
                                            state.player = None;
                                            match self
                                                .store
                                                .delete_segment_clip(&episode, &draft.id, asset_id)
                                            {
                                                Ok(()) => {
                                                    state.open_take = None;
                                                    state.delete_take = None;
                                                    self.message = "Clip deleted".into();
                                                }
                                                Err(error) => self.message = error.to_string(),
                                            }
                                        }
                                    });
                                }
                                if state.player_asset.as_deref() == Some(asset_id) {
                                    if let Some(error) = &state.player_error {
                                        ui.colored_label(crate::GOLD, error);
                                    }
                                    if let Some(player) = &mut state.player {
                                        state.player_visible = true;
                                        ui.add_space(8.);
                                        player.ui(ui);
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        ui.label(error.to_string());
                    }
                }
            }
        });
        if generate_segment {
            let result = state
                .catalog
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Still checking model capabilities"))
                .and_then(|catalog| self.store.save_and_enqueue_segment(plan, *index, catalog));
            match result {
                Ok((saved, job)) => {
                    *plan = saved;
                    self.message = if job.status == "queued" {
                        match crate::worker::spawn_detached(&self.store.path) {
                            Ok(()) => "Options saved; video queued".into(),
                            Err(error) => {
                                format!("Video queued but worker could not start: {error}")
                            }
                        }
                    } else {
                        format!(
                            "This exact request already exists ({}). Showing its result; no duplicate submitted.",
                            job.status
                        )
                    };
                    state.section = 1;
                }
                Err(error) => {
                    state.generation_error = Some(error.to_string());
                    self.message = error.to_string();
                }
            }
        }
    }
}
