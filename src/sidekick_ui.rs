use crate::{GOLD, MUTED, Page, Studio, sidekick::Message};
enum MessageAction {
    Edit(Message),
    Save(Message, String),
    Cancel,
    Delete(Message),
}
#[derive(Clone, Copy)]
enum MessageIcon {
    Copy,
    Edit,
    Delete,
}
fn message_icon(
    ui: &mut eframe::egui::Ui,
    icon: MessageIcon,
    enabled: bool,
) -> eframe::egui::Response {
    use eframe::egui::{self, vec2};
    let label = match icon {
        MessageIcon::Copy => "Copy message",
        MessageIcon::Edit => "Edit message",
        MessageIcon::Delete => "Delete message",
    };
    let response = ui.add_enabled(enabled, egui::Button::new("").min_size(vec2(26., 24.)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let center = response.rect.center();
    let point = |x, y| center + vec2(x, y);
    let stroke = egui::Stroke::new(
        1.4,
        if enabled {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        },
    );
    let painter = ui.painter();
    match icon {
        MessageIcon::Copy => {
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-3., -3.), point(6., 7.)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([point(-6., 3.), point(-6., -6.)], stroke);
            painter.line_segment([point(-6., -6.), point(3., -6.)], stroke);
        }
        MessageIcon::Edit => {
            painter.add(egui::Shape::closed_line(
                vec![
                    point(-6., 6.),
                    point(-5., 2.),
                    point(3., -6.),
                    point(6., -3.),
                    point(-2., 5.),
                ],
                stroke,
            ));
            painter.line_segment([point(1., -4.), point(4., -1.)], stroke);
        }
        MessageIcon::Delete => {
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-4., -3.), point(4., 7.)),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([point(-6., -5.), point(6., -5.)], stroke);
            painter.line_segment([point(-2., -7.), point(2., -7.)], stroke);
            painter.line_segment([point(-1., -1.), point(-1., 4.)], stroke);
            painter.line_segment([point(2., -1.), point(2., 4.)], stroke);
        }
    }
    response.on_hover_text(label)
}
fn message_bubble(
    ui: &mut eframe::egui::Ui,
    message: &Message,
    markdown: &mut egui_commonmark::CommonMarkCache,
    editing: &mut Option<(Message, String)>,
    enabled: bool,
    history_width: f32,
) -> Option<MessageAction> {
    let mut action = None;
    use eframe::egui::{Align, Color32, Frame, Layout, RichText, Stroke};
    let user = message.role == "user";
    let row = ui.available_rect_before_wrap();
    let width = history_width * 0.88;
    let mut bounds = row;
    if user {
        bounds.max.x = row.left() + history_width;
        bounds.min.x = bounds.max.x - width;
    } else {
        bounds.max.x = row.left() + width;
    }
    let bounds_id = ui.make_persistent_id(("sidekick-bubble-bounds", &message.id));
    let hover = ui
        .data(|data| data.get_temp::<eframe::egui::Rect>(bounds_id))
        .is_some_and(|rect| ui.rect_contains_pointer(rect));
    ui.scope_builder(
        eframe::egui::UiBuilder::new()
            .max_rect(bounds)
            .layout(Layout::top_down(Align::LEFT)),
        |ui| {
            let frame = Frame::new()
                .fill(if user {
                    Color32::from_rgb(48, 43, 35)
                } else {
                    Color32::from_rgb(25, 27, 32)
                })
                .stroke(Stroke::new(
                    1_f32,
                    if user {
                        Color32::from_rgb(73, 63, 46)
                    } else {
                        Color32::from_rgb(40, 43, 49)
                    },
                ))
                .corner_radius(12)
                .inner_margin(12.)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                        ui.spacing_mut().item_spacing.y = 4.;
                        ui.scope(|ui| {
                            // Header controls share the speaker row rather than reserving
                            // an invisible, full-height action row below every message.
                            ui.spacing_mut().interact_size.y =
                                ui.text_style_height(&eframe::egui::TextStyle::Small);
                            ui.spacing_mut().button_padding = eframe::egui::vec2(4., 2.);
                            ui.spacing_mut().item_spacing.x = 6.;
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if user { "You" } else { "Sidekick" })
                                        .size(18.)
                                        .strong()
                                        .color(if user { GOLD } else { MUTED }),
                                );
                                if editing
                                    .as_ref()
                                    .is_none_or(|(original, _)| original.id != message.id)
                                {
                                    ui.scope_builder(
                                        if hover {
                                            eframe::egui::UiBuilder::new()
                                        } else {
                                            eframe::egui::UiBuilder::new().invisible()
                                        }
                                        .layout(Layout::right_to_left(Align::Center)),
                                        |ui| {
                                            if user
                                                && message_icon(ui, MessageIcon::Delete, enabled)
                                                    .clicked()
                                            {
                                                action =
                                                    Some(MessageAction::Delete(message.clone()));
                                            }
                                            if user
                                                && message_icon(ui, MessageIcon::Edit, enabled)
                                                    .clicked()
                                            {
                                                action = Some(MessageAction::Edit(message.clone()));
                                            }
                                            if message_icon(ui, MessageIcon::Copy, true).clicked() {
                                                ui.ctx().copy_text(message.text.clone());
                                            }
                                        },
                                    );
                                }
                            });
                        });
                        if !message.images.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                for image in &message.images {
                                    image.thumbnail(ui);
                                }
                            });
                        }
                        if let Some((original, text)) = editing
                            .as_mut()
                            .filter(|(original, _)| original.id == message.id)
                        {
                            ui.add(
                                eframe::egui::TextEdit::multiline(text)
                                    .desired_width(ui.available_width())
                                    .desired_rows(3),
                            );
                            ui.small("Save changes only; the sidekick will not be called again.");
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        enabled && !text.trim().is_empty(),
                                        eframe::egui::Button::new("Save changes"),
                                    )
                                    .clicked()
                                {
                                    action =
                                        Some(MessageAction::Save(original.clone(), text.clone()));
                                }
                                if ui.button("Cancel").clicked() {
                                    action = Some(MessageAction::Cancel);
                                }
                            });
                        } else if !user {
                            ui.push_id(&message.id, |ui| {
                                ui.visuals_mut().override_text_color =
                                    Some(Color32::from_rgb(235, 237, 242));
                                crate::chat_markdown::show(ui, markdown, &message.text);
                            });
                        } else {
                            ui.add(
                                eframe::egui::Label::new(
                                    RichText::new(&message.text)
                                        .color(Color32::from_rgb(235, 237, 242)),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        }
                    });
                });
            ui.data_mut(|data| data.insert_temp(bounds_id, frame.response.rect));
        },
    );
    ui.add_space(10.);
    action
}
type PendingReply = (
    String,
    String,
    std::sync::mpsc::Receiver<Result<crate::sidekick_edit::Reply, String>>,
    Option<crate::synopsis::SynopsisDraft>,
    Vec<crate::character_edit::CharacterDraft>,
);
#[derive(Default)]
pub struct Sidekick {
    live_instance: String,
    live_tick: Option<std::time::Instant>,
    external: bool,
    manual_external: bool,
    automatic_running: bool,
    external_request: Option<String>,
    external_sender: Option<std::sync::mpsc::Sender<Result<crate::sidekick_edit::Reply, String>>>,
    markdown: egui_commonmark::CommonMarkCache,
    image_drafts: std::collections::HashMap<
        String,
        (
            Vec<crate::chat_images::ChatImage>,
            Vec<crate::chat_images::ChatImage>,
        ),
    >,
    drafts: std::collections::HashMap<String, String>,
    saved_drafts: std::collections::HashMap<String, String>,
    editing_message: Option<(Message, String)>,
    progress: std::sync::Arc<std::sync::atomic::AtomicU8>,
    request_started: Option<std::time::Instant>,
    pending: Option<PendingReply>,
    cast_queue: Option<crate::cast_queue::Queue>,
    request_context: Option<String>,
    canceled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    error: String,
    plot_request: Option<(String, crate::story::SeriesPlan)>,
    episode_request: Option<crate::sidekick_episode::EpisodeDraft>,
    generation_request: Option<crate::sidekick_generation::Snapshot>,
    publishing_request: Option<crate::sidekick_publish::Snapshot>,
    pub(crate) generation_undo: std::collections::HashMap<
        String,
        (
            crate::sidekick_generation::Snapshot,
            crate::sidekick_generation::Snapshot,
        ),
    >,
    episode_creation_request: Option<(String, String, crate::story::SeriesPlan)>,
    pub(crate) episode_undo: std::collections::HashMap<
        String,
        (
            crate::sidekick_episode::EpisodeDraft,
            crate::sidekick_episode::EpisodeDraft,
        ),
    >,
    pub(crate) plot_undo:
        std::collections::HashMap<String, (crate::story::SeriesPlan, crate::story::SeriesPlan)>,
    pub(crate) character_undo: std::collections::HashMap<
        String,
        (
            crate::character_edit::CharacterDraft,
            crate::character_edit::CharacterDraft,
        ),
    >,
    pub(crate) synopsis_undo: std::collections::HashMap<
        String,
        (
            crate::synopsis::SynopsisDraft,
            crate::synopsis::SynopsisDraft,
        ),
    >,
}
impl Sidekick {
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn reset_context(&mut self) {
        self.editing_message = None;
        self.error.clear();
        // Keep per-series drafts and in-flight replies attached to their origin.
    }
}
impl Sidekick {
    fn switch_mode(&mut self, external: bool) {
        if self.external != external {
            self.stop();
            self.external = external;
            self.error.clear();
        }
    }
    fn stop(&mut self) {
        self.automatic_running = false;
        self.external_sender = None;
        self.external_request = None;
        self.canceled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.cast_queue = None;
        self.request_context = None;
        self.pending = None; // Disconnect the old result: it can never apply edits or queue media.
        self.request_started = None;
        self.plot_request = None;
        self.episode_request = None;
        self.generation_request = None;
        self.publishing_request = None;
        self.episode_creation_request = None;
        self.error = "Stopped. Your message is retained; no reply changes will be applied. The provider may still charge for a request already sent.".into();
    }
}
impl Studio {
    fn live_context(&mut self) -> serde_json::Value {
        let series = self.selected.clone().unwrap_or_default();
        let context = format!("{:?}", self.page);
        let character_snapshots = self.character_drafts(&series).unwrap_or_default();
        let generation_snapshot = self.generation_snapshot().ok();
        let publishing_snapshot = self
            .episode_id
            .as_deref()
            .and_then(|id| self.publisher.snapshot(&self.store, id).ok());
        let episode_snapshot = self.episode_snapshot().ok();
        serde_json::json!({
            "workspace":"All workspaces", "database":self.store.path, "visible_workspace":context,
                "timeline":self.editor.live_snapshot(),
                "other_drafts":{"library":self.library_drafts,"dialogue":self.dialogue_drafts,"sound":self.cue_drafts,"fades":self.fade_drafts,"trims":self.shot_trim_drafts,"directions":self.shot_directions,"cameras":self.camera_drafts,"reviews":self.review_drafts},
            "local_character_portraits":self.store.library(&series,"character").unwrap_or_default().iter().filter_map(|entry|entry.reference.as_ref().map(|path|serde_json::json!({"id":entry.id,"name":entry.name,"path":path}))).collect::<Vec<_>>(),
            "publishing_draft":publishing_snapshot.as_ref().map(|snapshot|snapshot.context()),
            "generation_draft":generation_snapshot.as_ref().map(|snapshot|serde_json::json!({"plan":snapshot.plan,"saved_episode":snapshot.context["saved_episode"],"image_style":snapshot.context["image_style"],"available_portraits":snapshot.context["available_portraits"].as_array().map(|items|items.iter().map(|item|serde_json::json!({"character_id":item["character_id"],"name":item["name"]})).collect::<Vec<_>>())})),
            "character_drafts":character_snapshots.iter().map(crate::character_edit::CharacterDraft::model_context).collect::<Vec<_>>(),
            "series":self.store.series().ok().and_then(|items|items.into_iter().find(|item|item.id==series)),
                            "series_plot_draft":self.story_drafts.get(&series).cloned().or_else(||self.store.series_plan(&series).ok()),
            "available_episodes":self.store.episodes(&series).unwrap_or_default().iter().map(|episode|serde_json::json!({"id":episode.id,"title":episode.title})).collect::<Vec<_>>(),
            "locations":self.store.library(&series,"location").unwrap_or_default().iter().map(|entry|serde_json::json!({"id":entry.id,"name":entry.name,"description":entry.description})).collect::<Vec<_>>(),
            "episode_id":self.episode_id,
            "music_editor":self.editor.music_context(),
            "music_cut_duration":self.editor.music_cut_duration(&self.store, self.episode_id.as_deref()),
            "music_capabilities":{"provider":"ElevenLabs","instrumental":true,"sidekick":"prepare direction and time range; user clicks Generate music to authorize payment","voices":false},
            "episode_draft":episode_snapshot.as_ref().map(|d|serde_json::json!({"episode_id":d.episode_id,"screenplay":d.screenplay,"outline":d.plan.episodes.iter().find(|o|o.episode_id==d.episode_id)})),
            "current_screenplay_draft":self.script,
            "synopsis_draft":self.synopsis_drafts.get(&series),
        })
    }

