use crate::{GOLD, MUTED, Page, Studio, sidekick::Message};
enum MessageAction {
    Edit(Message),
    Save(Message, String),
    Cancel,
    Delete(Message),
}
fn message_bubble(
    ui: &mut eframe::egui::Ui,
    message: &Message,
    markdown: &mut egui_commonmark::CommonMarkCache,
    editing: &mut Option<(Message, String)>,
    enabled: bool,
) -> Option<MessageAction> {
    let mut action = None;
    use eframe::egui::{Align, Color32, Frame, Layout, RichText, Stroke};
    let user = message.role == "user";
    let width = (ui.available_width() * 0.88).max(120.);
    let layout = if user {
        Layout::right_to_left(Align::TOP)
    } else {
        Layout::left_to_right(Align::TOP)
    };
    let bounds_id = ui.make_persistent_id(("sidekick-bubble-bounds", &message.id));
    let hover = ui
        .data(|data| data.get_temp::<eframe::egui::Rect>(bounds_id))
        .is_some_and(|rect| ui.rect_contains_pointer(rect));
    ui.with_layout(layout, |ui| {
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
                ui.set_max_width(width - 24.);
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.scope(|ui| {
                        // Header controls share the speaker row rather than reserving
                        // an invisible, full-height action row below every message.
                        ui.spacing_mut().interact_size.y =
                            ui.text_style_height(&eframe::egui::TextStyle::Small);
                        ui.spacing_mut().button_padding.x = 6.;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if user { "You" } else { "Sidekick" })
                                    .small()
                                    .color(if user { GOLD } else { MUTED }),
                            );
                            if user
                                && editing
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
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                eframe::egui::Button::new("Delete").small(),
                                            )
                                            .clicked()
                                        {
                                            action = Some(MessageAction::Delete(message.clone()));
                                        }
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                eframe::egui::Button::new("Edit").small(),
                                            )
                                            .clicked()
                                        {
                                            action = Some(MessageAction::Edit(message.clone()));
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
                                .desired_width(width - 24.)
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
                                action = Some(MessageAction::Save(original.clone(), text.clone()));
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
    });
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
    error: String,
    plot_request: Option<(String, crate::story::SeriesPlan)>,
    episode_request: Option<crate::sidekick_episode::EpisodeDraft>,
    generation_request: Option<crate::sidekick_generation::Snapshot>,
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
    pub(crate) fn reset_context(&mut self) {
        self.editing_message = None;
        self.error.clear();
        // Keep per-series drafts and in-flight replies attached to their origin.
    }
}
impl Studio {
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
                    match result {
                        Ok(reply) => {
                            let mut text = reply.message;
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
                                    self.video_planner.drafts.insert(id.clone(), plan);
                                    let after = self.generation_snapshot()?;
                                    self.sidekick.generation_undo.insert(id, (current, after));
                                    self.video_planner.section = 0;
                                    Ok(())
                                })();
                                match result {
                                    Ok(()) => text.push_str("\n\nVideo plan ready. Review the segments, then choose Generate video. Undo is available."),
                                    Err(error) => text.push_str(&format!("\n\n{error}\nGeneration proposal retained:\n{}", serde_json::to_string_pretty(&edit).unwrap_or_default())),
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
                                match result {Ok(())=>text.push_str("\n\nEpisode draft updated. Review the story beats and screenplay, then Save episode. Undo is available."),Err(error)=>text.push_str(&format!("\n\n{error}\nEpisode proposal retained:\n{}",serde_json::to_string_pretty(&edit).unwrap_or_default()))}
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
                            if let Some(proposal) = reply.new_character {
                                let id = uuid::Uuid::new_v4().to_string();
                                match self.store.create_sidekick_character(&series,&id,&proposal) {
                                    Ok(())=>{
                                        text.push_str("\n\nCharacter added to this series with its portrait prompt. Open Characters to review it.");
                                        if proposal.generate_portrait {
                                            match self.store.enqueue_grok_portrait(&id,&proposal.portrait_prompt) {
                                                Ok(job)=>{
                                                    text.push_str(&format!("\nPortrait generation queued: {}. Review the result before choosing it.",job.id));
                                                    if let Err(error)=crate::worker::spawn_detached(&self.store.path) {text.push_str(&format!("\nWorker could not start: {error}. The queued job is retained."));}
                                                },
                                                Err(error)=>text.push_str(&format!("\nCharacter saved, but portrait could not be queued: {error}")),
                                            }
                                        } else {text.push_str(" No image was generated.");}
                                    },
                                    Err(error)=>text.push_str(&format!("\n\nCharacter was not created: {error}\nProposal retained:\n{}",serde_json::to_string_pretty(&proposal).unwrap_or_default())),
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
                            }
                        }
                        Err(error) => self.sidekick.error = error,
                    };
                    self.sidekick.pending = None;
                    self.sidekick.plot_request = None;
                    self.sidekick.episode_request = None;
                    self.sidekick.generation_request = None;
                    self.sidekick.episode_creation_request = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.sidekick.error =
                        "Reply task stopped. Message retained; no automatic retry.".into();
                    self.sidekick.pending = None;
                    self.sidekick.plot_request = None;
                    self.sidekick.episode_request = None;
                    self.sidekick.generation_request = None;
                    self.sidekick.episode_creation_request = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.sidekick_visible {
            return;
        }
        eframe::egui::SidePanel::right("series-sidekick").resizable(true).default_width(350.).width_range(290. ..=520.)
        .frame(eframe::egui::Frame::new().fill(eframe::egui::Color32::from_rgb(21,23,27)).inner_margin(18.)).show(ctx,|ui|{
            ui.add_space(2.);ui.heading("Creative sidekick");ui.label(eframe::egui::RichText::new("Your story, in good company.").small().color(MUTED));
            let Some(series)=self.selected.clone() else{ui.label("Choose a series to begin brainstorming.");return;};
            let context=match self.page{Page::Synopsis=>"Synopsis",Page::Cast=>"Characters",Page::Locations=>"Locations",Page::Story=>"Overall plot",Page::Series=>"Episode plot",Page::Queue|Page::Scenes=>"Episode generation",_=>"Studio workspace"}.to_owned();
            ui.label(eframe::egui::RichText::new(format!("Working on: {context}")).color(GOLD));
            ui.small("Gemini 4 Argon · via OpenRouter");
            ui.separator();
            let messages=match self.store.sidekick_messages(&series){Ok(messages)=>messages,Err(error)=>{ui.label(error.to_string());return;}};
            // Size the footer from its actual content and anchor it to the bottom.
            // The history receives all remaining space, without a fixed reservation.
            eframe::egui::TopBottomPanel::bottom(eframe::egui::Id::new(("sidekick-footer", &series)))
                .resizable(false).default_height(90.).min_height(0.)
                .show_separator_line(false).frame(eframe::egui::Frame::NONE)
                .show_inside(ui, |ui| {
            if !self.sidekick.error.is_empty(){ui.label(&self.sidekick.error);}
            let character_snapshots=self.character_drafts(&series).unwrap_or_default();
            let generation_snapshot=if matches!(self.page,Page::Queue|Page::Scenes) {self.generation_snapshot().ok()}else{None};
            let episode_snapshot=if self.page==Page::Series{self.episode_snapshot().ok()}else{None};
            let project_context = serde_json::json!({
                "workspace":context,
                "generation_draft":generation_snapshot.as_ref().map(|snapshot|serde_json::json!({"plan":snapshot.plan,"saved_episode":snapshot.context["saved_episode"],"image_style":snapshot.context["image_style"],"available_portraits":snapshot.context["available_portraits"].as_array().map(|items|items.iter().map(|item|serde_json::json!({"character_id":item["character_id"],"name":item["name"]})).collect::<Vec<_>>())})),
                "character_drafts":character_snapshots.iter().map(crate::character_edit::CharacterDraft::model_context).collect::<Vec<_>>(),
                "series":self.store.series().ok().and_then(|items|items.into_iter().find(|item|item.id==series)),
                                "series_plot_draft":self.story_drafts.get(&series).cloned().or_else(||self.store.series_plan(&series).ok()),
                "available_episodes":self.store.episodes(&series).unwrap_or_default().iter().map(|episode|serde_json::json!({"id":episode.id,"title":episode.title})).collect::<Vec<_>>(),
                "locations":self.store.library(&series,"location").unwrap_or_default().iter().map(|entry|serde_json::json!({"id":entry.id,"name":entry.name,"description":entry.description})).collect::<Vec<_>>(),
                "episode_id":self.episode_id,
                "episode_draft":episode_snapshot.as_ref().map(|d|serde_json::json!({"episode_id":d.episode_id,"screenplay":d.screenplay,"outline":d.plan.episodes.iter().find(|o|o.episode_id==d.episode_id)})),
                "current_screenplay_draft":self.script,
                "synopsis_draft":self.synopsis_drafts.get(&series),
            }).to_string();
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
            // egui-winit consumes Cmd+V key-down for text paste; key-up still reaches us for image-only clipboards.
            let paste_shortcut=ui.memory(|memory|memory.has_focus(composer_id)) && ui.input(|input|input.events.iter().any(|event|matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::V,pressed:false,modifiers,..} if modifiers.command)));
            if paste_shortcut && images.len()<4 && let Ok(image) = crate::chat_images::ChatImage::clipboard() {
                images.push(image);
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
                    ui.add(eframe::egui::TextEdit::multiline(draft).margin(eframe::egui::vec2(10.,8.)).id(composer_id).return_key(eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::SHIFT,eframe::egui::Key::Enter)).desired_rows(2).desired_width(ui.available_width()).hint_text(crate::theme::placeholder("Discuss your series…")))
                        .on_hover_text("Enter to send · Shift+Enter for a new line");
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
                    send_clicked = ui.add_enabled(can_send, crate::theme::primary("Send")).on_hover_text("Enter to send").clicked();
                    if self.sidekick.pending.as_ref().is_some_and(|(id,_,_,_,_)| id == &series) {
                        let stage = match self.sidekick.progress.load(std::sync::atomic::Ordering::Relaxed) {
                            1 => "Authorizing", 2 => "Waiting for reply", 3 => "Awaiting full reply", _ => "Preparing",
                        };
                        let elapsed = self.sidekick.request_started.map(|started| started.elapsed().as_secs()).unwrap_or(0);
                        ui.label(eframe::egui::RichText::new(format!("{stage} · {elapsed}s")).small().color(MUTED));
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
                        self.sidekick.episode_creation_request=if matches!(self.page,Page::Series|Page::Story) {self.story_drafts.get(&series).cloned().or_else(||self.store.series_plan(&series).ok()).map(|plan|(message.id.clone(),series.clone(),plan))}else{None};
                        self.sidekick.generation_request=generation_snapshot;
                        self.sidekick.episode_request=episode_snapshot;
                        self.sidekick.plot_request = if self.page == Page::Story {
                            self.story_drafts.get(&series).cloned().or_else(||self.store.series_plan(&series).ok()).map(|plan|(series.clone(),plan))
                        } else { None };
                        self.sidekick.request_started=Some(std::time::Instant::now());
                        self.sidekick.progress.store(0,std::sync::atomic::Ordering::Relaxed);
                        let progress=self.sidekick.progress.clone();
                        self.sidekick.pending=Some((series.clone(),context.clone(),receiver,if self.page==Page::Synopsis{self.synopsis_drafts.get(&series).cloned()}else{None},character_snapshots));std::thread::spawn(move||{let _=sender.send(crate::sidekick::reply(&messages,&project_context,"google/gemini-4-argon",&progress).map_err(|error|error.to_string()));});},Err(error)=>self.sidekick.error=error.to_string()}},Err(error)=>self.sidekick.error=error.to_string()}
            }
            });
            eframe::egui::ScrollArea::vertical().id_salt(("sidekick-history",&series)).stick_to_bottom(true).auto_shrink([false, false]).max_height(ui.available_height()).show(ui,|ui|{
                if messages.is_empty(){ui.label("Tell me the kind of show you want to make, or an idea you want to explore.");}
                for message in &messages {
                    if let Some(action)=message_bubble(ui,message,&mut self.sidekick.markdown,&mut self.sidekick.editing_message,self.sidekick.pending.is_none()) {
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
            });
        });
    }
}
