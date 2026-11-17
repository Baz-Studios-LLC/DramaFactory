use crate::{
    Studio,
    editing_room::{Clip, Cut},
};
use eframe::egui;
#[derive(Default)]
pub struct Editor {
    episode: Option<String>,
    cut: Cut,
    saved: Cut,
    undo: Vec<Cut>,
    redo: Vec<Cut>,
    selected: Option<usize>,
    dragged: Option<usize>,
    drag_offset: f64,
    library_drag: Option<Clip>,
    trim_drag: Option<(usize, bool, Clip, f32)>,
    gesture_before: Option<Cut>,
    thumbnails: crate::take_gallery::Thumbnails,
    player: Option<crate::media::Preview>,
    player_title: String,
    timeline_player: Option<Cut>,
    timeline_origin: f64,
    preview_job: Option<(String, Cut, f64)>,
    export_job: Option<String>,
    error: String,
    split_at: f64,
    zoom: f32,
    playhead: f64,
    scrubber: crate::scrub_preview::Scrubber,
    scrubbing: bool,
}
impl Editor {
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

    pub fn stop(&mut self) {
        self.player = None;
        self.preview_job = None;
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
        let upper_height = (workspace.height() - 330.).max(180.);
        let viewer_height = upper_height - 48.;
        let viewer_width = (viewer_height * 9. / 16.).max(156.);
        let left_width = (workspace.width() - viewer_width - 20.).max(220.);
        ui.set_max_width(left_width);
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
        let mut open_timeline = false;
        let total = state.cut.duration();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{} clips · {:.1}s", state.cut.clips.len(), total));
            if ui
                .add_enabled(!state.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                state.redo.push(state.cut.clone());
                state.cut = state.undo.pop().unwrap();
                state.selected = None;
                history_action = true;
            }
            if ui
                .add_enabled(!state.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                state.undo.push(state.cut.clone());
                state.cut = state.redo.pop().unwrap();
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
                    .and_then(|()| self.store.export_episode_cut(&episode, &state.cut))
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
        if let Some((id, cut, origin)) = state.preview_job.clone() {
            if cut != state.cut {
                state.preview_job = None;
            } else if let Ok(job) = self.store.job(&id) {
                if job.status == "completed" {
                    if let Some(asset) = assets
                        .iter()
                        .find(|asset| Some(&asset.id) == job.output_id.as_ref())
                    {
                        play = Some((asset.id.clone(), "Timeline".into()));
                        state.timeline_origin = origin;
                        state.timeline_player = Some(cut);
                        open_timeline = true;
                        state.preview_job = None;
                    }
                } else if job.status == "failed" || job.status == "canceled" {
                    state.error = job.error;
                    state.preview_job = None;
                } else {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if state.timeline_player.as_ref() == Some(&state.cut)
            && let Some(player) = &state.player
        {
            state.playhead = (state.timeline_origin + player.playback_position()).min(total);
        }
        let pool_height = (workspace.top() + upper_height - ui.cursor().top() - 28.).max(80.);
        egui::CollapsingHeader::new("Media pool")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("editor-media-pool")
                        .auto_shrink([false, false])
                        .max_height(pool_height)
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
                                                    let (rect, drag) = ui.allocate_exact_size(egui::vec2(width, width*16./9.), egui::Sense::click_and_drag());
                                                    ui.painter().rect_filled(rect, 5, crate::theme::SURFACE);
                                                    if let Some(path) = thumb {
                                                        egui::Image::from_uri(format!("file://{}", path.display())).corner_radius(5).paint_at(ui, rect);
                                                    }
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
        ui.set_max_width(workspace.width());
        let viewer_rect = egui::Rect::from_min_size(
            egui::pos2(workspace.right() - viewer_width, workspace.top()),
            egui::vec2(viewer_width, upper_height),
        );
        {
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
            let video_rect = egui::Rect::from_min_max(
                egui::pos2(viewer_rect.left(), ui.cursor().top()),
                egui::pos2(viewer_rect.right(), caption_rect.top() - 4.),
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
                let caption = if state.preview_job.is_some() {
                    "Preparing playback…".to_owned()
                } else if state.scrubbing {
                    format!("Timeline · {:.2}s", state.playhead)
                } else {
                    String::new()
                };
                ui.put(
                    caption_rect,
                    egui::Label::new(egui::RichText::new(caption).small()).truncate(),
                );
            }
        }
        let remaining = workspace.top() + upper_height - ui.cursor().top();
        if remaining > 0. {
            ui.add_space(remaining);
        }
        ui.add_space(4.);
        let timeline_width = ui.available_width();
        ui.horizontal(|ui| {
            ui.strong("Timeline");
            if ui
                .add_enabled(
                    !state.cut.clips.is_empty() && state.preview_job.is_none(),
                    egui::Button::new("▶ / ⏸"),
                )
                .on_hover_text("Play / pause timeline · Space")
                .clicked()
            {
                play_timeline = true;
            }
            ui.weak("Drag to move · drag edges to trim");
            if state.zoom == 0. {
                state.zoom = 12.;
            }
            ui.add(
                egui::Slider::new(&mut state.zoom, 4. ..=40.)
                    .text("Zoom")
                    .show_value(false),
            );
            if ui.small_button("Fit").clicked() {
                state.zoom = ((timeline_width - 70.) / total.max(1.) as f32).clamp(4., 40.);
            }
        });
        if state.cut.clips.is_empty() {
            ui.label("Open the clip library above to add your first take.");
        }
        let mut slide = None;
        let mut insert_at = None;
        let mut trim_update = None;
        let mut sound_update = None;
        let mut menu_action = None;
        egui::ScrollArea::horizontal()
            .id_salt("edit-timeline")
            .show(ui, |ui| {
                let scale = state.zoom;
                let width = (total as f32 * scale + 240.).max(ui.available_width());
                let (canvas, _) =
                    ui.allocate_exact_size(egui::vec2(width, 166.), egui::Sense::hover());
                let painter = ui
                    .painter()
                    .with_clip_rect(canvas.intersect(ui.clip_rect()));
                painter.rect_filled(canvas, 4, egui::Color32::from_rgb(16, 19, 23));
                let origin = canvas.left() + 58.;
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
                let step = if scale < 8. {
                    10
                } else if scale < 20. {
                    5
                } else {
                    2
                };
                for second in (0..=((width - 58.) / scale) as usize).step_by(step) {
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
                    state.preview_job = None;
                    state.player = None;
                    state.scrubbing = true;
                    state.playhead = (f64::from((pos.x - origin) / scale)).clamp(0., total);
                    state.selected = None;
                    for (index, clip) in state.cut.clips.iter().enumerate() {
                        let start = state.cut.clip_time(index);
                        if state.playhead >= start && state.playhead < start + clip.duration {
                            state.selected = Some(index);
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
                                            .selectable_label(
                                                (*fade - seconds).abs() < 0.001,
                                                label,
                                            )
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
                        && let Some(path) =
                            state.thumbnails.get(ui, asset, &self.store.root(), &ffmpeg)
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
                        state.preview_job = None;
                        state.player = None;
                        state.scrubbing = true;
                        state.selected = Some(index);
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
                            state.gesture_before = Some(state.cut.clone());
                            state.trim_drag = Some((
                                index,
                                left,
                                clip.clone(),
                                ui.input(|i| i.pointer.interact_pos().map_or(0., |p| p.x)),
                            ));
                            state.dragged = None;
                        }
                    }
                    if response.drag_started() && state.trim_drag.is_none() {
                        state.dragged = Some(index);
                        state.selected = Some(index);
                        state.drag_offset = ui
                            .input(|i| i.pointer.press_origin())
                            .map_or(0., |p| f64::from((p.x - rect.left()) / scale));
                    }
                    if ui.rect_contains_pointer(rect) && state.library_drag.is_some() {
                        let at = if ui
                            .input(|i| i.pointer.hover_pos().is_some_and(|p| p.x > rect.center().x))
                        {
                            index + 1
                        } else {
                            index
                        };
                        let x = if at == index {
                            rect.left()
                        } else {
                            rect.right()
                        };
                        ui.painter().line_segment(
                            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                            egui::Stroke::new(3., crate::GOLD),
                        );
                        if ui.input(|i| i.pointer.any_released()) {
                            insert_at = Some(at);
                        }
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
                if ui.rect_contains_pointer(drop_rect) && ui.input(|i| i.pointer.any_released()) {
                    insert_at = Some(state.cut.clips.len());
                }
                if let Some(index) = state.dragged
                    && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
                {
                    let target =
                        (f64::from((pointer.x - origin) / scale) - state.drag_offset).max(0.);
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
            let delta = f64::from(pointer.x - x) / f64::from(state.zoom);
            let mut clip = original.clone();
            let source_end = assets
                .iter()
                .find(|a| a.id == clip.asset_id)
                .map_or(clip.start + clip.duration, |a| a.duration);
            if *left {
                clip.start =
                    (original.start + delta).clamp(0., original.start + original.duration - 0.04);
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
        if let Some(index) = insert_at
            && let Some(clip) = state.library_drag.take()
        {
            state.cut.clips.insert(index, clip);
            state.selected = Some(index);
        }
        if ui.input(|i| i.pointer.any_released()) {
            state.library_drag = None;
            state.trim_drag = None;
            if let Some(original) = state.gesture_before.take()
                && original != state.cut
            {
                state.undo.push(original);
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
        if let Some(index) = state.selected.filter(|i| *i < state.cut.clips.len()) {
            let mut remove = delete_selected;
            if delete_selected {
                ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Delete);
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace);
                });
            }
            let mut split = false;
            let mut move_to = None;
            let clip = &mut state.cut.clips[index];
            ui.add_space(4.);

            let source_duration = assets
                .iter()
                .find(|a| a.id == clip.asset_id)
                .map(|a| a.duration)
                .unwrap_or(clip.start + clip.duration);
            ui.horizontal_wrapped(|ui| {
                ui.strong(&clip.title);
                if ui.button("Play source").clicked() {
                    play = Some((
                        clip.asset_id.clone(),
                        format!("{} · full source", clip.title),
                    ));
                }
                if ui
                    .add_enabled(index > 0, egui::Button::new("← Earlier"))
                    .clicked()
                {
                    move_to = Some(index - 1);
                }
                if ui.button("Later →").clicked() {
                    move_to = Some(index + 1);
                }
                remove |= ui
                    .button("Ripple delete")
                    .on_hover_text("Delete selected timeline clip · Delete / Backspace")
                    .clicked();
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("In");
                let end = clip.start + clip.duration;
                if ui
                    .add(
                        egui::DragValue::new(&mut clip.start)
                            .speed(0.05)
                            .max_decimals(2)
                            .range(0. ..=(end - 0.04).max(0.))
                            .suffix("s"),
                    )
                    .changed()
                {
                    clip.duration = end - clip.start;
                }
                let mut end = clip.start + clip.duration;
                ui.label("Out");
                if ui
                    .add(
                        egui::DragValue::new(&mut end)
                            .speed(0.05)
                            .max_decimals(2)
                            .range((clip.start + 0.04)..=source_duration)
                            .suffix("s"),
                    )
                    .changed()
                {
                    clip.duration = end - clip.start;
                }
                state.split_at = state.split_at.clamp(0.02, (clip.duration - 0.02).max(0.02));
                ui.label("Split at");
                ui.add(
                    egui::DragValue::new(&mut state.split_at)
                        .speed(0.05)
                        .max_decimals(2)
                        .range(0.02..=(clip.duration - 0.02).max(0.02))
                        .suffix("s"),
                );
                split = ui
                    .add_enabled(
                        clip.duration >= 0.08
                            && state.split_at >= 0.04
                            && clip.duration - state.split_at >= 0.04,
                        egui::Button::new("Split at cursor"),
                    )
                    .clicked();
            });
            if state.timeline_player.is_none() && !play_timeline {
                state.playhead = state.cut.clip_time(index) + state.split_at;
            }
            if remove {
                state.cut.remove_clip(index, true);
                state.selected = None;
            } else if split {
                state.cut.split_clip(index, state.split_at);
            } else if let Some(to) = move_to
                && to < state.cut.clips.len()
            {
                state.cut.clips.swap(index, to);
                state.selected = Some(to);
            }
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
            state.preview_job = None;
            if state.timeline_player.take().is_some() {
                state.player = None;
            }
            if !history_action && state.gesture_before.is_none() {
                state.undo.push(before);
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
        if play_timeline && !open_timeline {
            // Timeline transport wins over any source-preview request this frame.
            play = None;
        }
        if play_timeline
            && !open_timeline
            && !state.cut.clips.is_empty()
            && state.preview_job.is_none()
        {
            if state.timeline_player.as_ref() == Some(&state.cut)
                && state
                    .player
                    .as_ref()
                    .is_some_and(|player| !player.finished())
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
                let playback_cut = state.cut.playback_from(origin);
                match self
                    .store
                    .preview_episode_cut(&episode, &playback_cut)
                    .and_then(|job| {
                        if job.status != "completed" {
                            crate::worker::spawn_detached(&self.store.path)?;
                        }
                        Ok(job)
                    }) {
                    Ok(job) => {
                        state.preview_job = Some((job.id, state.cut.clone(), origin));
                        ui.ctx().request_repaint();
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
            && (open_timeline || state.preview_job.is_none())
            && let Some(asset) = assets.iter().find(|a| a.id == id)
        {
            state.scrubbing = false;
            if !open_timeline {
                state.timeline_player = None;
                state.preview_job = None;
            }
            state.player = None;
            match crate::media::Preview::open(
                ffmpeg,
                asset.path.clone(),
                asset.width as f32 / asset.height.max(1) as f32,
                true,
            ) {
                Ok(player) => {
                    state.player = Some(player);
                    state.player_title = title;
                }
                Err(error) => state.error = error.to_string(),
            }
        }
    }
}