    pub(crate) fn save_project_chat(&mut self, series: &str) -> anyhow::Result<()> {
        if let Some(draft) = self.sidekick.drafts.get(series) {
            let expected = self
                .sidekick
                .saved_drafts
                .get(series)
                .cloned()
                .unwrap_or(self.store.sidekick_draft(series)?);
            self.store.save_sidekick_draft(series, &expected, draft)?;
            self.sidekick
                .saved_drafts
                .insert(series.into(), draft.clone());
        }
        if let Some((message, text)) = &self.sidekick.editing_message {
            self.store
                .change_user_message(series, message, Some(text))?;
            self.sidekick.editing_message = None;
        }
        Ok(())
    }
    pub(crate) fn prepare_episode_discussion(&mut self) {
        self.sidekick_visible = true;
        let Some(series) = self.selected.clone() else {
            return;
        };
        let saved = self.store.sidekick_draft(&series).unwrap_or_default();
        let draft = self
            .sidekick
            .drafts
            .entry(series.clone())
            .or_insert(saved.clone());
        self.sidekick.saved_drafts.entry(series).or_insert(saved);
        if draft.trim().is_empty() {
            *draft=if self.episode_id.is_none() {
                "Create and fill the first episode from the overall plot and cast. Give it a title, purpose, conflict, emotional turn, payoff, hook, continuity consequences and a complete screenplay for the target duration. Create the record directly; do not ask me to create a blank episode. Do not generate video."
            } else {
                "Develop this selected episode from the overall plot, cast and adjacent episode outlines. Fill its purpose, conflict, emotional turn, payoff, hook and continuity consequences, then write a complete performable screenplay with natural dialogue for the target duration. Preserve established facts and flag contradictions. Update the episode draft directly; do not generate media."
            }.into();
        }
    }
    pub(crate) fn prepare_plot_discussion(&mut self, series: &str) {
        self.sidekick_visible = true;
        let saved = self.store.sidekick_draft(series).unwrap_or_default();
        let draft = self
            .sidekick
            .drafts
            .entry(series.into())
            .or_insert(saved.clone());
        self.sidekick
            .saved_drafts
            .entry(series.into())
            .or_insert(saved);
        if draft.trim().is_empty() {
            *draft = "Develop the Overall plot draft from our synopsis, cast and prior plot discussion. Organize the series arc, act roadmap, ongoing storylines, character journeys and ending. Keep the current episode target unless we explicitly agreed otherwise. Identify conflicting story decisions before resolving them. Update the form directly; do not write screenplay drafts or generate media.".into();
        }
    }
    pub(crate) fn sidekick_panel(&mut self, ctx: &eframe::egui::Context) {
        let received = self.sidekick.pending.as_ref().map(
            |(series, context, receiver, snapshot, characters)| {
                (
                    series.clone(),
                    context.clone(),
                    receiver.try_recv(),
                    snapshot.clone(),
                    characters.clone(),
                )
            },
        );
        if let Some((series, context, received, snapshot, characters)) = received {
            match received {
                Ok(result) => {
                    let result = result.and_then(|reply| {
                        if let Some(queue) = &self.sidekick.cast_queue {
                            crate::cast_queue::validate_reply(
                                &reply,
                                queue.remaining.front().expect("active cast task"),
                            )
                            .map_err(|error| error.to_string())?;
                        }
                        Ok(reply)
                    });
                    match result {
                        Ok(mut reply) => {
                            let mut text = reply.message;
                            if let Some(music) = reply.music {
                                let request: serde_json::Value = serde_json::from_str(
                                    self.sidekick.request_context.as_deref().unwrap_or("{}"),
                                )
                                .unwrap_or_default();
                                let result = (|| -> anyhow::Result<()> {
                                    anyhow::ensure!(
                                        request["episode_id"].as_str()
                                            == Some(music.episode_id.as_str())
                                            && self.episode_id.as_deref()
                                                == Some(music.episode_id.as_str())
                                            && self.selected.as_deref() == Some(series.as_str()),
                                        "Episode changed; music draft was not applied"
                                    );
                                    self.editor.prepare_music(
                                        &self.store,
                                        &music,
                                        &request["music_editor"],
                                    )?;
                                    self.page = crate::Page::EditingRoom;
                                    Ok(())
                                })();
                                match result {
                                    Ok(()) => text.push_str("\n\nMusic direction is ready in the Editing room. Click Generate music to authorize the paid ElevenLabs request. No music has been generated yet."),
                                    Err(error) => text.push_str(&format!("\n\nMusic draft was not applied: {error}")),
                                }
                            }
                            if let Some(tasks) = reply.cast_queue.take() {
                                let source =
                                    self.sidekick.request_context.as_deref().unwrap_or("{}");
                                let queued_context: serde_json::Value =
                                    serde_json::from_str(source).unwrap_or_default();
                                if queued_context["workspace"] != "All workspaces"
                                    && context != "Characters"
                                    && context != "Locations"
                                {
                                    text.push_str("\nCast queue was not started outside the character workspace.");
                                } else {
                                    let total = tasks.len();
                                    self.sidekick.cast_queue = Some(crate::cast_queue::Queue {
                                        series: series.clone(),
                                        context: queued_context,
                                        remaining: tasks.into(),
                                        total,
                                        completed: 0,
                                    });
                                    text.push_str(&format!("\nQueued {total} characters. Creating one at a time; Stop cancels the remaining queue. No portraits will be generated."));
                                }
                            }
                            if let Some(proposals) = reply.new_episodes {
                                let result = (|| -> anyhow::Result<Vec<String>> {
                                    let (request_id, origin, expected) = self
                                        .sidekick
                                        .episode_creation_request
                                        .as_ref()
                                        .ok_or_else(|| {
                                            anyhow::anyhow!(
                                                "No episode-creation workspace was supplied"
                                            )
                                        })?;
                                    if origin != &series {
                                        anyhow::bail!("Episode creation belongs to another series");
                                    }
                                    let current = self
                                        .story_drafts
                                        .get(&series)
                                        .cloned()
                                        .unwrap_or(self.store.series_plan(&series)?);
                                    if !crate::sidekick_plot::same(&current, expected) {
                                        anyhow::bail!(
                                            "Your overall plot draft changed while the sidekick was replying; episode proposals retained"
                                        );
                                    }
                                    let ids = self.store.create_sidekick_episodes(
                                        &series,
                                        request_id,
                                        expected.revision,
                                        &proposals,
                                    )?;
                                    let saved = self.store.series_plan(&series)?;
                                    let mut draft = current;
                                    for id in &ids {
                                        if let Some(outline) =
                                            saved.episodes.iter().find(|o| &o.episode_id == id)
                                        {
                                            crate::sidekick_episode::replace_outline(
                                                &mut draft,
                                                id,
                                                Some(outline.clone()),
                                            );
                                        }
                                    }
                                    draft.revision = saved.revision;
                                    self.story_drafts.insert(series.clone(), draft);
                                    self.story_reports.remove(&series);
                                    if self.selected.as_ref() == Some(&series)
                                        && self.episode_id.is_none()
                                        && let Some(first) = ids.first()
                                    {
                                        self.select_episode(first);
                                    }
                                    Ok(ids)
                                })();
                                match result {Ok(ids)=>text.push_str(&format!("\n\nCreated and saved {} episode(s), including story beats and screenplays. Open Episode plot to review them. No video was generated.",ids.len())),Err(error)=>text.push_str(&format!("\n\n{error}\nEpisode creation proposals retained:\n{}",serde_json::to_string_pretty(&proposals).unwrap_or_default()))}
                            }
                            if let Some(edit) = reply.publishing {
                                let result = (|| -> anyhow::Result<()> {
                                    let expected =
                                        self.sidekick.publishing_request.as_ref().ok_or_else(
                                            || anyhow::anyhow!("No publishing draft was supplied"),
                                        )?;
                                    anyhow::ensure!(
                                        self.episode_id.as_deref()
                                            == Some(expected.episode_id.as_str())
                                            && self.selected.as_deref()
                                                == Some(
                                                    self.store
                                                        .get_episode(&expected.episode_id)?
                                                        .series_id
                                                        .as_str()
                                                ),
                                        "Episode changed while Sidekick was replying"
                                    );
                                    self.publisher.apply_sidekick(&self.store, expected, &edit)
                                })();
                                match result { Ok(())=>text.push_str("\n\nPublishing draft filled in. Review it in Publish, then click Publish to upload and schedule through Buffer. Undo is available. Nothing was uploaded or scheduled."),Err(error)=>text.push_str(&format!("\n\n{error}\nPublishing proposal retained:\n{}",serde_json::to_string_pretty(&edit).unwrap_or_default())) }
                            }
                            if let Some(edit) = reply.generation {
                                let result = (|| -> anyhow::Result<()> {
                                    let expected = self
                                        .sidekick
                                        .generation_request
                                        .clone()
                                        .ok_or_else(|| {
                                            anyhow::anyhow!(
                                                "No generation plan was supplied to this request"
                                            )
                                        })?;
                                    let current = self.generation_snapshot()?;
                                    let plan = crate::sidekick_generation::apply(
                                        &current, &expected, &edit,
                                    )?;
                                    let id = plan.episode_id.clone();
                                    for segment in &plan.segments {
                                        if segment.last_frame_from.is_some()
                                            && segment.frame_source_asset.is_none()
                                        {
                                            self.video_planner
                                                .handoff
                                                .queue
                                                .push_back((id.clone(), segment.id.clone()));
                                        }
                                    }
                                    self.video_planner.drafts.insert(id.clone(), plan);
                                    let after = self.generation_snapshot()?;
                                    self.sidekick.generation_undo.insert(id, (current, after));
                                    self.video_planner.section = 0;
                                    Ok(())
                                })();
                                match result {
                                    Ok(()) => text.push_str("\n\nVideo plan updated. Any requested starting frames are being prepared; check their status before generating. Undo is available."),
                                    Err(error) => { text = format!("Generation proposal not applied.\n\n{error}\n\nProposed change:\n{text}\n\nGeneration proposal retained:\n{}", serde_json::to_string_pretty(&edit).unwrap_or_default()); },
                                }
                            }
                            if let Some(edit) = reply.episode {
                                let result = (|| -> anyhow::Result<()> {
                                    let expected =
                                        self.sidekick.episode_request.as_ref().ok_or_else(
                                            || {
                                                anyhow::anyhow!(
                                                    "No episode draft was supplied to this request"
                                                )
                                            },
                                        )?;
                                    let current = self.episode_snapshot()?;
                                    let after = crate::sidekick_episode::apply(
                                        &self.store,
                                        &current,
                                        expected,
                                        &edit,
                                    )?;
                                    self.sidekick
                                        .episode_undo
                                        .insert(after.episode_id.clone(), (current, after.clone()));
                                    self.put_episode_draft(after);
                                    Ok(())
                                })();
                                match result {Ok(())=>text.push_str("\n\nEpisode draft updated. Review the story beats and screenplay, then Save episode. Undo is available."),Err(error)=>{text = format!("Episode proposal not applied.\n\n{error}\n\nProposed change:\n{text}\n\nEpisode proposal retained:\n{}",serde_json::to_string_pretty(&edit).unwrap_or_default());}}
                            }
                            if let Some(edit) = reply.plot {
                                let result = (|| -> anyhow::Result<()> {
                                    let (request_series, expected) =
                                        self.sidekick.plot_request.as_ref().ok_or_else(|| {
                                            anyhow::anyhow!(
                                                "No Overall plot draft was supplied to this request"
                                            )
                                        })?;
                                    if request_series != &series {
                                        anyhow::bail!(
                                            "Plot proposal belongs to a different series"
                                        );
                                    }
                                    let current = self
                                        .story_drafts
                                        .get(&series)
                                        .cloned()
                                        .unwrap_or(self.store.series_plan(&series)?);
                                    let after = crate::sidekick_plot::apply(
                                        &self.store,
                                        &current,
                                        expected,
                                        &edit,
                                    )?;
                                    self.sidekick
                                        .plot_undo
                                        .insert(series.clone(), (current, after.clone()));
                                    self.story_drafts.insert(series.clone(), after);
                                    Ok(())
                                })();
                                match result {
                                    Ok(()) => text.push_str("\n\nOverall plot draft updated. Review it, then Save plot. Undo is available."),
                                    Err(error) => text.push_str(&format!("\n\n{error}\nPlot proposal retained:\n{}", serde_json::to_string_pretty(&edit).unwrap_or_default())),
                                }
                            }
                            if let Some(edit) = reply.synopsis {
                                let proposal =
                                    serde_json::to_string_pretty(&edit).unwrap_or_default();
                                let applied=match (snapshot,self.synopsis_drafts.get_mut(&series)) {
                                    (Some(expected),Some(current))=>{
                                        match crate::sidekick_edit::apply(current,&expected,&edit) {
                                            Ok(before)=>{self.sidekick.synopsis_undo.insert(series.clone(),(before,current.clone()));Ok(())},
                                            Err(error)=>Err(error.to_string()),
                                        }
                                    },
                                    _=>Err("No matching synopsis draft was captured; proposal retained without applying.".into()),
                                };
                                match applied {Ok(())=>text.push_str("\n\nSynopsis draft updated. Review it in Synopsis; Undo is available. Nothing was saved."),Err(error)=>{text.push_str(&format!("\n\n{error}\nProposed synopsis:\n{proposal}"));}}
                            }
                            if let Some(edit) = reply.character {
                                let result = (|| -> anyhow::Result<()> {
                                    let expected = characters
                                        .iter()
                                        .find(|draft| draft.brief.id == edit.id)
                                        .ok_or_else(|| {
                                            anyhow::anyhow!(
                                                "Character was not in this request's series"
                                            )
                                        })?;
                                    let current = self
                                        .character_drafts(&series)?
                                        .into_iter()
                                        .find(|draft| draft.brief.id == edit.id)
                                        .ok_or_else(|| {
                                            anyhow::anyhow!("Character no longer exists")
                                        })?;
                                    let after =
                                        crate::character_edit::apply(&current, expected, &edit)?;
                                    self.sidekick
                                        .character_undo
                                        .insert(edit.id.clone(), (current, after.clone()));
                                    self.put_character_draft(after);
                                    Ok(())
                                })();
                                match result {Ok(())=>text.push_str("\n\nCharacter profile and portrait prompt drafts updated. Save the character in Character profile and the prompt in Manage portrait when ready. Undo is available in Character profile. No image was generated."),Err(error)=>text.push_str(&format!("\n\n{error}\nProposal retained:\n{}",serde_json::to_string_pretty(&edit).unwrap_or_default()))}
                            }
                            for proposal in reply
                                .new_character
                                .into_iter()
                                .chain(reply.new_characters.into_iter().flatten())
                            {
                                let id = uuid::Uuid::new_v4().to_string();
                                match self
                                    .store
                                    .create_sidekick_character(&series, &id, &proposal)
                                {
                                    Ok(()) => {
                                        text.push_str(&format!("\n\nCreated {} in Characters with its profile and portrait prompt.", proposal.name));
                                        if let Some(queue) = &mut self.sidekick.cast_queue {
                                            queue.remaining.pop_front();
                                            queue.completed += 1;
                                            text.push_str(&format!(
                                                " Cast progress: {}/{}.",
                                                queue.completed, queue.total
                                            ));
                                            if queue.remaining.is_empty() {
                                                text.push_str(" Cast queue complete.");
                                                self.sidekick.cast_queue = None;
                                            }
                                        }
                                        if proposal.generate_portrait {
                                            match self.store.enqueue_grok_portrait(&id,&proposal.portrait_prompt) {
                                                Ok(job)=>{
                                                    text.push_str(&format!("\nPortrait generation queued: {}. Review the result before choosing it.",job.id));
                                                    if let Err(error)=crate::worker::spawn_detached(&self.store.path) {text.push_str(&format!("\nWorker could not start: {error}. The queued job is retained."));}
                                                },
                                                Err(error)=>text.push_str(&format!("\nCharacter saved, but portrait could not be queued: {error}")),
                                            }
                                        } else {
                                            text.push_str(" No image was generated.");
                                        }
                                    }
                                    Err(error) => {
                                        self.sidekick.cast_queue = None;
                                        text.push_str(&format!("\n\nCharacter was not created: {error}. Any remaining cast queue was stopped; completed characters are retained.\nProposal retained:\n{}",serde_json::to_string_pretty(&proposal).unwrap_or_default()));
                                    }
                                }
                            }
                            let message = Message {
                                images: Vec::new(),
                                id: uuid::Uuid::new_v4().to_string(),
                                role: "assistant".into(),
                                text,
                                context: context.clone(),
                            };
                            if let Err(error) =
                                self.store.append_sidekick_message(&series, &message)
                            {
                                self.sidekick.error = error.to_string();
                                self.sidekick.cast_queue = None;
                            }
                        }
                        Err(error) => {
                            self.sidekick.error = if self.sidekick.cast_queue.take().is_some() {
                                format!(
                                    "{error} Any remaining cast queue was stopped; completed characters are retained."
                                )
                            } else {
                                error
                            };
                        }
                    };
                    self.sidekick.pending = None;
                    self.sidekick.automatic_running = false;
                    self.sidekick.external_request = None;
                    self.sidekick.plot_request = None;
                    self.sidekick.episode_request = None;
                    self.sidekick.generation_request = None;
                    self.sidekick.publishing_request = None;
                    self.sidekick.episode_creation_request = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.sidekick.cast_queue = None;
                    self.sidekick.error =
                        "Reply task stopped. Message retained; no automatic retry.".into();
                    self.sidekick.pending = None;
                    self.sidekick.automatic_running = false;
                    self.sidekick.external_request = None;
                    self.sidekick.plot_request = None;
                    self.sidekick.episode_request = None;
                    self.sidekick.generation_request = None;
                    self.sidekick.publishing_request = None;
                    self.sidekick.episode_creation_request = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.sidekick.pending.is_none() && self.sidekick.cast_queue.is_some() {
            if let Err(error) = self.start_next_cast_task() {
                self.sidekick.cast_queue = None;
                self.sidekick.error =
                    format!("Cast queue stopped: {error}. Completed characters are retained.");
            }
            ctx.request_repaint();
        }
        if !self.sidekick_visible {
            if self.sidekick.pending.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        }
        let mut stop_requested = false;
        eframe::egui::SidePanel::right("series-sidekick").resizable(true).default_width(350.).width_range(290. ..=520.)
        .frame(eframe::egui::Frame::new().fill(eframe::egui::Color32::from_rgb(21,23,27)).inner_margin(18.)).show(ctx,|ui|{
            ui.add_space(2.);ui.heading("Creative sidekick");ui.label(eframe::egui::RichText::new("Your story, in good company.").small().color(MUTED));
            let mut external=self.sidekick.external;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut external,false,"Sidekick chat");
                ui.selectable_value(&mut external,true,"MCP chat");
            });
            if external!=self.sidekick.external {self.sidekick.switch_mode(external);}
            if self.sidekick.external {
                let mut automatic = !self.sidekick.manual_external;
                if ui.checkbox(&mut automatic,"Automatic Codex answers").changed() {
                    self.sidekick.stop(); self.sidekick.error.clear();
                    self.sidekick.manual_external = !automatic;
                }
                ui.small(if automatic {"Uses your Codex ChatGPT login and this series chat."} else {"Waiting for a connected MCP client to answer."});
            }
            let Some(series)=self.selected.clone() else{ui.label("Choose a series to begin brainstorming.");return;};
            let context=match self.page{Page::Publish=>"Publish",Page::Synopsis=>"Synopsis",Page::Cast=>"Characters",Page::Locations=>"Locations",Page::Story=>"Overall plot",Page::Series=>"Episode plot",Page::Queue|Page::Scenes=>"Episode generation",_=>"Studio workspace"}.to_owned();
            ui.label(eframe::egui::RichText::new(format!("Working on: {context}")).color(GOLD));
            if !self.sidekick.external { match crate::sidekick_provider::Connection::load() {
                Ok(mut connection) => {
                    let previous = connection.provider.clone();
                    ui.add_enabled_ui(!self.sidekick.external && self.sidekick.pending.is_none() && self.sidekick.cast_queue.is_none(), |ui| {
                        eframe::egui::ComboBox::from_id_salt("sidekick-picker")
                            .selected_text(match connection.provider {
                                crate::sidekick_provider::Provider::OpenAi => format!("{} · OpenAI", connection.openai_model),
                                crate::sidekick_provider::Provider::OpenRouter => "Gemini 4 Argon · OpenRouter".into(),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut connection.provider, crate::sidekick_provider::Provider::OpenAi, format!("{} · OpenAI", connection.openai_model));
                                ui.selectable_value(&mut connection.provider, crate::sidekick_provider::Provider::OpenRouter, "Gemini 4 Argon · OpenRouter");
                            });
                    });
                    if connection.provider != previous {
                        match connection.save() {
                            Ok(()) => {
                                self.provider_settings.sidekick_connection = None;
                                self.sidekick.error.clear();
                            }
                            Err(error) => self.sidekick.error = error.to_string(),
                        }
                    }
                }
                Err(_) => { ui.small("Check Sidekick connection in Settings"); }
            }}
            if let Some(queue) = &self.sidekick.cast_queue && let Some(task) = queue.remaining.front() { ui.label(format!("Cast queue · {} of {} · {}", queue.completed + 1, queue.total, task.name)); }
            ui.separator();
            let messages=match self.store.sidekick_messages(&series){Ok(messages)=>messages,Err(error)=>{ui.label(error.to_string());return;}};
            // Size the footer from its actual content and anchor it to the bottom.
            // The history receives all remaining space, without a fixed reservation.
            eframe::egui::TopBottomPanel::bottom(eframe::egui::Id::new(("sidekick-footer", &series)))
                .resizable(false).default_height(90.).min_height(0.)
                .show_separator_line(false).frame(eframe::egui::Frame::NONE)
                .show_inside(ui, |ui| {
            if !self.sidekick.error.is_empty(){ui.label(&self.sidekick.error);}
            if !self.synopsis_drafts.contains_key(&series) && let Ok(saved) = self.store.series() && let Some(saved) = saved.into_iter().find(|item| item.id == series) {
                self.synopsis_drafts.insert(series.clone(), crate::synopsis::SynopsisDraft { id: series.clone(), title: saved.name.clone(), premise: saved.premise.clone(), expected_title: saved.name, expected_premise: saved.premise });
            }
            let character_snapshots=self.character_drafts(&series).unwrap_or_default();
            let generation_snapshot=self.generation_snapshot().ok();
            let publishing_snapshot=self.episode_id.as_deref().and_then(|episode|self.publisher.snapshot(&self.store,episode).ok());
            let episode_snapshot=self.episode_snapshot().ok();
            let project_context = self.live_context().to_string();
            if !self.sidekick.drafts.contains_key(&series) {
                match self.store.sidekick_draft(&series) {
                    Ok(text) => {
                        self.sidekick.saved_drafts.insert(series.clone(),text.clone());
                        self.sidekick.drafts.insert(series.clone(),text);
                    },
                    Err(error) => {ui.label(format!("Could not restore message draft: {error}"));return;}
                }
            }
            let draft=self.sidekick.drafts.get_mut(&series).expect("loaded composer");
            let composer_id=ui.make_persistent_id(("sidekick-composer",&series));
            if !self.sidekick.image_drafts.contains_key(&series) {
                match self.store.chat_image_draft(&series) {
                    Ok(images) => { self.sidekick.image_drafts.insert(series.clone(),(images.clone(),images)); }
                    Err(error) => { ui.label(format!("Could not restore attached images: {error}")); return; }
                }
            }
            let (images,saved_images)=self.sidekick.image_drafts.get_mut(&series).expect("loaded image draft");
            let paste_requested = ui.memory(|memory|memory.has_focus(composer_id))
                && ui.input(|input|input.events.iter().any(|event|matches!(event,eframe::egui::Event::Paste(_))));
            if paste_requested {
                match crate::chat_images::ChatImage::clipboard() {
                    Ok(image) => {
                        // A clipboard may contain both an image and a URL/filename. Attach it once,
                        // and don't also insert that fallback text into the message.
                        ui.input_mut(|input|input.events.retain(|event|!matches!(event,eframe::egui::Event::Paste(_))));
                        if images.len() >= 4 {self.sidekick.error="Attach up to four images per message. Remove one before pasting another.".into();}
                        else {images.push(image);self.sidekick.error.clear();}
                    },
                    Err(error) => {
                        let text_paste=ui.input(|input|input.events.iter().any(|event|matches!(event,eframe::egui::Event::Paste(text) if !text.is_empty())));
                        if !text_paste {self.sidekick.error=error.to_string();}
                    }
                }
            }
            let mut remove=None;
            ui.horizontal_wrapped(|ui| {
                for (index,image) in images.iter().enumerate() {
                    ui.vertical(|ui| { image.thumbnail(ui); if ui.small_button("Remove").clicked() {remove=Some(index);} });
                }
            });
            if let Some(index)=remove {images.remove(index);}
            let mut images_saved=true;
            if images != saved_images {
                match self.store.save_chat_image_draft(&series,saved_images,images) {
                    Ok(()) => *saved_images=images.clone(),
                    Err(error) => {images_saved=false;ui.label(error.to_string());},
                }
            }
            let send_with_enter=ui.memory(|memory|memory.has_focus(composer_id))&&ui.input_mut(|input|{
                let send=input.events.iter().any(|event|matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::Enter,pressed:true,repeat:false,modifiers,..} if *modifiers==eframe::egui::Modifiers::NONE));
                input.events.retain(|event|!matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::Enter,pressed:true,modifiers,..} if *modifiers==eframe::egui::Modifiers::NONE));
                send
            });
            ui.spacing_mut().item_spacing.y = 6.;
            eframe::egui::ScrollArea::vertical().id_salt(("sidekick-input", &series)).max_height(90.).show(ui, |ui| {
                // Keep the focus outline inside the scroll area's clipping rectangle.
                eframe::egui::Frame::NONE.inner_margin(3).show(ui, |ui| {
                    let mut output = eframe::egui::TextEdit::multiline(draft).margin(eframe::egui::vec2(10.,8.)).id(composer_id).return_key(eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::SHIFT,eframe::egui::Key::Enter)).desired_rows(2).desired_width(ui.available_width()).hint_text(crate::theme::placeholder("Discuss your series…")).show(ui);
                    if crate::context_menus::text_menu(&mut output, draft, true) {
                        match crate::chat_images::ChatImage::clipboard() {
                            Ok(image) if images.len() < 4 => { images.push(image); self.sidekick.error.clear(); }
                            Ok(_) => self.sidekick.error = "Attach up to four images per message.".into(),
                            Err(error) => self.sidekick.error = error.to_string(),
                        }
                        ui.ctx().request_repaint();
                    }
                    output.response.on_hover_text("Enter to send · Shift+Enter for a new line");
                });
            });
            let baseline=self.sidekick.saved_drafts.get_mut(&series).expect("loaded baseline");
            let mut draft_saved=true;
            if draft != baseline {
                match self.store.save_sidekick_draft(&series,baseline,draft) {
                    Ok(())=>*baseline=draft.clone(),
                    Err(error)=>{draft_saved=false;ui.label(format!("Draft not saved: {error}"));}
                }
            }
            let can_send=draft_saved&&images_saved&&self.sidekick.pending.is_none()&&(!draft.trim().is_empty()||!images.is_empty());
            let mut send_clicked = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding = eframe::egui::vec2(10., 5.);
                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    if self.sidekick.pending.is_some() {
                        stop_requested = ui.button("Stop").on_hover_text("Stop waiting and discard this reply. Provider billing may continue.").clicked();
                    } else {
                        send_clicked = ui.add_enabled(can_send, crate::theme::primary("Send")).on_hover_text("Enter to send").clicked();
                    }
                    if self.sidekick.pending.as_ref().is_some_and(|(id,_,_,_,_)| id == &series) {
                        let stage = if self.sidekick.automatic_running {"Codex is responding"} else if self.sidekick.external {"Waiting for MCP Sidekick"} else {match self.sidekick.progress.load(std::sync::atomic::Ordering::Relaxed) {
                            1 => "Authorizing", 2 => "Waiting for reply", 3 => "Awaiting full reply", _ => "Preparing",
                        }};
                        let elapsed = self.sidekick.request_started.map(|started| started.elapsed().as_secs()).unwrap_or(0);
                        ui.label(eframe::egui::RichText::new(format!("{stage} · {}:{:02}{}", elapsed / 60, elapsed % 60, if self.sidekick.external {""} else {" / 10:00"})).small().color(MUTED));
                        ui.add(eframe::egui::Spinner::new().size(14.));
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                    } else {
                        ui.label(eframe::egui::RichText::new(if draft_saved {"Saved"} else {"Not saved"}).small().color(MUTED)).on_hover_text("Message draft saved locally · Shift+Enter for a new line");
                        if messages.last().is_some_and(|message| message.role == "assistant") && draft.trim().is_empty()
                            && ui.small_button("Continue reply").on_hover_text("Prepare a continuation, then press Send").clicked() {
                            *draft = "Continue your previous reply from where it ended. Complete any unfinished sentences and promised list items without repeating the introduction. Do not create or modify characters or other documents; only finish the explanation.".into();
                        }
                    }
                });
            });
            if can_send&&(send_clicked||send_with_enter){
                let message=Message{id:uuid::Uuid::new_v4().to_string(),role:"user".into(),text:if draft.trim().is_empty(){"Please look at the attached image(s).".into()}else{draft.clone()},context:context.clone(),images:images.clone()};
                match self.store.append_sidekick_message(&series,&message){Ok(())=>{draft.clear();self.sidekick.error.clear();
                    match self.store.save_chat_image_draft(&series,saved_images,&[]) {
                        Ok(()) => {images.clear();saved_images.clear();},
                        Err(error) => self.sidekick.error=format!("Message sent; image draft cleanup failed: {error}"),
                    }
                    match self.store.save_sidekick_draft(&series,baseline,"") {
                        Ok(())=>baseline.clear(),Err(error)=>self.sidekick.error=format!("Message retained in chat, but composer cleanup failed: {error}"),
                    }
                    match self.store.sidekick_messages(&series){Ok(messages)=>{let (sender,receiver)=std::sync::mpsc::channel();
                        self.sidekick.episode_creation_request=self.story_drafts.get(&series).cloned().or_else(||self.store.series_plan(&series).ok()).map(|plan|(message.id.clone(),series.clone(),plan));
                        self.sidekick.generation_request=generation_snapshot;
                        self.sidekick.publishing_request=publishing_snapshot;
                        self.sidekick.episode_request=episode_snapshot;
                        self.sidekick.plot_request = self.story_drafts.get(&series).cloned().or_else(||self.store.series_plan(&series).ok()).map(|plan|(series.clone(),plan));
                        self.sidekick.request_context=Some(project_context.clone());
                        self.sidekick.request_started=Some(std::time::Instant::now());
                        self.sidekick.progress=Default::default();
                        self.sidekick.canceled=Default::default();
                        let canceled=self.sidekick.canceled.clone();
                        let progress=self.sidekick.progress.clone();
                        self.sidekick.pending=Some((series.clone(),context.clone(),receiver,self.synopsis_drafts.get(&series).cloned(),character_snapshots));
                        if self.sidekick.external && !self.sidekick.manual_external {
                            self.sidekick.automatic_running=true;
                            self.sidekick.external_request=Some(message.id.clone());
                            std::thread::spawn(move||{let _=sender.send(crate::codex_sidekick::reply(&messages,&project_context,&canceled).map_err(|error|error.to_string()));});
                        } else if self.sidekick.external {
                            self.sidekick.external_request=Some(message.id.clone());
                            self.sidekick.external_sender=Some(sender);
                        } else { std::thread::spawn(move||{let _=sender.send(crate::sidekick::reply(&messages,&project_context,"google/gemini-4-argon",&progress,&canceled).map_err(|error|error.to_string()));}); }},Err(error)=>self.sidekick.error=error.to_string()}},Err(error)=>self.sidekick.error=error.to_string()}
            }
            });
            eframe::egui::ScrollArea::vertical().id_salt(("sidekick-history",&series)).stick_to_bottom(true).horizontal_scroll_offset(0.).auto_shrink([false, false]).max_height(ui.available_height()).show_viewport(ui,|ui, viewport|{
                ui.set_width(viewport.width());
                let copy_selection = crate::context_menus::history_copy_menu(ui);
                if messages.is_empty(){ui.label("Tell me the kind of show you want to make, or an idea you want to explore.");}
                for message in &messages {
                    if let Some(action)=message_bubble(ui,message,&mut self.sidekick.markdown,&mut self.sidekick.editing_message,self.sidekick.pending.is_none(), viewport.width()) {
                        match action {

                            MessageAction::Edit(message)=>self.sidekick.editing_message=Some((message.clone(),message.text.clone())),
                            MessageAction::Cancel=>self.sidekick.editing_message=None,
                            MessageAction::Save(original,text)=>match self.store.change_user_message(&series,&original,Some(&text)) {
                                Ok(())=>{self.sidekick.editing_message=None;self.sidekick.error.clear();},Err(error)=>self.sidekick.error=error.to_string()
                            },
                            MessageAction::Delete(original)=>match self.store.change_user_message(&series,&original,None) {
                                Ok(())=>{self.sidekick.editing_message=None;self.sidekick.error.clear();},Err(error)=>self.sidekick.error=error.to_string()
                            }
                        }
                    }
                }
                copy_selection.finish(ui);
            });
        });
        if stop_requested {
            self.sidekick.stop();
        }
    }
}

