//! Episode scoring in the Editing Room. Only Generate music contacts ElevenLabs.
use crate::{editing_room::Cut, production::Asset, sound::AudioCue, store::Store};
use anyhow::{Context, Result, ensure};
use eframe::egui;
use serde_json::json;
use std::{path::PathBuf, process::Command, sync::mpsc, time::Duration};

const MAX_MUSIC_SECONDS: f64 = 300.;

#[cfg(test)]
mod duration_tests {
    use super::*;

    #[test]
    fn custom_track_can_outlast_short_or_empty_video() {
        let mut panel = Panel::default();
        let mut draft = MusicDraft {
            episode_id: "episode".into(),
            direction: "Warm instrumental".into(),
            start: 30.,
            duration: 300.,
        };
        panel.prepare(&draft).unwrap();
        assert_eq!(panel.music_range(143.358, None), (30., 300.));
        assert_eq!(panel.music_range(0., None), (30., 300.));
        draft.duration = 300.001;
        assert!(panel.prepare(&draft).is_err());
        assert_eq!(panel.music_range(0., None), (30., 300.));
        for bad in [2.999, f64::NAN, f64::INFINITY] {
            draft.duration = bad;
            assert!(draft.validate().is_err());
        }
        panel.custom = false;
        assert_eq!(panel.music_range(143.358, None), (0., 143.358));
        panel.selected_clip = true;
        assert_eq!(panel.music_range(143.358, Some((10., 12.))), (10., 12.));
    }

    #[test]
    fn minute_fields_round_trip_and_handle_rollover() {
        assert_eq!(music_time(143.358), "2:23.358");
        assert_eq!(music_time(300.), "5:00");
        assert_eq!(music_time(59.9999), "1:00");
        assert_eq!(parse_music_time("2:23.358"), Some(143.358));
        assert_eq!(parse_music_time("5:00"), Some(300.));
        assert_eq!(parse_music_time("143.358"), Some(143.358));
        for bad in ["1:60", "-1:00", "NaN", "1:NaN", "1:2:3", "-1"] {
            assert_eq!(parse_music_time(bad), None);
        }
    }
}

fn music_time(seconds: f64) -> String {
    let millis = (seconds.max(0.) * 1000.).round() as u64;
    let base = format!("{}:{:02}", millis / 60000, millis / 1000 % 60);
    if millis.is_multiple_of(1000) {
        base
    } else {
        format!("{base}.{:03}", millis % 1000)
            .trim_end_matches('0')
            .to_owned()
    }
}

fn parse_music_time(value: &str) -> Option<f64> {
    let value = value.trim();
    let seconds = if let Some((minutes, seconds)) = value.split_once(':') {
        let minutes = minutes.parse::<u32>().ok()?;
        let seconds = seconds.parse::<f64>().ok()?;
        if !(0. ..60.).contains(&seconds) {
            return None;
        }
        f64::from(minutes) * 60. + seconds
    } else {
        value.parse::<f64>().ok()?
    };
    (seconds.is_finite() && seconds >= 0.).then_some(seconds)
}

fn music_time_input(value: &mut f64, range: std::ops::RangeInclusive<f64>) -> egui::DragValue<'_> {
    egui::DragValue::new(value)
        .range(range)
        .speed(0.1)
        .custom_formatter(|value, _| music_time(value))
        .custom_parser(parse_music_time)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MusicDraft {
    pub episode_id: String,
    pub direction: String,
    pub start: f64,
    pub duration: f64,
}
impl MusicDraft {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(!self.episode_id.is_empty(), "Choose an episode for music");
        ensure!(
            (1..=4100).contains(&self.direction.trim().chars().count()),
            "Music direction must contain 1–4,100 characters"
        );
        ensure!(
            self.start.is_finite()
                && self.start >= 0.
                && self.duration.is_finite()
                && (3. ..=MAX_MUSIC_SECONDS).contains(&self.duration),
            "Music needs a valid start and a duration from 0:03 to 5:00"
        );
        Ok(())
    }
}

