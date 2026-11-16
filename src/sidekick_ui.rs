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
                Color32::from_rgb(77, 53, 31)
            } else {
                Color32::from_rgb(27, 32, 41)
            })
            .stroke(Stroke::new(
                1_f32,
                if user {
                    Color32::from_rgb(112, 78, 43)
                } else {
                    Color32::from_rgb(43, 49, 60)
                },
            ))
            .corner_radius(12)
            .inner_margin(12.)
            .show(ui, |ui| {
                ui.set_max_width(width - 24.);
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(if user { "You" } else { "Sidekick" })
                            .small()
                            .color(if user { GOLD } else { MUTED }),
                    );
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
                    } else {
                        ui.add(
                            eframe::egui::Label::new(
                                RichText::new(&message.text)
                                    .color(Color32::from_rgb(235, 237, 242)),
                            )
                            .wrap()
                            .selectable(true),
                        );
                        if user {
                            ui.scope_builder(
                                if hover {
                                    eframe::egui::UiBuilder::new()
                                } else {
                                    eframe::egui::UiBuilder::new().invisible()
                                },
                                |ui| {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                eframe::egui::Button::new("Edit").small(),
                                            )
                                            .clicked()
                                        {
                                            action = Some(MessageAction::Edit(message.clone()));
                                        }
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                eframe::egui::Button::new("Delete").small(),
                                            )
                                            .clicked()
                                        {
                                            action = Some(MessageAction::Delete(message.clone()));
                                        }
                                    });
                                },
                            );
                        }
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
    drafts: std::collections::HashMap<String, String>,
    saved_drafts: std::collections::HashMap<String, String>,
    editing_message: Option<(Message, String)>,
    progress: std::sync::Arc<std::sync::atomic::AtomicU8>,
    request_started: Option<std::time::Instant>,
    pending: Option<PendingReply>,
    error: String,
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
impl Studio {
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
                                match result {Ok(())=>text.push_str("\n\nCharacter brief and portrait prompt drafts updated. Save each when ready; Undo is available in the portrait section. No image was generated."),Err(error)=>text.push_str(&format!("\n\n{error}\nProposal retained:\n{}",serde_json::to_string_pretty(&edit).unwrap_or_default()))}
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
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.sidekick.error =
                        "Reply task stopped. Message retained; no automatic retry.".into();
                    self.sidekick.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        eframe::egui::SidePanel::right("series-sidekick").resizable(true).default_width(330.).width_range(270. ..=500.).show(ctx,|ui|{
            ui.add_space(12.);ui.heading("Creative sidekick");ui.small("One conversation across every workspace");
            let Some(series)=self.selected.clone() else{ui.label("Choose a series to begin brainstorming.");return;};
            let context=match self.page{Page::Synopsis=>"Synopsis",Page::Cast=>"Characters",Page::Locations=>"Locations",Page::Story=>"Overall plot",Page::Series=>"Episode plot",Page::Queue=>"Episode generation",_=>"Studio workspace"}.to_owned();
            ui.label(eframe::egui::RichText::new(format!("Working on: {context}")).color(GOLD));
            ui.small("Gemini 4 Argon · via OpenRouter");
            ui.separator();
            let messages=match self.store.sidekick_messages(&series){Ok(messages)=>messages,Err(error)=>{ui.label(error.to_string());return;}};
            eframe::egui::ScrollArea::vertical().id_salt(("sidekick-history",&series)).stick_to_bottom(true).max_height((ui.available_height()-230.).max(100.)).show(ui,|ui|{
                if messages.is_empty(){ui.label("Tell me the kind of show you want to make, or an idea you want to explore.");}
                for message in &messages {
                    if let Some(action)=message_bubble(ui,message,&mut self.sidekick.editing_message,self.sidekick.pending.is_none()) {
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
            if self.sidekick.pending.as_ref().is_some_and(|(id,_,_,_,_)|id==&series){let stage=match self.sidekick.progress.load(std::sync::atomic::Ordering::Relaxed) {1=>"Waiting for Keychain authorization",2=>"Waiting for OpenRouter reply",3=>"Reading OpenRouter response",_=>"Preparing message"};
                let elapsed=self.sidekick.request_started.map(|started|started.elapsed().as_secs()).unwrap_or(0);
                ui.horizontal(|ui|{ui.spinner();ui.label(format!("{stage} · {elapsed}s"));});
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));}
            if !self.sidekick.error.is_empty(){ui.label(&self.sidekick.error);}
            let character_snapshots=self.character_drafts(&series).unwrap_or_default();
            let project_context = serde_json::json!({
                "workspace":context,
                "character_drafts":character_snapshots,
                "series":self.store.series().ok().and_then(|items|items.into_iter().find(|item|item.id==series)),
                "characters":self.store.library(&series,"character").unwrap_or_default().iter().map(|entry|serde_json::json!({"id":entry.id,"name":entry.name,"description":entry.description})).collect::<Vec<_>>(),
                "series_plot":self.store.series_plan(&series).ok(),
                "episode_id":self.episode_id,
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
            let send_with_enter=ui.memory(|memory|memory.has_focus(composer_id))&&ui.input_mut(|input|{
                let send=input.events.iter().any(|event|matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::Enter,pressed:true,repeat:false,modifiers,..} if *modifiers==eframe::egui::Modifiers::NONE));
                input.events.retain(|event|!matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::Enter,pressed:true,modifiers,..} if *modifiers==eframe::egui::Modifiers::NONE));
                send
            });
            ui.add(eframe::egui::TextEdit::multiline(draft).id(composer_id).return_key(eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::SHIFT,eframe::egui::Key::Enter)).desired_rows(3).desired_width(ui.available_width()).hint_text("Discuss your series…"));
            let baseline=self.sidekick.saved_drafts.get_mut(&series).expect("loaded baseline");
            let mut draft_saved=true;
            if draft != baseline {
                match self.store.save_sidekick_draft(&series,baseline,draft) {
                    Ok(())=>*baseline=draft.clone(),
                    Err(error)=>{draft_saved=false;ui.label(format!("Draft not saved: {error}"));}
                }
            }
            ui.small(if draft_saved {"Enter to send · Shift+Enter for a new line · draft saved locally"} else {"Enter to send · Shift+Enter for a new line · draft not saved"});
            let can_send=draft_saved&&self.sidekick.pending.is_none()&&!draft.trim().is_empty();
            let send_clicked=ui.add_enabled(can_send,eframe::egui::Button::new("Send to sidekick")).clicked();
            if can_send&&(send_clicked||send_with_enter){
                let message=Message{id:uuid::Uuid::new_v4().to_string(),role:"user".into(),text:draft.clone(),context:context.clone()};
                match self.store.append_sidekick_message(&series,&message){Ok(())=>{draft.clear();self.sidekick.error.clear();
                    match self.store.save_sidekick_draft(&series,baseline,"") {
                        Ok(())=>baseline.clear(),Err(error)=>self.sidekick.error=format!("Message retained in chat, but composer cleanup failed: {error}"),
                    }
                    match self.store.sidekick_messages(&series){Ok(messages)=>{let (sender,receiver)=std::sync::mpsc::channel();
                        self.sidekick.request_started=Some(std::time::Instant::now());
                        self.sidekick.progress.store(0,std::sync::atomic::Ordering::Relaxed);
                        let progress=self.sidekick.progress.clone();
                        self.sidekick.pending=Some((series.clone(),context.clone(),receiver,if self.page==Page::Synopsis{self.synopsis_drafts.get(&series).cloned()}else{None},character_snapshots));std::thread::spawn(move||{let _=sender.send(crate::sidekick::reply(&messages,&project_context,"google/gemini-4-argon",&progress).map_err(|error|error.to_string()));});},Err(error)=>self.sidekick.error=error.to_string()}},Err(error)=>self.sidekick.error=error.to_string()}
            }
            ui.label(eframe::egui::RichText::new("Send uses your paid OpenRouter account. Synopsis edits can update its unsaved draft with Undo. Character briefs and portrait prompts also support draft edits. New characters can be created with saved briefs and portrait prompts. Other documents and automatic portrait actions are being connected.").small().color(MUTED));
        });
    }
}