impl Studio {
    fn start_next_cast_task(&mut self) -> anyhow::Result<()> {
        let queue = self.sidekick.cast_queue.as_ref().expect("cast queue");
        let task = queue
            .remaining
            .front()
            .expect("nonempty cast queue")
            .clone();
        let series = queue.series.clone();
        let mut context = queue.context.clone();
        let characters = self.character_drafts(&series)?;
        if characters.iter().any(|character| {
            character
                .brief
                .name
                .trim()
                .eq_ignore_ascii_case(task.name.trim())
        }) {
            anyhow::bail!(
                "{} already exists; review the cast before requesting the remaining characters",
                task.name
            );
        }
        context["workspace"] = "Characters".into();
        context["character_drafts"] = serde_json::json!(
            characters
                .iter()
                .map(crate::character_edit::CharacterDraft::model_context)
                .collect::<Vec<_>>()
        );
        let mut messages = self.store.sidekick_messages(&series)?;
        messages.push(Message { id: uuid::Uuid::new_v4().to_string(), role: "user".into(), context: "Characters".into(), images: vec![], text: format!("Continue the authorized cast queue. Create ONLY this character now: {}. Story brief: {}. Return exactly one new_characters item with a complete profile. Do not replan the queue, edit existing characters, or generate a portrait. The app will advance to the next character automatically.", task.name, task.brief) });
        let (sender, receiver) = std::sync::mpsc::channel();
        self.sidekick.progress = Default::default();
        self.sidekick.canceled = Default::default();
        self.sidekick.request_started = Some(std::time::Instant::now());
        let progress = self.sidekick.progress.clone();
        let canceled = self.sidekick.canceled.clone();
        self.sidekick.pending = Some((series, "Characters".into(), receiver, None, characters));
        std::thread::spawn(move || {
            let _ = sender.send(
                crate::sidekick::reply(
                    &messages,
                    &context.to_string(),
                    "google/gemini-4-argon",
                    &progress,
                    &canceled,
                )
                .map_err(|error| error.to_string()),
            );
        });
        Ok(())
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    #[test]
    fn stopped_reply_cannot_arrive_or_poison_a_new_request() {
        let mut sidekick = Sidekick::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        sidekick.pending = Some(("series".into(), "Synopsis".into(), receiver, None, vec![]));
        let canceled = sidekick.canceled.clone();
        sidekick.cast_queue = Some(crate::cast_queue::Queue {
            series: "series".into(),
            context: serde_json::json!({}),
            remaining: vec![crate::cast_queue::Task {
                name: "Lead".into(),
                brief: "Healer".into(),
            }]
            .into(),
            total: 1,
            completed: 0,
        });
        sidekick.stop();
        assert!(sidekick.cast_queue.is_none());
        assert!(canceled.load(std::sync::atomic::Ordering::Relaxed));
        assert!(sender.send(Err("late result".into())).is_err());
        assert!(sidekick.pending.is_none());
        sidekick.canceled = Default::default();
        assert!(!sidekick.canceled.load(std::sync::atomic::Ordering::Relaxed));
        assert!(canceled.load(std::sync::atomic::Ordering::Relaxed));
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn conversation_bubbles_reflow_inside_the_resized_history() {
        let ctx = egui::Context::default();
        let mut cache = egui_commonmark::CommonMarkCache::default();
        for width in [520., 290., 410.] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show_viewport(ui, |ui, viewport| {
                            for role in ["assistant", "user"] {
                                let message = Message {
                                    id: role.into(),
                                    role: role.into(),
                                    context: String::new(),
                                    text: format!(
                                        "Retained response\n\n`{}`",
                                        "a1b2-c3d4".repeat(4096)
                                    ),
                                    images: vec![],
                                };
                                let row = ui.available_rect_before_wrap();
                                let id =
                                    ui.make_persistent_id(("sidekick-bubble-bounds", &message.id));
                                message_bubble(
                                    ui,
                                    &message,
                                    &mut cache,
                                    &mut None,
                                    true,
                                    viewport.width(),
                                );
                                let bubble = ui.data(|d| d.get_temp::<egui::Rect>(id)).unwrap();
                                assert!(
                                    bubble.left() >= row.left() - 1.
                                        && bubble.right() <= row.right() + 1.,
                                    "{role}: {bubble:?} outside {row:?}"
                                );
                            }
                        });
                });
            });
        }
    }
}

