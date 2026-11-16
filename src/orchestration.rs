//! Resumable production steps operate on saved authoring state. No creative
//! writing or editorial approval is delegated to these queue operations.
use crate::{
    production::VoiceRequest,
    store::{Job, Store},
};
use anyhow::{Result, bail};
use rusqlite::params;
use serde::Serialize;

#[derive(Serialize)]
pub struct DialogueBatch {
    pub episode_id: String,
    pub ready_lines: usize,
    pub newly_queued: usize,
    pub reused_pending: usize,
    pub retried: usize,
    pub jobs: Vec<Job>,
}
impl Store {
    /// One immediate transaction makes repeated requests, including requests
    /// from different producer processes, converge on the same pending jobs.
    pub fn produce_episode_dialogue(&self, episode: &str) -> Result<DialogueBatch> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.get_episode(episode)?;
        let lines = self
            .shots(episode)?
            .into_iter()
            .map(|shot| self.dialogue(&shot.id))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if lines.is_empty() || lines.len() > 300 {
            bail!("Save between 1 and 300 dialogue lines before producing episode speech");
        }
        let assets = self.assets()?;
        let mut missing = Vec::new();
        let mut ready = 0;
        for line in lines {
            crate::production::validate_dialogue(&line.text, &line.voice, line.speed)?;
            let usable = assets
                .iter()
                .find(|asset| Some(&asset.id) == line.asset_id.as_ref())
                .is_some_and(|asset| {
                    asset.width == 0
                        && asset.duration > 0.
                        && asset.duration.is_finite()
                        && asset.review != "rejected"
                        && asset.path.is_file()
                        && crate::worker::hash_file(&asset.path)
                            .is_ok_and(|hash| hash == asset.hash)
                });
            if usable {
                ready += 1;
            } else {
                missing.push(line);
            }
        }
        if !missing.is_empty() {
            let config = self.config()?;
            let root = self.speech_root()?;
            if !config.python.is_file()
                || !root.join("kokoro-v1.0.int8.onnx").is_file()
                || !root.join("voices-v1.0.bin").is_file()
            {
                bail!(
                    "Local Kokoro model and Python worker are not configured; saved dialogue is preserved"
                );
            }
        }
        let candidates=self.conn.prepare("SELECT id,request,status,cancel_requested FROM jobs WHERE kind='dialogue' AND status IN ('queued','running','failed','canceled','blocked') ORDER BY rowid DESC")?.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,bool>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        let mut result = DialogueBatch {
            episode_id: episode.into(),
            ready_lines: ready,
            newly_queued: 0,
            reused_pending: 0,
            retried: 0,
            jobs: Vec::new(),
        };
        for line in missing {
            let voice_revision: u64 = self.conn.query_row(
                "SELECT voice_revision FROM dialogue WHERE id=?1",
                [&line.id],
                |row| row.get(0),
            )?;
            let existing = candidates.iter().find(|(_, request, _, _)| {
                serde_json::from_str::<VoiceRequest>(request).is_ok_and(|request| {
                    request.line_id == line.id
                        && request.line_revision == Some(voice_revision)
                        && request.text == line.text
                        && request.voice == line.voice
                        && request.speed == line.speed
                })
            });
            let id = if let Some((id, _, status, _)) = existing {
                if status == "queued" || status == "running" {
                    // Cancellation remains requested until the consumer exits;
                    // a later explicit resume can retry that terminal attempt.
                    result.reused_pending += 1;
                } else {
                    tx.execute("UPDATE jobs SET status='queued',error='',progress=0,cancel_requested=0,phase='',updated_at=datetime('now') WHERE id=?1",[id])?;
                    result.retried += 1;
                }
                id.clone()
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                let request = VoiceRequest {
                    text: line.text.clone(),
                    voice: line.voice.clone(),
                    speed: line.speed,
                    line_id: line.id.clone(),
                    line_revision: Some(voice_revision + 1),
                };
                tx.execute("UPDATE dialogue SET revision=revision+1,voice_revision=voice_revision+1,asset_id=NULL WHERE id=?1",[&line.id])?;
                tx.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,?2,'queued','',?3,'dialogue',datetime('now'),datetime('now'))",params![id,format!("Dialogue · {}",line.text),serde_json::to_string(&request)?])?;
                result.newly_queued += 1;
                id
            };
            result.jobs.push(self.job(&id)?);
        }
        tx.commit()?;
        Ok(result)
    }
}
