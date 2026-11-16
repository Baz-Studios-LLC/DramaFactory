use crate::{Studio, production::LibraryEntry};
impl Studio {
    pub(crate) fn grok_portrait_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
    ) {
        eframe::egui::CollapsingHeader::new("Portrait · Grok Imagine Image 2.0").id_salt((&entry.id,"grok-portrait")).show(ui,|ui|{
            if !self.portrait_prompts.contains_key(&entry.id) {
                match self.store.portrait_prompt(entry){Ok(draft)=>{self.portrait_prompts.insert(entry.id.clone(),draft);},Err(error)=>{ui.label(error.to_string());return;}}
            }
            if self.sidekick.character_undo.contains_key(&entry.id) && ui.button("Undo sidekick character edit").clicked() {
                let result=self.undo_character_sidekick(&entry.series_id,&entry.id);self.result(result,"Character and portrait drafts restored");
            }
            let mut draft=self.portrait_prompts[&entry.id].clone();
            Self::field(ui,"PORTRAIT PROMPT",&mut draft.prompt,4);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save portrait prompt").clicked(){match self.store.save_portrait_prompt(&draft){Ok(saved)=>{draft=saved;self.message="Portrait prompt saved".into();},Err(error)=>self.message=error.to_string()}}
                if ui.button("Reload portrait prompt").clicked(){match self.store.portrait_prompt(entry){Ok(saved)=>draft=saved,Err(error)=>self.message=error.to_string()}}
            });
            let prompt=draft.prompt.clone();
            self.portrait_prompts.insert(entry.id.clone(),draft);
            ui.small("OpenRouter · 1K · 3:4 · medium quality. Creates a new option, not a replacement. Uses your paid account; cost is reported by the provider. Text-to-image only in this first pass.");
            if ui.add_enabled(!prompt.trim().is_empty(),eframe::egui::Button::new("Generate portrait · paid")).clicked(){
                let result=self.store.enqueue_grok_portrait(&entry.id,&prompt).and_then(|_|crate::worker::spawn_detached(&self.store.path));
                self.result(result,"Grok portrait queued; review the result before choosing it");
            }
            let assets=self.load(self.store.assets());
            for asset in assets.iter().filter(|asset|asset.metadata["provider"]=="openrouter"&&asset.metadata["request"]["library_id"]==entry.id) {
                self.reference_preview(ui,&asset.path,eframe::egui::vec2(200.,260.));
                ui.small(format!("{} · {}",asset.metadata["model"].as_str().unwrap_or("Grok"),asset.review));
                if ui.button("Choose this portrait").clicked(){let result=self.store.review(&asset.id,"approved");self.result(result,"Character portrait selected");}
            }
        });
    }
}