impl Studio {
    pub(crate) fn poll_live_control(&mut self, ctx: &eframe::egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if self
            .sidekick
            .live_tick
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(500))
        {
            return;
        }
        self.sidekick.live_tick = Some(std::time::Instant::now());
        if self.sidekick.live_instance.is_empty() {
            self.sidekick.live_instance = uuid::Uuid::new_v4().to_string();
        }
        let result = (|| -> anyhow::Result<()> {
            let c = crate::live_control::open(&self.store.path)?;
            let commands:Vec<(String,String,String)>=c.prepare("SELECT id,action,payload FROM commands WHERE instance=?1 AND result IS NULL AND created>=unixepoch()-30 ORDER BY rowid LIMIT 8")?.query_map([&self.sidekick.live_instance],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
            for (id, action, raw) in commands {
                // Mark before execution: a crashed GUI must never replay a command.
                c.execute(
                    "UPDATE commands SET result=?2 WHERE id=?1 AND result IS NULL",
                    rusqlite::params![
                        id,
                        serde_json::json!({"status":"interrupted_or_running","request_id":id})
                            .to_string()
                    ],
                )?;
                let result = self.execute_live_action(&action, &serde_json::from_str(&raw)?);
                let receipt = match result {
                    Ok(value) => {
                        serde_json::json!({"request_id":id,"status":"completed","result":value})
                    }
                    Err(error) => {
                        serde_json::json!({"request_id":id,"status":"failed","error":error.to_string()})
                    }
                };
                c.execute(
                    "UPDATE commands SET result=?2 WHERE id=?1",
                    rusqlite::params![id, receipt.to_string()],
                )?;
            }
            let data = serde_json::json!({"instance":self.sidekick.live_instance,"database":self.store.path,"series_id":self.selected,"episode_id":self.episode_id,"page":self.page,"external_sidekick":self.sidekick.external,"automatic_answers":!self.sidekick.manual_external,"automatic_running":self.sidekick.automatic_running,"busy":self.sidekick.busy(),"request_id":self.sidekick.external_request,"request_context":self.sidekick.request_context.as_deref().and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok()),"workspace":self.live_context(),"messages":self.selected.as_deref().map(|id|self.store.sidekick_messages(id)).transpose()?,"last_error":self.sidekick.error});
            c.execute("INSERT INTO live_state VALUES(1,?1,unixepoch(),?2) ON CONFLICT(id) DO UPDATE SET instance=excluded.instance,touched=excluded.touched,data=excluded.data",rusqlite::params![self.sidekick.live_instance,data.to_string()])?;
            Ok(())
        })();
        if let Err(error) = result {
            self.message = format!("MCP connection: {error}");
        }
    }
    fn execute_live_action(
        &mut self,
        action: &str,
        payload: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        use anyhow::{Context, ensure};
        use serde_json::json;
        if let Some(request) = payload["_automatic_request"].as_str() {
            ensure!(
                self.sidekick.automatic_running
                    && self.sidekick.external_request.as_deref() == Some(request)
                    && !self
                        .sidekick
                        .canceled
                        .load(std::sync::atomic::Ordering::Relaxed),
                "Automatic request was stopped or replaced; action not applied"
            );
        }
        match action {
            "read_episode_batch" => self.episode_batch_snapshot(
                payload["series_id"]
                    .as_str()
                    .context("series_id required")?,
            ),
            "apply_episode_batch" => self.apply_episode_batch(payload),
            "take_over" => {
                ensure!(
                    !self.sidekick.busy(),
                    "Sidekick is replying. Wait for it or click Stop before taking over."
                );
                self.sidekick.switch_mode(true);
                self.sidekick_visible = true;
                self.sidekick.manual_external = true;
                Ok(json!({"external_sidekick":true}))
            }
            "release" => {
                self.sidekick.stop();
                self.sidekick.switch_mode(false);
                self.sidekick.error.clear();
                Ok(json!({"external_sidekick":false}))
            }
            "save_drafts" => {
                ensure!(
                    payload["expected_workspace"] == self.live_context(),
                    "Workspace changed; read it again before saving"
                );
                let series = self.selected.clone().context("Select a series first")?;
                let issues = self.save_project_inner(&series)?;
                Ok(
                    json!({"saved":issues.is_empty(),"issues":issues,"scope":"local working copy; portable folder is not published"}),
                )
            }
            "undo" => {
                let series = self.selected.clone().context("Select a series first")?;
                let target = payload["target"].as_str().context(
                    "Specify synopsis, plot, episode, generation, character or publishing",
                )?;
                match target {
                    "synopsis" => {
                        let (before, after) = self
                            .sidekick
                            .synopsis_undo
                            .get(&series)
                            .context("No synopsis edit to undo")?
                            .clone();
                        crate::sidekick_edit::undo(
                            self.synopsis_drafts
                                .get_mut(&series)
                                .context("No synopsis draft")?,
                            &before,
                            &after,
                        )?;
                        self.sidekick.synopsis_undo.remove(&series);
                    }
                    "plot" => {
                        let (before, after) = self
                            .sidekick
                            .plot_undo
                            .get(&series)
                            .context("No plot edit to undo")?
                            .clone();
                        let current = self.story_drafts.get(&series).context("No plot draft")?;
                        let restored = crate::sidekick_plot::undo(current, &before, &after)?;
                        self.story_drafts.insert(series.clone(), restored);
                        self.sidekick.plot_undo.remove(&series);
                    }
                    "episode" => {
                        let current = self.episode_snapshot()?;
                        let (before, after) = self
                            .sidekick
                            .episode_undo
                            .get(&current.episode_id)
                            .context("No episode edit to undo")?
                            .clone();
                        let restored = crate::sidekick_episode::undo(&current, &before, &after)?;
                        self.sidekick.episode_undo.remove(&current.episode_id);
                        self.put_episode_draft(restored);
                    }
                    "generation" => {
                        let current = self.generation_snapshot()?;
                        let id = current.plan.episode_id.clone();
                        let (before, after) = self
                            .sidekick
                            .generation_undo
                            .get(&id)
                            .context("No generation edit to undo")?
                            .clone();
                        ensure!(
                            crate::sidekick_generation::same(&current, &after),
                            "Generation draft changed; cannot undo over newer work"
                        );
                        self.video_planner.drafts.insert(id.clone(), before.plan);
                        self.sidekick.generation_undo.remove(&id);
                    }
                    "character" => self.undo_character_sidekick(
                        &series,
                        payload["character_id"]
                            .as_str()
                            .context("character_id required")?,
                    )?,
                    "publishing" => self.publisher.undo_sidekick(
                        &self.store,
                        self.episode_id.as_deref().context("Select an episode")?,
                    )?,
                    _ => anyhow::bail!("Unknown undo target"),
                }
                Ok(json!({"undone":true,"saved":false}))
            }
            "edit_timeline" => {
                self.editor.apply_live_cut(
                    &self.store,
                    &payload["expected"],
                    serde_json::from_value(payload["cut"].clone())?,
                )?;
                Ok(json!({"updated":true,"saved":false,"undo":true}))
            }
            "navigate" => {
                if let Some(series) = payload["series_id"].as_str()
                    && self.selected.as_deref() != Some(series)
                {
                    ensure!(
                        self.preserve_series_work(),
                        "Could not preserve current work: {}",
                        self.message
                    );
                    ensure!(
                        self.activate_series(series),
                        "Could not select series: {}",
                        self.message
                    );
                }
                let page: crate::Page = serde_json::from_value(payload["page"].clone())?;
                if let Some(id) = payload["episode_id"].as_str() {
                    ensure!(
                        self.select_episode(id),
                        "Could not change episode: {}",
                        self.message
                    );
                }
                self.page = page;
                self.sidekick_visible = true;
                Ok(json!({"page":page,"episode_id":self.episode_id}))
            }
            "capture" => {
                ensure!(self.sidekick.external, "Take over Sidekick first");
                ensure!(
                    !self.sidekick.busy(),
                    "A reply is already pending; answer its request_id first"
                );
                let series = self.selected.clone().context("Select a series first")?;
                let id = uuid::Uuid::new_v4().to_string();
                if !self.synopsis_drafts.contains_key(&series) {
                    let saved = self
                        .store
                        .series()?
                        .into_iter()
                        .find(|s| s.id == series)
                        .context("Series not found")?;
                    self.synopsis_drafts.insert(
                        series.clone(),
                        crate::synopsis::SynopsisDraft {
                            id: series.clone(),
                            title: saved.name.clone(),
                            premise: saved.premise.clone(),
                            expected_title: saved.name,
                            expected_premise: saved.premise,
                        },
                    );
                }
                let context = self.live_context().to_string();
                let characters = self.character_drafts(&series)?;
                self.sidekick.generation_request = self.generation_snapshot().ok();
                self.sidekick.publishing_request = self
                    .episode_id
                    .as_deref()
                    .and_then(|e| self.publisher.snapshot(&self.store, e).ok());
                self.sidekick.episode_request = self.episode_snapshot().ok();
                let plan = self
                    .story_drafts
                    .get(&series)
                    .cloned()
                    .unwrap_or(self.store.series_plan(&series)?);
                self.sidekick.plot_request = Some((series.clone(), plan.clone()));
                self.sidekick.episode_creation_request = Some((id.clone(), series.clone(), plan));
                let (sender, receiver) = std::sync::mpsc::channel();
                self.sidekick.pending = Some((
                    series.clone(),
                    format!("{:?}", self.page),
                    receiver,
                    self.synopsis_drafts.get(&series).cloned(),
                    characters,
                ));
                self.sidekick.request_context = Some(context.clone());
                self.sidekick.external_sender = Some(sender);
                self.sidekick.external_request = Some(id.clone());
                self.sidekick.request_started = Some(std::time::Instant::now());
                self.sidekick.error.clear();
                Ok(
                    json!({"request_id":id,"context":serde_json::from_str::<serde_json::Value>(&context)?}),
                )
            }
            "reply" => {
                ensure!(self.sidekick.external, "MCP Sidekick is no longer active");
                ensure!(
                    payload["request_id"].as_str() == self.sidekick.external_request.as_deref()
                        && self.sidekick.external_request.is_some(),
                    "Reply belongs to an old or stopped request"
                );
                let reply = crate::sidekick_edit::parse_reply(&payload["reply"].to_string())?;
                ensure!(
                    reply.cast_queue.is_none(),
                    "MCP authors each character directly; cast queue would call another model"
                );
                ensure!(
                    !reply
                        .new_character
                        .as_ref()
                        .is_some_and(|c| c.generate_portrait)
                        && !reply
                            .new_characters
                            .as_ref()
                            .is_some_and(|cs| cs.iter().any(|c| c.generate_portrait)),
                    "Use explicit generation tools after spending approval; replies only edit drafts"
                );
                self.sidekick
                    .external_sender
                    .take()
                    .context("Request already answered")?
                    .send(Ok(reply))
                    .map_err(|_| anyhow::anyhow!("Reply was stopped"))?;
                self.sidekick.external_request = None;
                Ok(
                    json!({"delivery":"accepted","note":"Read live messages for per-edit application or conflict results. Draft changes are not automatically saved."}),
                )
            }
            _ => anyhow::bail!("Unknown live action"),
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (tempfile::TempDir, Studio, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("Original", "Original premise").unwrap();
        store.remember_gui_series(&series.id).unwrap();
        (dir, Studio::from_store(store), series.id)
    }
    fn render(app: &mut Studio) {
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(eframe::egui::RawInput::default(), |ctx| {
            app.sidekick_panel(ctx)
        });
    }
    #[test]
    fn external_reply_edits_shared_draft_and_undo_restores_it() {
        let (_dir, mut app, series) = fixture();
        app.execute_live_action("take_over", &json!({})).unwrap();
        let captured = app.execute_live_action("capture", &json!({})).unwrap();
        app.execute_live_action("reply",&json!({"request_id":captured["request_id"],"reply":{"message":"Updated the premise.","synopsis":{"title":"Revised","premise":"New premise"}}})).unwrap();
        render(&mut app);
        assert_eq!(app.synopsis_drafts[&series].title, "Revised");
        assert_eq!(app.store.series().unwrap()[0].name, "Original");
        assert!(
            app.store
                .sidekick_messages(&series)
                .unwrap()
                .last()
                .unwrap()
                .text
                .contains("Synopsis draft updated")
        );
        app.execute_live_action("undo", &json!({"target":"synopsis"}))
            .unwrap();
        assert_eq!(app.synopsis_drafts[&series].title, "Original");
    }
    #[test]
    fn external_reply_never_overwrites_newer_draft_or_survives_release() {
        let (_dir, mut app, series) = fixture();
        app.execute_live_action("take_over", &json!({})).unwrap();
        let captured = app.execute_live_action("capture", &json!({})).unwrap();
        app.synopsis_drafts.get_mut(&series).unwrap().premise = "User typing".into();
        let reply = json!({"request_id":captured["request_id"],"reply":{"message":"Proposed change","synopsis":{"title":"Revised","premise":"Model typing"}}});
        app.execute_live_action("reply", &reply).unwrap();
        assert!(app.execute_live_action("reply", &reply).is_err());
        render(&mut app);
        assert_eq!(app.synopsis_drafts[&series].premise, "User typing");
        assert!(
            app.store
                .sidekick_messages(&series)
                .unwrap()
                .last()
                .unwrap()
                .text
                .contains("retained")
        );
        app.execute_live_action("capture", &json!({})).unwrap();
        app.execute_live_action("release", &json!({})).unwrap();
        assert!(app.execute_live_action("reply", &reply).is_err());
        assert!(!app.sidekick.busy());
    }
    #[test]
    fn live_mailbox_reaches_gui_and_restart_invalidates_instance() {
        let (_dir, mut app, _series) = fixture();
        let ctx = eframe::egui::Context::default();
        app.poll_live_control(&ctx);
        let state = crate::live_control::state(&app.store.path).unwrap();
        crate::live_control::submit(
            &app.store.path,
            "one",
            state["instance"].as_str().unwrap(),
            "take_over",
            &json!({}),
        )
        .unwrap();
        app.sidekick.live_tick = None;
        app.poll_live_control(&ctx);
        assert!(app.sidekick.external);
        assert_eq!(
            crate::live_control::status(&app.store.path, "one").unwrap()["status"],
            "completed"
        );
        let mut restarted =
            Studio::from_store(crate::store::Store::open(app.store.path.clone()).unwrap());
        restarted.poll_live_control(&ctx);
        assert!(
            crate::live_control::submit(
                &app.store.path,
                "two",
                state["instance"].as_str().unwrap(),
                "take_over",
                &json!({})
            )
            .is_err()
        );
    }
}

impl Drop for Sidekick {
    fn drop(&mut self) {
        self.canceled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod automatic_batch_stop_tests {
    use super::*;
    #[test]
    fn stopped_automatic_request_cannot_apply_queued_commands() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::Store::open(directory.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("Test", "Premise").unwrap();
        let mut app = Studio::from_store(store);
        app.sidekick.automatic_running = true;
        app.sidekick.external_request = Some("request-one".into());
        let payload = serde_json::json!({"series_id":series.id,"_automatic_request":"request-one"});
        assert!(
            app.execute_live_action("read_episode_batch", &payload)
                .is_ok()
        );
        app.sidekick.stop();
        assert!(
            app.execute_live_action("read_episode_batch", &payload)
                .is_err()
        );
        app.sidekick.automatic_running = true;
        app.sidekick.external_request = Some("request-two".into());
        app.sidekick.canceled = Default::default();
        assert!(
            app.execute_live_action("read_episode_batch", &payload)
                .is_err()
        );
    }
}
