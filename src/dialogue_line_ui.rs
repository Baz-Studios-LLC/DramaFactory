use crate::production::{Asset, Dialogue, Shot};
use crate::{GOLD, MUTED, Studio, production, worker};
use eframe::egui::{self, RichText};
pub(crate) struct LineContext<'a> {
    pub shots: &'a [Shot],
    pub assets: &'a [Asset],
    pub shot_start: f64,
    pub voices: &'a [String],
}
impl Studio {
    pub(crate) fn dialogue_line_inspector(
        &mut self,
        ui: &mut egui::Ui,
        mut line: Dialogue,
        context: LineContext<'_>,
    ) {
        let LineContext {
            shots,
            assets,
            shot_start,
            voices,
        } = context;
        let shot = line.shot_id.clone();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if line.asset_id.is_some() {
                    "Regenerate line"
                } else {
                    "Generate speech"
                })
                .clicked()
            {
                let r = self
                    .store
                    .enqueue_dialogue(&shot, &line.text, &line.voice, line.speed, Some(&line.id))
                    .and_then(|_| worker::spawn_detached(&self.store.path));
                self.result(r, "Line regeneration queued");
            }
            if let Some(asset) = assets
                .iter()
                .find(|asset| Some(&asset.id) == line.asset_id.as_ref())
                && ui.button("Listen").clicked()
            {
                self.stop_playback();
                if let Some(child) = &mut self.audio {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                #[cfg(target_os = "macos")]
                match std::process::Command::new("/usr/bin/afplay")
                    .arg(&asset.path)
                    .spawn()
                {
                    Ok(child) => self.audio = Some(child),
                    Err(e) => self.message = e.to_string(),
                }
            }
        });
        ui.label(RichText::new("Edit this line").strong());
        ui.collapsing("Speaker & shot placement", |ui| {
            self.speaker_visibility_control(ui, &line.id);
            self.dialogue_move_control(ui, &line, shots);
        });
        egui::CollapsingHeader::new("Words & voice").id_salt((&line.id,"words")).default_open(true).show(ui, |ui| {
                    let mut draft = self.dialogue_drafts.get(&line.id).cloned().unwrap_or_else(||line.clone());
                    if draft.revision != line.revision {
                        ui.label(RichText::new("This line changed elsewhere. Your draft is preserved; reload before applying another edit.").color(GOLD));
                    }
                    let mut changed = ui.add(egui::TextEdit::multiline(&mut draft.text).desired_rows(3).desired_width(f32::INFINITY)).changed();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Voice");
                        changed |= crate::voice_ui::picker(ui,(&line.id,"edit-voice"),&mut draft.voice,voices);
                        ui.label("Speed");
                        changed |= ui.add(egui::DragValue::new(&mut draft.speed).range(0.5..=2.).speed(0.05)).changed();
                    });
                    if changed { self.dialogue_drafts.insert(line.id.clone(),draft.clone()); }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Save changes").clicked() {
                            let result=self.store.save_dialogue_draft(&draft);
                            if result.is_ok() { self.dialogue_drafts.remove(&line.id); }
                            self.result(result.map(|_|()), "Dialogue saved; generate speech for changed words");
                        }
                        if ui.button("Reload saved line").clicked() { self.dialogue_drafts.remove(&line.id); }
                    });
                });
        ui.label(
            RichText::new(format!(
                "{} · {:.2}x speed",
                crate::voice_ui::name(&line.voice),
                line.speed
            ))
            .color(GOLD),
        );
        ui.collapsing("Audio timing across cuts",|ui|{
                let mut bridge = line.timeline_start.is_some();
                let mut timing_changed=ui.checkbox(&mut bridge,"Continue this line across camera cuts").on_hover_text("Place the complete line on the episode timeline; it will continue over reaction shots instead of being trimmed to this shot.").changed();
                if timing_changed {
                    line.timeline_start = if bridge { Some(shot_start) } else { None };
                }
                if let Some(start) = &mut line.timeline_start {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Episode start");
                        timing_changed |= ui
                            .add(
                                egui::DragValue::new(start)
                                    .range(0. ..=1800.)
                                    .speed(0.05)
                                    .suffix(" s"),
                            )
                            .changed();
                    });
                }
                if timing_changed {
                    let r = self
                        .store
                        .set_dialogue_timing(&line.id, line.timeline_start);
                    self.result(r, "Dialogue timing saved; voice preserved");
                }

                });
        if let Some(asset) = assets
            .iter()
            .find(|a| Some(&a.id) == line.asset_id.as_ref())
        {
            ui.label(format!("{:.2}s generated", asset.duration));
            ui.collapsing("Talking footage · experimental",|ui|{
                    let speaker_visible = self
                        .store
                        .dialogue_direction(&line.id)
                        .is_ok_and(|d| d.speaker_visible);
                    let mut target = self
                        .performance_targets
                        .get(&line.id)
                        .cloned()
                        .unwrap_or(shot.clone());
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Acting coverage");
                        egui::ComboBox::from_id_salt((&line.id, "performance-target"))
                            .selected_text(
                                shots
                                    .iter()
                                    .find(|s| s.id == target)
                                    .map(|s| s.title.as_str())
                                    .unwrap_or("Choose a shot"),
                            )
                            .show_ui(ui, |ui| {
                                for item in shots {
                                    ui.selectable_value(&mut target, item.id.clone(), &item.title);
                                }
                            });
                    });
                    self.performance_targets
                        .insert(line.id.clone(), target.clone());
                    if ui.add_enabled(speaker_visible,egui::Button::new("Generate acting with this voice · experimental")).on_hover_text("Generate this coverage window from the matching part of the visible speaker's recorded line. Continuations must use the same primary character. Full episode audio is preserved; acting and lip sync still need review.").clicked() {
                        let result=self.store.enqueue_performance_window(&target,&asset.id,None,Some(&line.id)).and_then(|_|worker::spawn_detached(&self.store.path));
                        self.result(result,"Experimental windowed performance queued; review before choosing a take");
                    }
                    if speaker_visible
                        && line.timeline_start.is_none()
                        && let Ok(planned) = self.store.shot(&shot)
                        && let Some(video) = planned.asset_id
                        && ui.button("Synchronize lips · experimental").clicked()
                    {
                        let request = production::LipSyncRequest {
                            video_asset_id: video,
                            audio_asset_id: asset.id.clone(),
                            shot_id: Some(shot.clone()),
                        };
                        let r = self
                            .store
                            .enqueue_lipsync(&request)
                            .and_then(|_| worker::spawn_detached(&self.store.path));
                        self.result(r, "Experimental lip synchronization queued");
                    }
                    });
        } else {
            ui.label(
                RichText::new("Draft · generate speech when the words are ready").color(MUTED),
            );
        }
    }
}
