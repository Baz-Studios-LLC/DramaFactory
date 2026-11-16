use crate::{GOLD, MUTED, Page, Studio, production::Dialogue, worker};
use eframe::egui::{self, RichText, Vec2};
#[derive(Default)]
pub struct DialogueWorkspace {
    pub selected: Option<String>,
    pub search: String,
    pub adding: bool,
    pub anchor: Option<String>,
}
#[derive(Clone)]
pub(crate) struct NewLineDraft {
    text: String,
    voice: String,
    speed: f64,
    shot: Option<String>,
}
impl Studio {
    pub(crate) fn remember_new_line(&mut self) {
        if let Some(episode) = &self.episode_id {
            self.new_dialogue_drafts.insert(
                episode.clone(),
                NewLineDraft {
                    text: self.dialogue_text.clone(),
                    voice: self.dialogue_voice.clone(),
                    speed: self.dialogue_speed,
                    shot: self.dialogue_shot.clone(),
                },
            );
        }
    }
    pub(crate) fn restore_new_line(&mut self) {
        let draft = self
            .episode_id
            .as_ref()
            .and_then(|id| self.new_dialogue_drafts.get(id));
        self.dialogue_text = draft.map(|d| d.text.clone()).unwrap_or_default();
        self.dialogue_voice = draft
            .map(|d| d.voice.clone())
            .unwrap_or_else(|| "af_heart".into());
        self.dialogue_speed = draft.map(|d| d.speed).unwrap_or(1.);
        self.dialogue_shot = draft.and_then(|d| d.shot.clone());
    }

    pub(crate) fn snapshot_new_dialogue(&self) -> anyhow::Result<()> {
        if !self.dialogue_text.trim().is_empty() {
            let directory = self.store.root().join("drafts");
            std::fs::create_dir_all(&directory)?;
            let draft = serde_json::json!({"kind":"new_dialogue_ui_draft","episode_id":self.episode_id,"shot_id":self.dialogue_shot,"text":self.dialogue_text,"voice":self.dialogue_voice,"speed":self.dialogue_speed});
            std::fs::write(
                directory.join(format!("new-dialogue-{}.json", uuid::Uuid::new_v4())),
                serde_json::to_vec_pretty(&draft)?,
            )?;
        }
        Ok(())
    }

