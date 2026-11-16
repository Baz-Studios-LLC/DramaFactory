use crate::{GOLD, MUTED, Page, Studio};
use eframe::egui::{self, RichText, Vec2};
impl Studio {
    pub(crate) fn episode_navigation(&mut self, ui: &mut egui::Ui) {
        let Some(series) = self.selected.clone() else {
            ui.label("Choose the current series in the sidebar.");
            return;
        };
        let episodes = self.load(self.store.ordered_gui_episodes(&series));
        let index = episodes
            .iter()
            .position(|episode| Some(&episode.id) == self.episode_id.as_ref());
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(index.is_some_and(|i| i > 0), egui::Button::new("Previous"))
                .clicked()
                && let Some(index) = index
            {
                self.select_episode(&episodes[index - 1].id);
            }
            ui.label(
                index
                    .map(|i| {
                        format!(
                            "Episode {:02} of {} · {}",
                            i + 1,
                            episodes.len(),
                            episodes[i].title
                        )
                    })
                    .unwrap_or("No episode selected".into()),
            );
            if ui
                .add_enabled(
                    index.is_some_and(|i| i + 1 < episodes.len()),
                    egui::Button::new("Next"),
                )
                .clicked()
                && let Some(index) = index
            {
                self.select_episode(&episodes[index + 1].id);
            }
            if ui.button("Browse episodes").clicked() {
                self.page = Page::Series;
            }
        });
        ui.add_space(12.);
    }
    pub(crate) fn episode_workspace(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "EPISODES & SCRIPTS",
            "Write one episode at a time.",
            "Find an episode on the left. Write its screenplay, save it, then plan its shots.",
        );
        let Some(series) = self.selected.clone() else {
            Self::card(ui, |ui| {
                ui.label("Choose the current series in the sidebar, or create a new series from the workflow guide.");
            });
            return;
        };
        let episodes = self.load(self.store.ordered_gui_episodes(&series));
        let browser_width = (ui.available_width() * 0.28).clamp(190., 280.);
        ui.horizontal_top(|ui|{
   ui.allocate_ui_with_layout(Vec2::new(browser_width,550.),egui::Layout::top_down(egui::Align::LEFT),|ui|{
    ui.set_max_width(browser_width);
    Self::card(ui,|ui|{
     ui.label(RichText::new("EPISODE BROWSER").color(GOLD).size(11.));
     ui.add(egui::TextEdit::singleline(&mut self.episode_search).hint_text("Search title or number").desired_width(ui.available_width()));
     let term=self.episode_search.trim().to_lowercase();let matches:Vec<_>=episodes.iter().enumerate().filter(|(i,episode)|term.is_empty()||episode.title.to_lowercase().contains(&term)||format!("{:02}",i+1).contains(&term)).collect();
     ui.small(format!("{} of {} episodes",matches.len(),episodes.len()));
     egui::ScrollArea::vertical().id_salt("episode-browser").max_height(440.).auto_shrink([false,false]).show(ui,|ui|{
      if matches.is_empty(){ui.label("No matching episodes. Clear the search to see the full list.");}
      for (index,episode) in matches {let active=Some(&episode.id)==self.episode_id.as_ref();let label=format!("{:02}  {}\n{}",index+1,episode.title,if episode.screenplay.trim().is_empty(){"No screenplay yet"}else{"Screenplay saved"});
       if ui.add_sized([ui.available_width(),58.],egui::Button::new(RichText::new(label).color(if active{GOLD}else{egui::Color32::from_rgb(228,230,235)})).selected(active).wrap()).clicked(){self.select_episode(&episode.id);}
      }
     });
     ui.collapsing("+ Add an episode",|ui|{
      ui.add(egui::TextEdit::singleline(&mut self.episode_title).hint_text("Episode title").desired_width(ui.available_width()));
      if ui.add_enabled(!self.episode_title.trim().is_empty(),egui::Button::new("Create episode")).clicked() && self.save_script(){match self.store.create_episode(&series,&self.episode_title){Ok(episode)=>{self.select_episode(&episode.id);self.episode_title.clear();self.episode_search.clear();self.message="Episode created".into();},Err(error)=>self.message=error.to_string()}}
     });
    });
   });
   ui.add_space(8.);
   ui.vertical(|ui|{
    ui.set_width(ui.available_width());
    Self::card(ui,|ui|{
     let current=episodes.iter().enumerate().find(|(_,episode)|Some(&episode.id)==self.episode_id.as_ref());
     if let Some((index,episode))=current {
      ui.label(RichText::new(format!("EPISODE {:02}",index+1)).size(11.).color(GOLD));ui.heading(&episode.title);
      ui.horizontal_wrapped(|ui|{
       let dirty=self.script!=self.script_baseline;ui.label(RichText::new(if dirty{"Unsaved changes"}else{"Saved"}).color(if dirty{GOLD}else{MUTED}));
       ui.small(format!("{} words",self.script.split_whitespace().count()));
       if ui.button("Save screenplay").clicked() && self.save_script(){self.message="Screenplay saved".into();}
       if ui.button("Reload saved screenplay").clicked(){match self.store.get_episode(&episode.id){Ok(saved)=>{self.script=saved.screenplay;self.script_baseline=self.script.clone();},Err(error)=>self.message=error.to_string()}}
       if ui.button(RichText::new("Plan shots").color(GOLD)).clicked() && self.save_script(){self.page=Page::Scenes;}
      });
      ui.add_space(8.);egui::ScrollArea::vertical().id_salt(("screenplay",&episode.id)).max_height(540.).auto_shrink([false,false]).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.script).desired_rows(26).desired_width(ui.available_width()).hint_text("Write the scene, action and dialogue here…"));});
     }else{ui.heading("Choose an episode");ui.label("Select a title in the episode browser to begin. Your selection carries into shots, dialogue and editing.");}
    });
   });
  });
    }
}