enum Output {
    Direction(String),
    Take,
}
#[derive(Default)]
pub struct Panel {
    pub focus_cue: Option<String>,
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
    ensure!(
        duration.is_finite() && (3. ..=MAX_MUSIC_SECONDS).contains(&duration),
        "Choose 0:03 to 5:00 of music."
    );
    let length = (duration * 1000.).round() as u64;
    ensure!(
        (3000..=300000).contains(&length),
        "Choose 0:03 to 5:00 of music."
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
    register_track(
        &store,
        &wav,
        json!({"kind":"episode_music","episode_id":episode,"direction":direction,"start":start,"duration":duration}),
    )
}

fn register_track(store: &Store, wav: &std::path::Path, metadata: serde_json::Value) -> Result<()> {
    let tx = store.conn.unchecked_transaction()?;
    let asset = store.import_audio(wav)?;
    store.conn.execute(
        "UPDATE assets SET metadata=?1 WHERE id=?2",
        rusqlite::params![metadata.to_string(), asset.id],
    )?;
    tx.commit()?;
    Ok(())
}

fn import_track(
    db: PathBuf,
    episode: String,
    path: PathBuf,
    start: f64,
    duration: f64,
) -> Result<Output> {
    let store = Store::open(db)?;
    let temp = tempfile::tempdir()?;
    let wav = temp.path().join("track.wav");
    let status = Command::new(store.config()?.ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&path)
        .args([
            "-map",
            "0:a:0",
            "-ac",
            "2",
            "-ar",
            "44100",
            "-c:a",
            "pcm_s16le",
            "-t",
            "1801",
        ])
        .arg(&wav)
        .status()
        .context("Could not start FFmpeg for music import")?;
    ensure!(status.success(), "Could not read audio from this file.");
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported track");
    let mut metadata =
        json!({"kind":"episode_music","episode_id":episode,"name":name,"start":start});
    if duration > 0. {
        metadata["duration"] = json!(duration);
    }
    register_track(&store, &wav, metadata)?;
    Ok(Output::Take)
}

impl Panel {
    fn music_range(&self, cut_duration: f64, clip: Option<(f64, f64)>) -> (f64, f64) {
        if self.custom {
            (self.from, (self.until - self.from).max(0.))
        } else if self.selected_clip {
            clip.unwrap_or_default()
        } else {
            (0., cut_duration)
        }
    }
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        json!({"direction":self.direction,"from":self.from,"until":self.until,"custom":self.custom,"selected_clip":self.selected_clip})
    }
    pub(crate) fn prepare(&mut self, draft: &MusicDraft) -> Result<()> {
        draft.validate()?;
        ensure!(
            self.pending.is_none(),
            "Wait for the current music task to finish"
        );
        self.direction = draft.direction.clone();
        self.from = draft.start;
        self.until = draft.start + draft.duration;
        self.custom = true;
        self.selected_clip = false;
        self.focus_cue = None;
        self.status = "Sidekick prepared this instrumental score. Generate music sends a paid ElevenLabs request; no music has been generated yet.".into();
        Ok(())
    }
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
                        self.until = self.from + cut.duration().clamp(3., MAX_MUSIC_SECONDS);
                    }
                }
            });
            if self.custom {
                ui.horizontal(|ui| {
                    ui.label("From");
                    ui.add(music_time_input(&mut self.from, 0. ..=1800.));
                    ui.label("To");
                    ui.add(music_time_input(
                        &mut self.until,
                        self.from..=self.from + MAX_MUSIC_SECONDS,
                    ));
                });
            }
            let (start, duration) = self.music_range(cut.duration(), clip);
            ui.label(format!(
                "{} - {} | Length {}",
                music_time(start),
                music_time(start + duration),
                music_time(duration)
            ));
            ui.weak("Generate 0:03 to 5:00. Custom music can extend beyond the video.");
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Import track…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "Audio",
                        &["wav", "mp3", "m4a", "aac", "aiff", "aif", "flac", "ogg"],
                    )
                    .pick_file()
            {
                let db = store.path.clone();
                let episode = episode.to_owned();
                self.run(ui.ctx(), move || {
                    import_track(db, episode, path, start, duration)
                });
            }
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
                let valid=(3. ..=MAX_MUSIC_SECONDS).contains(&duration) && (1..=4100).contains(&self.direction.trim().chars().count());
                if ui.add_enabled(self.pending.is_none() && valid,crate::theme::primary("Generate music")).clicked(){
                    let db=store.path.clone();let episode=episode.to_owned();let direction=self.direction.clone();
                    self.run(ui.ctx(),move||{compose(db,episode,direction,start,duration)?;Ok(Output::Take)});
                }
            });
            if !(3. ..=MAX_MUSIC_SECONDS).contains(&duration) {
                ui.weak("Choose a range from 0:03 to 5:00.");
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
                let name = asset.metadata["name"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Score {}", i + 1));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("{name} · {}", music_time(asset.duration)))
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
                            muted: false,
                            fade_in: 0.25,
                            fade_out: 1.,
                        };
                        match store.save_audio_cue(&cue, None) {
                            Ok(()) => changed = true,
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                });
                for old in cues.iter().filter(|c| c.asset_id == asset.id) {
                    if self.focus_cue.as_deref() == Some(&old.id) {
                        ui.scroll_to_cursor(Some(egui::Align::Center));
                        self.focus_cue = None;
                    }
                    ui.label(format!(
                        "{} · starts at {}",
                        old.name,
                        music_time(old.start)
                    ));
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
                            ui.add(music_time_input(value, 0. ..=max));
                        }
                        ui.checkbox(&mut cue.muted, "Mute");
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
        ui.strong("Episode music");
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("docked-episode-music")
            .auto_shrink([false, false])
            .show(ui, &mut contents);
        changed
    }
}
