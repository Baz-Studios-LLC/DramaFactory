use crate::{GOLD, MUTED, Page, Studio, production::Shot};
use eframe::egui::{self, RichText, Vec2};
#[derive(Default)]
pub struct ShotWorkspace {
    pub selected: Option<String>,
    pub search: String,
    pub adding: bool,
    pub takes: bool,
    pub all_takes: bool,
}
pub(crate) fn shot_display_title(shot: &Shot) -> &str {
    shot.title
        .strip_prefix(&format!("{:02} · ", shot.position + 1))
        .unwrap_or(&shot.title)
}
impl Studio {
    pub(crate) fn snapshot_shot_form(&self) -> anyhow::Result<()> {
        if self.edit_shot.is_some() || self.shot_workspace.adding {
            let directory = self.store.root().join("drafts");
            std::fs::create_dir_all(&directory)?;
            let draft = serde_json::json!({"kind":"shot_plan_ui_draft","episode_id":self.episode_id,"shot_id":self.edit_shot,"title":self.shot_title,"prompt":self.shot_prompt,"camera":self.shot_camera,"duration":self.shot_duration});
            std::fs::write(
                directory.join(format!("shot-plan-{}.json", uuid::Uuid::new_v4())),
                serde_json::to_vec_pretty(&draft)?,
            )?;
        }
        Ok(())
    }
    pub(crate) fn shot_workspace(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "SHOT PLANNING",
            "Direct the episode.",
            "Choose a shot, define what happens, then prepare its performance and review the takes.",
        );
        self.episode_picker(ui);
        let Some(episode) = self.episode_id.clone() else {
            ui.label("Open the episode browser to choose the episode you want to direct.");
            return;
        };
        let shots = self.load(self.store.shots(&episode));
        if !shots
            .iter()
            .any(|shot| Some(&shot.id) == self.shot_workspace.selected.as_ref())
        {
            self.shot_workspace.selected = shots.first().map(|shot| shot.id.clone());
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} shots · {:.2}s planned",
                shots.len(),
                shots.iter().map(|shot| shot.duration).sum::<f64>()
            ));
            if ui
                .add_enabled(
                    self.edit_shot.is_none() && !self.shot_workspace.adding,
                    egui::Button::new("+ Add shot"),
                )
                .clicked()
            {
                self.shot_workspace.adding = true;
                self.shot_title.clear();
                self.shot_prompt.clear();
                self.shot_camera.clear();
                self.shot_duration = 3.;
            }
            if ui.button("Open dialogue").clicked() {
                self.page = Page::Dialogue;
            }
        });
        if let Some(profile) = self
            .selected
            .as_ref()
            .and_then(|id| self.store.series_plan(id).ok())
            .map(|p| p.creative)
        {
            ui.small(format!("Cut guide: {:.1}–{:.1}s · episode target: {:.0}s. Use longer holds when the performance needs them.",profile.min_shot_seconds,profile.max_shot_seconds,profile.episode_seconds));
        }
        ui.add_space(12.);
        let width = (ui.available_width() * 0.28).clamp(190., 280.);
        ui.horizontal_top(|ui|{
   ui.allocate_ui_with_layout(Vec2::new(width,550.),egui::Layout::top_down(egui::Align::LEFT),|ui|{
    ui.set_max_width(width);Self::card(ui,|ui|{
     ui.label(RichText::new("SHOT LIST").color(GOLD).size(11.));ui.add(egui::TextEdit::singleline(&mut self.shot_workspace.search).hint_text(crate::theme::placeholder("Search shots")).desired_width(ui.available_width()));
     let search=self.shot_workspace.search.trim().to_lowercase();
     egui::ScrollArea::vertical().id_salt("shot-list").max_height(520.).auto_shrink([false,false]).show(ui,|ui|{
      let mut found=false;
      for shot in shots.iter().filter(|shot|search.is_empty()||shot.title.to_lowercase().contains(&search)||format!("{:02}",shot.position+1).contains(&search)) {
       found=true;
       let enabled=self.edit_shot.is_none()&&!self.shot_workspace.adding;
       let label=format!("{:02}  {}\n{:.2}s · {}",shot.position+1,shot_display_title(shot),shot.duration,if shot.asset_id.is_some(){"Take selected"}else{"Needs footage"});
       if ui.add_enabled(enabled,egui::Button::new(RichText::new(label).color(if Some(&shot.id)==self.shot_workspace.selected.as_ref(){GOLD}else{egui::Color32::from_rgb(228,230,235)})).selected(Some(&shot.id)==self.shot_workspace.selected.as_ref()).min_size(Vec2::new(ui.available_width(),58.)).wrap()).clicked(){self.shot_workspace.selected=Some(shot.id.clone());}
      }
      if shots.is_empty(){ui.label("No shots yet. Add the first shot for this screenplay.");}else if !found {ui.label("No shots match this search. Clear it to see the full shot list.");}
     });
    });
   });ui.add_space(8.);
   ui.vertical(|ui|{
    ui.set_width(ui.available_width());
    if self.shot_workspace.adding||self.edit_shot.is_some(){Self::card(ui,|ui|self.shot_plan_form(ui,&episode));return;}
    let Some(shot)=shots.iter().find(|shot|Some(&shot.id)==self.shot_workspace.selected.as_ref())else{Self::card(ui,|ui|{ui.heading("Plan the first shot");ui.label("Add a short visual beat, choose its camera framing and duration, then link the characters and location.");});return;};
    Self::card(ui,|ui|{
     ui.label(RichText::new(format!("SHOT {:02}",shot.position+1)).color(GOLD).size(11.));ui.heading(shot_display_title(shot));ui.small(format!("{:.2}s · {}",shot.duration,shot.status));
     ui.horizontal_wrapped(|ui|{
      if ui.button("Edit shot").clicked(){self.edit_shot=Some(shot.id.clone());self.shot_title=shot.title.clone();self.shot_prompt=shot.prompt.clone();self.shot_camera=shot.camera.clone();self.shot_duration=shot.duration;}
      if ui.add_enabled(shot.position>0,egui::Button::new("Move earlier")).clicked(){let result=self.store.move_shot(&shot.id,-1);self.result(result,"Shot order updated");}
      if ui.add_enabled(shot.position+1<shots.len() as u32,egui::Button::new("Move later")).clicked(){let result=self.store.move_shot(&shot.id,1);self.result(result,"Shot order updated");}
     });ui.separator();
     ui.separator();ui.horizontal_wrapped(|ui|{
      if ui.button(RichText::new("Prepare video render").color(GOLD)).clicked(){match self.store.prepare_shot_render(&shot.id){Ok(prepared)=>{self.request=prepared.request;self.message=if prepared.notes.is_empty(){"Render prepared from saved shot direction".into()}else{prepared.notes.join(" ")};self.page=Page::Queue;},Err(error)=>self.message=error.to_string()}}
      if ui.button("Work on dialogue").clicked(){self.dialogue_shot=Some(shot.id.clone());self.page=Page::Dialogue;}
     });ui.small("Prepare opens generation settings; it does not start a render. Review generated acting before choosing a take.");
     ui.separator();
     ui.horizontal_wrapped(|ui|{ui.selectable_value(&mut self.shot_workspace.takes,false,"Plan & references");ui.selectable_value(&mut self.shot_workspace.takes,true,"Takes & review");});ui.add_space(8.);
     if self.shot_workspace.takes {self.shot_take_panel(ui,shot);}else{
      ui.label(RichText::new("WHAT HAPPENS").size(11.).color(GOLD));ui.label(&shot.prompt);
      if !shot.camera.is_empty(){ui.label(RichText::new(format!("Camera: {}",shot.camera)).color(MUTED));}
      self.shot_direction_editor(ui,shot);
      if let Ok(direction)=self.store.shot_direction(&shot.id) && let Some(series)=self.selected.clone(){
       let mut library=self.load(self.store.library(&series,"character"));library.extend(self.load(self.store.library(&series,"location")));
       ui.horizontal_wrapped(|ui|{for entry in library.iter().filter(|entry|direction.character_ids.contains(&entry.id)||direction.location_id.as_ref()==Some(&entry.id)).take(3){if let Some(path)=&entry.reference{ui.vertical(|ui|{ui.label(&entry.name);self.reference_preview(ui,path,if entry.kind=="character"{Vec2::new(160.,220.)}else{Vec2::new(280.,200.)});});}}});
      }
     }

    });
   });
  });
    }
    fn shot_plan_form(&mut self, ui: &mut egui::Ui, episode: &str) {
        ui.heading(if self.edit_shot.is_some() {
            "Edit shot"
        } else {
            "New shot"
        });
        Self::field(ui, "SHOT TITLE", &mut self.shot_title, 1);
        Self::field(ui, "WHAT HAPPENS ON SCREEN", &mut self.shot_prompt, 4);
        Self::field(ui, "CAMERA FRAMING & MOVEMENT", &mut self.shot_camera, 2);
        ui.horizontal_wrapped(|ui| {
            ui.label("Duration");
            ui.add(
                egui::DragValue::new(&mut self.shot_duration)
                    .range(0.1..=30.)
                    .speed(1. / 24.)
                    .suffix(" s"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.shot_title.trim().is_empty() && !self.shot_prompt.trim().is_empty(),
                    egui::Button::new(RichText::new("Save shot").color(GOLD)),
                )
                .clicked()
            {
                if let Some(id) = self.edit_shot.clone() {
                    let result = self.store.update_shot(
                        &id,
                        &self.shot_title,
                        &self.shot_prompt,
                        &self.shot_camera,
                        self.shot_duration,
                    );
                    if result.is_ok() {
                        self.edit_shot = None;
                    }
                    self.result(result, "Shot saved");
                } else {
                    let result = self.store.add_shot(
                        episode,
                        &self.shot_title,
                        &self.shot_prompt,
                        &self.shot_camera,
                        self.shot_duration,
                    );
                    match result {
                        Ok(shot) => {
                            self.shot_workspace.selected = Some(shot.id);
                            self.shot_workspace.adding = false;
                            self.shot_workspace.search.clear();
                            self.message = "Shot added".into();
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
            }
            if ui.button("Cancel").clicked() {
                self.edit_shot = None;
                self.shot_workspace.adding = false;
            }
        });
        ui.small("Save or cancel this edit before choosing another shot. Original generated takes are preserved.");
    }
    fn shot_take_panel(&mut self, ui: &mut egui::Ui, shot: &Shot) {
        let assets = self.load(self.store.assets());
        ui.checkbox(
            &mut self.shot_workspace.all_takes,
            "Include video takes from other shots",
        );
        let related: Vec<_> = assets
            .iter()
            .filter(|asset| {
                asset.width > 0
                    && asset.duration > 0.
                    && (self.shot_workspace.all_takes
                        || asset.metadata["request"]["shot_id"].as_str() == Some(&shot.id)
                        || shot.asset_id.as_ref() == Some(&asset.id))
            })
            .collect();
        if related.is_empty() {
            ui.label(
                "No takes for this shot yet. Prepare a render, then return here to review it.",
            );
        }
        for asset in related {
            ui.push_id(&asset.id, |ui| {
                ui.separator();
                ui.label(RichText::new(self.store.asset_label(asset)).strong());
                ui.label(format!(
                    "{:.2}s · {} · {}",
                    asset.duration,
                    asset.review,
                    asset.id.chars().take(8).collect::<String>()
                ));
                if let Ok(review) = self.store.asset_review(&asset.id)
                    && !review.note.trim().is_empty()
                {
                    ui.label(
                        RichText::new(review.note.chars().take(240).collect::<String>())
                            .color(MUTED),
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Review take").clicked() {
                        self.asset_browser.show_asset(&asset.id);
                        self.page = Page::Assets;
                    }
                    if shot.asset_id.as_ref() == Some(&asset.id) {
                        ui.label(RichText::new("Selected").color(GOLD));
                    } else if ui.button("Use this take").clicked() {
                        let result = self.store.assign_take(&shot.id, &asset.id);
                        self.result(result, "Shot take selected; export still requires approval");
                    }
                });
            });
        }
    }
}
