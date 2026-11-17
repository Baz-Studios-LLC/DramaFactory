//! Episode scoring in the Editing Room. Only Generate music contacts ElevenLabs.
use crate::{editing_room::Cut, production::Asset, sound::AudioCue, store::Store};
use anyhow::{Context, Result, ensure};
use eframe::egui;
use serde_json::json;
use std::{path::PathBuf, process::Command, sync::mpsc, time::Duration};

enum Output {
    Direction(String),
    Take,
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    direction: String,
    selected_clip: bool,
    custom: bool,
    from: f64,
    until: f64,
    pending: Option<mpsc::Receiver<Result<Output>>>,
    status: String,
}

fn compose(
    db: PathBuf,
    episode: String,
    direction: String,
    start: f64,
    duration: f64,
) -> Result<()> {
    let length = (duration * 1000.).round() as u64;
    ensure!(
        (3000..=600000).contains(&length),
        "Choose 3 seconds to 10 minutes of music."
    );
    ensure!(
        (1..=4100).contains(&direction.trim().chars().count()),
        "Enter up to 4,100 characters of music direction."
    );
    let store = Store::open(db)?;
    let ffmpeg = store.config()?.ffmpeg;
    ensure!(
        Command::new(&ffmpeg)
            .arg("-version")
            .output()?
            .status
            .success(),
        "FFmpeg is unavailable."
    );
    let key = crate::credentials::Provider::ElevenLabs
        .read()?
        .context("Add your ElevenLabs key in Settings.")?;
    let temp = tempfile::tempdir()?;
    let mp3 = temp.path().join("score.mp3");
    let wav = temp.path().join("score.wav");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let mut response = agent
        .post("https://api.elevenlabs.io/v1/music?output_format=mp3_44100_128")
        .header("xi-api-key", &key)
        .send_json(
            json!({"prompt":direction,"music_length_ms":length,"force_instrumental":true}),
        )?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(32 * 1024 * 1024)
        .reader();
    std::io::copy(&mut reader, &mut std::fs::File::create(&mp3)?)?;
    ensure!(
        Command::new(ffmpeg)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(mp3)
            .args(["-c:a", "pcm_s16le"])
            .arg(&wav)
            .status()?
            .success(),
        "Could not decode the generated music."
    );
    let tx = store.conn.unchecked_transaction()?;
    let asset = store.import_audio(&wav)?;
    store.conn.execute("UPDATE assets SET metadata=?1 WHERE id=?2",rusqlite::params![json!({"kind":"episode_music","episode_id":episode,"direction":direction,"start":start,"duration":duration}).to_string(),asset.id])?;
    tx.commit()?;
    Ok(())
}

