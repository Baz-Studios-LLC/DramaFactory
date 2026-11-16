//! Non-destructive episode cuts, independent of generation plans and review labels.
use crate::{
    production::{ExportAudio, ExportClip, ExportRequest},
    store::Store,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub asset_id: String,
    pub title: String,
    pub start: f64,
    pub duration: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub clips: Vec<Clip>,
}
impl Cut {
    pub fn source_at(&self, seconds: f64) -> Option<(&str, f64)> {
        let total: f64 = self.clips.iter().map(|clip| clip.duration).sum();
        let mut time = seconds.max(0.).min((total - 1. / 24.).max(0.));
        for clip in &self.clips {
            if time < clip.duration {
                return Some((&clip.asset_id, clip.start + time));
            }
            time -= clip.duration;
        }
        None
    }
    pub fn playback_from(&self, seconds: f64) -> Self {
        let mut remaining = seconds.max(0.);
        let mut clips = Vec::new();
        for source in &self.clips {
            if remaining >= source.duration {
                remaining -= source.duration;
                continue;
            }
            let mut clip = source.clone();
            clip.start += remaining;
            clip.duration -= remaining;
            remaining = 0.;
            if clip.duration >= 0.04 {
                clips.push(clip);
            }
        }
        Self { clips }
    }
}
impl Store {
    pub fn episode_cut(&self, episode: &str) -> Result<Cut> {
        self.get_episode(episode)?;
        use rusqlite::OptionalExtension;
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("episode-cut:{episode}")],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|text| serde_json::from_str(&text).map_err(Into::into))
            .unwrap_or_else(|| Ok(Cut::default()))
    }
    pub fn latest_cut_export(&self, episode: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self.conn.query_row("SELECT id FROM jobs WHERE kind='export' AND prompt='Editing room export' AND json_extract(request,'$.episode_id')=?1 ORDER BY rowid DESC LIMIT 1",[episode],|row|row.get(0)).optional()?)
    }
    pub fn validate_cut(&self, episode: &str, cut: &Cut) -> Result<()> {
        self.get_episode(episode)?;
        if cut.clips.len() > 500 {
            bail!("A cut can contain at most 500 clips");
        }
        let assets = self.assets()?;
        let mut ids = std::collections::HashSet::new();
        for clip in &cut.clips {
            if clip.id.is_empty() || !ids.insert(&clip.id) {
                bail!("Duplicate timeline clip");
            }
            let asset = assets
                .iter()
                .find(|a| a.id == clip.asset_id)
                .context("A timeline clip was deleted")?;
            let owned: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM segment_generations WHERE episode_id=?1 AND job_id=?2)",rusqlite::params![episode,asset.job_id],|r|r.get(0))?;
            if !owned {
                bail!("Choose clips from this episode");
            }
            if !clip.start.is_finite()
                || !clip.duration.is_finite()
                || clip.start < 0.
                || clip.duration < 0.04
                || clip.start + clip.duration > asset.duration + 0.01
            {
                bail!("Trim must stay within the source clip");
            }
        }
        Ok(())
    }
    pub fn save_episode_cut(&self, episode: &str, expected: &Cut, cut: &Cut) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = self.episode_cut(episode)?;
        if &current != expected && &current != cut {
            bail!(
                "The edit changed in another window. Your draft is preserved; reopen the episode before editing further."
            );
        }
        self.validate_cut(episode, cut)?;
        tx.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![format!("episode-cut:{episode}"),serde_json::to_string(cut)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn cut_export_request(&self, episode: &str, cut: &Cut) -> Result<ExportRequest> {
        self.validate_cut(episode, cut)?;
        if cut.clips.is_empty() {
            bail!("Add clips to the timeline first");
        }
        let assets = self.assets()?;
        let mut clips = Vec::new();
        for item in &cut.clips {
            let asset = assets
                .iter()
                .find(|a| a.id == item.asset_id)
                .context("Clip missing")?;
            if crate::worker::hash_file(&asset.path)? != asset.hash {
                bail!("Source clip changed");
            }
            clips.push(ExportClip {
                asset_id: asset.id.clone(),
                path: asset.path.clone(),
                hash: asset.hash.clone(),
                source_start: item.start,
                duration: item.duration,
                offscreen_dialogue_ids: vec![],
                picture_fades: Default::default(),
                direction_context: serde_json::json!({"editing_room_clip":item.id}),
                audio: vec![ExportAudio {
                    path: asset.path.clone(),
                    hash: asset.hash.clone(),
                    duration: item.duration,
                    source_start: item.start,
                }],
            });
        }
        Ok(ExportRequest {
            episode_id: episode.into(),
            screenplay: self.get_episode(episode)?.screenplay,
            continuity_context: serde_json::json!({"editing_room":cut}),
            clips,
            cues: vec![],
            captions: vec![],
            burn_captions: false,
            width: 480,
            height: 854,
            fps: 24,
        })
    }
    pub fn preview_episode_cut(&self, episode: &str, cut: &Cut) -> Result<crate::store::Job> {
        use rusqlite::OptionalExtension;
        let mut request = self.cut_export_request(episode, cut)?;
        request.width = 288;
        request.height = 512;
        let json = serde_json::to_string(&request)?;
        let existing: Option<String> = self.conn.query_row("SELECT id FROM jobs WHERE kind='export' AND prompt='Timeline preview' AND request=?1 AND status IN ('queued','running','completed') ORDER BY rowid DESC LIMIT 1", [&json], |r| r.get(0)).optional()?;
        if let Some(id) = existing {
            let job = self.job(&id)?;
            if job.status != "completed"
                || self
                    .assets()?
                    .iter()
                    .any(|a| Some(&a.id) == job.output_id.as_ref() && a.path.is_file())
            {
                return Ok(job);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,kind,request,created_at,updated_at) VALUES(?1,'Timeline preview','queued','export',?2,datetime('now'),datetime('now'))",rusqlite::params![id,json])?;
        self.job(&id)
    }
    pub fn export_episode_cut(&self, episode: &str, cut: &Cut) -> Result<crate::store::Job> {
        let request = self.cut_export_request(episode, cut)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,kind,request,created_at,updated_at) VALUES(?1,'Editing room export','queued','export',?2,datetime('now'),datetime('now'))",rusqlite::params![id,serde_json::to_string(&request)?])?;
        self.job(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (std::path::PathBuf, Store, String, String) {
        let root = std::env::temp_dir().join(format!("df-edit-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Test", "").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let mut plan = store.generation_plan(&episode.id).unwrap();
        plan.segments[0].prompt = "Scene".into();
        let plan = store.save_generation_plan(&plan).unwrap();
        let job = store
            .enqueue_episode_segment(&episode.id, plan.revision, &plan.segments[0])
            .unwrap();
        store
            .conn
            .execute("UPDATE jobs SET status='completed' WHERE id=?1", [&job.id])
            .unwrap();
        (root, store, episode.id, job.id)
    }
    #[test]
    fn playback_from_cursor_preserves_source_trims_and_order() {
        let cut = Cut {
            clips: vec![
                Clip {
                    id: "a".into(),
                    asset_id: "one".into(),
                    title: "One".into(),
                    start: 2.,
                    duration: 3.,
                },
                Clip {
                    id: "b".into(),
                    asset_id: "two".into(),
                    title: "Two".into(),
                    start: 7.,
                    duration: 4.,
                },
            ],
        };
        assert_eq!(cut.source_at(0.), Some(("one", 2.)));
        assert_eq!(cut.source_at(3.), Some(("two", 7.)));
        assert_eq!(cut.source_at(4.), Some(("two", 8.)));
        assert!(cut.source_at(7.).unwrap().1 < 11.);
        assert!(Cut::default().source_at(0.).is_none());
        let playback = cut.playback_from(4.);
        assert_eq!(playback.clips.len(), 1);
        assert_eq!(playback.clips[0].asset_id, "two");
        assert_eq!(playback.clips[0].start, 8.);
        assert_eq!(playback.clips[0].duration, 3.);
        assert_eq!(cut.playback_from(0.), cut);
        assert!(cut.playback_from(7.).clips.is_empty());
        assert_eq!(cut.clips[1].start, 7.);
    }
    #[test]
    fn cuts_save_reorder_split_and_protect_newer_edits() {
        let (root, store, episode, job) = fixture();
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('a',?1,'/fixture','hash',480,854,30,'{}','rejected')",
                [job],
            )
            .unwrap();
        let original = Cut::default();
        let first = Clip {
            id: "one".into(),
            asset_id: "a".into(),
            title: "Take 1".into(),
            start: 2.,
            duration: 5.,
        };
        let mut cut = Cut {
            clips: vec![first.clone()],
        };
        store.save_episode_cut(&episode, &original, &cut).unwrap();
        let saved = cut.clone();
        let mut second = first;
        second.id = "two".into();
        second.start = 15.;
        second.duration = 3.;
        cut.clips.insert(0, second);
        store.save_episode_cut(&episode, &saved, &cut).unwrap();
        assert_eq!(store.episode_cut(&episode).unwrap(), cut);
        assert!(
            store
                .save_episode_cut(&episode, &saved, &Cut::default())
                .is_err()
        );
        let mut bad = cut.clone();
        bad.clips[0].start = 29.;
        assert!(store.save_episode_cut(&episode, &cut, &bad).is_err());
        let mut bad = cut.clone();
        bad.clips[0].asset_id = "foreign".into();
        assert!(store.validate_cut(&episode, &bad).is_err());
        store.save_episode_cut(&episode, &cut, &saved).unwrap();
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn actual_export_trims_picture_and_linked_audio() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let (root, store, episode, job) = fixture();
        let path = root.join("source.mp4");
        assert!(
            std::process::Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:s=96x170:r=24:d=4",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=4",
                    "-af",
                    "volume=0:enable='lt(t,2)'",
                    "-c:v",
                    "libx264",
                    "-c:a",
                    "aac",
                    "-shortest"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let hash = crate::worker::hash_file(&path).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('a',?1,?2,?3,96,170,4,'{}','rejected')",
                rusqlite::params![job, path.to_string_lossy(), hash],
            )
            .unwrap();
        store.conn.execute("INSERT INTO settings(key,value) VALUES('worker',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::json!({"ffmpeg":ffmpeg.to_string_lossy()}).to_string()]).unwrap();
        let cut = Cut {
            clips: vec![
                Clip {
                    id: "one".into(),
                    asset_id: "a".into(),
                    title: "Sound".into(),
                    start: 2.,
                    duration: 1.,
                },
                Clip {
                    id: "two".into(),
                    asset_id: "a".into(),
                    title: "Silence".into(),
                    start: 0.,
                    duration: 0.5,
                },
            ],
        };
        let export = store.preview_episode_cut(&episode, &cut).unwrap();
        assert_eq!(
            store.preview_episode_cut(&episode, &cut).unwrap().id,
            export.id
        );
        crate::worker::run(store.path.clone(), true).unwrap();
        let result = store.job(&export.id).unwrap();
        assert_eq!(result.status, "completed", "{}", result.error);
        let asset = store
            .assets()
            .unwrap()
            .into_iter()
            .find(|a| Some(&a.id) == result.output_id.as_ref())
            .unwrap();
        assert!((asset.duration - 1.5).abs() < 0.1);
        assert_eq!((asset.width, asset.height), (288, 512));
        assert_eq!(
            store.preview_episode_cut(&episode, &cut).unwrap().id,
            export.id
        );
        assert!(store.latest_cut_export(&episode).unwrap().is_none());
        let pcm = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&asset.path)
            .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "f32le", "pipe:1"])
            .output()
            .unwrap();
        assert!(pcm.status.success());
        let samples: Vec<f32> = pcm
            .stdout
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let power =
            |range: std::ops::Range<usize>| samples[range].iter().map(|v| v * v).sum::<f32>();
        assert!(power(800..4000) > 1., "trimmed-in speech should be audible");
        assert!(
            power(10000..11600) < 0.01,
            "second cut should contain original silence"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
