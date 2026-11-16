use crate::{GOLD, MUTED, Studio, directing::CameraSetup};
use eframe::egui::{self, RichText};
impl Studio {
    pub(crate) fn shot_direction_editor(
        &mut self,
        ui: &mut egui::Ui,
        shot: &crate::production::Shot,
    ) {
        egui::CollapsingHeader::new("Cast, location & camera continuity").id_salt((&shot.id,"direction-editor")).show(ui,|ui| {
            let Some(series)=self.store.get_episode(&shot.episode_id).ok().map(|e|e.series_id) else{return;};
            let cast=self.load(self.store.library(&series,"character"));
            let locations=self.load(self.store.library(&series,"location"));
            let mut direction=self.shot_directions.remove(&shot.id).unwrap_or_else(||self.store.shot_direction(&shot.id).unwrap_or_default());
            ui.label(RichText::new("The first selected character supplies the image reference. Use single-character coverage when consistent faces are critical.").color(MUTED));
            for character in &cast {
                let mut selected=direction.character_ids.contains(&character.id);
                if ui.checkbox(&mut selected,&character.name).changed() {
                    if selected {direction.character_ids.push(character.id.clone());}else{direction.character_ids.retain(|id|id!=&character.id);}
                }
                if selected && direction.character_ids.first()!=Some(&character.id) && ui.small_button(format!("Make {} primary",character.name)).clicked() {
                    direction.character_ids.retain(|id|id!=&character.id);direction.character_ids.insert(0,character.id.clone());
                }
            }
            if let Some(primary)=direction.character_ids.first().and_then(|id|cast.iter().find(|c|&c.id==id)){ui.label(RichText::new(format!("Primary reference: {}",primary.name)).color(GOLD));}
            let old=direction.location_id.clone();
            egui::ComboBox::from_id_salt((&shot.id,"location")).selected_text(locations.iter().find(|l|Some(&l.id)==direction.location_id.as_ref()).map(|l|l.name.as_str()).unwrap_or("Choose location")).show_ui(ui,|ui| {
                ui.selectable_value(&mut direction.location_id,None,"No location link");
                for location in &locations {ui.selectable_value(&mut direction.location_id,Some(location.id.clone()),&location.name);}
            });
            if old!=direction.location_id {direction.camera_setup_id=None;}
            if let Some(location)=&direction.location_id {
                let cameras=self.load(self.store.camera_setups(location));
                egui::ComboBox::from_id_salt((&shot.id,"camera")).selected_text(cameras.iter().find(|c|Some(&c.id)==direction.camera_setup_id.as_ref()).map(|c|c.name.as_str()).unwrap_or("Choose camera setup")).show_ui(ui,|ui| {
                    ui.selectable_value(&mut direction.camera_setup_id,None,"Use shot's camera notes");
                    for camera in &cameras {ui.selectable_value(&mut direction.camera_setup_id,Some(camera.id.clone()),&camera.name);}
                });
                ui.collapsing("Manage cameras at this location",|ui| {
                    let mut draft=self.camera_drafts.remove(location).unwrap_or_else(||CameraSetup{id:uuid::Uuid::new_v4().to_string(),location_id:location.clone(),..Default::default()});
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("New camera").clicked(){draft=CameraSetup{id:uuid::Uuid::new_v4().to_string(),location_id:location.clone(),..Default::default()};}
                        for camera in &cameras {if ui.small_button(&camera.name).clicked(){draft=camera.clone();}}
                    });
                    for (label,text) in [("SETUP NAME — E.G. A / RECEPTION REVERSE",&mut draft.name),("FRAMING",&mut draft.framing),("CAMERA PLACEMENT",&mut draft.placement),("EYELINE",&mut draft.eyeline),("SCREEN AXIS — WHO STAYS LEFT / RIGHT",&mut draft.screen_axis)] {Self::field(ui,label,text,1);}
                    if ui.button("Save camera setup").clicked(){match self.store.save_camera_setup(&draft){Ok(saved)=>{draft=saved;self.message="Camera geometry saved for reuse".into();},Err(e)=>self.message=e.to_string()}}
                    self.camera_drafts.insert(location.clone(),draft);
                });
            }
            Self::field(ui,"STORY BEAT — WHY THIS SHOT EXISTS",&mut direction.dramatic_beat,2);
            Self::field(ui,"ACTING & MOVEMENT",&mut direction.performance,2);
            Self::field(ui,"WARDROBE & PROPS TO PRESERVE",&mut direction.wardrobe,2);
            ui.small("Describe the visible cast here. Off-screen costume details can confuse generation; use cast links to keep the prompt focused.");
            ui.horizontal(|ui| {
                if ui.button("Save shot direction").clicked(){match self.store.save_shot_direction(&shot.id,&direction){Ok(saved)=>{direction=saved;self.message="Shot direction saved".into();},Err(e)=>self.message=e.to_string()}}
                if ui.button("Reload direction").on_hover_text("Reload the latest saved direction; unsaved field changes are discarded").clicked(){match self.store.shot_direction(&shot.id){Ok(saved)=>direction=saved,Err(e)=>self.message=e.to_string()}}
            });
            self.shot_directions.insert(shot.id.clone(),direction);
        });
    }
}
