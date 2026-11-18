//! One explicit save action for the current series, using guarded domain services.
use crate::Studio;
use anyhow::Result;
fn equal(a: &impl serde::Serialize, b: &impl serde::Serialize) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}
impl Studio {
    pub(crate) fn save_project(&mut self) {
        if self.portable_task.is_some() {
            return;
        }
        let Some(series) = self.selected.clone() else {
            return;
        };
        let result = self.save_project_inner(&series);
        let complete = matches!(&result, Ok(issues) if issues.is_empty());
        self.message = match result {
            Ok(issues) if issues.is_empty() => "Project saved across all workspaces".into(),
            Ok(issues) => format!(
                "Project saved with items needing attention: {}. Unsaved drafts were preserved.",
                issues.join(" · ")
            ),
            Err(error) => format!("Project save stopped: {error}. Your drafts remain in place."),
        };
        if complete {
            match crate::portable_project::link(&self.store, &series) {
                Ok(Some(link)) => {
                    if matches!(link.folder.try_exists(), Ok(false)) {
                        self.message =
                            "Saved locally. Choose where to save the portable project.".into();
                        self.save_portable_as();
                    } else {
                        self.begin_portable_save(link.folder, Some(link.revision));
                    }
                    return;
                }
                Err(e) => {
                    self.message =
                        format!("Saved locally, but could not read the project folder: {e}");
                    self.project_save_notice =
                        Some((false, self.message.clone(), std::time::Instant::now()));
                    return;
                }
                Ok(None) => {
                    match crate::portable_project::default_project_folder(&self.store, &series) {
                        Ok(folder) => self.begin_portable_save(folder, None),
                        Err(error) => {
                            self.message =
                                format!("Saved on this Mac; iCloud save needs attention: {error}");
                            self.project_save_notice =
                                Some((false, self.message.clone(), std::time::Instant::now()));
                        }
                    }
                    return;
                }
            }
        }
        self.project_save_notice =
            Some((complete, self.message.clone(), std::time::Instant::now()));
    }
    pub(crate) fn project_save_confirmation(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let Some((complete, message, shown_at)) = self.project_save_notice.clone() else {
            return;
        };
        let mut dismiss = false;
        if complete {
            let lifetime = std::time::Duration::from_secs(4);
            if shown_at.elapsed() >= lifetime {
                self.project_save_notice = None;
                return;
            }
            ctx.request_repaint_after(lifetime.saturating_sub(shown_at.elapsed()));
            egui::Area::new(egui::Id::new("project-saved-toast"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0., 20.))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(12.)
                        .corner_radius(10.)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(16., 16.),
                                    egui::Sense::click(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        "Dismiss notification",
                                    )
                                });
                                let center = rect.center();
                                ui.painter().circle_filled(
                                    center,
                                    6.,
                                    egui::Color32::from_rgb(255, 95, 87),
                                );
                                if response.hovered() || response.has_focus() {
                                    let stroke =
                                        egui::Stroke::new(1., egui::Color32::from_rgb(80, 20, 18));
                                    ui.painter().line_segment(
                                        [
                                            center + egui::vec2(-2., -2.),
                                            center + egui::vec2(2., 2.),
                                        ],
                                        stroke,
                                    );
                                    ui.painter().line_segment(
                                        [
                                            center + egui::vec2(-2., 2.),
                                            center + egui::vec2(2., -2.),
                                        ],
                                        stroke,
                                    );
                                }
                                dismiss = response.on_hover_text("Dismiss").clicked();
                                ui.add_space(5.);
                                ui.label(
                                    egui::RichText::new("Project saved")
                                        .strong()
                                        .color(crate::GOLD),
                                );
                            });
                            ui.add(egui::Label::new(&message).wrap());
                        });
                });
        } else {
            let mut open = true;
            egui::Window::new("Project save needs attention")
                .id(egui::Id::new("project-save-issues-compact"))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .auto_sized()
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_width(380.);
                    egui::ScrollArea::vertical()
                        .max_height(260.)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(message).wrap());
                        });
                    ui.add_space(10.);
                    dismiss = ui.add(crate::theme::primary("OK")).clicked();
                });
            dismiss |= !open;
        }
        if dismiss {
            self.project_save_notice = None;
        }
    }
    pub(crate) fn save_project_inner(&mut self, series: &str) -> Result<Vec<String>> {
        // Preserve all drafts before any service advances its saved baseline.
        let directory = self.store.root().join("drafts");
        std::fs::create_dir_all(&directory)?;
        let snapshot = serde_json::json!({"series_id":series,"synopsis":self.synopsis_drafts,"profiles":self.profile_drafts,"portrait_prompts":self.portrait_prompts,"story_plans":self.story_drafts,"episode_id":self.episode_id,"screenplay":self.script,"screenplay_baseline":self.script_baseline,"generation_plans":self.video_planner.drafts,"library":self.library_drafts,"dialogue":self.dialogue_drafts,"sound":self.cue_drafts,"fades":self.fade_drafts,"trims":self.shot_trim_drafts,"directions":self.shot_directions,"cameras":self.camera_drafts,"reviews":self.review_drafts,"new_item":{"name":self.library_name,"description":self.library_description,"profile":self.new_character_profile,"reference":self.library_reference,"style":self.new_portrait_style,"episode_title":self.episode_title}});
        std::fs::write(
            directory.join(format!("project-{series}-{}.json", uuid::Uuid::new_v4())),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        self.snapshot_new_dialogue()?;
        self.snapshot_shot_form()?;
        let episodes: std::collections::HashSet<_> = self
            .store
            .episodes(series)?
            .into_iter()
            .map(|e| e.id)
            .collect();
        let entries: std::collections::HashSet<_> = self
            .store
            .library(series, "character")?
            .into_iter()
            .chain(self.store.library(series, "location")?)
            .map(|e| e.id)
            .collect();
        let mut issues = Vec::new();
        if let Err(error) = self.editor.save_for_series(&self.store, series) {
            issues.push(format!("Editing room: {error}"));
        }
        if let Err(error) = self.publisher.save_for_series(&self.store, series) {
            issues.push(format!("Publish: {error}"));
        }
        macro_rules! attempt {
            ($label:expr,$expr:expr,$ok:expr) => {
                match $expr {
                    Ok(value) => $ok(value),
                    Err(error) => issues.push(format!("{}: {}", $label, error)),
                }
            };
        }
        if let Some(draft) = self.synopsis_drafts.get_mut(series)
            && (draft.title != draft.expected_title || draft.premise != draft.expected_premise)
        {
            attempt!("Synopsis", self.store.save_synopsis(draft), |_| {
                draft.expected_title = draft.title.clone();
                draft.expected_premise = draft.premise.clone();
            });
        }
        for (id, draft) in &mut self.profile_drafts {
            if entries.contains(id) && draft.dirty() {
                attempt!(
                    &draft.brief.name,
                    self.store.save_character_profile(draft),
                    |saved| {
                        *draft = saved;
                        self.library_drafts.remove(id);
                    }
                );
            }
        }
        for (id, draft) in &mut self.library_drafts {
            if entries.contains(id)
                && (draft.name != draft.expected_name
                    || draft.description != draft.expected_description)
            {
                attempt!(
                    &draft.name,
                    self.store.update_library_entry(draft),
                    |saved: crate::production::LibraryEntry| {
                        draft.expected_name = saved.name;
                        draft.expected_description = saved.description;
                    }
                );
            }
        }
        for (id, draft) in &mut self.portrait_prompts {
            if entries.contains(id) && draft.expected.as_deref() != Some(&draft.prompt) {
                attempt!(
                    "Portrait prompt",
                    self.store.save_portrait_prompt(draft),
                    |saved| *draft = saved
                );
            }
        }
        if let Some(plan) = self.story_drafts.get_mut(series) {
            match self.store.series_plan(series) {
                Ok(saved) if !equal(&saved, plan) => {
                    attempt!("Overall plot", self.store.save_series_plan(plan), |saved| {
                        *plan = saved
                    });
                }
                Err(error) => issues.push(format!("Overall plot: {error}")),
                _ => {}
            }
        }
        if self
            .episode_id
            .as_ref()
            .is_some_and(|id| episodes.contains(id))
            && !self.save_script()
        {
            issues.push(self.message.clone());
        }
        for (id, plan) in &mut self.video_planner.drafts {
            if episodes.contains(id) {
                attempt!(
                    "Generation plan",
                    self.store.save_generation_plan(plan),
                    |saved| *plan = saved
                );
            }
        }
        for (id, draft) in &mut self.dialogue_drafts {
            if self
                .store
                .shot(&draft.shot_id)
                .is_ok_and(|shot| episodes.contains(&shot.episode_id))
            {
                attempt!(
                    format!("Dialogue {id}"),
                    self.store.save_dialogue_draft(draft),
                    |saved| *draft = saved
                );
            }
        }
        for draft in self.cue_drafts.values_mut() {
            if episodes.contains(&draft.cue.episode_id) && draft.cue != draft.expected {
                attempt!(
                    "Sound cue",
                    self.store.save_audio_cue(&draft.cue, Some(&draft.expected)),
                    |_| draft.expected = draft.cue.clone()
                );
            }
        }
        for (id, draft) in &mut self.shot_directions {
            if self
                .store
                .shot(id)
                .is_ok_and(|s| episodes.contains(&s.episode_id))
                && self
                    .store
                    .shot_direction(id)
                    .is_ok_and(|saved| !equal(&saved, draft))
            {
                attempt!(
                    "Shot direction",
                    self.store.save_shot_direction(id, draft),
                    |saved| *draft = saved
                );
            }
        }
        for draft in self.camera_drafts.values_mut() {
            if entries.contains(&draft.location_id) {
                let saved = self
                    .store
                    .camera_setups(&draft.location_id)?
                    .into_iter()
                    .find(|c| c.id == draft.id);
                if saved.as_ref().is_none_or(|s| !equal(s, draft)) {
                    attempt!("Camera", self.store.save_camera_setup(draft), |saved| {
                        *draft = saved
                    });
                }
            }
        }
        for draft in self.fade_drafts.values_mut() {
            if self
                .store
                .shot(&draft.shot_id)
                .is_ok_and(|s| episodes.contains(&s.episode_id))
                && draft.fades != draft.expected
            {
                attempt!(
                    "Picture fades",
                    self.store.set_picture_fades(draft),
                    |saved| draft.expected = saved
                );
            }
        }
        for draft in self.shot_trim_drafts.values_mut() {
            if self
                .store
                .shot(&draft.shot_id)
                .is_ok_and(|s| episodes.contains(&s.episode_id))
                && (draft.source_start != draft.expected_source_start
                    || draft.duration != draft.expected_duration)
            {
                attempt!(
                    "Picture trim",
                    self.store.trim_shot(draft),
                    |saved: crate::production::Shot| {
                        draft.expected_source_start = saved.source_start;
                        draft.expected_duration = saved.duration;
                    }
                );
            }
        }
        let assets = self.store.assets()?;
        for (id, draft) in &mut self.review_drafts {
            let belongs = assets.iter().find(|a| &a.id == id).is_some_and(|a| {
                let request = &a.metadata["request"];
                request["library_id"]
                    .as_str()
                    .is_some_and(|id| entries.contains(id))
                    || request["episode_id"]
                        .as_str()
                        .is_some_and(|id| episodes.contains(id))
                    || request["shot_id"].as_str().is_some_and(|id| {
                        self.store
                            .shot(id)
                            .is_ok_and(|s| episodes.contains(&s.episode_id))
                    })
            });
            if self
                .store
                .asset_review(id)
                .is_ok_and(|saved| !equal(&saved, draft))
            {
                if belongs {
                    attempt!(
                        "Media review",
                        self.store.save_asset_review(draft),
                        |saved| *draft = saved
                    );
                } else {
                    issues.push("Unscoped media review retained in recovery; save it in the asset workspace".into());
                }
            }
        }
        if let Some(id) = &self.edit_shot
            && let Ok(saved) = self.store.shot(id)
            && (saved.title != self.shot_title
                || saved.prompt != self.shot_prompt
                || saved.camera != self.shot_camera
                || saved.duration != self.shot_duration)
        {
            issues.push(
                "Legacy shot form preserved in recovery; use Save shot in Visual plan".into(),
            );
        }
        if let Err(error) = self.save_project_chat(series) {
            issues.push(format!("Chat draft: {error}"));
        }
        self.story_reports.remove(series);
        // Creation forms remain drafts rather than creating unnamed records or paid jobs.
        if !self.library_name.trim().is_empty()
            || !self.library_description.trim().is_empty()
            || !self.episode_title.trim().is_empty()
        {
            issues.push("Unfinished new-item forms are retained in the project recovery snapshot; finish creating them in their workspace".into());
        }
        Ok(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_save_covers_workspaces_and_preserves_conflicting_and_other_series_drafts() {
        let root = std::env::temp_dir().join(format!("df-project-save-{}", uuid::Uuid::new_v4()));
        let store = crate::store::Store::open(root.join("test.sqlite")).unwrap();
        let series = store.create_series("Original", "Premise").unwrap();
        let other = store.create_series("Other", "Other premise").unwrap();
        let episode = store.create_episode(&series.id, "Pilot").unwrap();
        let cast = store
            .add_library(&series.id, "character", "Lead", "Original brief", None)
            .unwrap();
        let other_cast = store
            .add_library(&other.id, "character", "Other lead", "Keep", None)
            .unwrap();
        let mut app = Studio::from_store(store);
        app.selected = Some(series.id.clone());
        app.episode_id = Some(episode.id.clone());
        app.script = "Saved screenplay".into();
        app.synopsis_drafts.insert(
            series.id.clone(),
            crate::synopsis::SynopsisDraft {
                id: series.id.clone(),
                title: "Revised".into(),
                premise: "Revised premise".into(),
                expected_title: series.name,
                expected_premise: series.premise,
            },
        );
        let mut profile = app.store.character_profile(&cast.id).unwrap();
        profile
            .profile
            .fields
            .insert("height".into(), "170 cm".into());
        profile.refresh_brief();
        app.profile_drafts.insert(cast.id.clone(), profile);
        let mut profile = app.store.character_profile(&other_cast.id).unwrap();
        profile.profile.summary = "Do not save this project".into();
        profile.refresh_brief();
        app.profile_drafts.insert(other_cast.id.clone(), profile);
        let mut prompt = app.store.portrait_prompt(&cast).unwrap();
        prompt.prompt = "Portrait direction".into();
        app.portrait_prompts.insert(cast.id.clone(), prompt);
        let mut plan = app.store.series_plan(&series.id).unwrap();
        plan.ending = "Saved ending".into();
        app.story_drafts.insert(series.id.clone(), plan);
        let mut generation = app.store.generation_plan(&episode.id).unwrap();
        generation.segments[0].prompt = "Segment plan".into();
        app.video_planner
            .drafts
            .insert(episode.id.clone(), generation);
        assert!(app.save_project_inner(&series.id).unwrap().is_empty());
        assert_eq!(
            app.store.get_episode(&episode.id).unwrap().screenplay,
            "Saved screenplay"
        );
        assert_eq!(
            app.store.series_plan(&series.id).unwrap().ending,
            "Saved ending"
        );
        assert_eq!(
            app.store
                .character_profile(&cast.id)
                .unwrap()
                .profile
                .fields["height"],
            "170 cm"
        );
        assert_eq!(
            app.store
                .character_profile(&other_cast.id)
                .unwrap()
                .profile
                .summary,
            "Keep"
        );
        assert_eq!(
            app.store.generation_plan(&episode.id).unwrap().segments[0].prompt,
            "Segment plan"
        );
        assert!(app.store.jobs().unwrap().is_empty());
        let revision = app.store.series_plan(&series.id).unwrap().revision;
        assert!(app.save_project_inner(&series.id).unwrap().is_empty());
        assert_eq!(
            app.store.series_plan(&series.id).unwrap().revision,
            revision
        );
        app.store
            .conn
            .execute(
                "UPDATE library SET description='External edit' WHERE id=?1",
                [&cast.id],
            )
            .unwrap();
        let draft = app.profile_drafts.get_mut(&cast.id).unwrap();
        draft.profile.summary = "Local unsaved".into();
        draft.refresh_brief();
        assert!(!app.save_project_inner(&series.id).unwrap().is_empty());
        assert!(app.profile_drafts[&cast.id].dirty());
        assert_eq!(
            app.store.library(&series.id, "character").unwrap()[0].description,
            "External edit"
        );
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
