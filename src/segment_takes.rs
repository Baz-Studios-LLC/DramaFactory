//! Explicit take selection and soundtrack-preserving assembly for hosted episodes.
use crate::{
    production::{ExportAudio, ExportClip, ExportRequest},
    store::Store,
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
impl Store {
    pub fn selected_segment_take(&self, episode: &str, segment: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT asset_id FROM segment_takes WHERE episode_id=?1 AND segment_id=?2",
                params![episode, segment],
                |row| row.get(0),
            )
            .optional()?)
    }
    fn validate_segment_take(
        &self,
        episode: &str,
        segment: &str,
        asset: &str,
    ) -> Result<crate::production::Asset> {
        let plan = self.generation_plan(episode)?;
        let current = plan
            .segments
            .iter()
            .find(|item| item.id == segment)
            .context("Segment no longer exists")?;
        let snapshot:String=self.conn.query_row("SELECT g.snapshot FROM segment_generations g JOIN assets a ON a.job_id=g.job_id JOIN jobs j ON j.id=g.job_id WHERE a.id=?1 AND g.episode_id=?2 AND g.segment_id=?3 AND j.status='completed'",params![asset,episode,segment],|row|row.get(0)).context("Take does not belong to this completed episode segment")?;
        let snapshot: crate::generation_plan::SegmentDraft = serde_json::from_str(&snapshot)?;
        if &snapshot != current {
            bail!(
                "Segment changed since this take was generated. Review a take from the current plan."
            );
        }
        for reference in &current.cast {
            self.validate_cast_reference(episode, reference)?;
        }
        let asset = self
            .assets()?
            .into_iter()
            .find(|item| item.id == asset)
            .context("Take not found")?;
        if asset.review != "approved" || asset.width == 0 || asset.duration <= 0. {
            bail!("Approve a valid video take before selecting it");
        }
        if crate::worker::hash_file(&asset.path)? != asset.hash {
            bail!("Selected take bytes changed");
        }
        let info = crate::media::probe(&self.config()?.ffmpeg, &asset.path)?;
        if !info.audio
            || info.width != asset.width
            || info.height != asset.height
            || (info.duration - asset.duration).abs() > 0.1
        {
            bail!("Take metadata or generated soundtrack no longer matches");
        }
        Ok(asset)
    }
    pub fn select_segment_take(
        &self,
        episode: &str,
        segment: &str,
        asset: &str,
        expected: Option<&str>,
    ) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.validate_segment_take(episode, segment, asset)?;
        let current = self.selected_segment_take(episode, segment)?;
        if current.as_deref() != Some(asset) && current.as_deref() != expected {
            bail!("Selected take changed elsewhere; reload before choosing another");
        }
        tx.execute("INSERT INTO segment_takes(episode_id,segment_id,asset_id) VALUES(?1,?2,?3) ON CONFLICT(episode_id,segment_id) DO UPDATE SET asset_id=excluded.asset_id",params![episode,segment,asset])?;
        tx.commit()?;
        Ok(())
    }
    pub fn hosted_episode_export_request(&self, episode: &str) -> Result<ExportRequest> {
        let episode_data = self.get_episode(episode)?;
        let plan = self.generation_plan(episode)?;
        if plan.revision == 0 || plan.segments.is_empty() {
            bail!("Save an episode generation plan first");
        }
        let mut clips = Vec::new();
        for segment in &plan.segments {
            let id = self
                .selected_segment_take(episode, &segment.id)?
                .context("Choose an approved take for every segment before export")?;
            let asset = self.validate_segment_take(episode, &segment.id, &id)?;
            clips.push(ExportClip {
                asset_id: id,
                path: asset.path.clone(),
                hash: asset.hash.clone(),
                duration: asset.duration,
                source_start: 0.,
                offscreen_dialogue_ids: Vec::new(),
                picture_fades: Default::default(),
                direction_context: serde_json::json!({"segment_id":segment.id}),
                audio: vec![ExportAudio {
                    path: asset.path,
                    hash: asset.hash,
                    duration: asset.duration,
                }],
            });
        }
        Ok(ExportRequest {
            episode_id: episode.into(),
            screenplay: episode_data.screenplay,
            continuity_context: serde_json::json!({"hosted_segments":plan,"audio":"Each original generated segment soundtrack, once, in saved segment order"}),
            clips,
            cues: Vec::new(),
            captions: Vec::new(),
            burn_captions: false,
            width: 720,
            height: 1280,
            fps: 24,
        })
    }
    pub fn enqueue_hosted_episode_export(&self, episode: &str) -> Result<crate::store::Job> {
        let request = self.hosted_episode_export_request(episode)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,kind,request,created_at,updated_at) VALUES(?1,'Assemble hosted episode · original soundtracks','queued','export',?2,datetime('now'),datetime('now'))",params![id,serde_json::to_string(&request)?])?;
        self.job(&id)
    }
}
