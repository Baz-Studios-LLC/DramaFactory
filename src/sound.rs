//! Persistent, non-destructive episode sound beds and effects.
use crate::{production::Asset, store::Store};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioCue {
    pub id: String,
    pub episode_id: String,
    pub asset_id: String,
    pub name: String,
    pub kind: String,
    pub start: f64,
    pub source_start: f64,
    pub duration: f64,
    pub gain_db: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CueDraft {
    pub cue: AudioCue,
    pub expected: AudioCue,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExportCue {
    #[serde(default, skip_serializing_if = "zero")]
    pub skip: f64,
    pub cue: AudioCue,
    pub path: std::path::PathBuf,
    pub hash: String,
}
fn zero(value: &f64) -> bool {
    *value == 0.
}
impl Store {
    pub fn import_audio(&self, path: &Path) -> Result<Asset> {
        // WAV is intentionally the initial import format. Preserve the source
        // bytes; decoded samples are inspected before adding to the library.
        if std::fs::metadata(path)?.len() > 200_000_000 {
            bail!("WAV imports are limited to 200 MB");
        }
        let mut reader = hound::WavReader::open(path).context("Choose a PCM or float WAV file")?;
        let spec = reader.spec();
        if !(1..=2).contains(&spec.channels) || !(8000..=192000).contains(&spec.sample_rate) {
            bail!("WAV must be mono or stereo, 8–192 kHz");
        }
        let duration = reader.duration() as f64 / spec.sample_rate as f64;
        if !duration.is_finite() || duration <= 0. || duration > 1800. {
            bail!("Audio must be between 0 and 1800 seconds");
        }
        let expected = reader.len() as usize;
        let count = if spec.sample_format == hound::SampleFormat::Float {
            let mut count = 0;
            for sample in reader.samples::<f32>() {
                if !sample?.is_finite() {
                    bail!("WAV contains invalid floating-point samples");
                }
                count += 1;
            }
            count
        } else {
            reader
                .samples::<i32>()
                .try_fold(0usize, |count, sample| sample.map(|_| count + 1))?
        };
        if count != expected {
            bail!("WAV sample data is incomplete");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.root().join("imports").join(&id);
        std::fs::create_dir_all(&dir)?;
        let destination = dir.join("audio.wav");
        std::fs::copy(path, &destination)?;
        let hash = crate::worker::hash_file(&destination)?;
        if hash != crate::worker::hash_file(path)? {
            bail!("Source changed during import; retry");
        }
        let metadata = serde_json::json!({"kind":"imported_audio","name":path.file_stem().and_then(|s|s.to_str()).unwrap_or("Audio"),"sample_rate":spec.sample_rate,"channels":spec.channels});
        self.conn.execute("INSERT INTO assets(id,job_id,path,hash,width,height,duration,metadata,review) VALUES(?1,'import',?2,?3,0,0,?4,?5,'unreviewed')",params![id,destination.to_string_lossy(),hash,duration,metadata.to_string()])?;
        self.assets()?
            .into_iter()
            .find(|a| a.id == id)
            .context("Imported audio missing")
    }
    pub fn audio_cues(&self, episode: &str) -> Result<Vec<AudioCue>> {
        let values = self
            .conn
            .prepare("SELECT data FROM audio_cues WHERE episode_id=?1 ORDER BY start,rowid")?
            .query_map([episode], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        values
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn save_audio_cue(&self, cue: &AudioCue, expected: Option<&AudioCue>) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.get_episode(&cue.episode_id)?;
        uuid::Uuid::parse_str(&cue.id).context("Invalid cue ID")?;
        if cue.name.trim().is_empty() || !["music", "sfx"].contains(&cue.kind.as_str()) {
            bail!("Cue needs a name and music or sfx kind");
        }
        if [
            cue.start,
            cue.source_start,
            cue.duration,
            cue.gain_db,
            cue.fade_in,
            cue.fade_out,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || cue.start < 0.
            || cue.start > 1800.
            || cue.source_start < 0.
            || cue.duration <= 0.
            || cue.duration > 1800.
            || !(-60. ..=12.).contains(&cue.gain_db)
            || cue.fade_in < 0.
            || cue.fade_out < 0.
            || cue.fade_in + cue.fade_out > cue.duration
        {
            bail!("Invalid cue timing, gain, or fades");
        }
        let asset = self
            .assets()?
            .into_iter()
            .find(|a| a.id == cue.asset_id)
            .context("Audio asset not found")?;
        if asset.width != 0
            || asset.duration <= 0.
            || cue.source_start + cue.duration > asset.duration + 0.001
        {
            bail!("Cue must fit a playable audio asset");
        }
        let previous = self.audio_cue(&cue.id)?;
        match previous {
            Some(current) => {
                if current.episode_id != cue.episode_id {
                    bail!("Cue belongs to another episode");
                }
                if current == *cue {
                    tx.commit()?;
                    return Ok(());
                }
                if expected != Some(&current) {
                    bail!(
                        "Sound cue changed in another session; keep your draft and reload before saving. Existing cues require the expected saved cue."
                    );
                }
            }
            None if expected.is_some() => {
                bail!("Sound cue was removed in another session; your draft must not recreate it")
            }
            None => {}
        }

        self.conn.execute("INSERT INTO audio_cues(id,episode_id,start,data) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET start=excluded.start,data=excluded.data",params![cue.id,cue.episode_id,cue.start,serde_json::to_string(cue)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn audio_cue(&self, id: &str) -> Result<Option<AudioCue>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT data FROM audio_cues WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }
    pub fn remove_audio_cue(&self, id: &str, expected: &AudioCue) -> Result<()> {
        if id != expected.id {
            bail!("Expected cue ID does not match the removal target");
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(current) = self.audio_cue(id)? {
            if current != *expected {
                bail!(
                    "Sound cue changed in another session; keep your draft and reload before removing it"
                );
            }
            tx.execute("DELETE FROM audio_cues WHERE id=?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }
}
