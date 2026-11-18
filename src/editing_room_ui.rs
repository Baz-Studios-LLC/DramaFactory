use crate::{
    Studio,
    editing_room::{Clip, Cut},
};
use eframe::egui;
fn snap_music_position(value: f64, targets: &[f64], min: f64, max: f64, threshold: f64) -> f64 {
    let value = value.clamp(min, max);
    targets
        .iter()
        .copied()
        .filter(|target| *target >= min && *target <= max && (*target - value).abs() <= threshold)
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
        .unwrap_or(value)
}

enum TimelineEdit {
    Picture(Cut),
    Music {
        before: Vec<crate::sound::AudioCue>,
        after: Vec<crate::sound::AudioCue>,
    },
}
#[derive(Default)]
pub struct Editor {
    episode: Option<String>,
    music: crate::episode_music::Panel,
    cliffhanger: crate::cliffhanger::Panel,
    cut: Cut,
    saved: Cut,
    undo: Vec<TimelineEdit>,
    redo: Vec<TimelineEdit>,
    selected: Option<usize>,
    dragged: Option<usize>,
    drag_offset: f64,
    library_drag: Option<Clip>,
    trim_drag: Option<(usize, bool, Clip, f32)>,
    gesture_before: Option<Cut>,
    thumbnails: crate::take_gallery::Thumbnails,
    player: Option<crate::timeline_preview::Player>,
    player_title: String,
    timeline_player: Option<Cut>,
    music_mix_changed: bool,
    selected_music: Option<String>,
    music_drag_origin: f32,
    music_drag: Option<(crate::sound::AudioCue, crate::sound::AudioCue, i8)>,
    timeline_origin: f64,
    export_job: Option<String>,
    error: String,
    split_at: f64,
    zoom: f32,
    timeline_lead: f32,
    playhead: f64,
    scrubber: crate::scrub_preview::Scrubber,
    scrubbing: bool,
}
impl Editor {
    pub(crate) fn music_cut_duration(
        &self,
        store: &crate::store::Store,
        episode: Option<&str>,
    ) -> Option<f64> {
        let episode = episode?;
        if self.episode.as_deref() == Some(episode) {
            Some(self.cut.duration())
        } else {
            store.episode_cut(episode).ok().map(|cut| cut.duration())
        }
    }
    pub(crate) fn music_context(&self) -> serde_json::Value {
        serde_json::json!({"episode_id":self.episode,"draft":self.music.snapshot(),"cut_duration":self.cut.duration()})
    }
    pub(crate) fn prepare_music(
        &mut self,
        store: &crate::store::Store,
        draft: &crate::episode_music::MusicDraft,
        expected: &serde_json::Value,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            &self.music_context() == expected,
            "Music settings changed while Sidekick replied. Your current draft is preserved."
        );
        draft.validate()?;
        anyhow::ensure!(
            !self.music.busy(),
            "Wait for the current music task to finish"
        );
        if self.episode.as_deref() != Some(&draft.episode_id) {
            let cut = store.episode_cut(&draft.episode_id)?;
            let series = store.get_episode(&draft.episode_id)?.series_id;
            self.save_for_series(store, &series)?;
            *self = Self {
                episode: Some(draft.episode_id.clone()),
                cut: cut.clone(),
                saved: cut,
                ..Default::default()
            };
        }
        self.music.prepare(draft)
    }
    pub(crate) fn save_for_series(
        &mut self,
        store: &crate::store::Store,
        series: &str,
    ) -> anyhow::Result<()> {
        if let Some(episode) = &self.episode
            && store.get_episode(episode)?.series_id == series
            && self.cut != self.saved
        {
            store.save_episode_cut(episode, &self.saved, &self.cut)?;
            self.saved = self.cut.clone();
            self.error.clear();
        }
        Ok(())
    }

    fn music_edit(
        &mut self,
        store: &crate::store::Store,
        before: Vec<crate::sound::AudioCue>,
        after: Vec<crate::sound::AudioCue>,
    ) {
        if before == after {
            return;
        }
        match store.edit_audio_cues(&before, &after) {
            Ok(()) => {
                self.undo.push(TimelineEdit::Music { before, after });
                self.redo.clear();
                self.music_mix_changed = true;
                self.error.clear();
            }
            Err(error) => self.error = error.to_string(),
        }
    }
    fn apply_history(&mut self, store: &crate::store::Store, redo: bool) {
        let stack = if redo { &mut self.redo } else { &mut self.undo };
        let Some(edit) = stack.pop() else {
            return;
        };
        let inverse = match edit {
            TimelineEdit::Picture(cut) => {
                TimelineEdit::Picture(std::mem::replace(&mut self.cut, cut))
            }
            TimelineEdit::Music { before, after } => {
                let result = if redo {
                    store.edit_audio_cues(&before, &after)
                } else {
                    store.edit_audio_cues(&after, &before)
                };
                if let Err(error) = result {
                    self.error = error.to_string();
                    let stack = if redo { &mut self.redo } else { &mut self.undo };
                    stack.push(TimelineEdit::Music { before, after });
                    return;
                }
                self.music_mix_changed = true;
                TimelineEdit::Music { before, after }
            }
        };
        let stack = if redo { &mut self.undo } else { &mut self.redo };
        stack.push(inverse);
        self.error.clear();
    }
    pub fn stop(&mut self) {
        self.player = None;
        self.timeline_player = None;
        self.scrubber = Default::default();
        self.scrubbing = false;
    }
}
impl Studio {
    pub(crate) fn editing_room(&mut self, ui: &mut egui::Ui) {
        let editing_text = ui
            .ctx()
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::TextEdit::load_state(ui.ctx(), id).is_some());
        let space = !editing_text && ui.input(|input| input.events.iter().any(|event| matches!(event, egui::Event::Key { key: egui::Key::Space, pressed: true, repeat: false, modifiers, .. } if modifiers.is_none())));
        if space {
            ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            });
        }
        ui.spacing_mut().item_spacing.y = 4.;
        ui.spacing_mut().button_padding = egui::vec2(8., 5.);
        let workspace = ui.available_rect_before_wrap();
        let track_height = 166.;
        let toolbar_height = ui.spacing().interact_size.y.max(
            ui.text_style_height(&egui::TextStyle::Button) + 2. * ui.spacing().button_padding.y,
        );
        let upper_height = (workspace.height()
            - track_height
            - toolbar_height
            - ui.spacing().scroll.allocated_width()
            - 3. * ui.spacing().item_spacing.y)
            .max(100.);
        let viewer_height = upper_height - 48.;
        let aspect = if self.store.video_aspect_ratio() == "16:9" {
            16. / 9.
        } else {
            9. / 16.
        };
        let viewer_width = (viewer_height * aspect)
            .max(156.)
            .min((workspace.width() * 0.36).max(156.));
        let left_width = (workspace.width() - viewer_width - 20.).max(220.);
        let dock_width = (left_width * 0.48).clamp(180., 380.);
        let pool_width = (left_width - dock_width - 12.).max(120.);
        ui.set_max_width(pool_width);
        ui.heading("Editing room");
        self.episode_picker(ui);
        let Some(episode) = self.episode_id.clone() else {
            ui.label("Choose an episode to edit.");
            return;
        };
        if self
            .store
            .shots(&episode)
            .is_ok_and(|shots| !shots.is_empty())
        {
            ui.collapsing("Earlier shot-based projects", |ui| {
                if ui.button("Open shot export").clicked() {
                    self.page = crate::Page::Timeline;
                }
            });
        }
        let state = &mut self.editor;
        if state.episode.as_deref() != Some(&episode) {
            match self.store.episode_cut(&episode) {
                Ok(cut) => {
                    *state = Editor {
                        episode: Some(episode.clone()),
                        export_job: self.store.latest_cut_export(&episode).ok().flatten(),
                        cut: cut.clone(),
                        saved: cut,
                        ..Default::default()
                    };
                }
                Err(error) => {
                    ui.label(error.to_string());
                    return;
                }
            }
        }
        if let Some(result) = state.cliffhanger.poll() {
            match result {
                Ok((expected, next)) if state.cut == expected => {
                    match self.store.save_episode_cut(&episode, &expected, &next) {
                        Ok(()) => {
                            state.undo.push(TimelineEdit::Picture(expected));
                            state.redo.clear();
                            state.cut = next.clone();
                            state.saved = next;
                            state.stop();
                            state.cliffhanger.status="Ending added. Play the timeline to review; Undo restores your previous cut.".into();
                            state.cliffhanger.open = false;
                        }
                        Err(e) => state.cliffhanger.status = e.to_string(),
                    }
                }
                Ok(_) => {
                    state.cliffhanger.status =
                        "Timeline changed while rendering. Your current edit was preserved.".into()
                }
                Err(e) => state.cliffhanger.status = e.to_string(),
            }
        }
        let before = state.cut.clone();
        let mut history_action = false;
        let assets = match self.store.assets() {
            Ok(assets) => assets,
            Err(error) => {
                ui.label(error.to_string());
                return;
            }
        };
        let ffmpeg = self.store.config().map(|c| c.ffmpeg).unwrap_or_default();
        let mut play = None;
        let mut play_timeline = space;
        let total = state.cut.duration();
        let undo_shortcut = !editing_text
            && !state.undo.is_empty()
            && state.dragged.is_none()
            && state.trim_drag.is_none()
            && state.music_drag.is_none()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{} clips · {:.1}s", state.cut.clips.len(), total));
            if ui
                .add_enabled(!state.undo.is_empty(), egui::Button::new("Undo"))
                .on_hover_text("Undo timeline edit · ⌘Z")
                .clicked()
                || undo_shortcut
            {
                state.apply_history(&self.store, false);
                state.selected = None;
                history_action = true;
            }
            if ui
                .add_enabled(!state.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                state.apply_history(&self.store, true);
                state.selected = None;
                history_action = true;
            }
            if ui
                .add_enabled(
                    !state.cut.clips.is_empty(),
                    crate::theme::primary("Export episode"),
                )
                .clicked()
            {
                let result = self
                    .store
                    .save_episode_cut(&episode, &state.saved, &state.cut)
                    .and_then(|()| {
                        // Saving succeeded even if exporting or launching the worker fails.
                        state.saved = state.cut.clone();
                        self.store.export_episode_cut(&episode, &state.cut)
                    })
                    .and_then(|job| {
                        crate::worker::spawn_detached(&self.store.path)?;
                        Ok(job)
                    });
                match result {
                    Ok(job) => {
                        state.saved = state.cut.clone();
                        state.export_job = Some(job.id);
                        state.error.clear();
                    }
                    Err(error) => state.error = error.to_string(),
                }
            }
            ui.weak(if state.cut == state.saved {
                "Saved"
            } else {
                "Unsaved changes"
            });
        });
        let music_before = self.store.audio_cues(&episode).ok();
        let music_height = music_before
            .as_ref()
            .map_or(0., |cues| cues.len() as f32 * 24.);
        let upper_height = (upper_height - music_height).max(80.);
        let music_rect = egui::Rect::from_min_size(
            egui::pos2(workspace.left() + pool_width + 12., workspace.top()),
            egui::vec2(dock_width, upper_height),
        );
        let mut music_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("music-inspector")
                .max_rect(music_rect),
        );
        music_ui.set_clip_rect(music_rect.intersect(ui.clip_rect()));
        let music_changed = {
            egui::Frame::new()
                .fill(crate::theme::SURFACE)
                .corner_radius(6)
                .inner_margin(10.)
                .show(&mut music_ui, |ui| {
                    ui.set_min_height(ui.available_height());
                    state.music.show(
                        ui,
                        &self.store,
                        &episode,
                        &state.cut,
                        state.selected,
                        &assets,
                    )
                })
                .inner
        };
        if state.cliffhanger.open {
            egui::Window::new("Cliffhanger setup")
                .vscroll(true)
                .show(ui.ctx(), |ui| {
                    state.cliffhanger.show(ui, &self.store, &episode)
                });
        }
        if music_changed {
            // Keep the rendered preview playing; rebuild its mix on the next start.
            state.music_mix_changed = true;
            if let Some(before) = music_before
                && let Ok(after) = self.store.audio_cues(&episode)
                && before != after
            {
                state.undo.push(TimelineEdit::Music { before, after });
                state.redo.clear();
            }
        }
        if state.music_mix_changed {
            ui.weak("Music changes saved.");
        }
        let music = self.store.audio_cues(&episode).unwrap_or_default();
        if state.music_mix_changed
            && state.timeline_player.is_some()
            && let Some(player) = &mut state.player
        {
            match crate::timeline_preview::manifest(
                &state.cut,
                &music,
                &assets,
                self.store.video_aspect_ratio(),
                state.playhead,
            )
            .and_then(|m| player.update_mix(m))
            {
                Ok(()) => state.music_mix_changed = false,
                Err(error) => state.error = error.to_string(),
            }
        }
        if !state.error.is_empty() {
            ui.colored_label(crate::GOLD, &state.error);
        }
        if let Some(job_id) = &state.export_job
            && let Ok(job) = self.store.job(job_id)
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Export · {}", job.status));
                if let Some(id) = &job.output_id
                    && let Some(asset) = assets.iter().find(|a| &a.id == id)
                {
                    if ui.button("Play episode").clicked() {
                        play = Some((asset.id.clone(), "Exported episode".into()));
                    }
                    if ui.button("Open MP4").clicked() {
                        let _ = std::process::Command::new("/usr/bin/open")
                            .arg(&asset.path)
                            .spawn();
                    }
                }
                if !job.error.is_empty() {
                    ui.label(&job.error);
                }
            });
        }
        ui.add_space(4.);
        if state.timeline_player.is_some()
            && let Some(player) = &state.player
        {
            state.playhead = (state.timeline_origin + player.playback_position()).min(total);
        }
        let pool_top = ui.cursor().top().min(workspace.top() + upper_height);
        let pool_rect = egui::Rect::from_min_max(
            egui::pos2(workspace.left(), pool_top),
            egui::pos2(
                workspace.left() + pool_width,
                workspace.top() + upper_height,
            ),
        );
        let mut pool_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("editor-pool-panel")
                .max_rect(pool_rect),
        );
        pool_ui.set_clip_rect(ui.clip_rect().intersect(pool_rect));
        egui::Frame::new().fill(crate::theme::SURFACE).corner_radius(6).inner_margin(8.).show(&mut pool_ui, |ui| {
        ui.set_min_height(ui.available_height());
        egui::CollapsingHeader::new("Media pool")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("editor-media-pool")
                        .auto_shrink([false, false])
                        .max_height(ui.available_height())
                        .show(ui, |ui| {
                            let plan = self.store.generation_plan(&episode).ok();
                            let columns = ((ui.available_width() + 8.) / 78.)
                                .floor()
                                .clamp(1., 24.) as usize;
                            let width = 70_f32.min(ui.available_width());
                            egui::Grid::new(("edit-library", &episode))
                                .num_columns(columns)
                                .spacing([8., 6.])
                                .show(ui, |ui| {
                                    let mut position = 0;
                                    if let Some(plan) = plan {
                                        for (segment_number, segment) in
                                            plan.segments.iter().enumerate()
                                        {
                                            if let Ok(jobs) = self
                                                .store
                                                .segment_generation_jobs(&episode, &segment.id)
                                            {
                                                for (take_number, job) in jobs
                                                    .iter()
                                                    .rev()
                                                    .filter(|job| job["status"] == "completed")
                                                    .enumerate()
                                                {
                                                    let Some(asset) =
                                                        job["asset_id"].as_str().and_then(|id| {
                                                            assets.iter().find(|a| a.id == id)
                                                        })
                                                    else {
                                                        continue;
                                                    };
                                                    let title = format!(
                                                        "Segment {} · Take {}",
                                                        segment_number + 1,
                                                        take_number + 1
                                                    );
                                                    let thumb = state.thumbnails.get(
                                                        ui,
                                                        asset,
                                                        &self.store.root(),
                                                        &ffmpeg,
                                                    );
                                                    let (rect, drag) = ui.allocate_exact_size(egui::vec2(width, width/aspect), egui::Sense::click_and_drag());
                                                    ui.painter().rect_filled(rect, 5, crate::theme::SURFACE);
                                                    if let Some(path) = thumb {
                                                        egui::Image::from_uri(format!("file://{}", path.display())).corner_radius(5).paint_at(ui, rect);
                                                    }
                                                    let label = format!("S{}T{}", segment_number + 1, take_number + 1);
                                                    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
                                                    let text = painter.layout_no_wrap(label, egui::FontId::proportional(11.), egui::Color32::WHITE);
                                                    let origin = rect.left_top() + egui::vec2(6., 5.);
                                                    painter.rect_filled(egui::Rect::from_min_size(origin, text.size()).expand(3.), 3, egui::Color32::from_black_alpha(200));
                                                    painter.galley(origin, text, egui::Color32::WHITE);
                                                    let drag = drag.on_hover_text(format!("{title} · {:.1}s\nClick to preview · drag to timeline", asset.duration));
                                                    let add_rect = egui::Rect::from_min_size(rect.right_top()+egui::vec2(-25., 3.), egui::vec2(22.,22.));
                                                    let add = ui.interact(add_rect, ui.id().with(("add-take", &asset.id)), egui::Sense::click()).on_hover_text(format!("Add {title} to timeline"));
                                                    if ui.rect_contains_pointer(rect) || add.has_focus() {
                                                        ui.painter().circle_filled(add_rect.center(), 11., egui::Color32::from_black_alpha(210));
                                                        ui.painter().text(add_rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(19.), egui::Color32::WHITE);
                                                    }
                                                    let badge = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom()-19.), rect.max);
                                                    ui.painter().rect_filled(badge, 0, egui::Color32::from_black_alpha(180));
                                                    ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, format!("{:.1}s", asset.duration), egui::FontId::proportional(11.), egui::Color32::WHITE);
                                                    if add.clicked() {
                                                        state.cut.clips.push(Clip { id: uuid::Uuid::new_v4().to_string(), asset_id: asset.id.clone(), title: title.clone(), gap_before: 0., audio_fade_out: 0., audio_fade_in: 0., audio_gain_db: 0., audio_muted: false, start: 0., duration: asset.duration });
                                                        state.selected = Some(state.cut.clips.len()-1);
                                                        state.split_at = asset.duration/2.;
                                                    } else if drag.clicked() && !ui.rect_contains_pointer(add_rect) {
                                                        play = Some((asset.id.clone(), title.clone()));
                                                    }
                                                    if drag.drag_started() {
                                                        state.library_drag = Some(Clip {
                                                            id: uuid::Uuid::new_v4().to_string(),
                                                            asset_id: asset.id.clone(),
                                                            title: title.clone(),
                                                            gap_before: 0., audio_fade_out: 0., audio_fade_in: 0., audio_gain_db: 0., audio_muted: false,
                                                            start: 0.,
                                                            duration: asset.duration,
                                                        });
                                                    }
                                                    if drag.dragged() {
                                                        ui.ctx().set_cursor_icon(
                                                            egui::CursorIcon::Grabbing,
                                                        );
                                                    }
                                                    position += 1;
                                                    if position % columns == 0 {
                                                        ui.end_row();
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if position == 0 {
                                        ui.label("Generate clips in Episode generation first.");
                                    }
                                });
                        });
                });
        });
        ui.set_max_width(workspace.width());
        let viewer_rect = egui::Rect::from_min_size(
            egui::pos2(workspace.right() - viewer_width, workspace.top()),
            egui::vec2(viewer_width, upper_height),
        );
        {
            ui.painter()
                .rect_filled(viewer_rect, 6, crate::theme::SURFACE);
            let viewer_rect = viewer_rect.shrink(8.);
            let mut viewer_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(viewer_rect)
                    .id_salt("fixed-editor-viewer"),
            );
            viewer_ui.set_clip_rect(viewer_rect.intersect(ui.clip_rect()));
            let ui = &mut viewer_ui;
            ui.horizontal(|ui| {
                ui.strong("Viewer");

                if state.player.is_some() && ui.small_button("Close").clicked() {
                    state.player = None;
                }
            });
            let caption_height = ui.text_style_height(&egui::TextStyle::Small) + 8.;
            let caption_rect = egui::Rect::from_min_max(
                egui::pos2(viewer_rect.left(), viewer_rect.bottom() - caption_height),
                viewer_rect.right_bottom(),
            );
            let available_video_rect = egui::Rect::from_min_max(
                egui::pos2(viewer_rect.left(), ui.cursor().top()),
                egui::pos2(viewer_rect.right(), caption_rect.top() - 4.),
            );
            let canvas_height = available_video_rect
                .height()
                .min(available_video_rect.width() / aspect)
                .max(1.);
            let video_rect = egui::Rect::from_center_size(
                available_video_rect.center(),
                egui::vec2(canvas_height * aspect, canvas_height),
            );
            let viewer_height = video_rect.height().max(1.);
            if let Some(player) = &mut state.player {
                let mut video_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("editor-video-body")
                        .max_rect(video_rect),
                );
                video_ui.set_clip_rect(video_rect.intersect(ui.clip_rect()));
                let (toggle, restart) = player.timeline_viewer(&mut video_ui, viewer_height);
                play_timeline |= toggle || restart;
                if restart {
                    state.playhead = 0.;
                    state.timeline_player = None;
                }
                ui.put(
                    caption_rect,
                    egui::Label::new(egui::RichText::new(&state.player_title).small()).truncate(),
                )
                .on_hover_text(&state.player_title);
            } else {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), viewer_height),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    play_timeline = true;
                }
                ui.painter().rect_filled(rect, 4, egui::Color32::BLACK);
                if state.scrubbing
                    && let Some(texture) = &state.scrubber.texture
                {
                    let aspect = texture.size()[0] as f32 / texture.size()[1] as f32;
                    let height = rect.height().min(rect.width() / aspect);
                    let image_rect = egui::Rect::from_center_size(
                        rect.center(),
                        egui::vec2(height * aspect, height),
                    );
                    ui.painter().image(
                        texture.id(),
                        image_rect,
                        egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Play timeline\nSpace",
                        egui::FontId::proportional(13.),
                        crate::MUTED,
                    );
                }
                let caption = if state.scrubbing {
                    format!("Timeline · {:.2}s", state.playhead)
                } else {
                    String::new()
                };
                ui.put(
                    caption_rect,
                    egui::Label::new(egui::RichText::new(caption).small()).truncate(),
                );
            }
            let interaction = ui.interact(
                video_rect,
                ui.id().with("cliffhanger-viewer"),
                egui::Sense::click(),
            );
            interaction.context_menu(|ui| {
                if ui
                    .add_enabled(
                        state.cut.source_at(state.playhead).is_some()
                            && (state.timeline_player.is_some() || state.scrubbing)
                            && !state.cliffhanger.busy(),
                        egui::Button::new("Create cliffhanger ending"),
                    )
                    .clicked()
                {
                    if let Some(player) = &mut state.player
                        && !player.is_paused()
                        && !player.finished()
                    {
                        let _ = player.toggle_playback();
                    }
                    state.cliffhanger.begin(&state.cut, state.playhead);
                    ui.close();
                }
            });
            if state.cliffhanger.open {
                if interaction.clicked()
                    && !state.cliffhanger.busy()
                    && let Some(pos) = interaction.interact_pointer_pos()
                {
                    state.cliffhanger.recipe.x =
                        ((pos.x - video_rect.left()) / video_rect.width()).clamp(0., 1.) as f64;
                    state.cliffhanger.recipe.y =
                        ((pos.y - video_rect.top()) / video_rect.height()).clamp(0., 1.) as f64;
                    play_timeline = false;
                }
                let r = &state.cliffhanger.recipe;
                let size = video_rect.size() / r.zoom as f32;
                let left = (video_rect.left() + r.x as f32 * video_rect.width() - size.x / 2.)
                    .clamp(video_rect.left(), video_rect.right() - size.x);
                let top = (video_rect.top() + r.y as f32 * video_rect.height() - size.y / 2.)
                    .clamp(video_rect.top(), video_rect.bottom() - size.y);
                ui.painter().rect_stroke(
                    egui::Rect::from_min_size(egui::pos2(left, top), size),
                    3,
                    egui::Stroke::new(2., crate::GOLD),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let remaining = workspace.top() + upper_height - ui.cursor().top();
        if remaining > 0. {
            ui.add_space(remaining);
        }
        ui.add_space(4.);
        let timeline_width = ui.available_width();
        let mut scroll_to = None;
        ui.horizontal(|ui| {
            ui.strong("Timeline");
            if ui
                .add_enabled(!state.cut.clips.is_empty(), egui::Button::new("▶ / ⏸"))
                .on_hover_text("Play / pause timeline · Space")
                .clicked()
            {
                play_timeline = true;
            }
            ui.weak("Drag to move · drag edges to trim");
            if state.zoom == 0. {
                state.zoom = 12.;
            }
            if ui
                .add(
                    egui::Slider::new(&mut state.zoom, 4. ..=40.)
                        .text("Zoom")
                        .show_value(false),
                )
                .changed()
            {
                let time_x = state.playhead as f32 * state.zoom;
                state.timeline_lead = (timeline_width * 0.5 - time_x - 58.).max(0.);
                scroll_to =
                    Some((58. + state.timeline_lead + time_x - timeline_width * 0.5).max(0.));
            }
            if ui.small_button("Fit").clicked() {
                state.timeline_lead = 0.;
                scroll_to = Some(0.);
                state.zoom = ((timeline_width - 70.) / total.max(1.) as f32).clamp(4., 40.);
            }
        });
        let mut slide = None;
        let mut place_at = None;
        let mut trim_update = None;
        let mut sound_update = None;
        let mut move_to = None;
        let mut menu_action = None;
        let mut timeline_scroll = egui::ScrollArea::horizontal().id_salt("edit-timeline");
        if let Some(offset) = scroll_to {
            timeline_scroll = timeline_scroll.horizontal_scroll_offset(offset);
        }
        timeline_scroll.show(ui, |ui| {
            let scale = state.zoom;
            let lead = 58. + state.timeline_lead;
            let width = (lead + total as f32 * scale + (timeline_width * 0.5).max(182.))
                .max(ui.available_width());
            let (canvas, _) = ui.allocate_exact_size(
                egui::vec2(width, track_height + music_height),
                egui::Sense::hover(),
            );
            let painter = ui
                .painter()
                .with_clip_rect(canvas.intersect(ui.clip_rect()));
            painter.rect_filled(canvas, 4, egui::Color32::from_rgb(16, 19, 23));
            let origin = canvas.left() + lead;
            let video_y = canvas.top() + 38.;
            let audio_y = video_y + 76.;
            for (y, height, label) in [(video_y, 70., "V1"), (audio_y, 32., "A1")] {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(canvas.left(), y),
                        egui::vec2(width, height),
                    ),
                    0,
                    egui::Color32::from_rgb(27, 31, 37),
                );
                painter.text(
                    egui::pos2(canvas.left() + 12., y + height / 2.),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(12.),
                    crate::theme::TEXT,
                );
            }
            if !music.is_empty() {
                let y = audio_y + 37.;
                painter.text(
                    egui::pos2(canvas.left() + 8., y + 10.),
                    egui::Align2::LEFT_CENTER,
                    "Music",
                    egui::FontId::proportional(12.),
                    crate::theme::TEXT,
                );
                for (lane, saved_cue) in music.iter().enumerate() {
                    let y = y + lane as f32 * 24.;
                    let cue = state
                        .music_drag
                        .as_ref()
                        .filter(|(old, _, _)| old.id == saved_cue.id)
                        .map(|(_, draft, _)| draft)
                        .unwrap_or(saved_cue)
                        .clone();
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(origin + cue.start as f32 * scale, y),
                        egui::vec2(cue.duration as f32 * scale, 20.),
                    );
                    painter.rect_filled(rect, 3, egui::Color32::from_rgb(97, 64, 121));
                    painter.with_clip_rect(rect.intersect(ui.clip_rect())).text(
                        rect.left_center() + egui::vec2(10., 0.),
                        egui::Align2::LEFT_CENTER,
                        if cue.muted { format!("{} · Muted", cue.name) } else { cue.name.clone() },
                        egui::FontId::proportional(11.),
                        crate::theme::TEXT,
                    );
                    let handle = 6_f32.min(rect.width() / 3.);
                    for x in [rect.left() + 2., rect.right() - 2.] {
                        painter.line_segment(
                            [egui::pos2(x, y + 4.), egui::pos2(x, y + 16.)],
                            egui::Stroke::new(2., crate::theme::TEXT),
                        );
                    }
                    let response = ui.interact(
                        rect,
                        ui.id().with(("music-cue", &cue.id)),
                        egui::Sense::click_and_drag(),
                    );
                    let edge = response
                        .hover_pos()
                        .map(|pos| {
                            if pos.x < rect.left() + handle {
                                -1
                            } else if pos.x > rect.right() - handle {
                                1
                            } else {
                                0
                            }
                        })
                        .unwrap_or(0);
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(if edge == 0 {
                            egui::CursorIcon::Grab
                        } else {
                            egui::CursorIcon::ResizeHorizontal
                        });
                    }
                    if response.drag_started() {
                        let press = ui.input(|i| i.pointer.press_origin());
                        state.music_drag_origin = press.map_or(rect.left(), |pos| pos.x);
                        let mode = press
                            .map(|pos| {
                                if pos.x < rect.left() + handle {
                                    -1
                                } else if pos.x > rect.right() - handle {
                                    1
                                } else {
                                    0
                                }
                            })
                            .unwrap_or(0);
                        state.music_drag = Some((saved_cue.clone(), saved_cue.clone(), mode));
                    }
                    if response.dragged()
                        && let Some((old, draft, mode)) = &mut state.music_drag
                        && old.id == cue.id
                    {
                        let delta = response.interact_pointer_pos().map_or(0., |pos| (pos.x - state.music_drag_origin) as f64 / scale as f64);
                        *draft = old.clone();
                        let threshold = 8. / f64::from(scale);
                        let playhead = state.playhead;
                        let mut trim_targets = vec![playhead - old.start];
                        for (index, clip) in state.cut.clips.iter().enumerate() {
                            let start = state.cut.clip_time(index);
                            trim_targets.extend([start - old.start, start + clip.duration - old.start]);
                        }
                        match *mode {
                            -1 => {
                                let delta = snap_music_position(delta, &trim_targets,
                                    -old.start.min(old.source_start),
                                    (old.duration - 0.01).max(0.), threshold);

                                draft.start += delta;
                                draft.source_start += delta;
                                draft.duration -= delta;
                            }
                            1 => {
                                let available = assets
                                    .iter()
                                    .find(|a| a.id == old.asset_id)
                                    .map(|a| a.duration - old.source_start)
                                    .unwrap_or(old.duration);
                                draft.duration = snap_music_position(old.duration + delta,
                                    &trim_targets, 0.01_f64.min(available), available.min(1800.), threshold);
                            }
                            _ => draft.start = snap_music_position(old.start + delta,
                                &[playhead, playhead - old.duration], 0., 1800., threshold),
                        }
                        draft.fade_in = draft.fade_in.min(draft.duration);
                        draft.fade_out = draft.fade_out.min(draft.duration - draft.fade_in);
                        ui.ctx().request_repaint();
                    }
                    if response.drag_stopped()
                        && let Some((old, draft, _)) = state.music_drag.take()
                        && draft != old
                    {
                        state.music_edit(&self.store, vec![old], vec![draft]);
                    }
                    if response.clicked() || response.drag_started() || response.secondary_clicked() {
                        state.selected_music = Some(cue.id.clone());
                        state.selected = None;
                    }
                    if state.selected_music.as_deref() == Some(&cue.id) {
                        painter.rect_stroke(rect, 3, egui::Stroke::new(2., crate::GOLD), egui::StrokeKind::Inside);
                    }
                    if response.double_clicked() { state.music.focus_cue = Some(cue.id.clone()); }
                    response.context_menu(|ui| {
                        let offset = state.playhead - cue.start;
                        let mut edited = cue.clone();
                        let available = assets.iter().find(|a| a.id == cue.asset_id).map_or(cue.source_start + cue.duration, |a| a.duration);
                        ui.menu_button("Position & trim", |ui| {
                            for (label, value, max) in [
                                ("Position", &mut edited.start, 1800.),
                                ("Trim in", &mut edited.source_start, (available - 0.01).max(0.)),
                                ("Length", &mut edited.duration, available),
                            ] {
                                ui.horizontal(|ui| { ui.label(label); ui.add(egui::DragValue::new(value).speed(0.05).range(0. ..=max).suffix("s")); });
                            }
                            if ui.button("Reset trim").clicked() { edited.source_start = 0.; edited.duration = available.min(1800.); }
                            if ui.button("Move to playhead").clicked() { edited.start = state.playhead.min(1800.); }
                        });
                        ui.menu_button("Sound", |ui| {
                            ui.checkbox(&mut edited.muted, "Mute");
                            ui.horizontal(|ui| { ui.label("Volume"); ui.add(egui::DragValue::new(&mut edited.gain_db).speed(0.5).range(-60. ..=12.).suffix(" dB")); });
                            for (label, value) in [("Fade in", &mut edited.fade_in), ("Fade out", &mut edited.fade_out)] {
                                ui.horizontal(|ui| { ui.label(label); ui.add(egui::DragValue::new(value).speed(0.05).range(0. ..=cue.duration).suffix("s")); });
                            }
                        });
                        edited.duration = edited.duration.max(0.01).min(available - edited.source_start).min(1800.);
                        edited.fade_in = edited.fade_in.min(edited.duration);
                        edited.fade_out = edited.fade_out.min(edited.duration - edited.fade_in);
                        if edited != cue { state.music_edit(&self.store, vec![cue.clone()], vec![edited]); }

                        if ui.add_enabled(offset > 0.01 && offset < cue.duration - 0.01, egui::Button::new("Split at playhead")).clicked() {
                            let mut left = cue.clone();
                            let mut right = cue.clone();
                            left.duration = offset;
                            left.fade_in = left.fade_in.min(offset);
                            left.fade_out = 0.;
                            right.id = uuid::Uuid::new_v4().to_string();
                            right.start += offset;
                            right.source_start += offset;
                            right.duration -= offset;
                            right.fade_in = 0.;
                            right.fade_out = right.fade_out.min(right.duration);
                            state.music_edit(&self.store, vec![cue.clone()], vec![left, right]);
                            ui.close();
                        }
                        if ui.button("Duplicate").clicked() {
                            let mut copy = cue.clone();
                            copy.id = uuid::Uuid::new_v4().to_string();
                            copy.start = (copy.start + copy.duration).min(1800.);
                            state.music_edit(&self.store, vec![], vec![copy]);
                            ui.close();
                        }
                        if ui.button("Ripple delete music").on_hover_text("Remove this cue and close its gap in later music cues; video stays in place.").clicked() {
                            let mut before = vec![cue.clone()];
                            let mut after = Vec::new();
                            for later in music.iter().filter(|c| c.id != cue.id && c.start >= cue.start + cue.duration) {
                                before.push(later.clone());
                                let mut shifted = later.clone();
                                shifted.start -= cue.duration;
                                after.push(shifted);
                            }
                            state.music_edit(&self.store, before, after);
                            state.selected_music = None;
                            ui.close();
                        }
                        if ui.button("Remove").clicked() {
                            state.music_edit(&self.store, vec![cue.clone()], vec![]);
                            state.selected_music = None;
                            ui.close();
                        }
                        if ui.button("Position, trim, volume & fades…").clicked() {
                            state.music.focus_cue = Some(cue.id.clone());
                            ui.close();
                        }
                    });
                    response.on_hover_text(
                        "Drag to move · drag edges to trim · double-click for volume and fades · right-click for editing",
                    );
                }
            }
            let step = if scale < 8. {
                10
            } else if scale < 20. {
                5
            } else {
                2
            };
            for second in (0..=((width - lead) / scale) as usize).step_by(step) {
                let x = origin + second as f32 * scale;
                painter.line_segment(
                    [
                        egui::pos2(x, canvas.top() + 26.),
                        egui::pos2(x, canvas.bottom()),
                    ],
                    egui::Stroke::new(1., egui::Color32::from_rgb(44, 48, 55)),
                );
                painter.text(
                    egui::pos2(x + 4., canvas.top() + 10.),
                    egui::Align2::LEFT_CENTER,
                    format!("{:02}:{:02}", second / 60, second % 60),
                    egui::FontId::monospace(11.),
                    crate::MUTED,
                );
            }
            let ruler = egui::Rect::from_min_max(
                egui::pos2(origin, canvas.top()),
                egui::pos2(canvas.right(), video_y),
            );
            let ruler_response = ui.interact(
                ruler,
                ui.id().with("cut-ruler"),
                egui::Sense::click_and_drag(),
            );
            if (ruler_response.clicked() || ruler_response.dragged())
                && let Some(pos) = ruler_response.interact_pointer_pos()
            {
                state.timeline_player = None;

                state.player = None;
                state.scrubbing = true;
                state.playhead = (f64::from((pos.x - origin) / scale)).clamp(0., total);
                state.selected = None;
                for (index, clip) in state.cut.clips.iter().enumerate() {
                    let start = state.cut.clip_time(index);
                    if state.playhead >= start && state.playhead < start + clip.duration {
                        state.selected = Some(index);
                        state.selected_music = None;
                        state.split_at = state.playhead - start;
                        break;
                    }
                }
            }
            let mut time = 0.;
            for (index, clip) in state.cut.clips.iter().enumerate() {
                time += clip.gap_before;
                let width = clip.duration as f32 * scale;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(origin + time as f32 * scale, video_y),
                    egui::vec2(width, 70.),
                );
                let response = ui.interact(
                    rect,
                    ui.id().with(("cut-clip", &clip.id)),
                    egui::Sense::click_and_drag(),
                );
                response.context_menu(|ui| {
                    let offset = state.playhead - state.cut.clip_time(index);
                    if ui.button("Play source").clicked() {
                        play = Some((
                            clip.asset_id.clone(),
                            format!("{} · full source", clip.title),
                        ));
                        ui.close();
                    }
                    ui.menu_button("Move", |ui| {
                        for (label, target) in [
                            ("Earlier", index.checked_sub(1)),
                            (
                                "Later",
                                (index + 1 < state.cut.clips.len()).then_some(index + 1),
                            ),
                        ] {
                            if ui
                                .add_enabled(target.is_some(), egui::Button::new(label))
                                .clicked()
                                && let Some(to) = target
                            {
                                move_to = Some((index, to));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Trim", |ui| {
                        let mut edited = clip.clone();
                        let source_duration = assets
                            .iter()
                            .find(|a| a.id == clip.asset_id)
                            .map_or(clip.start + clip.duration, |asset| asset.duration);
                        ui.horizontal(|ui| {
                            ui.label("In");
                            let end = edited.start + edited.duration;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut edited.start)
                                        .speed(0.05)
                                        .max_decimals(2)
                                        .range(0. ..=(end - 0.04).max(0.))
                                        .suffix("s"),
                                )
                                .changed()
                            {
                                edited.duration = end - edited.start;
                            }
                            let mut end = edited.start + edited.duration;
                            ui.label("Out");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut end)
                                        .speed(0.05)
                                        .max_decimals(2)
                                        .range((edited.start + 0.04)..=source_duration)
                                        .suffix("s"),
                                )
                                .changed()
                            {
                                edited.duration = end - edited.start;
                            }
                        });
                        if clip.duration >= 0.08 {
                            ui.horizontal(|ui| {
                                ui.label("Split at");
                                ui.add(
                                    egui::DragValue::new(&mut state.split_at)
                                        .range(0.04..=clip.duration - 0.04)
                                        .speed(0.05)
                                        .suffix("s"),
                                );
                                if ui.button("Split").clicked() {
                                    menu_action = Some(("split", index, state.split_at));
                                    ui.close();
                                }
                            });
                        }
                        if edited != *clip {
                            trim_update = Some((index, edited));
                        }
                    });
                    if ui
                        .add_enabled(
                            offset >= 0.04 && clip.duration - offset >= 0.04,
                            egui::Button::new("Split at playhead"),
                        )
                        .clicked()
                    {
                        menu_action = Some(("split", index, offset));
                        ui.close();
                    }
                    if ui
                        .button("Duplicate")
                        .on_hover_text("Insert a copy after this clip; later clips move right")
                        .clicked()
                    {
                        menu_action = Some(("duplicate", index, 0.));
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Sound", |ui| {
                        let mut edited = clip.clone();
                        ui.checkbox(&mut edited.audio_muted, "Mute");
                        ui.horizontal(|ui| {
                            ui.label("Volume");
                            ui.add(
                                egui::DragValue::new(&mut edited.audio_gain_db)
                                    .speed(0.5)
                                    .range(-60. ..=12.)
                                    .max_decimals(1)
                                    .suffix(" dB"),
                            );
                            if ui.small_button("Reset").clicked() {
                                edited.audio_gain_db = 0.;
                            }
                        });
                        for (label, fade) in [
                            ("Fade in", &mut edited.audio_fade_in),
                            ("Fade out", &mut edited.audio_fade_out),
                        ] {
                            ui.menu_button(label, |ui| {
                                for (label, seconds) in [
                                    ("Off", 0_f64),
                                    ("0.25 seconds", 0.25),
                                    ("0.5 seconds", 0.5),
                                    ("1 second", 1.),
                                    ("2 seconds", 2.),
                                ] {
                                    let seconds = seconds.min(clip.duration);
                                    if ui
                                        .selectable_label((*fade - seconds).abs() < 0.001, label)
                                        .clicked()
                                    {
                                        *fade = seconds;
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.label("Custom");
                                    ui.add(
                                        egui::DragValue::new(fade)
                                            .speed(0.05)
                                            .range(0. ..=clip.duration)
                                            .max_decimals(2)
                                            .suffix("s"),
                                    );
                                });
                            });
                        }
                        if edited != *clip {
                            sound_update = Some((index, edited));
                        }
                    });
                    ui.separator();
                    if ui
                        .button("Remove")
                        .on_hover_text("Leave a gap; keep the original in the media pool")
                        .clicked()
                    {
                        menu_action = Some(("remove", index, 0.));
                        ui.close();
                    }
                    if ui
                        .button("Ripple delete")
                        .on_hover_text("Remove clip and close its space; keep the original")
                        .clicked()
                    {
                        menu_action = Some(("ripple", index, 0.));
                        ui.close();
                    }
                });
                let active = state.selected == Some(index);
                let color = if active {
                    egui::Color32::from_rgb(65, 101, 133)
                } else {
                    egui::Color32::from_rgb(43, 68, 90)
                };
                painter.rect_filled(rect.shrink(1.), 3, color);
                if let Some(asset) = assets.iter().find(|a| a.id == clip.asset_id)
                    && let Some(path) = state.thumbnails.get(ui, asset, &self.store.root(), &ffmpeg)
                {
                    let strip = egui::Rect::from_min_max(
                        rect.min + egui::vec2(2., 24.),
                        rect.max - egui::vec2(2., 3.),
                    );
                    let mut image_ui = ui.new_child(egui::UiBuilder::new().max_rect(strip));
                    image_ui.set_clip_rect(strip.intersect(ui.clip_rect()));
                    let image = egui::Image::from_uri(format!("file://{}", path.display()));
                    let tile_width = 48.;
                    let mut x = strip.left();
                    while x < strip.right() {
                        image.paint_at(
                            &image_ui,
                            egui::Rect::from_min_size(
                                egui::pos2(x, strip.top()),
                                egui::vec2(tile_width, strip.height()),
                            ),
                        );
                        x += tile_width;
                    }
                }
                let audio_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), audio_y),
                    egui::vec2(width, 32.),
                );
                painter.rect_filled(
                    audio_rect.shrink(1.),
                    3,
                    egui::Color32::from_rgb(34, 78, 66),
                );
                let clip_painter =
                    painter.with_clip_rect(rect.shrink(2.).intersect(ui.clip_rect()));
                clip_painter.text(
                    rect.min + egui::vec2(7., 12.),
                    egui::Align2::LEFT_CENTER,
                    &clip.title,
                    egui::FontId::proportional(11.),
                    egui::Color32::WHITE,
                );
                painter
                    .with_clip_rect(audio_rect.shrink(2.).intersect(ui.clip_rect()))
                    .text(
                        audio_rect.left_center() + egui::vec2(8., 0.),
                        egui::Align2::LEFT_CENTER,
                        if clip.audio_muted {
                            "Audio · muted"
                        } else if clip.audio_fade_in > 0. || clip.audio_fade_out > 0. {
                            "Audio · fades"
                        } else if clip.audio_gain_db != 0. {
                            "Audio · volume adjusted"
                        } else {
                            "Linked source audio"
                        },
                        egui::FontId::proportional(11.),
                        egui::Color32::from_rgb(172, 216, 196),
                    );
                for bounds in [rect, audio_rect] {
                    painter.rect_stroke(
                        bounds.shrink(1.),
                        3,
                        egui::Stroke::new(
                            if active { 2. } else { 1. },
                            if active {
                                crate::GOLD
                            } else {
                                egui::Color32::from_rgb(75, 91, 105)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                if response.double_clicked() {
                    state.player = None;
                    state.timeline_player = None;
                    play_timeline = true;
                }
                if response.clicked() {
                    state.timeline_player = None;

                    state.player = None;
                    state.scrubbing = true;
                    state.selected = Some(index);
                        state.selected_music = None;
                    state.split_at = response
                        .interact_pointer_pos()
                        .map_or(clip.duration / 2., |p| {
                            f64::from((p.x - rect.left()) / scale)
                        })
                        .clamp(0., clip.duration);
                    state.playhead = time + state.split_at;
                }
                for left in [true, false] {
                    let edge = if left {
                        egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.left() + 9., rect.bottom()),
                        )
                    } else {
                        egui::Rect::from_min_max(
                            egui::pos2(rect.right() - 9., rect.top()),
                            rect.max,
                        )
                    };
                    let handle = ui
                        .interact(
                            edge,
                            ui.id().with(("trim", &clip.id, left)),
                            egui::Sense::drag(),
                        )
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                    ui.painter().line_segment(
                        [
                            egui::pos2(edge.center().x, rect.top() + 28.),
                            egui::pos2(edge.center().x, rect.bottom() - 12.),
                        ],
                        egui::Stroke::new(2., crate::GOLD),
                    );
                    if handle.drag_started() {
                        state.selected = Some(index);
                        state.selected_music = None;
                        state.gesture_before = Some(state.cut.clone());
                        state.trim_drag = Some((
                            index,
                            left,
                            clip.clone(),
                            ui.input(|i| i.pointer.press_origin().map_or(edge.center().x, |p| p.x)),
                        ));
                        state.dragged = None;
                    }
                }
                if response.drag_started() && state.trim_drag.is_none() {
                    state.dragged = Some(index);
                    state.selected = Some(index);
                        state.selected_music = None;
                    state.drag_offset = ui
                        .input(|i| i.pointer.press_origin())
                        .map_or(0., |p| f64::from((p.x - rect.left()) / scale));
                }
                time += clip.duration;
            }
            let drop_rect = egui::Rect::from_min_max(
                egui::pos2(origin + time as f32 * scale, video_y),
                egui::pos2(canvas.right(), audio_y + 32.),
            );
            painter.text(
                drop_rect.center(),
                egui::Align2::CENTER_CENTER,
                "+ Drop a take",
                egui::FontId::proportional(12.),
                crate::MUTED,
            );
            let tracks = egui::Rect::from_min_max(
                egui::pos2(origin, video_y),
                egui::pos2(canvas.right(), audio_y + 32.),
            );
            if let Some(clip) = state.library_drag.as_ref()
                && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
                && tracks.intersect(ui.clip_rect()).contains(pointer)
            {
                let mut target = f64::from((pointer.x - origin) / scale).max(0.);
                // Snap either edge to nearby clip boundaries.
                let desired = target;
                let mut nearest = 8. / f64::from(scale);
                for boundary in std::iter::once(0.).chain(
                    state.cut.clips.iter().enumerate().flat_map(|(i, c)| {
                        let start = state.cut.clip_time(i);
                        [start, start + c.duration]
                    }),
                ) {
                    for position in [boundary, boundary - clip.duration] {
                        let distance = (desired - position).abs();
                        if position >= 0. && distance < nearest {
                            nearest = distance;
                            target = position;
                        }
                    }
                }
                let mut candidate = state.cut.clone();
                let valid = candidate.place_clip(clip.clone(), target);
                let color = if valid {
                    crate::GOLD
                } else {
                    egui::Color32::LIGHT_RED
                };
                let ghost = egui::Rect::from_min_size(
                    egui::pos2(origin + target as f32 * scale, video_y),
                    egui::vec2(clip.duration as f32 * scale, 70.),
                );
                painter.rect_filled(ghost, 3., color.gamma_multiply(0.25));
                if let Some(asset) = assets.iter().find(|a| a.id == clip.asset_id)
                    && let Some(path) = state.thumbnails.get(ui, asset, &self.store.root(), &ffmpeg)
                {
                    let strip = ghost.shrink(3.);
                    let mut image_ui = ui.new_child(egui::UiBuilder::new().max_rect(strip));
                    image_ui.set_clip_rect(strip.intersect(ui.clip_rect()));
                    let image = egui::Image::from_uri(format!("file://{}", path.display()))
                        .tint(egui::Color32::from_white_alpha(150));
                    let mut x = strip.left();
                    while x < strip.right() {
                        image.paint_at(
                            &image_ui,
                            egui::Rect::from_min_size(
                                egui::pos2(x, strip.top()),
                                egui::vec2(44., strip.height()),
                            ),
                        );
                        x += 44.;
                    }
                }
                painter.rect_stroke(
                    ghost,
                    3.,
                    egui::Stroke::new(2., color),
                    egui::StrokeKind::Inside,
                );
                let audio = egui::Rect::from_min_size(
                    egui::pos2(ghost.left(), audio_y),
                    egui::vec2(ghost.width(), 32.),
                );
                painter.rect_filled(audio, 3., color.gamma_multiply(0.25));
                painter.rect_stroke(
                    audio,
                    3.,
                    egui::Stroke::new(2., color),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    ghost.left_top() + egui::vec2(5., 5.),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{} · {:.2}s{}",
                        clip.title,
                        target,
                        if valid { "" } else { " · Overlap" }
                    ),
                    egui::FontId::proportional(11.),
                    egui::Color32::WHITE,
                );
                if ui.input(|i| i.pointer.any_released()) && valid {
                    place_at = Some(target);
                }
            }
            if let Some(index) = state.dragged
                && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            {
                let target = (f64::from((pointer.x - origin) / scale) - state.drag_offset).max(0.);
                let target = state
                    .cut
                    .snapped_slide_target(index, target, 8. / f64::from(scale));
                let mut candidate = state.cut.clone();
                let valid = candidate.slide(index, target);
                let ghost = egui::Rect::from_min_size(
                    egui::pos2(origin + target as f32 * scale, video_y),
                    egui::vec2(state.cut.clips[index].duration as f32 * scale, 108.),
                );
                painter.rect_stroke(
                    ghost,
                    3.,
                    egui::Stroke::new(
                        2.,
                        if valid {
                            crate::GOLD
                        } else {
                            egui::Color32::LIGHT_RED
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                if ui.input(|i| i.pointer.any_released()) && valid {
                    slide = Some((index, target));
                }
            }
            let x = origin + state.playhead.min(total) as f32 * scale;
            painter.line_segment(
                [
                    egui::pos2(x, canvas.top() + 23.),
                    egui::pos2(x, canvas.bottom()),
                ],
                egui::Stroke::new(2., egui::Color32::from_rgb(232, 101, 95)),
            );
            painter.circle_filled(
                egui::pos2(x, canvas.top() + 25.),
                4.,
                egui::Color32::from_rgb(232, 101, 95),
            );
        });
        if let Some((index, left, original, x)) = &state.trim_drag
            && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
        {
            let scale = f64::from(state.zoom);
            let original_start = state.cut.clip_time(*index) - state.cut.clips[*index].gap_before
                + original.gap_before;
            let edge_time = if *left {
                original_start
            } else {
                original_start + original.duration
            };
            let mut delta = f64::from(pointer.x - x) / scale;
            if (edge_time + delta - state.playhead).abs() <= 8. / scale {
                delta = state.playhead - edge_time;
            }
            let mut clip = original.clone();
            let source_end = assets
                .iter()
                .find(|a| a.id == clip.asset_id)
                .map_or(clip.start + clip.duration, |a| a.duration);
            if *left {
                let delta = delta.clamp(
                    -original.start.min(original.gap_before),
                    (original.duration - 0.04).max(0.),
                );
                clip.start += delta;
                clip.gap_before += delta;
                clip.duration = original.start + original.duration - clip.start;
            } else {
                clip.duration =
                    (original.duration + delta).clamp(0.04, source_end - original.start);
            }
            trim_update = Some((*index, clip));
        }
        if let Some((index, clip)) = sound_update {
            state.cut.clips[index] = clip;
        }
        if let Some((index, clip)) = trim_update {
            state.cut.clips[index] = clip;
        }
        if let Some(target) = place_at
            && let Some(clip) = state.library_drag.take()
        {
            let id = clip.id.clone();
            if state.cut.place_clip(clip, target) {
                state.selected = state.cut.clips.iter().position(|c| c.id == id);
            }
        }
        if ui.input(|i| i.pointer.any_released()) {
            state.library_drag = None;
            state.trim_drag = None;
            if let Some(original) = state.gesture_before.take()
                && original != state.cut
            {
                state.undo.push(TimelineEdit::Picture(original));
                state.redo.clear();
                history_action = true;
            }
            state.dragged = None;
        }
        if let Some((index, target)) = slide {
            let id = state.cut.clips[index].id.clone();
            if state.cut.slide(index, target) {
                state.selected = state.cut.clips.iter().position(|c| c.id == id);
            }
        }
        let delete_selected = !ui.ctx().wants_keyboard_input()
            && state.trim_drag.is_none()
            && state.dragged.is_none()
            && ui.input(|input| input.events.iter().any(|event| matches!(event,
                egui::Event::Key { key: egui::Key::Delete | egui::Key::Backspace, pressed: true, repeat: false, modifiers, .. } if modifiers.is_none()
            )));
        if delete_selected
            && let Some(id) = state.selected_music.take()
            && let Some(cue) = music.iter().find(|cue| cue.id == id)
        {
            state.music_edit(&self.store, vec![cue.clone()], vec![]);
        }
        if let Some(index) = state.selected.filter(|i| *i < state.cut.clips.len())
            && delete_selected
        {
            ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Delete);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace);
            });
            state.cut.remove_clip(index, true);
            state.selected = None;
        }
        if let Some((from, to)) = move_to {
            state.cut.clips.swap(from, to);
            state.selected = Some(to);
        }
        if let Some((action, index, offset)) = menu_action {
            match action {
                "split" => {
                    state.cut.split_clip(index, offset);
                }
                "duplicate" => {
                    state.cut.duplicate_clip(index);
                }
                "remove" => {
                    state.cut.remove_clip(index, false);
                }
                "ripple" => {
                    state.cut.remove_clip(index, true);
                }
                _ => {}
            }
            state.selected = None;
        }
        if state.cut != before {
            // A slide edits the cut without interrupting the preview already in flight.
            if slide.is_none() && state.timeline_player.take().is_some() {
                state.player = None;
            }
            if !history_action && state.gesture_before.is_none() {
                state.undo.push(TimelineEdit::Picture(before));
                if state.undo.len() > 100 {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
            match self
                .store
                .save_episode_cut(&episode, &state.saved, &state.cut)
            {
                Ok(()) => {
                    state.saved = state.cut.clone();
                    state.error.clear();
                }
                Err(error) => {
                    state.error = error.to_string();
                    let dir = self.store.root().join("drafts");
                    let _ = std::fs::create_dir_all(&dir);
                    if let Ok(bytes) = serde_json::to_vec_pretty(&state.cut) {
                        let _ = std::fs::write(dir.join(format!("edit-{episode}.json")), bytes);
                    }
                }
            }
        }
        if state.scrubbing && state.cut.source_at(state.playhead).is_none() {
            state.scrubber = Default::default();
        }
        if state.scrubbing
            && state.player.is_none()
            && let Some((asset_id, source_time)) = state.cut.source_at(state.playhead)
            && let Some(asset) = assets.iter().find(|asset| asset.id == asset_id)
        {
            state
                .scrubber
                .request(ui.ctx(), &ffmpeg, &asset.path, source_time);
        }
        if play_timeline {
            // Timeline transport wins over any source-preview request this frame.
            play = None;
        }
        if play_timeline && !state.cut.clips.is_empty() {
            if state.timeline_player.is_some()
                && state.player.as_ref().is_some_and(|player| {
                    !player.finished()
                        && (!player.is_paused()
                            || (!state.music_mix_changed
                                && state.timeline_player.as_ref() == Some(&state.cut)))
                })
            {
                if let Some(player) = &mut state.player
                    && let Err(error) = player.toggle_playback()
                {
                    state.error = error.to_string();
                }
            } else {
                state.player = None;
                state.timeline_player = None;
                let origin = if total - state.playhead < 0.04 {
                    0.
                } else {
                    state.playhead
                };
                let result = crate::timeline_preview::manifest(
                    &state.cut,
                    &music,
                    &assets,
                    self.store.video_aspect_ratio(),
                    origin,
                )
                .and_then(crate::timeline_preview::Native::open);
                match result {
                    Ok(player) => {
                        state.player = Some(player);
                        state.timeline_player = Some(state.cut.clone());
                        state.timeline_origin = 0.;
                        state.music_mix_changed = false;
                        state.player_title = "Timeline · direct playback".into();
                        state.error.clear();
                    }
                    Err(error) => state.error = error.to_string(),
                }
            }
        }
        if let Some(clip) = state.library_drag.clone()
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && let Some(asset) = assets.iter().find(|asset| asset.id == clip.asset_id)
        {
            let thumbnail = state.thumbnails.get(ui, asset, &self.store.root(), &ffmpeg);
            egui::Area::new(egui::Id::new("editing-drag-thumbnail"))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .movable(false)
                .fixed_pos(pointer + egui::vec2(16., 16.))
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(crate::theme::SURFACE)
                        .stroke(egui::Stroke::new(1.5, crate::GOLD))
                        .corner_radius(6)
                        .inner_margin(4.)
                        .show(ui, |ui| {
                            ui.set_width(84.);
                            if let Some(path) = thumbnail {
                                ui.add(
                                    egui::Image::from_uri(format!("file://{}", path.display()))
                                        .fit_to_exact_size(egui::vec2(84., 149.))
                                        .corner_radius(4),
                                );
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(84., 149.),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 4, egui::Color32::BLACK);
                            }
                            ui.add(
                                egui::Label::new(egui::RichText::new(&clip.title).size(10.)).wrap(),
                            );
                        });
                });
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if let Some((id, title)) = play
            && let Some(asset) = assets.iter().find(|a| a.id == id)
        {
            state.scrubbing = false;
            state.timeline_player = None;
            state.player = None;
            match crate::media::Preview::open(
                ffmpeg,
                asset.path.clone(),
                asset.width as f32 / asset.height.max(1) as f32,
                true,
            ) {
                Ok(player) => {
                    state.player = Some(crate::timeline_preview::Player::Source(player));
                    state.player_title = title;
                }
                Err(error) => state.error = error.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod music_snap_tests {
    use super::snap_music_position;
    #[test]
    fn snaps_nearest_edge_only_within_pixels_and_source_bounds() {
        assert_eq!(snap_music_position(9.8, &[10., 4.], 0., 1800., 0.4), 10.);
        assert_eq!(snap_music_position(4.2, &[10., 4.], 0., 1800., 0.4), 4.);
        assert_eq!(snap_music_position(7., &[10., 4.], 0., 1800., 0.4), 7.);
        assert_eq!(snap_music_position(2.9, &[3.1], 0., 3., 0.4), 2.9);
        assert_eq!(snap_music_position(-0.1, &[-0.1], 0., 3., 0.4), 0.);
        assert_eq!(snap_music_position(9.8, &[10.], 0., 1800., 0.1), 9.8);
    }
}
