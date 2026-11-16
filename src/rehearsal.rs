//! Audio-first editorial review before committing to expensive video takes.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct RehearsalLine {
    pub line_id: String,
    pub text: String,
    pub voice: String,
    pub start: f64,
    pub duration: f64,
    pub path: PathBuf,
    pub hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RehearsalRequest {
    pub episode_id: String,
    pub screenplay: String,
    pub duration: f64,
    pub lines: Vec<RehearsalLine>,
}
impl Store {
    pub fn rehearsal_request(&self, episode: &str) -> Result<RehearsalRequest> {
        let episode = self.get_episode(episode)?;
        let shots = self.shots(&episode.id)?;
        let duration: f64 = shots.iter().map(|shot| shot.duration).sum();
        if !duration.is_finite() || !(0.1..=1800.).contains(&duration) {
            bail!(
                "Plan an episode timeline of up to 30 minutes before exporting a dialogue rehearsal"
            );
        }
        let assets = self.assets()?;
        let mut lines = Vec::new();
        let mut shot_start = 0.;
        for shot in shots {
            let mut local_start = 0.;
            for line in self.dialogue(&shot.id)? {
                let asset = assets
                    .iter()
                    .find(|asset| Some(&asset.id) == line.asset_id.as_ref())
                    .with_context(|| {
                        format!(
                            "Generate speech for ‘{}’ before exporting the rehearsal",
                            line.text
                        )
                    })?;
                if asset.width != 0
                    || asset.duration <= 0.
                    || !asset.duration.is_finite()
                    || asset.review == "rejected"
                {
                    bail!("Dialogue needs a usable speech take: {}", line.text);
                }
                let start = line.timeline_start.unwrap_or(shot_start + local_start);
                if !start.is_finite() || start < 0. || start + asset.duration > duration + 0.01 {
                    bail!(
                        "Dialogue extends past the episode; adjust timing or shot lengths: {}",
                        line.text
                    );
                }
                if line.timeline_start.is_none() {
                    local_start += asset.duration;
                    if local_start > shot.duration + 0.01 {
                        bail!(
                            "Dialogue outlasts its shot; enable a dialogue bridge or extend the shot: {}",
                            line.text
                        );
                    }
                }
                if crate::worker::hash_file(&asset.path)? != asset.hash {
                    bail!("Speech source changed; regenerate or restore the take");
                }
                lines.push(RehearsalLine {
                    line_id: line.id,
                    text: line.text,
                    voice: line.voice,
                    start,
                    duration: asset.duration,
                    path: asset.path.clone(),
                    hash: asset.hash.clone(),
                });
            }
            shot_start += shot.duration;
        }
        if lines.is_empty() || lines.len() > 300 {
            bail!("Dialogue rehearsals need between 1 and 300 generated lines");
        }
        Ok(RehearsalRequest {
            episode_id: episode.id,
            screenplay: episode.screenplay,
            duration,
            lines,
        })
    }
    pub fn enqueue_rehearsal(&self, episode: &str) -> Result<crate::store::Job> {
        if !self.config()?.ffmpeg.is_file() {
            bail!("Configure FFmpeg before exporting audio");
        }
        let request = self.rehearsal_request(episode)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,'Dialogue rehearsal · audio only','queued','',?2,'rehearsal',datetime('now'),datetime('now'))",params![id,serde_json::to_string(&request)?])?;
        self.job(&id)
    }
}
