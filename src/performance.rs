//! Experimental native audio/image-conditioned performance, independent of Wan.
use crate::{
    production::{Dialogue, WorkerConfig},
    store::Store,
};
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const COMPONENTS: &[&str] = &[
    "config.json",
    "transformer-distilled.safetensors",
    "connector.safetensors",
    "audio_vae.safetensors",
    "vocoder.safetensors",
    "vae_encoder.safetensors",
    "vae_decoder.safetensors",
    "spatial_upscaler_x2_v1_1.safetensors",
];
pub fn validate_setup(config: &WorkerConfig) -> Result<()> {
    if !config.performance_python.is_file() || !config.ffmpeg.is_file() {
        bail!(
            "Configure the experimental performance Python and FFmpeg executables in Preferences"
        );
    }
    for file in COMPONENTS {
        if !config.performance_model.join(file).is_file() {
            bail!("LTX 2.3 performance model is incomplete: missing {file}");
        }
    }
    for file in ["config.json", "tokenizer.json"] {
        if !config.performance_encoder.join(file).is_file() {
            bail!("Performance prompt encoder is incomplete: missing {file}");
        }
    }
    // The pinned MLX loader scans model*.safetensors, not the published index
    // (the converted two-shard pack retains an obsolete five-shard index).
    let files = std::fs::read_dir(&config.performance_encoder)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<std::collections::BTreeSet<_>>();
    if !files.contains("model.safetensors") {
        let total = files
            .iter()
            .find_map(|name| {
                name.strip_prefix("model-")
                    .and_then(|name| name.strip_suffix(".safetensors"))
                    .and_then(|name| name.split_once("-of-"))
                    .and_then(|(_, total)| total.parse::<usize>().ok())
            })
            .context("Performance encoder weights are missing")?;
        if !(1..=32).contains(&total)
            || !(1..=total)
                .all(|index| files.contains(&format!("model-{index:05}-of-{total:05}.safetensors")))
        {
            bail!("Performance encoder shard set is incomplete");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converted_encoder_shards_do_not_depend_on_an_obsolete_index() {
        let root = std::env::temp_dir().join(format!("df-perf-{}", uuid::Uuid::new_v4()));
        let model = root.join("model");
        let encoder = root.join("encoder");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::create_dir_all(&encoder).unwrap();
        let executable = root.join("explicit-readiness-fixture");
        std::fs::write(
            &executable,
            b"file availability fixture, not an executable/inference claim",
        )
        .unwrap();
        for file in COMPONENTS {
            std::fs::write(model.join(file), b"fixture").unwrap();
        }
        for file in [
            "config.json",
            "tokenizer.json",
            "model-00001-of-00002.safetensors",
        ] {
            std::fs::write(encoder.join(file), b"fixture").unwrap();
        }
        std::fs::write(
            encoder.join("model.safetensors.index.json"),
            br#"{"weight_map":{"obsolete":"model-00001-of-00005.safetensors"}}"#,
        )
        .unwrap();
        let config = WorkerConfig {
            performance_python: executable.clone(),
            ffmpeg: executable,
            performance_model: model,
            performance_encoder: encoder.clone(),
            ..Default::default()
        };
        assert!(validate_setup(&config).is_err());
        std::fs::write(encoder.join("model-00002-of-00002.safetensors"), b"fixture").unwrap();
        assert!(validate_setup(&config).is_ok());
    }
    #[test]
    fn native_take_selection_rejects_changed_speech_and_audio_placement() {
        use serde_json::json;
        let root =
            std::env::temp_dir().join(format!("df-performance-selection-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store
            .create_series("Explicit registration fixtures", "")
            .unwrap();
        let episode = store.create_episode(&series.id, "Test").unwrap();
        let shot = store
            .add_shot(&episode.id, "A", "Explicit fixture", "Medium", 2.)
            .unwrap();
        let path = root.join("fixture.dat");
        std::fs::write(&path, b"explicit fixture, no generated-media claim").unwrap();
        store.conn.execute("INSERT INTO assets VALUES('voice','fixture',?1,'voice-hash',0,0,1,'{}','unreviewed')",[path.to_string_lossy().to_string()]).unwrap();
        let draft = Dialogue {
            id: uuid::Uuid::new_v4().to_string(),
            shot_id: shot.id.clone(),
            text: "The current line.".into(),
            voice: "af_heart".into(),
            speed: 1.,
            asset_id: None,
            timeline_start: None,
            revision: 0,
        };
        store.save_dialogue_draft(&draft).unwrap();
        store
            .conn
            .execute(
                "UPDATE dialogue SET asset_id='voice' WHERE id=?1",
                [&draft.id],
            )
            .unwrap();
        let line = store.dialogue(&shot.id).unwrap().remove(0);
        let request = PerformanceRequest {
            shot_id: shot.id.clone(),
            source_line: line.clone(),
            reference: path.clone(),
            reference_hash: "fixture".into(),
            audio: path.clone(),
            audio_hash: "voice-hash".into(),
            audio_start: 0.,
            audio_window: None,
            prompt: "Explicit fixture".into(),
            frames: 49,
            width: 704,
            height: 1280,
            seed: 1,
            context: json!({}),
            setup: WorkerConfig::default(),
        };
        let metadata = json!({"kind":"performance","request":request});
        store.conn.execute("INSERT INTO assets VALUES('performance','fixture',?1,'picture-hash',704,1280,3,?2,'unreviewed')",params![path.to_string_lossy(),metadata.to_string()]).unwrap();
        store.assign_take(&shot.id, "performance").unwrap();
        store.set_dialogue_timing(&line.id, Some(0.25)).unwrap();
        assert!(store.assign_take(&shot.id, "performance").is_err());
        store.set_dialogue_timing(&line.id, None).unwrap();
        store.assign_take(&shot.id, "performance").unwrap();
        store
            .conn
            .execute(
                "UPDATE assets SET hash='different-voice-waveform' WHERE id='voice'",
                [],
            )
            .unwrap();
        assert!(store.assign_take(&shot.id, "performance").is_err());
        store
            .conn
            .execute("UPDATE assets SET hash='voice-hash' WHERE id='voice'", [])
            .unwrap();
        store.assign_take(&shot.id, "performance").unwrap();
        let mut edited = store.dialogue(&shot.id).unwrap().remove(0);
        edited.text = "Revised words.".into();
        store.save_dialogue_draft(&edited).unwrap();
        assert!(store.assign_take(&shot.id, "performance").is_err());
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceRequest {
    pub shot_id: String,
    pub source_line: Dialogue,
    pub reference: PathBuf,
    pub reference_hash: String,
    pub audio: PathBuf,
    pub audio_hash: String,
    pub audio_start: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_window: Option<crate::performance_window::PerformanceAudioWindow>,
    pub prompt: String,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub seed: u32,
    pub context: serde_json::Value,
    pub setup: WorkerConfig,
}
impl PerformanceRequest {
    pub fn validate(&self) -> Result<()> {
        validate_setup(&self.setup)?;
        if self.prompt.trim().is_empty()
            || !(9..=121).contains(&self.frames)
            || !(self.frames - 1).is_multiple_of(8)
            || self.width != 704
            || self.height != 1280
            || !self.audio_start.is_finite()
            || self.audio_start < 0.
            || self.audio_start >= self.frames as f64 / 24.
        {
            bail!(
                "Unsupported performance request; use the bounded portrait profile with 8n+1 frames"
            );
        }
        if let Some(window) = &self.audio_window {
            let wave = hound::WavReader::open(&self.audio)?;
            let duration = wave.duration() as f64 / wave.spec().sample_rate as f64;
            if !window.source_start.is_finite()
                || window.source_start < 0.
                || window.source_start >= duration
            {
                bail!("Unsupported performance speech source window");
            }
        }
        if crate::worker::hash_file(&self.reference)? != self.reference_hash
            || crate::worker::hash_file(&self.audio)? != self.audio_hash
        {
            bail!("Performance reference or speech changed; prepare a new request");
        }
        Ok(())
    }
}
impl Store {
    fn visible_performance_lines(&self, shot: &str) -> Result<Vec<Dialogue>> {
        let mut visible = Vec::new();
        for line in self.dialogue(shot)? {
            if self.dialogue_direction(&line.id)?.speaker_visible {
                visible.push(line);
            }
        }
        Ok(visible)
    }
    pub(crate) fn validate_performance_take(
        &self,
        shot: &crate::production::Shot,
        asset: &crate::production::Asset,
    ) -> Result<()> {
        if asset.metadata["kind"] == "lipsync"
            && asset.metadata["request"]["shot_id"].as_str() == Some(shot.id.as_str())
        {
            for line in self.dialogue(&shot.id)? {
                if line.asset_id.as_deref() == asset.metadata["request"]["audio_asset_id"].as_str()
                {
                    self.require_visible_speaker(&line.id)?;
                }
            }
        }
        if asset.metadata["kind"] != "performance" {
            return Ok(());
        }
        let request: PerformanceRequest =
            serde_json::from_value(asset.metadata["request"].clone())?;
        let (line, audio_start, source_start) = if request.audio_window.is_some() {
            let line = self.performance_line(&request.source_line.id, &shot.episode_id)?;
            let audio = self
                .assets()?
                .into_iter()
                .find(|a| Some(&a.id) == line.asset_id.as_ref())
                .context("Generate current speech before selecting this performance")?;
            let (delay, window) = self.performance_window(shot, &line, audio.duration)?;
            (line, delay, window.source_start)
        } else {
            let lines = self.visible_performance_lines(&shot.id)?;
            if lines.len() != 1 {
                bail!(
                    "This performance needs its on-screen dialogue; the source is off screen or missing"
                );
            }
            let line = lines[0].clone();
            let shot_start: f64 = self
                .shots(&shot.episode_id)?
                .iter()
                .take_while(|s| s.id != shot.id)
                .map(|s| s.duration)
                .sum();
            let start =
                shot.source_start + line.timeline_start.map(|v| v - shot_start).unwrap_or(0.);
            (line, start, 0.)
        };
        self.require_visible_speaker(&line.id)?;
        let audio = self
            .assets()?
            .into_iter()
            .find(|a| Some(&a.id) == line.asset_id.as_ref())
            .context("Generate current speech before selecting this performance")?;
        if request.audio_hash != audio.hash
            || request.source_line.text != line.text
            || (request.audio_start - audio_start).abs() > 1. / 48000.
            || (request
                .audio_window
                .as_ref()
                .map(|w| w.source_start)
                .unwrap_or(0.)
                - source_start)
                .abs()
                > 1. / 48000.
        {
            bail!(
                "This native performance belongs to earlier speech or timing; regenerate it for the current dialogue and picture in-point"
            );
        }
        Ok(())
    }
    pub fn enqueue_performance_window(
        &self,
        shot_id: &str,
        audio_id: &str,
        reference: Option<&Path>,
        source_line_id: Option<&str>,
    ) -> Result<crate::store::Job> {
        let shot = self.shot(shot_id)?;
        let line = if let Some(id) = source_line_id {
            self.performance_line(id, &shot.episode_id)?
        } else {
            for line in self.dialogue(shot_id)? {
                if line.asset_id.as_deref() == Some(audio_id) {
                    self.require_visible_speaker(&line.id)?;
                }
            }
            let lines = self.visible_performance_lines(shot_id)?;
            if lines.len() != 1 {
                bail!(
                    "Native performance needs one visible source line; select its source explicitly for continuation coverage"
                );
            }
            lines[0].clone()
        };
        self.require_visible_speaker(&line.id)?;
        let setup = self.config()?;
        validate_setup(&setup)?;
        if line.asset_id.as_deref() != Some(audio_id) {
            bail!("Choose the current generated speech for this shot");
        }
        let assets = self.assets()?;
        let audio = assets
            .iter()
            .find(|asset| asset.id == audio_id)
            .context("Speech asset not found")?;
        if audio.width != 0
            || !audio.duration.is_finite()
            || audio.duration <= 0.
            || audio.review == "rejected"
        {
            bail!("Choose a usable speech take before generating a performance");
        }
        let (audio_start, audio_window) = self.performance_window(&shot, &line, audio.duration)?;
        let duration = shot.source_start + shot.duration;
        if !duration.is_finite() || duration > 121. / 24. {
            bail!("This picture take exceeds the five-second local profile; shorten the coverage");
        }
        let frames = (((duration * 24. - 1.) / 8.).ceil().max(1.) as u32) * 8 + 1;
        if frames > 121 {
            bail!(
                "This performance exceeds the five-second local profile; split the line or coverage"
            );
        }
        let prepared = self.prepare_shot_render(shot_id)?;
        let image = reference
            .map(Path::to_path_buf)
            .or(prepared.request.image)
            .context("Link a cast reference to this shot, or supply a reference image")?;
        let request = PerformanceRequest {
            shot_id: shot_id.into(),
            source_line: line.clone(),
            reference_hash: crate::worker::hash_file(&image)?,
            reference: image,
            audio: audio.path.clone(),
            audio_hash: audio.hash.clone(),
            audio_start,
            audio_window: Some(audio_window),
            prompt: format!(
                "Live-action dialogue performance. Follow only the supplied speech segment, including words already in progress at the start. Do not invent or compress unheard words. Keep natural mouth shapes synchronized with that segment and listen when it ends. {}",
                prepared.request.prompt
            ),
            frames,
            width: 704,
            height: 1280,
            seed: 186,
            context: self.shot_context(shot_id)?,
            setup,
        };
        request.validate()?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,'Native dialogue performance · experimental','queued','',?2,'performance',datetime('now'),datetime('now'))",params![id,serde_json::to_string(&request)?])?;
        self.job(&id)
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    #[test]
    fn long_speech_queues_bounded_continuations_and_rejects_listener_or_stale_timing() {
        let root =
            std::env::temp_dir().join(format!("df-performance-window-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let model = root.join("model");
        let encoder = root.join("encoder");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::create_dir_all(&encoder).unwrap();
        for file in COMPONENTS {
            std::fs::write(model.join(file), b"file availability fixture, not weights").unwrap();
        }
        for file in ["config.json", "tokenizer.json", "model.safetensors"] {
            std::fs::write(encoder.join(file), b"fixture").unwrap();
        }
        let executable = root.join("runtime-fixture");
        std::fs::write(&executable, b"not executed").unwrap();
        store
            .save_config(&WorkerConfig {
                performance_python: executable.clone(),
                ffmpeg: executable,
                performance_model: model,
                performance_encoder: encoder,
                ..Default::default()
            })
            .unwrap();
        let series = store.create_series("Window fixtures", "").unwrap();
        let episode = store.create_episode(&series.id, "Fixture").unwrap();
        let a = store
            .add_shot(&episode.id, "Speaking", "Explicit fixture", "", 3.)
            .unwrap();
        let b = store
            .add_shot(&episode.id, "Continuation", "Explicit fixture", "", 3.)
            .unwrap();
        let c = store
            .add_shot(&episode.id, "Listener", "Explicit fixture", "", 3.)
            .unwrap();
        let image = root.join("reference.png");
        image::RgbImage::new(32, 32).save(&image).unwrap();
        let actor = store
            .add_library(
                &series.id,
                "character",
                "Speaker",
                "Adult fixture",
                Some(&image),
            )
            .unwrap();
        let listener = store
            .add_library(
                &series.id,
                "character",
                "Listener",
                "Adult fixture",
                Some(&image),
            )
            .unwrap();
        for (shot, actor) in [(&a, &actor), (&b, &actor), (&c, &listener)] {
            store
                .save_shot_direction(
                    &shot.id,
                    &crate::directing::ShotDirection {
                        character_ids: vec![actor.id.clone()],
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let wave = root.join("six-seconds.wav");
        let mut writer = hound::WavWriter::create(
            &wave,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..96000 {
            writer.write_sample(1000i16).unwrap();
        }
        writer.finalize().unwrap();
        let hash = crate::worker::hash_file(&wave).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('voice','fixture',?1,?2,0,0,6,'{}','unreviewed')",
                params![wave.to_string_lossy(), hash],
            )
            .unwrap();
        let line = store
            .save_dialogue_draft(&Dialogue {
                id: uuid::Uuid::new_v4().to_string(),
                shot_id: a.id.clone(),
                text: "A long fixture line spanning multiple camera cuts.".into(),
                voice: "af_heart".into(),
                speed: 1.,
                asset_id: None,
                timeline_start: Some(0.5),
                revision: 0,
            })
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE dialogue SET asset_id='voice' WHERE id=?1",
                [&line.id],
            )
            .unwrap();
        let first = store
            .enqueue_performance_window(&a.id, "voice", None, None)
            .unwrap();
        let request: PerformanceRequest = serde_json::from_str(
            &store
                .conn
                .query_row("SELECT request FROM jobs WHERE id=?1", [&first.id], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(request.frames, 73);
        assert_eq!(request.audio_start, 0.5);
        assert_eq!(request.audio_window.unwrap().source_start, 0.);
        let job = store
            .enqueue_performance_window(&b.id, "voice", None, Some(&line.id))
            .unwrap();
        let request: PerformanceRequest = serde_json::from_str(
            &store
                .conn
                .query_row("SELECT request FROM jobs WHERE id=?1", [&job.id], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(request.frames, 73);
        assert_eq!(request.audio_start, 0.);
        assert_eq!(request.audio_window.as_ref().unwrap().source_start, 2.5);
        assert!(
            store
                .enqueue_performance_window(&c.id, "voice", None, Some(&line.id))
                .err()
                .unwrap()
                .to_string()
                .contains("different primary character")
        );
        let metadata = serde_json::json!({"kind":"performance","request":request,"explicit_metadata_only_fixture":true});
        store.conn.execute("INSERT INTO assets VALUES('performance','fixture',?1,'fixture',704,1280,3.0416666667,?2,'unreviewed')",params![wave.to_string_lossy(),metadata.to_string()]).unwrap();
        store.assign_take(&b.id, "performance").unwrap();
        store.set_dialogue_timing(&line.id, Some(0.75)).unwrap();
        assert!(store.assign_take(&b.id, "performance").is_err());
        store.set_dialogue_timing(&line.id, Some(20.)).unwrap();
        assert!(
            store
                .enqueue_performance_window(&b.id, "voice", None, Some(&line.id))
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
