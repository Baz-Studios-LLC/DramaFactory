mod asset_browser;
mod asset_catalog;
mod captions;
mod cast_reference;
mod character_deletion;
mod character_edit;
mod character_grid;
mod character_profile;
mod chat_images;
mod chat_markdown;
mod completion;
mod credentials;
mod credit_status;
mod dialogue_direction;
mod dialogue_editing;
mod dialogue_line_ui;
mod dialogue_timing;
mod dialogue_workspace;
mod directing;
mod directing_ui;
mod editing;
mod editing_room;
mod editing_room_ui;
mod episode_ui;
mod generation_plan;
mod hosted_image;
mod hosted_video;
mod image_inspector;
mod job_timing;
mod library_editing;
mod mcp;
mod media;
mod models;
mod openrouter;
mod orchestration;
mod performance;
mod performance_window;
mod portable_project;
mod portrait_deletion;
mod portrait_delivery;
mod portrait_gallery;
mod portrait_prompt;
mod portrait_style;
mod portrait_ui;
mod preview;
mod production;
mod project_context;
mod project_save;
mod provider_settings;
mod readiness;
mod rehearsal;
mod reviews;
mod runtime;
mod scrub_preview;
mod segment_takes;
mod shot_workspace;
mod sidekick;
mod sidekick_edit;
mod sidekick_episode;
mod sidekick_generation;
mod sidekick_plot;
mod sidekick_ui;
mod sound;
mod store;
mod story;
mod story_ui;
mod synopsis;
mod take_gallery;
mod theme;
mod transitions;
mod updater;
mod updates;
mod video_plan;
mod video_plan_ui;
mod voice_ui;
mod worker;
mod workflow_ui;
mod workspace_jobs;
use eframe::egui::{self, Color32, RichText, Vec2};
use production::{ImageRequest, RenderRequest, WorkerConfig};
use std::path::PathBuf;
use store::Store;
const GOLD: Color32 = Color32::from_rgb(214, 182, 126);
const MUTED: Color32 = Color32::from_rgb(157, 160, 169);
const BG: Color32 = Color32::from_rgb(18, 20, 24);
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Studio,
    Synopsis,
    Story,
    Series,
    Cast,
    Locations,
    Scenes,
    Assets,
    Timeline,
    Dialogue,
    Images,
    Queue,
    Settings,
    EditingRoom,
}
struct Studio {
    store: Store,
    delete_character: Option<production::LibraryEntry>,
    open_character: Option<(String, String)>,
    profile_drafts: std::collections::HashMap<String, character_profile::ProfileDraft>,
    new_character_profile: character_profile::CharacterProfile,
    new_portrait_style: portrait_style::PortraitStyle,
    portrait_prompts: std::collections::HashMap<String, portrait_prompt::PromptDraft>,
    sidekick: sidekick_ui::Sidekick,
    synopsis_drafts: std::collections::HashMap<String, synopsis::SynopsisDraft>,
    shot_directions: std::collections::BTreeMap<String, directing::ShotDirection>,
    camera_drafts: std::collections::BTreeMap<String, directing::CameraSetup>,
    story_section: story_ui::StorySection,
    story_search: String,
    story_drafts: std::collections::BTreeMap<String, story::SeriesPlan>,
    story_exports: std::collections::BTreeMap<String, String>,
    story_reports: std::collections::BTreeMap<String, Result<story::StoryReport, String>>,
    story_check: Option<std::sync::mpsc::Receiver<(String, Result<story::StoryReport, String>)>>,
    provider_settings: provider_settings::ProviderSettings,
    video_planner: video_plan_ui::VideoPlanner,
    editor: editing_room_ui::Editor,
    page: Page,
    asset_browser: asset_browser::AssetBrowser,
    sidekick_visible: bool,
    selected: Option<String>,
    episode_id: Option<String>,
    episode_title: String,
    episode_search: String,
    episode_section: episode_ui::EpisodeSection,
    script: String,
    script_baseline: String,
    last_sync: std::time::Instant,
    request: RenderRequest,
    image_request: ImageRequest,
    shot_title: String,
    shot_prompt: String,
    shot_workspace: shot_workspace::ShotWorkspace,
    edit_shot: Option<String>,
    shot_camera: String,
    shot_duration: f64,
    shot_trim_drafts: std::collections::HashMap<String, editing::ShotTrim>,
    library_name: String,
    library_description: String,
    library_reference: Option<PathBuf>,
    message: String,
    portable_task: Option<portable_project::Task>,
    project_save_notice: Option<(bool, String, std::time::Instant)>,
    config: WorkerConfig,
    update_rx: Option<std::sync::mpsc::Receiver<updates::UpdateStatus>>,
    update_status: String,
    automatic_updates: bool,
    last_update_check: std::time::Instant,
    dialogue_workspace: dialogue_workspace::DialogueWorkspace,
    new_dialogue_drafts: std::collections::HashMap<String, dialogue_workspace::NewLineDraft>,
    dialogue_text: String,
    review_drafts: std::collections::HashMap<String, reviews::AssetReview>,
    readiness: Option<readiness::EpisodeReadiness>,
    dialogue_timing_report: Option<dialogue_timing::DialogueTiming>,
    fade_drafts: std::collections::HashMap<String, transitions::FadeEdit>,
    cue_drafts: std::collections::HashMap<String, sound::CueDraft>,
    library_drafts: std::collections::HashMap<String, library_editing::LibraryEdit>,
    dialogue_drafts: std::collections::HashMap<String, production::Dialogue>,
    dialogue_voice: String,
    dialogue_speed: f64,
    dialogue_shot: Option<String>,
    dialogue_move_targets: std::collections::HashMap<String, String>,
    performance_targets: std::collections::HashMap<String, String>,
    audio: Option<std::process::Child>,
    preview: Option<media::Preview>,
    image_inspector: Option<image_inspector::Inspector>,
    preview_asset: Option<String>,
}
impl Studio {
    fn new(cc: &eframe::CreationContext<'_>, store: Store) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::install(&cc.egui_ctx);
        let mut app = Self::from_store(store);
        app.update_rx = Some(updates::check(
            app.store.path.clone(),
            app.automatic_updates,
        ));
        app.message = worker::spawn_detached(&app.store.path)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        app
    }
    fn from_store(store: Store) -> Self {
        let selected = store.current_gui_series().unwrap_or_default();
        let remembered_episode = selected
            .as_deref()
            .and_then(|series| store.current_gui_episode(series).ok().flatten())
            .and_then(|id| store.get_episode(&id).ok());
        let episode_id = remembered_episode
            .as_ref()
            .map(|episode| episode.id.clone());
        let script = remembered_episode
            .map(|episode| episode.screenplay)
            .unwrap_or_default();
        let config = store.config().unwrap_or_default();
        let automatic_updates = store.automatic_updates().unwrap_or(true);
        Self {
            store,
            delete_character: None,
            open_character: None,
            profile_drafts: Default::default(),
            new_character_profile: Default::default(),
            new_portrait_style: Default::default(),
            portrait_prompts: Default::default(),
            sidekick: Default::default(),
            synopsis_drafts: Default::default(),
            shot_directions: Default::default(),
            camera_drafts: Default::default(),
            story_section: Default::default(),
            story_search: String::new(),
            story_drafts: Default::default(),
            story_exports: Default::default(),
            story_reports: Default::default(),
            story_check: None,
            provider_settings: Default::default(),
            video_planner: Default::default(),
            editor: Default::default(),
            page: Page::Synopsis,
            asset_browser: Default::default(),
            sidekick_visible: true,
            selected,
            episode_id,
            episode_title: String::new(),
            episode_search: String::new(),
            episode_section: Default::default(),
            script_baseline: script.clone(),
            script,
            last_sync: std::time::Instant::now(),
            request: RenderRequest::default(),
            image_request: ImageRequest::default(),
            shot_title: String::new(),
            shot_prompt: String::new(),
            shot_workspace: Default::default(),
            edit_shot: None,
            shot_camera: String::new(),
            shot_duration: 3.,
            shot_trim_drafts: Default::default(),
            library_name: String::new(),
            library_description: String::new(),
            library_reference: None,
            message: String::new(),
            project_save_notice: None,
            portable_task: None,
            config,
            update_rx: None,
            automatic_updates,
            last_update_check: std::time::Instant::now(),
            dialogue_workspace: Default::default(),
            new_dialogue_drafts: Default::default(),
            dialogue_text: String::new(),
            review_drafts: Default::default(),
            readiness: None,
            dialogue_timing_report: None,
            fade_drafts: Default::default(),
            cue_drafts: Default::default(),
            library_drafts: Default::default(),
            dialogue_drafts: Default::default(),
            dialogue_voice: "af_heart".into(),
            dialogue_speed: 1.,
            dialogue_shot: None,
            dialogue_move_targets: Default::default(),
            performance_targets: Default::default(),
            audio: None,
            update_status: "Checking published releases…".into(),
            preview: None,
            image_inspector: None,
            preview_asset: None,
        }
    }
    fn stop_playback(&mut self) {
        self.preview = None;
        if let Some(mut child) = self.audio.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn load<T: Default>(&mut self, result: anyhow::Result<T>) -> T {
        match result {
            Ok(v) => v,
            Err(e) => {
                self.message = format!("{e:#}");
                T::default()
            }
        }
    }
    fn result<T>(&mut self, result: anyhow::Result<T>, ok: &str) {
        self.message = match result {
            Ok(_) => ok.into(),
            Err(e) => format!("{e:#}"),
        };
    }
    fn title(ui: &mut egui::Ui, kicker: &str, title: &str, sub: &str) {
        ui.label(RichText::new(kicker).size(10.).color(GOLD).strong());
        ui.add_space(4.);
        ui.label(RichText::new(title).size(28.).strong());
        ui.label(RichText::new(sub).color(MUTED));
        ui.add_space(20.);
    }
    fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1_f32, theme::BORDER))
            .corner_radius(12)
            .inner_margin(20.)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                content(ui)
            });
    }
    fn field(ui: &mut egui::Ui, label: &str, value: &mut String, rows: usize) {
        ui.label(RichText::new(label).size(10.).color(MUTED));
        if rows == 1 {
            ui.add(
                egui::TextEdit::singleline(value)
                    .margin(Vec2::new(11., 9.))
                    .desired_width(f32::INFINITY),
            );
        } else {
            ui.add(
                egui::TextEdit::multiline(value)
                    .margin(Vec2::new(12., 11.))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows),
            );
        }
    }
    fn save_script(&mut self) -> bool {
        if self.script == self.script_baseline {
            return true;
        }
        if let Some(id) = &self.episode_id {
            match self
                .store
                .save_screenplay_if_unchanged(id, &self.script_baseline, &self.script)
            {
                Ok(_) => {
                    self.script_baseline = self.script.clone();
                    true
                }
                Err(e) => {
                    self.message = e.to_string();
                    false
                }
            }
        } else {
            true
        }
    }
    fn episode_picker(&mut self, ui: &mut egui::Ui) {
        self.episode_navigation(ui);
    }
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        self.workflow_home(ui);
    }
    fn series(&mut self, ui: &mut egui::Ui) {
        self.episode_workspace(ui);
    }
    fn library(&mut self, ui: &mut egui::Ui, kind: &str) {
        Self::title(
            ui,
            if kind == "character" {
                "CASTING"
            } else {
                "WORLD BUILDING"
            },
            if kind == "character" {
                "An unforgettable cast."
            } else {
                "Set the scene."
            },
            "Keep identities and references consistent across your series.",
        );
        let Some(series) = self.selected.clone() else {
            ui.label("Choose a series first.");
            return;
        };
        let entries = self.load(self.store.library(&series, kind));
        if kind == "character"
            && self.open_character.as_ref().is_some_and(|(owner, id)| {
                owner != &series || !entries.iter().any(|entry| &entry.id == id)
            })
        {
            self.open_character = None;
        }
        if kind != "character" || self.open_character.is_none() {
            egui::CollapsingHeader::new(if kind == "character" {
            "+ Add a character"
        } else {
            "+ Add a location"
        })
        .id_salt((&series, kind, "new-library-entry"))
        .default_open(entries.is_empty())
        .show(ui, |ui| {
            Self::card(ui, |ui| {
                Self::field(ui, "NAME", &mut self.library_name, 1);
                Self::field(
                    ui,
                    "DESCRIPTION / CONTINUITY NOTES",
                    &mut self.library_description,
                    3,
                );
                if kind == "character" {
                    self.new_character_profile.summary = self.library_description.clone();
                    ui.collapsing("Character details (optional)", |ui| {
                        character_profile::fields_ui(ui, "new-character", &mut self.new_character_profile);
                        ui.collapsing("Full character brief", |ui| {
                            ui.label(self.new_character_profile.brief());
                        });
                    });
                    self.new_portrait_style = self.store.series_portrait_style(&series).unwrap_or_default().unwrap_or_default();
                    ui.small(format!("Image style: {} · set on Synopsis", self.new_portrait_style.label()));
                }
                ui.horizontal(|ui| {
                    if ui.button("Choose reference image").clicked() {
                        self.library_reference = rfd::FileDialog::new()
                            .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                            .pick_file();
                    }
                    if let Some(p) = &self.library_reference {
                        ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                    }
                    if kind == "character" && ui.add_enabled(
                        !self.library_name.trim().is_empty() && !self.new_character_profile.brief().trim().is_empty(),
                        egui::Button::new("Create character & portrait"),
                    ).clicked() {
                        let result=self.store.create_profile_character(&series,&self.library_name,&self.new_character_profile,self.library_reference.as_deref(),self.new_portrait_style);
                        match result {
                            Ok(entry)=>{
                                self.library_name.clear();self.library_description.clear();self.library_reference=None;
                                self.new_character_profile=Default::default();
                                let result=self.store.portrait_prompt(&entry).and_then(|draft|self.store.enqueue_grok_portrait(&entry.id,&draft.prompt));
                                match result {
                                    Ok(_)=>{let start=crate::worker::spawn_detached(&self.store.path);self.result(start,"Character created and portrait queued; review before choosing it");},
                                    Err(error)=>self.message=format!("Character created; image was not queued: {error}"),
                                }
                            },Err(error)=>self.message=error.to_string(),
                        }
                    }
                    if ui
                        .button(RichText::new("Add to library").color(GOLD))
                        .clicked()
                    {
                        let r = if kind == "character" {
                            self.store.create_profile_character(&series,&self.library_name,&self.new_character_profile,self.library_reference.as_deref(),self.new_portrait_style)
                        } else { self.store.add_library(
                            &series,
                            kind,
                            &self.library_name,
                            &self.library_description,
                            self.library_reference.as_deref(),
                        ) };
                        if r.is_ok() {
                            self.library_name.clear();
                            self.library_description.clear();
                            self.library_reference = None;
                            self.new_character_profile = Default::default();
                        }
                        self.result(r, "Library entry created");
                    }
                });
            });
        });
        }
        ui.add_space(18.);
        if kind == "character" {
            if self.open_character.is_none() {
                self.character_grid(ui, &series, &entries);
                self.deleted_character_list(ui, &series);
                return;
            }
            if ui.button("← Back to characters").clicked() {
                self.open_character = None;
                return;
            }
            ui.add_space(12.);
        }
        let deleted_portraits = self.store.deleted_portraits().unwrap_or_default();
        for entry in entries {
            if kind == "character"
                && self
                    .open_character
                    .as_ref()
                    .is_none_or(|(_, id)| id != &entry.id)
            {
                continue;
            }
            let candidate = if entry.reference.is_none() {
                self.store
                    .assets()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|asset| {
                        !deleted_portraits.contains(&asset.id)
                            && asset.metadata["provider"] == "openrouter"
                            && asset.metadata["request"]["library_id"] == entry.id
                    })
            } else {
                None
            };
            let displayed_portrait = entry
                .reference
                .as_deref()
                .or_else(|| candidate.as_ref().map(|asset| asset.path.as_path()));
            Self::card(ui, |ui| {
                let layout = if ui.available_width() < 620. {
                    egui::Layout::top_down(egui::Align::LEFT)
                } else {
                    egui::Layout::left_to_right(egui::Align::TOP)
                };
                ui.with_layout(layout, |ui| {
                    if let Some(p) = displayed_portrait {
                        ui.vertical(|ui| {
                            ui.set_width(220.);
                            ui.label(
                                RichText::new(if entry.reference.is_some() {
                                    "SELECTED REFERENCE"
                                } else {
                                    "PORTRAIT OPTION · NOT SELECTED"
                                })
                                .size(10.)
                                .color(MUTED),
                            );
                            self.reference_preview(
                                ui,
                                p,
                                if kind == "character" {
                                    Vec2::new(220., 280.)
                                } else {
                                    Vec2::new(260., 200.)
                                },
                            );
                            if let Some(candidate) = &candidate {
                                self.portrait_select_button(ui, &entry, &candidate.id);
                            }
                            if let Some(candidate) = &candidate {
                                self.portrait_delete_button(ui, &entry.id, &candidate.id);
                            }
                        });
                    }
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width());
                        let profile = self
                            .profile_drafts
                            .get(&entry.id)
                            .map(|draft| draft.profile.clone())
                            .or_else(|| {
                                self.store
                                    .saved_character_profile(&entry.id)
                                    .ok()
                                    .flatten()
                                    .filter(|profile| profile.brief() == entry.description)
                            });
                        let displayed = self
                            .profile_drafts
                            .get(&entry.id)
                            .map(|draft| draft.brief.clone())
                            .or_else(|| self.library_drafts.get(&entry.id).cloned());
                        ui.label(
                            RichText::new(
                                displayed
                                    .as_ref()
                                    .map(|draft| draft.name.as_str())
                                    .unwrap_or(&entry.name),
                            )
                            .size(23.)
                            .strong(),
                        );
                        ui.label(
                            RichText::new(
                                profile
                                    .as_ref()
                                    .map(|profile| profile.summary.as_str())
                                    .or_else(|| {
                                        displayed.as_ref().map(|draft| draft.description.as_str())
                                    })
                                    .unwrap_or(&entry.description),
                            )
                            .color(MUTED),
                        );
                        if let Some(profile) = &profile {
                            let details: Vec<_> = [
                                ("age", "Age"),
                                ("height", "Height"),
                                ("weight", "Weight"),
                                ("hair_color", "Hair"),
                                ("eye_color", "Eyes"),
                            ]
                            .iter()
                            .filter_map(|(key, label)| {
                                profile
                                    .fields
                                    .get(*key)
                                    .filter(|value| !value.trim().is_empty())
                                    .map(|value| format!("{label}: {value}"))
                            })
                            .collect();
                            if !details.is_empty() {
                                ui.small(details.join(" · "));
                            }
                        }
                        if kind != "character"
                            && displayed.as_ref().is_some_and(|draft| {
                                draft.name != entry.name || draft.description != entry.description
                            })
                        {
                            ui.label(
                                RichText::new("Unsaved draft · open Edit name and brief to save")
                                    .color(GOLD),
                            );
                        }
                        if kind == "character" {
                            self.character_profile_editor(ui, &entry);
                        } else {
                            self.library_brief_editor(ui, &entry);
                        }
                        self.grok_portrait_controls(ui, &entry, displayed_portrait);
                        if kind == "character" {
                            self.character_delete_controls(ui, &entry);
                        }
                    });
                });
            });
            ui.add_space(12.);
        }
        if kind == "character" {
            self.deleted_character_list(ui, &series);
        }
    }
    fn scenes(&mut self, ui: &mut egui::Ui) {
        self.queue(ui);
        if self
            .episode_id
            .as_ref()
            .is_some_and(|id| self.store.shots(id).is_ok_and(|shots| !shots.is_empty()))
        {
            ui.collapsing("Earlier shot plans", |ui| self.shot_workspace(ui));
        }
    }
    fn queue(&mut self, ui: &mut egui::Ui) {
        ui.heading("Episode generation");
        ui.add_space(8.);
        self.hosted_video_planner(ui);
        if self.video_planner.section == 0 || self.episode_id.is_none() {
            return;
        }
        ui.add_space(16.);
        ui.heading("Production activity");
        ui.checkbox(
            &mut self.video_planner.show_old_attempts,
            "Show older attempts",
        );
        let mut activity = Vec::new();
        for job in self.load(self.store.workspace_jobs(
            self.selected.as_deref().unwrap_or(""),
            self.episode_id.as_deref(),
        )) {
            if !self.video_planner.show_old_attempts
                && self.store.has_newer_segment_job(&job.id).unwrap_or(false)
                && !matches!(job.status.as_str(), "queued" | "running")
            {
                continue;
            }
            let kind = self
                .store
                .job_timing(&job.id)
                .ok()
                .map(|timing| timing.kind);
            if kind.as_deref()
                != Some(if self.video_planner.section == 1 {
                    "openrouter_video"
                } else {
                    "export"
                })
            {
                continue;
            }
            activity.push((job, kind));
        }
        let columns = ((ui.available_width() / 280.).floor() as usize).clamp(1, 3);
        for row in activity.chunks(columns) {
            ui.columns(columns, |uis| {
                for (column, (job, kind)) in row.iter().enumerate() {
                    let ui = &mut uis[column];
            Self::card(ui, |ui| {
                ui.set_height(280.);
                let retryable_status =
                    ["failed", "canceled", "blocked"].contains(&job.status.as_str());
                let uncertain_video = kind.as_deref() == Some("openrouter_video")
                    && retryable_status
                    && !self.store.video_retry_available(&job.id).unwrap_or(false);
                egui::ScrollArea::vertical()
                    .id_salt(("activity_card", &job.id))
                    .max_height(230.)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(job.status.to_uppercase())
                            .color(GOLD)
                            .size(10.),
                    );
                    ui.label(
                        RichText::new(self.store.job_created_local(&job.id).ok().flatten().unwrap_or_else(|| "Date unavailable".into()))
                            .color(MUTED)
                            .size(10.),
                    );
                });
                let summary: String = job.prompt.chars().take(110).collect();
                ui.label(
                    RichText::new(format!(
                        "{summary}{}",
                        if job.prompt.chars().count() > 110 {
                            "…"
                        } else {
                            ""
                        }
                    ))
                    .strong(),
                );
                ui.collapsing("Request details", |ui| {
                    ui.label(&job.prompt);
                });
                if let Ok(timing) = self.store.job_timing(&job.id) {
                    let hosted = timing.kind.starts_with("openrouter_");
                    if let Some(seconds) = timing.attempt_elapsed_seconds {
                        ui.small(format!(
                            "Elapsed {}",
                            job_timing::duration(seconds)
                        ));
                    }
                    if let Some(seconds) = timing.queued_seconds {
                        ui.small(format!(
                            "Waiting {}",
                            job_timing::duration(seconds)
                        ));
                    }
                    if job.status == "running" {
                        if hosted {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(&timing.stage);
                            });

                        } else {
                            ui.add(egui::ProgressBar::new(job.progress).show_percentage());
                            ui.label(RichText::new(&job.phase).color(MUTED));
                        }
                    }
                }
                if !job.error.is_empty() {
                    ui.label(RichText::new(&job.error).color(Color32::from_rgb(233, 151, 129)));
                }
                if uncertain_video {
                    ui.label("Submission outcome unknown. Retrying is blocked to avoid a duplicate charge.");
                    ui.hyperlink_to(
                        "Check OpenRouter video logs",
                        "https://openrouter.ai/workspaces/default/logs?tab=videos",
                    );
                }
                    });
                ui.add_space(12.);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(12., 8.);
                    ui.spacing_mut().item_spacing = egui::vec2(10., 8.);
                    if ["queued", "running", "blocked"].contains(&job.status.as_str())
                        && ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90., 36.))).clicked()
                    {
                        let r = self.store.cancel(&job.id);
                        self.result(r, "Cancellation requested");
                    }
                    if retryable_status && !uncertain_video && ui.add(egui::Button::new("Retry").min_size(egui::vec2(90., 36.))).clicked() {
                        let r = self
                            .store
                            .retry(&job.id)
                            .and_then(|_| worker::spawn_detached(&self.store.path));
                        self.result(r, "Retry queued");
                    }
                    if let Some(id) = &job.output_id
                        && ui.add(egui::Button::new("Play original").min_size(egui::vec2(90., 36.))).clicked()
                        && let Ok(assets) = self.store.assets()
                        && let Some(asset) = assets.iter().find(|asset| &asset.id == id)
                    {
                        #[cfg(target_os = "macos")]
                        {
                            let result = std::process::Command::new("/usr/bin/open")
                                .arg(&asset.path)
                                .spawn()
                                .map(|_| ())
                                .map_err(anyhow::Error::from);
                            self.result(result, "Opened original video");
                        }
                    }
                    if job.output_id.is_some() && ui.add(egui::Button::new("View asset").min_size(egui::vec2(90., 36.))).clicked() {
                        self.preview_asset = job.output_id.clone();
                        if let Some(id) = &job.output_id {
                            self.asset_browser.show_asset(id);
                        }
                        self.page = Page::Assets;
                    }
                });
            });
                }
            });
            ui.add_space(12.);
        }
    }
    fn assets(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "THE SCREENING ROOM",
            "Review the work.",
            "Original renders, full provenance, and a decision for every take.",
        );
        let Some(assets) = self.catalog_assets(ui) else {
            return;
        };
        if assets.is_empty() {
            Self::card(ui, |ui| {
                let filtered = !self.asset_browser.query.text.trim().is_empty()
                    || self.asset_browser.query.media != "all"
                    || self.asset_browser.query.review != "all"
                    || self.asset_browser.query.offset > 0;
                ui.label(if filtered {
                    "No media on this page matches the current view."
                } else {
                    "No generated assets yet."
                });
                ui.label(
                    RichText::new("Completed renders appear automatically. Clear filters or use Previous to return to other media.").color(MUTED),
                );
            });
        }
        if self.preview.is_some()
            && self
                .preview_asset
                .as_ref()
                .is_some_and(|id| !assets.iter().any(|asset| &asset.id == id))
        {
            self.stop_playback();
        }
        for asset in assets {
            Self::card(ui, |ui| {
                ui.horizontal(|ui| {
                    let poster = asset.path.with_file_name("poster.png");
                    if poster.is_file() {
                        ui.add(
                            egui::Image::from_uri(format!("file://{}", poster.display()))
                                .max_size(Vec2::new(180., 240.)),
                        );
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(self.store.asset_label(&asset)).size(20.));
                        let details = if asset.duration == 0. {
                            format!(
                                "Image · {} × {} · {}",
                                asset.width, asset.height, asset.review
                            )
                        } else if asset.width == 0 {
                            format!("Audio · {:.2}s · {}", asset.duration, asset.review)
                        } else {
                            format!(
                                "{} × {} · {:.2}s · {}",
                                asset.width, asset.height, asset.duration, asset.review
                            )
                        };
                        ui.label(RichText::new(details).color(GOLD));
                        if let Some(prompt) = asset.metadata["request"]["prompt"].as_str() {
                            ui.push_id((&asset.id,"prompt"),|ui|ui.collapsing("Generation prompt",|ui|{ui.label(prompt);}));
                        } else if let Some(text) = asset.metadata["request"]["text"].as_str() {
                            ui.label(text);
                        }
                        if asset.review=="rejected" && let Ok(review)=self.store.asset_review(&asset.id) && !review.note.trim().is_empty() {
                            ui.label(RichText::new(review.note.chars().take(240).collect::<String>()).color(MUTED));
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button(if asset.duration == 0. {
                                    "View image"
                                } else if asset.width == 0 {
                                    "Listen"
                                } else {
                                    "Play preview"
                                })
                                .clicked()
                            {
                                self.stop_playback();
                                if asset.duration == 0. {
                                    self.preview = None;
                                    self.preview_asset = Some(asset.id.clone());
                                } else if asset.width == 0 {
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
                                } else {
                                    match media::Preview::open(
                                        self.config.ffmpeg.clone(),
                                        asset.path.clone(),
                                        asset.width as f32 / asset.height as f32,
                                        asset.metadata["info"]["audio"].as_bool().unwrap_or(false),
                                    ) {
                                        Ok(p) => {
                                            self.preview = Some(p);
                                            self.preview_asset = Some(asset.id.clone());
                                        }
                                        Err(e) => self.message = e.to_string(),
                                    }
                                }
                            }
                            if asset.duration == 0. && ui.button("Animate image").clicked() {
                                self.request.shot_id = None;
                                self.request.image = Some(asset.path.clone());
                                self.request.prompt = asset.metadata["request"]["prompt"]
                                    .as_str()
                                    .unwrap_or("")
                                    .into();
                                self.page = Page::Queue;
                            }
                            #[cfg(target_os = "macos")]
                            if asset.duration > 0. && ui.button("Open in system player").on_hover_text("Review the original file at full resolution with the system player's synchronized playback.").clicked() {
                                self.stop_playback();
                                let result=std::process::Command::new("/usr/bin/open").arg(&asset.path).spawn().map(|_|()).map_err(Into::into);
                                self.result(result,"Opened original media in the system player");
                            }
                            if ui.button("Approve").clicked() {
                                self.review_choice(&asset.id, "approved");
                            }
                            if ui.button("Reject").clicked() {
                                self.review_choice(&asset.id, "rejected");
                            }
                            if let Some(path) = asset.metadata["subtitle_file"].as_str()
                                && ui.button("Save subtitles (.srt)").clicked()
                                && let Some(destination) = rfd::FileDialog::new()
                                    .set_file_name("DramaFactory-episode.srt")
                                    .add_filter("SubRip captions", &["srt"])
                                    .save_file()
                            {
                                let r = std::fs::copy(path, destination).map_err(Into::into);
                                self.result(r, "Subtitles copied");
                            }
                            if ui.button("Save a copy").clicked()
                                && let Some(dest) = rfd::FileDialog::new()
                                    .set_file_name(if asset.duration == 0. {
                                        "DramaFactory-image.png"
                                    } else if asset.width == 0 {
                                        "DramaFactory-dialogue.wav"
                                    } else {
                                        "DramaFactory-take.mp4"
                                    })
                                    .save_file()
                            {
                                let r = std::fs::copy(&asset.path, dest).map_err(Into::into);
                                self.result(r, "Media copied");
                            }
                        });
                        self.review_notes(ui, &asset.id);
                        ui.collapsing("Generation provenance", |ui| {
                            ui.label(asset.path.display().to_string());
                            ui.label(format!("SHA-256 {}", asset.hash));
                            ui.label(
                                serde_json::to_string_pretty(&asset.metadata).unwrap_or_default(),
                            );
                        });
                    });
                });
                if self.preview_asset.as_ref() == Some(&asset.id) && asset.duration == 0. {
                    self.reference_preview(ui, &asset.path, Vec2::new(700., 650.));
                }
                if self.preview_asset.as_ref() == Some(&asset.id)
                    && let Some(p) = &mut self.preview
                {
                    ui.add_space(14.);
                    p.ui(ui);
                    ui.label(RichText::new("Local preview").color(MUTED).size(11.));
                    if ui.small_button("Close preview").clicked() {
                        self.preview = None;
                    }
                }
            });
            ui.add_space(16.);
        }
    }
    fn images(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "CHARACTER & WORLD DESIGN",
            "Bring the world into focus.",
            "Local FLUX.2 image generation and reference-guided revisions.",
        );
        Self::card(ui, |ui| {
            Self::field(ui, "IMAGE DIRECTION", &mut self.image_request.prompt, 5);
            ui.horizontal(|ui| {
                ui.label("Width");
                ui.add(
                    egui::DragValue::new(&mut self.image_request.width)
                        .range(256..=1536)
                        .speed(16.),
                );
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut self.image_request.height)
                        .range(256..=1536)
                        .speed(16.),
                );
                ui.label("Steps");
                ui.add(egui::DragValue::new(&mut self.image_request.steps).range(1..=20));
                ui.label("Seed");
                ui.add(egui::DragValue::new(&mut self.image_request.seed));
            });
            ui.horizontal(|ui| {
                if ui.button("Reference image").clicked() {
                    self.image_request.reference = rfd::FileDialog::new()
                        .add_filter("Image", &["png", "jpg", "jpeg"])
                        .pick_file();
                }
                if let Some(path) = &self.image_request.reference {
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    if ui.small_button("Clear").clicked() {
                        self.image_request.reference = None;
                    }
                }
            });
            if let Some(path) = self.image_request.reference.clone() {
                self.reference_preview(ui, &path, Vec2::new(360., 320.));
            }
            if self.image_request.library_id.is_some() {
                ui.label(RichText::new("Linked to a library entry. Its current reference guides revisions; originals remain in the asset library.").color(GOLD));
                if ui.small_button("Unlink library entry").clicked() {
                    self.image_request.library_id = None;
                }
            }
            if ui
                .button(RichText::new("Queue local image").color(GOLD))
                .clicked()
            {
                match self.store.enqueue_image(&self.image_request) {
                    Ok(job) => {
                        if job.status == "queued" {
                            let r = worker::spawn_detached(&self.store.path);
                            self.result(r, "Image generation queued");
                        } else {
                            self.message = job.error;
                        }
                    }
                    Err(e) => self.message = e.to_string(),
                }
            }
            ui.label(
                RichText::new(
                    "One inference job at a time. Completed images appear in the screening room.",
                )
                .color(MUTED),
            );
        });
    }
    fn dialogue(&mut self, ui: &mut egui::Ui) {
        self.dialogue_workspace(ui);
    }
    fn timeline(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "THE EDITING ROOM",
            "Shape the final cut.",
            "Ordered shots, precise timing, and a deterministic portrait export.",
        );
        self.episode_picker(ui);
        let Some(episode) = self.episode_id.clone() else {
            ui.label("Choose an episode to assemble.");
            return;
        };
        self.readiness_panel(ui, &episode);
        let shots = self.load(self.store.shots(&episode));
        let assets = self.load(self.store.assets());
        let total: f64 = shots.iter().map(|s| s.duration).sum();
        let target = self
            .store
            .get_episode(&episode)
            .ok()
            .and_then(|e| self.store.series_plan(&e.series_id).ok())
            .map(|p| p.creative.episode_seconds)
            .unwrap_or(75.);
        ui.label(
            RichText::new(format!(
                "{:.2}s / {:.0}s target   ·   1080 × 1920   ·   24 fps",
                total, target
            ))
            .color(GOLD),
        );
        ui.add_space(14.);
        egui::ScrollArea::horizontal()
            .id_salt("timeline-strip")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut time = 0.;
                    for shot in &shots {
                        let asset = assets
                            .iter()
                            .find(|a| Some(&a.id) == shot.asset_id.as_ref());
                        egui::Frame::new()
                            .fill(Color32::from_rgb(37, 34, 30))
                            .corner_radius(8)
                            .inner_margin(14.)
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width((shot.duration as f32 * 30.).max(140.));
                                    ui.label(
                                        RichText::new(format!(
                                            "{:02}   {:.1}s",
                                            shot.position + 1,
                                            shot.duration
                                        ))
                                        .color(GOLD),
                                    );
                                    ui.label(&shot.title);
                                    ui.label(
                                        RichText::new(format!(
                                            "{:.1} – {:.1}",
                                            time,
                                            time + shot.duration
                                        ))
                                        .size(11.)
                                        .color(MUTED),
                                    );
                                    ui.label(
                                        RichText::new(
                                            asset
                                                .map(|a| a.review.as_str())
                                                .unwrap_or("awaiting render"),
                                        )
                                        .size(10.),
                                    );
                                    self.picture_trim_controls(ui, shot);
                                    self.picture_fade_controls(ui, shot);
                                });
                            });
                        time += shot.duration;
                    }
                });
            });
        ui.add_space(20.);
        self.sound_timeline(ui, &episode, total);
        Self::card(ui, |ui| {
            ui.label(RichText::new("Export profile").size(21.));
            ui.label("H.264 video · AAC stereo · MP4");
            let mut burn = self.store.burn_captions(&episode).unwrap_or(false);
            if ui
                .checkbox(&mut burn, "Include readable captions in the video")
                .changed()
            {
                let r = self.store.set_caption_mode(&episode, burn);
                self.result(r, "Caption export mode saved");
            }
            ui.label(RichText::new("Dialogue also exports as a separate SRT file. Line boundaries follow generated speech; timing inside long lines is estimated.").color(MUTED));

            ui.label(RichText::new("Clips are trimmed to their planned lengths, resized and cropped to fill the portrait frame. Every selected take must be approved. Shot-timed dialogue is mixed in sequence; timeline-timed dialogue can continue over camera cuts. Music and sound effects share the final mix. Lip synchronization remains experimental.").color(MUTED));
            if (total - target).abs() > 0.05 {
                ui.label(RichText::new(format!("This episode is {:.2}s. The series episode target is {:.0}s; shorter test exports are allowed.",total,target)).color(GOLD));
            }
            let unsaved_cues = self
                .cue_drafts
                .values()
                .filter(|draft| draft.cue.episode_id == episode)
                .count();
            if unsaved_cues > 0 {
                ui.label(RichText::new(format!("{unsaved_cues} local sound cue draft(s). Save or reload them before exporting the mix.")).color(GOLD));
            }
            if ui
                .add_enabled(
                    unsaved_cues == 0,
                    egui::Button::new(RichText::new("Queue episode export").color(GOLD)),
                )
                .clicked()
            {
                let r = self
                    .store
                    .enqueue_export(&episode)
                    .and_then(|_| worker::spawn_detached(&self.store.path));
                self.result(r, "Episode export queued");
            }
        });
    }
    fn sound_timeline(&mut self, ui: &mut egui::Ui, episode: &str, total: f64) {
        Self::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sound design").size(21.));
                if ui.button("Import WAV…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("WAV audio", &["wav"])
                        .pick_file()
                {
                    let result = self.store.import_audio(&path);
                    self.result(result, "Audio imported; original preserved");
                }
            });
            ui.label(RichText::new("Place music and effects in seconds. Source trims preserve your originals; fades and gain shape the final mix.").color(MUTED));
            ui.small(
                "Edit locally, then Save sound cue. Reload saved cue discards your local edits.",
            );
            let assets = self.load(self.store.assets());
            let cues = self.load(self.store.audio_cues(episode));
            let saved_ids: std::collections::HashSet<_> =
                cues.iter().map(|cue| cue.id.clone()).collect();
            for saved in cues {
                let mut draft =
                    self.cue_drafts
                        .get(&saved.id)
                        .cloned()
                        .unwrap_or(sound::CueDraft {
                            cue: saved.clone(),
                            expected: saved.clone(),
                        });
                let mut cue = draft.cue.clone();
                ui.push_id(cue.id.clone(), |ui| {
                    ui.separator();
                    let mut changed = false;
                    let mut discard=false;
                    if saved!=draft.expected {ui.label(RichText::new("Newer sound cue saved in another session; your draft is preserved.").color(GOLD));}
                    ui.horizontal(|ui| {
                        changed |= ui.text_edit_singleline(&mut cue.name).changed();
                        egui::ComboBox::from_id_salt("kind")
                            .selected_text(&cue.kind)
                            .show_ui(ui, |ui| {
                                changed |= ui
                                    .selectable_value(&mut cue.kind, "music".into(), "Music")
                                    .changed();
                                changed |= ui
                                    .selectable_value(&mut cue.kind, "sfx".into(), "Sound effect")
                                    .changed();
                            });
                        if ui.button("Remove saved cue").clicked() {
                            let r = self.store.remove_audio_cue(&cue.id,&draft.expected);
                            discard=r.is_ok();
                            self.result(r, "Cue removed; audio preserved");
                        }
                    });
                    egui::Grid::new("timing")
                        .num_columns(4)
                        .spacing([12., 8.])
                        .show(ui, |ui| {
                            for (index, (label, value, range)) in [
                                ("Timeline start", &mut cue.start, 0. ..=1800.),
                                ("Source start", &mut cue.source_start, 0. ..=1800.),
                                ("Duration", &mut cue.duration, 0.01..=1800.),
                                ("Gain (dB)", &mut cue.gain_db, -60. ..=12.),
                                ("Fade in", &mut cue.fade_in, 0. ..=1800.),
                                ("Fade out", &mut cue.fade_out, 0. ..=1800.),
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                ui.label(label);
                                changed |= ui
                                    .add(egui::DragValue::new(value).speed(0.05).range(range))
                                    .changed();
                                if index % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                    if cue.start + cue.duration > total + 0.001 {
                        ui.label(
                            RichText::new(
                                "Extends beyond the cut — trim or reposition before exporting.",
                            )
                            .color(GOLD),
                        );
                    }
                    if let Some(asset) = assets.iter().find(|a| a.id == cue.asset_id) {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} · {}",
                                self.store.asset_label(asset),
                                asset.review
                            ));
                            if asset.review != "approved" && ui.button("Approve audio").clicked() {
                                let r = self.store.review(&asset.id, "approved");
                                self.result(r, "Audio approved");
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Save sound cue").clicked() {
                            let result=self.store.save_audio_cue(&cue,Some(&draft.expected));
                            discard=result.is_ok();
                            self.result(result,"Sound cue saved; original audio preserved");
                        }
                        if ui.small_button("Reload saved cue").clicked(){discard=true;}
                    });
                    if discard {self.cue_drafts.remove(&cue.id);}else if changed {draft.cue=cue.clone();self.cue_drafts.insert(cue.id.clone(),draft);}
                });
            }
            let removed_drafts: Vec<_> = self
                .cue_drafts
                .values()
                .filter(|draft| {
                    draft.cue.episode_id == episode && !saved_ids.contains(&draft.cue.id)
                })
                .cloned()
                .collect();
            for draft in removed_drafts {
                ui.push_id((&draft.cue.id,"removed"),|ui|{
                ui.label(RichText::new(format!("{} was removed in another session. Your local draft is preserved and will be snapshotted on exit.",draft.cue.name)).color(GOLD));
                if ui.small_button("Discard local draft").clicked(){self.cue_drafts.remove(&draft.cue.id);}
            });
            }
            ui.collapsing("Add from audio library", |ui| {
                let available: Vec<_> = assets
                    .iter()
                    .filter(|a| a.width == 0 && a.duration > 0.)
                    .collect();
                if available.is_empty() {
                    ui.label("Import a WAV to begin. Mono/stereo, up to 200 MB.");
                }
                for asset in available {
                    ui.push_id(&asset.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} · {:.2}s",
                                self.store.asset_label(asset),
                                asset.duration
                            ));
                            for (kind, label) in [("music", "Add music"), ("sfx", "Add effect")] {
                                if ui
                                    .add_enabled(total > 0., egui::Button::new(label))
                                    .clicked()
                                {
                                    let duration = asset.duration.min(total);
                                    let cue = sound::AudioCue {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        episode_id: episode.into(),
                                        asset_id: asset.id.clone(),
                                        name: self.store.asset_label(asset),
                                        kind: kind.into(),
                                        start: 0.,
                                        source_start: 0.,
                                        duration,
                                        gain_db: if kind == "music" { -18. } else { -6. },
                                        fade_in: 0.,
                                        fade_out: 0.,
                                    };
                                    let r = self.store.save_audio_cue(&cue, None);
                                    self.result(r, "Audio cue added");
                                }
                            }
                        });
                    });
                }
            });
        });
        ui.add_space(18.);
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(ui.available_width().min(900.));
        Self::title(
            ui,
            "STUDIO PREFERENCES",
            "Made to stay yours.",
            "OpenRouter connection, application updates, and project storage.",
        );
        if ui.button("Studio overview").clicked() {
            self.page = Page::Studio;
        }
        self.provider_settings_panel(ui);
        ui.add_space(18.);
        Self::card(ui, |ui| {
            ui.heading("Application updates");
            if ui
                .checkbox(
                    &mut self.automatic_updates,
                    "Download signed updates silently; install after exit",
                )
                .changed()
            {
                let r = self.store.set_automatic_updates(self.automatic_updates);
                self.result(r, "Update preference saved");
            }
            ui.label(concat!(
                env!("DF_PUBLIC_VERSION"),
                " · MIT · Baz-Studios-LLC / DramaFactory"
            ));
            ui.label(&self.update_status);
            if ui
                .add_enabled(
                    self.update_rx.is_none(),
                    egui::Button::new("Check for updates"),
                )
                .clicked()
            {
                self.update_rx = Some(updates::check(
                    self.store.path.clone(),
                    self.automatic_updates,
                ));
                self.update_status = "Checking published releases…".into();
            }
            ui.label(RichText::new("Updates require a signature from Baz Studios and wait for active renders to finish. This local build is not Apple notarized.").color(MUTED));
        });
        ui.add_space(18.);
        Self::card(ui, |ui| {
            ui.heading("MCP connection");
            ui.label("Local stdio · launched on demand by your MCP client");
            if let Ok(exe) = std::env::current_exe() {
                ui.code(exe.display().to_string());
                ui.label("Arguments: --mcp");
                if ui.button("Copy MCP launch configuration").clicked() {
                    let config = serde_json::json!({"command":exe,"args":["--mcp"]});
                    ui.ctx()
                        .copy_text(serde_json::to_string_pretty(&config).unwrap_or_default());
                    self.message = "MCP launch configuration copied".into();
                }
            }
        });
        ui.add_space(18.);
        ui.label(RichText::new("PROJECT STORAGE").color(GOLD).size(10.));
        ui.label(self.store.path.display().to_string());
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.last_sync.elapsed() > std::time::Duration::from_secs(2) {
            if self.script == self.script_baseline
                && let Some(id) = &self.episode_id
                && let Ok(episode) = self.store.get_episode(id)
            {
                self.script = episode.screenplay;
                self.script_baseline = self.script.clone();
            }
            self.last_sync = std::time::Instant::now();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) && self.save_script() {
            self.message = "Screenplay saved".into();
        }
        if self.automatic_updates
            && self.update_rx.is_none()
            && self.last_update_check.elapsed() > std::time::Duration::from_secs(6 * 60 * 60)
        {
            self.last_update_check = std::time::Instant::now();
            self.update_rx = Some(updates::check(self.store.path.clone(), true));
        }
        if let Some(rx) = &self.update_rx
            && let Ok(s) = rx.try_recv()
        {
            self.update_status = s.message;
            self.update_rx = None;
        }
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(10, 12, 16))
                    .inner_margin(10.),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("DRAMAFACTORY").color(GOLD).size(10.));
                    ui.separator();
                    ui.label(
                        RichText::new(if self.message.is_empty() {
                            "Projects saved on this Mac"
                        } else {
                            &self.message
                        })
                        .color(MUTED)
                        .size(11.),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(concat!(env!("DF_PUBLIC_VERSION"), " / DEVELOPMENT"))
                                .color(MUTED)
                                .size(10.),
                        );
                    });
                });
            });
        egui::SidePanel::left("navigation")
            .exact_width(224.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(10, 12, 16))
                    .inner_margin(18.),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 5.;
                ui.spacing_mut().button_padding = egui::vec2(8., 6.);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../branding/icon.png"))
                            .fit_to_exact_size(Vec2::splat(36.)),
                    );
                    ui.label(RichText::new("DRAMAFACTORY").size(13.).strong());
                });
                ui.add_space(12.);
                self.current_series_picker(ui);
                ui.add_space(8.);
                self.workflow_navigation(ui);
            });
        egui::TopBottomPanel::top("workspace-toolbar")
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let name = self
                        .store
                        .series()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|s| Some(&s.id) == self.selected.as_ref())
                        .map(|s| s.name)
                        .unwrap_or("Your studio".into());
                    ui.label(RichText::new(name).size(13.).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .selectable_label(self.sidekick_visible, "Creative sidekick")
                            .on_hover_text("Show or hide the series conversation")
                            .clicked()
                        {
                            self.sidekick_visible = !self.sidekick_visible;
                        }
                    });
                });
            });
        // Process pending replies even while the conversation is collapsed.
        self.sidekick_panel(ctx);
        self.video_planner.player_visible = false;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(if self.page == Page::EditingRoom {
                        16.
                    } else {
                        0.
                    }),
            )
            .show(ctx, |ui| {
                if self.page == Page::EditingRoom {
                    self.editing_room(ui);
                    return;
                }
                let content_scroll_style = ui.spacing().scroll;
                ui.spacing_mut().scroll.floating = false;
                ui.spacing_mut().scroll.bar_width = 6.;
                ui.spacing_mut().scroll.bar_inner_margin = 0.;
                ui.spacing_mut().scroll.bar_outer_margin = 0.;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .id_salt(("studio-page", self.page as u8))
                    .show(ui, |ui| {
                        ui.spacing_mut().scroll = content_scroll_style;
                        egui::Frame::new().inner_margin(28.).show(ui, |ui| {
                            let width = ui.available_width().min(1120.);
                            ui.set_width(width);
                            self.workflow_location(ui);
                            match self.page {
                                Page::Studio => self.dashboard(ui),
                                Page::Series => self.series(ui),
                                Page::Synopsis => self.synopsis(ui),
                                Page::Story => self.story_development(ui),
                                Page::Cast => self.library(ui, "character"),
                                Page::Locations => self.library(ui, "location"),
                                Page::Scenes => self.scenes(ui),
                                Page::Assets => self.assets(ui),
                                Page::Timeline => self.timeline(ui),
                                Page::Dialogue => self.dialogue(ui),
                                Page::Images => self.images(ui),
                                Page::Queue => self.queue(ui),
                                Page::EditingRoom => self.editing_room(ui),
                                Page::Settings => self.settings(ui),
                            }
                        });
                    });
            });
        if !self.video_planner.player_visible {
            self.video_planner.player = None;
        }
        if self.page != Page::EditingRoom {
            self.editor.stop();
        }
        self.image_inspector_window(ctx);
        self.project_save_confirmation(ctx);
        self.portable_progress(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        let directory = self.store.root().join("drafts");
        if std::fs::create_dir_all(&directory).is_ok() {
            for draft in self.profile_drafts.values().filter(|draft| draft.dirty()) {
                if let Ok(bytes) = serde_json::to_vec_pretty(draft) {
                    let _ = std::fs::write(
                        directory.join(format!(
                            "character-profile-{}-{}.json",
                            draft.brief.id,
                            uuid::Uuid::new_v4()
                        )),
                        bytes,
                    );
                }
            }
            for draft in self.portrait_prompts.values() {
                if let Ok(bytes) = serde_json::to_vec_pretty(draft) {
                    let _ = std::fs::write(
                        directory.join(format!(
                            "portrait-prompt-{}-{}.json",
                            draft.id,
                            uuid::Uuid::new_v4()
                        )),
                        bytes,
                    );
                }
            }
        }
        let _ = self.snapshot_generation_plans();
        let _ = self.snapshot_shot_form();
        let _ = self.snapshot_new_dialogue();
        let draft_directory = self.store.root().join("drafts");
        for draft in self.synopsis_drafts.values().filter(|draft| {
            draft.title != draft.expected_title || draft.premise != draft.expected_premise
        }) {
            if std::fs::create_dir_all(&draft_directory).is_ok()
                && let Ok(bytes) = serde_json::to_vec_pretty(draft)
            {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "synopsis-{}-{}.json",
                        draft.id,
                        uuid::Uuid::new_v4()
                    )),
                    bytes,
                );
            }
        }
        for draft in self.cue_drafts.values() {
            if std::fs::create_dir_all(&draft_directory).is_ok() {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "sound-cue-{}-{}.json",
                        draft.cue.id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(draft).unwrap_or_default(),
                );
            }
        }
        for draft in self.fade_drafts.values() {
            if std::fs::create_dir_all(&draft_directory).is_ok() {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "picture-fades-{}-{}.json",
                        draft.shot_id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(draft).unwrap_or_default(),
                );
            }
        }
        for draft in self.library_drafts.values() {
            if std::fs::create_dir_all(&draft_directory).is_ok() {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "library-{}-{}.json",
                        draft.id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(draft).unwrap_or_default(),
                );
            }
        }
        for draft in self.review_drafts.values() {
            if std::fs::create_dir_all(&draft_directory).is_ok() {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "asset-review-{}-{}.json",
                        draft.asset_id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(draft).unwrap_or_default(),
                );
            }
        }
        for draft in self.shot_trim_drafts.values() {
            if std::fs::create_dir_all(&draft_directory).is_ok() {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "picture-trim-{}-{}.json",
                        draft.shot_id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(draft).unwrap_or_default(),
                );
            }
        }
        for draft in self.dialogue_drafts.values() {
            if std::fs::create_dir_all(&draft_directory).is_ok() {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "dialogue-{}-{}.json",
                        draft.id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(draft).unwrap_or_default(),
                );
            }
        }
        for (id, direction) in &self.shot_directions {
            if self
                .store
                .shot_direction(id)
                .ok()
                .and_then(|d| serde_json::to_string(&d).ok())
                != serde_json::to_string(direction).ok()
                && std::fs::create_dir_all(&draft_directory).is_ok()
            {
                let _ = std::fs::write(
                    draft_directory
                        .join(format!("shot-direction-{id}-{}.json", uuid::Uuid::new_v4())),
                    serde_json::to_vec_pretty(direction).unwrap_or_default(),
                );
            }
        }
        for camera in self
            .camera_drafts
            .values()
            .filter(|c| !c.name.trim().is_empty())
        {
            let saved = self
                .store
                .camera_setups(&camera.location_id)
                .ok()
                .and_then(|items| items.into_iter().find(|c| c.id == camera.id));
            if saved.and_then(|c| serde_json::to_string(&c).ok())
                != serde_json::to_string(camera).ok()
                && std::fs::create_dir_all(&draft_directory).is_ok()
            {
                let _ = std::fs::write(
                    draft_directory.join(format!(
                        "camera-{}-{}.json",
                        camera.id,
                        uuid::Uuid::new_v4()
                    )),
                    serde_json::to_vec_pretty(camera).unwrap_or_default(),
                );
            }
        }
        for plan in self.story_drafts.values() {
            if self
                .store
                .series_plan(&plan.series_id)
                .ok()
                .and_then(|p| serde_json::to_string(&p).ok())
                != serde_json::to_string(plan).ok()
            {
                let directory = self.store.root().join("drafts");
                if std::fs::create_dir_all(&directory).is_ok() {
                    let _ = std::fs::write(
                        directory.join(format!(
                            "story-{}-{}.json",
                            plan.series_id,
                            uuid::Uuid::new_v4()
                        )),
                        serde_json::to_vec_pretty(plan).unwrap_or_default(),
                    );
                }
            }
        }
        if self.script != self.script_baseline {
            let directory = self.store.root().join("drafts");
            if std::fs::create_dir_all(&directory).is_ok() {
                let _ = std::fs::write(
                    directory.join(format!("screenplay-{}.txt", uuid::Uuid::new_v4())),
                    &self.script,
                );
            }
        }
        self.save_script();
        if let Some(child) = &mut self.audio {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|x| x == "--help") {
        println!(
            "DramaFactory\n\n(no arguments) Launch studio\n--mcp Start local stdio MCP\n--worker Drain durable queue\n--worker-once Drain if no other worker owns the queue\n--data-path Print database path\n--configure-worker PYTHON WAN_MODEL FFMPEG KOKORO_MODEL\n--configure-images PYTHON FLUX_MODEL\n--configure-lipsync PYTHON MUSETALK_MODEL\n--configure-performance PYTHON LTX_MODEL ENCODER\n--version Print public version"
        );
        return Ok(());
    }
    if let Some(flag) = args.get(1)
        && ![
            "--mcp",
            "--worker",
            "--worker-once",
            "--data-path",
            "--configure-worker",
            "--configure-images",
            "--configure-lipsync",
            "--configure-performance",
            "--apply-update",
            "--install-staged-update",
            "--version",
        ]
        .contains(&flag.as_str())
    {
        anyhow::bail!("Unknown option '{flag}'. Use --help");
    }
    if args.get(1).is_some_and(|x| x == "--version") {
        println!("{}", updates::LABEL);
        return Ok(());
    }
    if args.get(1).is_some_and(|x| x == "--install-staged-update") {
        if args.len() != 4 {
            anyhow::bail!("Usage: --install-staged-update MANIFEST DATABASE");
        }
        return updater::schedule_install(
            std::path::Path::new(&args[2]),
            std::path::Path::new(&args[3]),
        );
    }
    if args.get(1).is_some_and(|x| x == "--apply-update") {
        return updater::apply(&args[2..]);
    }
    if args.get(1).is_some_and(|x| x == "--data-path") {
        println!("{}", store::default_path().display());
        return Ok(());
    }
    let store = Store::open(if args.len() == 1 {
        portable_project::startup_path()
    } else {
        store::default_path()
    })?;
    if args.get(1).is_some_and(|x| x == "--configure-worker") {
        if args.len() != 6 {
            anyhow::bail!("Usage: --configure-worker PYTHON WAN_MODEL FFMPEG KOKORO_DIRECTORY");
        }
        let mut config = store.config()?;
        config.python = args[2].clone().into();
        config.model = args[3].clone().into();
        config.ffmpeg = args[4].clone().into();
        config.speech_model = args[5].clone().into();
        store.save_config(&config)?;
        println!("Local worker configuration saved");
        return Ok(());
    }
    if args.get(1).is_some_and(|x| x == "--configure-images") {
        if args.len() != 4 {
            anyhow::bail!("Usage: --configure-images PYTHON MODEL_DIRECTORY");
        }
        let mut config = store.config()?;
        config.image_python = args[2].clone().into();
        config.image_model = args[3].clone().into();
        store.save_config(&config)?;
        println!("Image worker configuration saved");
        return Ok(());
    }
    if args.get(1).is_some_and(|x| x == "--configure-performance") {
        if args.len() != 5 {
            anyhow::bail!(
                "Usage: --configure-performance PYTHON MODEL_DIRECTORY ENCODER_DIRECTORY"
            );
        }
        let mut config = store.config()?;
        config.performance_python = args[2].clone().into();
        config.performance_model = args[3].clone().into();
        config.performance_encoder = args[4].clone().into();
        performance::validate_setup(&config)?;
        store.save_config(&config)?;
        println!("Experimental performance worker configuration saved");
        return Ok(());
    }
    if args.get(1).is_some_and(|x| x == "--configure-lipsync") {
        if args.len() != 4 {
            anyhow::bail!("Usage: --configure-lipsync PYTHON MODEL_DIRECTORY");
        }
        let mut config = store.config()?;
        config.lipsync_python = args[2].clone().into();
        config.lipsync_model = args[3].clone().into();
        store.save_config(&config)?;
        println!("Lip-sync worker configuration saved");
        return Ok(());
    }
    if std::env::args().any(|x| x == "--worker" || x == "--worker-once") {
        return worker::run(
            store.path.clone(),
            std::env::args().any(|x| x == "--worker-once"),
        );
    }
    if std::env::args().any(|x| x == "--mcp") {
        return mcp::run(store);
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title(format!("DramaFactory · {}", updates::LABEL))
        .with_inner_size([1380., 900.])
        .with_min_inner_size([1100., 740.]);
    // macOS resolves the system-rendered Icon Composer asset from the bundle.
    // eframe treats None as its own default "e" icon. An explicit empty icon
    // is its opt-out sentinel (AppTitleIconSetter::new), leaving NSApp's bundle
    // icon untouched so macOS can render the Icon Composer asset and mask.
    #[cfg(target_os = "macos")]
    let viewport = viewport.with_icon(egui::IconData::default());
    #[cfg(not(target_os = "macos"))]
    let viewport = {
        let icon = image::load_from_memory(include_bytes!("../branding/icon.png"))?
            .resize(256, 256, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        viewport.with_icon(egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        })
    };
    let opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "DramaFactory",
        opts,
        Box::new(|cc| Ok(Box::new(Studio::new(cc, store)))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("DramaFactory: {error:#}");
        if !std::env::args().skip(1).any(|a| a.starts_with("--")) {
            rfd::MessageDialog::new()
                .set_title("DramaFactory could not open this studio")
                .set_description(format!(
                    "{error:#}\n\nYour project files have not been removed."
                ))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        std::process::exit(1);
    }
}
