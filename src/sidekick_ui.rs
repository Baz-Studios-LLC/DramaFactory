use crate::{GOLD, MUTED, Page, Studio, sidekick::Message};
fn message_bubble(ui: &mut eframe::egui::Ui, message: &Message) {
    use eframe::egui::{Align, Color32, Frame, Layout, RichText, Stroke};
    let user = message.role == "user";
    let width = (ui.available_width() * 0.88).max(120.);
    let layout = if user {
        Layout::right_to_left(Align::TOP)
    } else {
        Layout::left_to_right(Align::TOP)
    };
    ui.with_layout(layout, |ui| {
        Frame::new()
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
                    ui.add(
                        eframe::egui::Label::new(
                            RichText::new(&message.text).color(Color32::from_rgb(235, 237, 242)),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });
            });
    });
    ui.add_space(10.);
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
                for message in &messages {message_bubble(ui,message);}
            });
            if self.sidekick.pending.as_ref().is_some_and(|(id,_,_,_,_)|id==&series){ui.label("Waiting for OpenRouter…");}
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
            let draft=self.sidekick.drafts.entry(series.clone()).or_default();
            let composer_id=ui.make_persistent_id(("sidekick-composer",&series));
            let send_with_enter=ui.memory(|memory|memory.has_focus(composer_id))&&ui.input_mut(|input|{
                let send=input.events.iter().any(|event|matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::Enter,pressed:true,repeat:false,modifiers,..} if *modifiers==eframe::egui::Modifiers::NONE));
                input.events.retain(|event|!matches!(event,eframe::egui::Event::Key{key:eframe::egui::Key::Enter,pressed:true,modifiers,..} if *modifiers==eframe::egui::Modifiers::NONE));
                send
            });
            ui.add(eframe::egui::TextEdit::multiline(draft).id(composer_id).return_key(eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::SHIFT,eframe::egui::Key::Enter)).desired_rows(3).desired_width(ui.available_width()).hint_text("Discuss your series…"));
            ui.small("Enter to send · Shift+Enter for a new line");
            let can_send=self.sidekick.pending.is_none()&&!draft.trim().is_empty();
            let send_clicked=ui.add_enabled(can_send,eframe::egui::Button::new("Send to sidekick")).clicked();
            if can_send&&(send_clicked||send_with_enter){
                let message=Message{id:uuid::Uuid::new_v4().to_string(),role:"user".into(),text:draft.clone(),context:context.clone()};
                match self.store.append_sidekick_message(&series,&message){Ok(())=>{draft.clear();self.sidekick.error.clear();match self.store.sidekick_messages(&series){Ok(messages)=>{let (sender,receiver)=std::sync::mpsc::channel();self.sidekick.pending=Some((series.clone(),context.clone(),receiver,if self.page==Page::Synopsis{self.synopsis_drafts.get(&series).cloned()}else{None},character_snapshots));std::thread::spawn(move||{let _=sender.send(crate::sidekick::reply(&messages,&project_context,"google/gemini-4-argon").map_err(|error|error.to_string()));});},Err(error)=>self.sidekick.error=error.to_string()}},Err(error)=>self.sidekick.error=error.to_string()}
            }
            ui.label(eframe::egui::RichText::new("Send uses your paid OpenRouter account. Synopsis edits can update its unsaved draft with Undo. Character briefs and portrait prompts also support draft edits. Other documents and automatic portrait actions are being connected.").small().color(MUTED));
        });
    }
}
