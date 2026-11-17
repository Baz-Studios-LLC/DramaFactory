//! Explicit take selection and soundtrack-preserving assembly for hosted episodes.
use crate::{
    production::{ExportAudio, ExportClip, ExportRequest},
    store::Store,
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
impl Store {
    pub fn delete_segment_clip(&self, episode: &str, segment: &str, id: &str) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let asset = self
            .assets()?
            .into_iter()
            .find(|asset| asset.id == id)
            .context("Clip no longer exists")?;
        let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM segment_generations g JOIN jobs j ON j.id=g.job_id WHERE g.job_id=?1 AND g.episode_id=?2 AND g.segment_id=?3 AND j.kind='openrouter_video' AND j.status='completed')", params![asset.job_id,episode,segment], |row|row.get(0))?;
        if !owned {
            bail!("Clip does not belong to this completed segment");
        }
        let path_text = asset.path.to_string_lossy();
        let used: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE id<>?1 AND (instr(request,?2)>0 OR instr(request,?3)>0)) OR EXISTS(SELECT 1 FROM assets WHERE id<>?3 AND path=?2) OR EXISTS(SELECT 1 FROM shots WHERE asset_id=?3) OR EXISTS(SELECT 1 FROM settings WHERE key LIKE 'episode-cut:%' AND instr(value,?3)>0)", params![asset.job_id,path_text,id], |row|row.get(0))?;
        if used {
            bail!("This clip is used by another saved production or export and cannot be deleted.");
        }
        let folder = self
            .root()
            .join("jobs")
            .join(&asset.job_id)
            .canonicalize()?;
        let path = asset.path.canonicalize()?;
        if !path.starts_with(folder)
            || std::fs::symlink_metadata(&asset.path)?
                .file_type()
                .is_symlink()
        {
            bail!("Only managed generated clips can be deleted");
        }
        let staged = path.with_extension(format!("delete-{}", uuid::Uuid::new_v4()));
        tx.execute("DELETE FROM segment_takes WHERE asset_id=?1", [id])?;
        tx.execute("DELETE FROM asset_review_notes WHERE asset_id=?1", [id])?;
        tx.execute("DELETE FROM asset_review_history WHERE asset_id=?1", [id])?;
        tx.execute(
            "UPDATE jobs SET output_id=NULL,phase='Clip deleted' WHERE output_id=?1",
            [id],
        )?;
        tx.execute("DELETE FROM assets WHERE id=?1", [id])?;
        std::fs::rename(&path, &staged)?;
        if let Err(error) = tx.commit() {
            std::fs::rename(&staged, &path)?;
            return Err(error.into());
        }
        std::fs::remove_file(staged)
            .context("Clip removed from library, but file cleanup failed")?;
        let _ = std::fs::remove_file(
            self.root()
                .join("cache/take-thumbnails")
                .join(format!("{id}.jpg")),
        );
        Ok(())
    }
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
        plan.segments
            .iter()
            .find(|item| item.id == segment)
            .context("Segment no longer exists")?;
        let snapshot:String=self.conn.query_row("SELECT g.snapshot FROM segment_generations g JOIN assets a ON a.job_id=g.job_id JOIN jobs j ON j.id=g.job_id WHERE a.id=?1 AND g.episode_id=?2 AND g.segment_id=?3 AND j.status='completed'",params![asset,episode,segment],|row|row.get(0)).context("Take does not belong to this completed episode segment")?;
        let _: crate::generation_plan::SegmentDraft = serde_json::from_str(&snapshot)?;
        let asset = self
            .assets()?
            .into_iter()
            .find(|item| item.id == asset)
            .context("Take not found")?;
        if asset.width == 0 || asset.duration <= 0. {
            bail!("Choose a valid video clip");
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
                .context("Choose a clip for every segment before export")?;
            let asset = self.validate_segment_take(episode, &segment.id, &id)?;
            clips.push(ExportClip {
                gap_before: 0.,
                audio_fade_out: 0.,
                audio_fade_in: 0.,
                audio_gain_db: 0.,
                audio_muted: false,
                asset_id: id,
                path: asset.path.clone(),
                hash: asset.hash.clone(),
                duration: asset.duration,
                source_start: 0.,
                offscreen_dialogue_ids: Vec::new(),
                picture_fades: Default::default(),
                direction_context: serde_json::json!({"segment_id":segment.id}),
                audio: vec![ExportAudio {
                    source_start: 0.,
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
            width: if plan
                .segments
                .iter()
                .any(|segment| segment.resolution == "1080p")
            {
                1080
            } else if plan
                .segments
                .iter()
                .any(|segment| segment.resolution == "720p")
            {
                720
            } else {
                480
            },
            height: if plan
                .segments
                .iter()
                .any(|segment| segment.resolution == "1080p")
            {
                1920
            } else if plan
                .segments
                .iter()
                .any(|segment| segment.resolution == "720p")
            {
                1280
            } else {
                854
            },
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clip_deletion_preserves_foreign_and_used_media_then_clears_selection() {
        let root = std::env::temp_dir().join(format!("df-clip-delete-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Test", "").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let mut plan = store.generation_plan(&episode.id).unwrap();
        plan.segments[0].prompt = "Scene".into();
        let plan = store.save_generation_plan(&plan).unwrap();
        let segment = &plan.segments[0];
        let job = store
            .enqueue_episode_segment(&episode.id, plan.revision, segment)
            .unwrap();
        let folder = root.join("jobs").join(&job.id).join("remote");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("take.mp4");
        std::fs::write(&path, b"fixture media").unwrap();
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('clip',?1,?2,'hash',480,854,30,'{}','rejected')",
                params![job.id, path.to_string_lossy()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE jobs SET status='completed',output_id='clip' WHERE id=?1",
                [&job.id],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO segment_takes VALUES(?1,?2,'clip')",
                params![episode.id, segment.id],
            )
            .unwrap();
        assert!(
            store
                .delete_segment_clip(&episode.id, "wrong", "clip")
                .is_err()
        );
        store.conn.execute("INSERT INTO jobs(id,prompt,status,kind,request,created_at,updated_at) VALUES('export','test','queued','export',?1,datetime('now'),datetime('now'))",[format!("{{\"path\":{}}}",serde_json::to_string(&path).unwrap())]).unwrap();
        assert!(
            store
                .delete_segment_clip(&episode.id, &segment.id, "clip")
                .is_err()
        );
        assert!(path.exists());
        store
            .conn
            .execute("DELETE FROM jobs WHERE id='export'", [])
            .unwrap();
        store
            .delete_segment_clip(&episode.id, &segment.id, "clip")
            .unwrap();
        assert!(!path.exists());
        assert!(
            store
                .selected_segment_take(&episode.id, &segment.id)
                .unwrap()
                .is_none()
        );
        assert!(store.assets().unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