    pub(crate) fn dialogue_workspace(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "DIALOGUE",
            "Make the words feel spoken.",
            "Choose a line, edit its words and voice, then generate and listen before working on the talking footage.",
        );
        self.episode_picker(ui);
        let Some(episode) = self.episode_id.clone() else {
            ui.label("Choose an episode in the episode browser to work on its dialogue.");
            return;
        };
        let shots = self.load(self.store.shots(&episode));
        let assets = self.load(self.store.assets());
        let mut lines = Vec::new();
        for shot in &shots {
            lines.extend(self.load(self.store.dialogue(&shot.id)));
        }
        if self.dialogue_workspace.anchor != self.dialogue_shot {
            self.dialogue_workspace.selected = lines
                .iter()
                .find(|line| Some(&line.shot_id) == self.dialogue_shot.as_ref())
                .map(|line| line.id.clone());
            self.dialogue_workspace.anchor = self.dialogue_shot.clone();
        }
        if !lines
            .iter()
            .any(|line| Some(&line.id) == self.dialogue_workspace.selected.as_ref())
        {
            self.dialogue_workspace.selected = lines.first().map(|line| line.id.clone());
        }
        if self
            .dialogue_shot
            .as_ref()
            .is_none_or(|id| !shots.iter().any(|shot| &shot.id == id))
        {
            self.dialogue_shot = shots.first().map(|shot| shot.id.clone());
            self.dialogue_workspace.anchor = self.dialogue_shot.clone();
        }
        let voices = self
            .selected
            .as_ref()
            .and_then(|id| self.store.library(id, "character").ok())
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.voice)
            .collect::<Vec<_>>();
        ui.horizontal_wrapped(|ui|{
   ui.label(format!("{} saved lines",lines.len()));
   if ui.add_enabled(!shots.is_empty(),egui::Button::new("+ Add line")).clicked(){self.dialogue_workspace.adding=true;self.dialogue_workspace.search.clear();}
   if ui.button("Generate missing speech").on_hover_text("Uses saved lines; keeps usable speech and reuses pending jobs. No video is generated.").clicked(){match self.store.produce_episode_dialogue(&episode){Ok(batch)=>{let message=format!("{} ready · {} queued · {} pending reused · {} retried",batch.ready_lines,batch.newly_queued,batch.reused_pending,batch.retried);let result=if batch.jobs.is_empty(){Ok(())}else{worker::spawn_detached(&self.store.path)};self.result(result,&message);},Err(error)=>self.message=error.to_string()}}
   if ui.button("Make listening rehearsal").on_hover_text("Creates an audio-only WAV from saved speech and timing, not a finished video episode.").clicked(){let result=self.store.enqueue_rehearsal(&episode).and_then(|_|worker::spawn_detached(&self.store.path));self.result(result,"Audio rehearsal queued; listen in Review takes");}
  });
        self.dialogue_timing_panel(ui, &episode);
        ui.add_space(12.);
        let browser_width = (ui.available_width() * 0.29).clamp(210., 300.);
        ui.horizontal_top(|ui|{
   ui.allocate_ui_with_layout(Vec2::new(browser_width,550.),egui::Layout::top_down(egui::Align::LEFT),|ui|{
    ui.set_max_width(browser_width);Self::card(ui,|ui|{
     ui.label(RichText::new(if self.dialogue_workspace.adding{"CHOOSE A SHOT FOR THE NEW LINE"}else{"EPISODE LINES"}).size(11.).color(GOLD));
     ui.add(egui::TextEdit::singleline(&mut self.dialogue_workspace.search).hint_text(crate::theme::placeholder(if self.dialogue_workspace.adding{"Search shots"}else{"Search words, shot or voice"})).desired_width(ui.available_width()));
     let search=self.dialogue_workspace.search.trim().to_lowercase();let mut matches=0;
     egui::ScrollArea::vertical().id_salt("dialogue-browser").max_height(520.).auto_shrink([false,false]).show(ui,|ui|{
      if self.dialogue_workspace.adding{
       for shot in shots.iter().filter(|shot|search.is_empty()||shot.title.to_lowercase().contains(&search)){matches+=1;if ui.add_sized([ui.available_width(),54.],egui::Button::new(&shot.title).wrap().selected(Some(&shot.id)==self.dialogue_shot.as_ref())).clicked(){self.dialogue_shot=Some(shot.id.clone());self.dialogue_workspace.anchor=self.dialogue_shot.clone();}}
      }else{
       for line in &lines{
        let Some(shot)=shots.iter().find(|shot|shot.id==line.shot_id)else{continue;};
        if !search.is_empty()&&!format!("{} {} {}",line.text,shot.title,crate::voice_ui::name(&line.voice)).to_lowercase().contains(&search){continue;}
        matches+=1;let text=line.text.chars().take(110).collect::<String>();let status=if line.asset_id.is_some(){"Speech take linked"}else{"Needs speech"};
        let label=format!("{}\nShot {:02} · {} · {}",text,shot.position+1,crate::voice_ui::name(&line.voice),status);
        if ui.add_sized([ui.available_width(),76.],egui::Button::new(RichText::new(label).color(if Some(&line.id)==self.dialogue_workspace.selected.as_ref(){GOLD}else{egui::Color32::from_rgb(228,230,235)})).wrap().selected(Some(&line.id)==self.dialogue_workspace.selected.as_ref())).clicked(){self.dialogue_workspace.selected=Some(line.id.clone());self.dialogue_shot=Some(line.shot_id.clone());self.dialogue_workspace.anchor=self.dialogue_shot.clone();}
       }
      }
      if matches==0{ui.label(if lines.is_empty()&&!self.dialogue_workspace.adding{"No dialogue saved yet. Add a line to one of your planned shots."}else{"No matches. Clear the search to see the full list."});}
     });
    });
   });ui.add_space(8.);
   ui.vertical(|ui|{
    ui.set_width(ui.available_width());Self::card(ui,|ui|{
     if self.dialogue_workspace.adding{self.new_dialogue_form(ui,&shots,&voices);return;}
     let Some(line)=lines.iter().find(|line|Some(&line.id)==self.dialogue_workspace.selected.as_ref())else{ui.heading("Choose a line");ui.label("Select dialogue on the left, or add the first line to a planned shot.");if shots.is_empty()&&ui.button("Open shot planning").clicked(){self.page=Page::Scenes;}return;};
     let shot=shots.iter().find(|shot|shot.id==line.shot_id).expect("loaded episode shot");
     ui.label(RichText::new(format!("SHOT {:02} · {}",shot.position+1,crate::shot_workspace::shot_display_title(shot))).size(11.).color(GOLD));
     ui.small("Words & voice changes stay in your draft until saved. Generation uses the saved line.");
     let shot_start=shots.iter().take_while(|shot|shot.id!=line.shot_id).map(|shot|shot.duration).sum();
     self.dialogue_line_inspector(ui,line.clone(),crate::dialogue_line_ui::LineContext { shots: &shots, assets: &assets, shot_start, voices: &voices });
    });
   });
  });
    }
    fn new_dialogue_form(
        &mut self,
        ui: &mut egui::Ui,
        shots: &[crate::production::Shot],
        voices: &[String],
    ) {
        ui.heading("New dialogue line");
        let target = shots
            .iter()
            .find(|shot| Some(&shot.id) == self.dialogue_shot.as_ref());
        ui.label(
            target
                .map(|shot| format!("Attached to: {}", shot.title))
                .unwrap_or("Choose a shot on the left.".into()),
        );
        Self::field(ui, "WORDS TO SPEAK", &mut self.dialogue_text, 5);
        ui.horizontal_wrapped(|ui| {
            ui.label("Voice");
            crate::voice_ui::picker(ui, "new-line-voice", &mut self.dialogue_voice, voices);
            ui.label("Speed");
            ui.add(
                egui::DragValue::new(&mut self.dialogue_speed)
                    .range(0.5..=2.)
                    .speed(0.05)
                    .suffix("x"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    target.is_some() && !self.dialogue_text.trim().is_empty(),
                    egui::Button::new(RichText::new("Save line").color(GOLD)),
                )
                .clicked()
                && let Some(target) = target
            {
                let draft = Dialogue {
                    id: uuid::Uuid::new_v4().to_string(),
                    shot_id: target.id.clone(),
                    text: self.dialogue_text.clone(),
                    voice: self.dialogue_voice.clone(),
                    speed: self.dialogue_speed,
                    asset_id: None,
                    timeline_start: None,
                    revision: 0,
                };
                match self.store.save_dialogue_draft(&draft) {
                    Ok(saved) => {
                        self.dialogue_workspace.selected = Some(saved.id);
                        self.dialogue_workspace.adding = false;
                        self.dialogue_workspace.search.clear();
                        self.dialogue_text.clear();
                        self.message = "Line saved; generate its speech when ready".into();
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            if ui.button("Back to saved lines").clicked() {
                self.dialogue_workspace.adding = false;
                self.dialogue_workspace.search.clear();
            }
        });
        ui.label(RichText::new("Saving does not start generation. Your unsaved new-line text stays here when you return to saved lines.").color(MUTED));
    }
}