impl Panel {
    fn run(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Result<Output> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.status.clear();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        episode: &str,
        cut: &Cut,
        selected: Option<usize>,
        assets: &[Asset],
    ) -> bool {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(Output::Direction(text)) => self.direction = text,
                        Ok(Output::Take) => {
                            self.status = "Music ready. Choose Use to hear it with the cut.".into()
                        }
                        Err(e) => self.status = format!("{e:#}"),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.status = "Music task stopped.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut changed = false;
        let mut contents = |ui: &mut egui::Ui| {
            let clip =
                selected.and_then(|i| cut.clips.get(i).map(|c| (cut.clip_time(i), c.duration)));
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(!self.custom && !self.selected_clip, "Whole episode")
                    .clicked()
                {
                    self.custom = false;
                    self.selected_clip = false;
                }
                ui.add_enabled_ui(clip.is_some(), |ui| {
                    if ui
                        .selectable_label(!self.custom && self.selected_clip, "Selected clip")
                        .clicked()
                    {
                        self.custom = false;
                        self.selected_clip = true;
                    }
                });
                if ui.selectable_label(self.custom, "Time range").clicked() {
                    self.custom = true;
                    if self.until <= self.from {
                        self.until = cut.duration();
                    }
                }
            });
            if self.custom {
                ui.horizontal(|ui| {
                    ui.label("From");
                    ui.add(
                        egui::DragValue::new(&mut self.from)
                            .range(0. ..=cut.duration())
                            .speed(0.1)
                            .suffix("s"),
                    );
                    ui.label("To");
                    ui.add(
                        egui::DragValue::new(&mut self.until)
                            .range(self.from..=cut.duration().max(self.from))
                            .speed(0.1)
                            .suffix("s"),
                    );
                });
            }
            let (start, duration) = if self.custom {
                (
                    self.from,
                    (self.until.min(cut.duration()) - self.from).max(0.),
                )
            } else if self.selected_clip {
                clip.unwrap_or_default()
            } else {
                (0., cut.duration())
            };
            ui.label(format!("{start:.1}s – {:.1}s", start + duration));
            ui.add_enabled(
                self.pending.is_none(),
                egui::TextEdit::multiline(&mut self.direction)
                    .hint_text("Music direction")
                    .desired_width(380.),
            );
            ui.horizontal(|ui|{
                if ui.add_enabled(self.pending.is_none() && duration>0.,egui::Button::new("Draft with Sidekick")).clicked(){
                    match store.get_episode(episode){
                        Ok(ep)=>{
                            let clips: Vec<_>=cut.clips.iter().enumerate().map(|(i,c)|json!({"take":c.title,"start":cut.clip_time(i),"duration":c.duration})).collect();
                            let context=json!({"workspace":"Editing room","episode":ep.title,"screenplay":ep.screenplay,"clips":clips,"music_start":start,"music_end":start+duration,"creative_direction":self.direction}).to_string();
                            self.run(ui.ctx(),move||{
                                let msg=crate::sidekick::Message{id:uuid::Uuid::new_v4().to_string(),role:"user".into(),context:"Editing room".into(),images:vec![],text:"Write only a concise instrumental score prompt for this selected time range of my edited episode. Describe instruments, pacing, emotional turns, space for dialogue and a musical ending. Honor my creative direction. Return the prompt in message, with no document edits.".into()};
                                let reply=crate::sidekick::reply(&[msg],&context,"google/gemini-4-argon",&Default::default(),&Default::default())?;
                                Ok(Output::Direction(reply.message))
                            });
                        }
                        Err(e)=>self.status=e.to_string(),
                    }
                }
                let valid=(3. ..=600.).contains(&duration) && (1..=4100).contains(&self.direction.trim().chars().count());
                if ui.add_enabled(self.pending.is_none() && valid,crate::theme::primary("Generate music")).clicked(){
                    let db=store.path.clone();let episode=episode.to_owned();let direction=self.direction.clone();
                    self.run(ui.ctx(),move||{compose(db,episode,direction,start,duration)?;Ok(Output::Take)});
                }
            });
            if !(3. ..=600.).contains(&duration) {
                ui.weak("Choose a range from 3 seconds to 10 minutes.");
            }
            if self.direction.chars().count() > 4100 {
                ui.weak("Shorten music direction to 4,100 characters.");
            }
            if self.pending.is_some() {
                ui.spinner();
            }
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
            let cues = match store.audio_cues(episode) {
                Ok(c) => c,
                Err(e) => {
                    ui.label(e.to_string());
                    return;
                }
            };
            for (i, asset) in assets
                .iter()
                .rev()
                .filter(|a| {
                    a.metadata["kind"] == "episode_music" && a.metadata["episode_id"] == episode
                })
                .enumerate()
            {
                let name = format!("Score {}", i + 1);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("{name} · {:.1}s", asset.duration))
                        .on_hover_text(asset.metadata["direction"].as_str().unwrap_or_default());
                    if !cues.iter().any(|c| c.asset_id == asset.id) && ui.button("Use").clicked() {
                        let duration = asset.duration.min(
                            asset.metadata["duration"]
                                .as_f64()
                                .unwrap_or(asset.duration),
                        );
                        let cue = AudioCue {
                            id: uuid::Uuid::new_v4().to_string(),
                            episode_id: episode.into(),
                            asset_id: asset.id.clone(),
                            name,
                            kind: "music".into(),
                            start: asset.metadata["start"].as_f64().unwrap_or_default(),
                            source_start: 0.,
                            duration,
                            gain_db: -18.,
                            fade_in: 0.25,
                            fade_out: 1.,
                        };
                        match store.save_audio_cue(&cue, None) {
                            Ok(()) => changed = true,
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                });
                if let Some(old) = cues.iter().find(|c| c.asset_id == asset.id) {
                    let mut cue = old.clone();
                    ui.horizontal_wrapped(|ui| {
                        for (label, value, max) in [
                            ("Position", &mut cue.start, 1800.),
                            ("Trim in", &mut cue.source_start, asset.duration),
                            ("Length", &mut cue.duration, asset.duration),
                            ("Fade in", &mut cue.fade_in, old.duration),
                            ("Fade out", &mut cue.fade_out, old.duration),
                        ] {
                            ui.label(label);
                            ui.add(
                                egui::DragValue::new(value)
                                    .speed(0.05)
                                    .range(0. ..=max)
                                    .suffix("s"),
                            );
                        }
                        ui.label("Volume");
                        ui.add(
                            egui::DragValue::new(&mut cue.gain_db)
                                .speed(0.5)
                                .range(-60. ..=12.)
                                .suffix(" dB"),
                        );
                    });
                    cue.duration = cue.duration.min(asset.duration - cue.source_start);
                    cue.fade_in = cue.fade_in.min(cue.duration);
                    cue.fade_out = cue.fade_out.min(cue.duration - cue.fade_in);
                    let result = if ui.small_button("Remove from mix").clicked() {
                        Some(store.remove_audio_cue(&old.id, old))
                    } else if &cue != old {
                        Some(store.save_audio_cue(&cue, Some(old)))
                    } else {
                        None
                    };
                    if let Some(result) = result {
                        match result {
                            Ok(()) => changed = true,
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                }
            }
        };
        egui::Window::new("Episode music")
            .open(&mut open)
            .vscroll(true)
            .show(ui.ctx(), &mut contents);
        self.open = open;
        changed
    }
}
