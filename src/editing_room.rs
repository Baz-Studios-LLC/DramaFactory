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
    #[serde(default)]
    pub gap_before: f64,
    #[serde(default)]
    pub audio_fade_out: f64,
    #[serde(default)]
    pub audio_fade_in: f64,
    #[serde(default)]
    pub audio_gain_db: f64,
    #[serde(default)]
    pub audio_muted: bool,
    pub start: f64,
    pub duration: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub clips: Vec<Clip>,
}
impl Cut {
    pub fn duration(&self) -> f64 {
        self.clips.iter().map(|c| c.gap_before + c.duration).sum()
    }
    pub fn clip_time(&self, index: usize) -> f64 {
        self.clips[..index]
            .iter()
            .map(|c| c.gap_before + c.duration)
            .sum::<f64>()
            + self.clips[index].gap_before
    }
    /// Place a new clip without moving existing clips; reject overlaps atomically.
    pub fn place_clip(&mut self, mut clip: Clip, destination: f64) -> bool {
        if !destination.is_finite()
            || destination < 0.
            || !clip.duration.is_finite()
            || clip.duration <= 0.
        {
            return false;
        }
        let mut candidate = self.clone();
        clip.gap_before = 0.;
        candidate.clips.push(clip);
        if !candidate.slide(candidate.clips.len() - 1, destination) {
            return false;
        }
        *self = candidate;
        true
    }
    pub fn snapped_slide_target(&self, index: usize, target: f64, threshold: f64) -> f64 {
        let Some(moving) = self.clips.get(index) else {
            return target;
        };
        let mut result = target;
        let mut nearest = threshold;
        let boundaries = std::iter::once(0.).chain(
            self.clips
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .flat_map(|(i, clip)| {
                    let start = self.clip_time(i);
                    [start, start + clip.duration]
                }),
        );
        for boundary in boundaries {
            for position in [boundary, boundary - moving.duration] {
                let distance = (target - position).abs();
                if position >= 0. && distance <= nearest {
                    let mut candidate = self.clone();
                    if candidate.slide(index, position) {
                        nearest = distance;
                        result = position;
                    }
                }
            }
        }
        result
    }
    pub fn slide(&mut self, index: usize, destination: f64) -> bool {
        if index >= self.clips.len() || !destination.is_finite() {
            return false;
        }
        let destination = destination.max(0.);
        let end = destination + self.clips[index].duration;
        let mut positions = Vec::new();
        for (i, clip) in self.clips.iter().enumerate() {
            let time = self.clip_time(i);
            if i != index && destination < time + clip.duration - 0.000001 && end > time + 0.000001
            {
                return false;
            }
            positions.push((if i == index { destination } else { time }, clip.clone()));
        }
        positions.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut end = 0.;
        self.clips = positions
            .into_iter()
            .map(|(time, mut clip)| {
                clip.gap_before = (time - end).max(0.);
                end = time + clip.duration;
                clip
            })
            .collect();
        true
    }
    pub fn split_clip(&mut self, index: usize, offset: f64) -> bool {
        let Some(clip) = self.clips.get(index) else {
            return false;
        };
        if !offset.is_finite() || offset < 0.04 || clip.duration - offset < 0.04 {
            return false;
        }
        let mut right = clip.clone();
        right.id = uuid::Uuid::new_v4().to_string();
        right.gap_before = 0.;
        right.start += offset;
        right.duration -= offset;
        right.audio_fade_in = 0.;
        self.clips[index].duration = offset;
        self.clips[index].audio_fade_out = 0.;
        self.clips.insert(index + 1, right);
        true
    }
    pub fn duplicate_clip(&mut self, index: usize) -> bool {
        let Some(clip) = self.clips.get(index) else {
            return false;
        };
        let mut copy = clip.clone();
        copy.id = uuid::Uuid::new_v4().to_string();
        copy.gap_before = 0.;
        self.clips.insert(index + 1, copy);
        true
    }
    pub fn remove_clip(&mut self, index: usize, ripple: bool) -> bool {
        if index >= self.clips.len() {
            return false;
        }
        let removed = self.clips.remove(index);
        if let Some(next) = self.clips.get_mut(index) {
            next.gap_before += removed.gap_before + if ripple { 0. } else { removed.duration };
        }
        true
    }
    pub fn source_at(&self, seconds: f64) -> Option<(&str, f64)> {
        let time = seconds.max(0.).min((self.duration() - 1. / 24.).max(0.));
        for (index, clip) in self.clips.iter().enumerate() {
            let start = self.clip_time(index);
            if time >= start && time < start + clip.duration {
                return Some((&clip.asset_id, clip.start + time - start));
            }
        }
        None
    }
    pub fn playback_from(&self, seconds: f64) -> Self {
        let seconds = seconds.max(0.);
        let mut clips = Vec::new();
        let mut end = seconds;
        for (index, source) in self.clips.iter().enumerate() {
            let time = self.clip_time(index);
            if time + source.duration <= seconds {
                continue;
            }
            let mut clip = source.clone();
            let trim = (seconds - time).max(0.);
            clip.audio_fade_in = (clip.audio_fade_in - trim).max(0.);
            clip.start += trim;
            clip.duration -= trim;
            clip.gap_before = (time - end).max(0.);
            end = time + source.duration;
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
            if !clip.audio_fade_in.is_finite()
                || clip.audio_fade_in < 0.
                || !clip.audio_gain_db.is_finite()
                || !(-60. ..=12.).contains(&clip.audio_gain_db)
            {
                bail!("Invalid clip sound settings");
            }
            if !clip.audio_fade_out.is_finite() || clip.audio_fade_out < 0. {
                bail!("Audio fade must be a nonnegative duration");
            }
            if !clip.gap_before.is_finite() || clip.gap_before < 0. || clip.gap_before > 3600. {
                bail!("Timeline gap must be between zero and one hour");
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
                gap_before: item.gap_before,
                audio_fade_out: item.audio_fade_out.min(item.duration),
                audio_fade_in: item.audio_fade_in.min(item.duration),
                audio_gain_db: item.audio_gain_db,
                audio_muted: item.audio_muted,
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
            cues: self
                .audio_cues(episode)?
                .into_iter()
                .map(|cue| {
                    let asset = assets
                        .iter()
                        .find(|a| a.id == cue.asset_id)
                        .context("Music file missing")?;
                    Ok(crate::sound::ExportCue {
                        cue,
                        path: asset.path.clone(),
                        hash: asset.hash.clone(),
                        skip: 0.,
                    })
                })
                .collect::<Result<_>>()?,
            captions: vec![],
            burn_captions: false,
            width: self.video_dimensions(480, 854).0,
            height: self.video_dimensions(480, 854).1,
            fps: 24,
        })
    }
    #[cfg(test)]
    pub fn preview_episode_cut(&self, episode: &str, cut: &Cut) -> Result<crate::store::Job> {
        self.preview_cut_at(episode, cut, 0.)
    }
    pub fn preview_cut_at(
        &self,
        episode: &str,
        cut: &Cut,
        origin: f64,
    ) -> Result<crate::store::Job> {
        use rusqlite::OptionalExtension;
        let mut request = self.cut_export_request(episode, cut)?;
        request.cues.retain_mut(|entry| {
            entry.skip = (origin - entry.cue.start).max(0.);
            entry.cue.start = (entry.cue.start - origin).max(0.);
            entry.skip < entry.cue.duration
        });
        (request.width, request.height) = self.video_dimensions(288, 512);
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
    fn menu_edits_preserve_sources_and_expected_timing() {
        let clip = Clip {
            id: "one".into(),
            asset_id: "source".into(),
            title: "Take".into(),
            gap_before: 2.,
            start: 3.,
            duration: 4.,
            audio_fade_in: 0.5,
            audio_fade_out: 1.,
            audio_gain_db: -6.,
            audio_muted: false,
        };
        let mut cut = Cut {
            clips: vec![clip.clone()],
        };
        assert!(!cut.split_clip(0, 4.));
        assert!(cut.split_clip(0, 1.5));
        assert_eq!(cut.duration(), 6.);
        assert_eq!(cut.clips[1].start, 4.5);
        assert_eq!(cut.clips[0].audio_fade_out, 0.);
        assert_eq!(cut.clips[1].audio_fade_in, 0.);
        assert_eq!(cut.clips[1].audio_fade_out, 1.);
        assert!(cut.duplicate_clip(0));
        assert_ne!(cut.clips[0].id, cut.clips[1].id);
        assert_eq!(cut.clips[0].asset_id, cut.clips[1].asset_id);
        let next_time = cut.clip_time(2);
        let mut ripple = cut.clone();
        cut.remove_clip(1, false);
        assert_eq!(cut.clip_time(1), next_time);
        ripple.remove_clip(1, true);
        assert_eq!(ripple.clip_time(1), next_time - 1.5);
        assert_eq!(ripple.clips[1].audio_gain_db, -6.);
        let restored: Cut = serde_json::from_str(&serde_json::to_string(&ripple).unwrap()).unwrap();
        assert_eq!(restored, ripple);
    }
    #[test]
    fn moving_clip_snaps_both_edges_without_overlapping() {
        let clip = |id: &str| Clip {
            id: id.into(),
            asset_id: id.into(),
            title: id.into(),
            start: 0.,
            duration: 3.,
            gap_before: 0.,
            audio_fade_out: 0.,
            audio_fade_in: 0.,
            audio_gain_db: 0.,
            audio_muted: false,
        };
        let mut cut = Cut::default();
        assert!(cut.place_clip(clip("left"), 0.));
        assert!(cut.place_clip(clip("moving"), 5.));
        assert!(cut.place_clip(clip("right"), 10.));
        assert_eq!(cut.snapped_slide_target(1, 3.2, 0.4), 3.);
        assert_eq!(cut.snapped_slide_target(1, 6.8, 0.4), 7.);
        assert_eq!(cut.snapped_slide_target(1, 5.2, 0.4), 5.2);
        assert_eq!(cut.snapped_slide_target(1, 5.2, 0.1), 5.2);
        assert_eq!(cut.snapped_slide_target(1, 2.9, 0.4), 3.);
    }
    #[test]
    fn library_drops_fill_gaps_without_moving_neighbors() {
        let clip = |id: &str| Clip {
            id: id.into(),
            asset_id: id.into(),
            title: id.into(),
            start: 0.,
            duration: 3.,
            gap_before: 0.,
            audio_fade_out: 0.,
            audio_fade_in: 0.,
            audio_gain_db: 0.,
            audio_muted: false,
        };
        let mut cut = Cut::default();
        assert!(cut.place_clip(clip("a"), 0.));
        assert!(cut.place_clip(clip("b"), 10.));
        assert!(cut.place_clip(clip("c"), 5.));
        assert_eq!(cut.clip_time(1), 5.);
        assert_eq!(cut.clip_time(2), 10.);
        let before = cut.clone();
        assert!(!cut.place_clip(clip("overlap"), 7.));
        assert_eq!(cut, before);
        assert!(cut.place_clip(clip("end"), 13.));
        assert_eq!(cut.clip_time(3), 13.);
        assert_eq!(cut.source_at(4.), None);
        let restored: Cut = serde_json::from_str(&serde_json::to_string(&cut).unwrap()).unwrap();
        assert_eq!(restored, cut);
    }
    #[test]
    fn slides_preserve_other_positions_and_gaps_survive_playback() {
        let clip = |id: &str| Clip {
            id: id.into(),
            asset_id: id.into(),
            title: id.into(),
            start: 2.,
            duration: 3.,
            gap_before: 0.,
            audio_fade_out: 0.,
            audio_fade_in: 0.,
            audio_gain_db: 0.,
            audio_muted: false,
        };
        let mut cut = Cut {
            clips: vec![clip("a"), clip("b")],
        };
        assert!(cut.slide(1, 8.));
        assert_eq!(cut.clip_time(0), 0.);
        assert_eq!(cut.clip_time(1), 8.);
        assert_eq!(cut.duration(), 11.);
        let saved = cut.clone();
        assert!(!cut.slide(0, 7.));
        assert_eq!(cut, saved);
        assert!(cut.slide(0, 4.));
        assert_eq!(cut.clip_time(1), 8.);
        assert_eq!(cut.source_at(3.), None);
        assert_eq!(cut.source_at(4.), Some(("a", 2.)));
        assert_eq!(cut.source_at(7.5), None);
        let tail = cut.playback_from(7.5);
        assert_eq!(tail.clips.len(), 1);
        assert_eq!(tail.clips[0].gap_before, 0.5);
        assert_eq!(tail.clips[0].start, 2.);
        assert_eq!(tail.duration(), 3.5);
        let restored: Cut = serde_json::from_str(&serde_json::to_string(&cut).unwrap()).unwrap();
        assert_eq!(restored, cut);
        let mut legacy = serde_json::to_value(&cut).unwrap();
        for clip in legacy["clips"].as_array_mut().unwrap() {
            clip.as_object_mut().unwrap().remove("gap_before");
        }
        let legacy: Cut = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.duration(), 6.);
    }
    #[test]
    fn playback_from_cursor_preserves_source_trims_and_order() {
        let cut = Cut {
            clips: vec![
                Clip {
                    id: "a".into(),
                    asset_id: "one".into(),
                    title: "One".into(),
                    gap_before: 0.,
                    audio_fade_out: 0.,
                    audio_fade_in: 0.,
                    audio_gain_db: 0.,
                    audio_muted: false,
                    start: 2.,
                    duration: 3.,
                },
                Clip {
                    id: "b".into(),
                    asset_id: "two".into(),
                    title: "Two".into(),
                    gap_before: 0.,
                    audio_fade_out: 0.,
                    audio_fade_in: 0.,
                    audio_gain_db: 0.,
                    audio_muted: false,
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
            gap_before: 0.,
            audio_fade_out: 0.,
            audio_fade_in: 0.,
            audio_gain_db: 0.,
            audio_muted: false,
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
        for ratio in ["9:16", "16:9"] {
            check_actual_export(ratio);
        }
    }
    fn check_actual_export(ratio: &str) {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let (root, store, episode, job) = fixture();
        store.set_video_aspect_ratio(ratio).unwrap();
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
                    gap_before: 0.,
                    audio_fade_out: 0.5,
                    audio_fade_in: 0.2,
                    audio_gain_db: -6.,
                    audio_muted: false,
                    start: 2.,
                    duration: 1.,
                },
                Clip {
                    id: "two".into(),
                    asset_id: "a".into(),
                    title: "Silence".into(),
                    gap_before: 0.5,
                    audio_fade_out: 0.,
                    audio_fade_in: 0.,
                    audio_gain_db: 0.,
                    audio_muted: false,
                    start: 0.,
                    duration: 0.5,
                },
            ],
        };
        let mut cut = cut;
        let mut muted = cut.clips[0].clone();
        muted.id = "muted".into();
        muted.duration = 0.5;
        muted.audio_muted = true;
        cut.clips.push(muted);
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
        assert!((asset.duration - 2.5).abs() < 0.1);
        assert_eq!(
            (asset.width, asset.height),
            store.video_dimensions(288, 512)
        );
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
            power(160..640) < power(2400..2880) * 0.2,
            "opening audio should fade in"
        );
        assert!(
            power(17600..19200) < 0.01,
            "muted source tone must be silent"
        );
        let mean_power = power(2400..3200) / 800.;
        assert!(
            mean_power > 0.001 && mean_power < 0.003,
            "-6 dB gain must reduce the test tone"
        );
        assert!(
            power(7200..7840) < power(4400..5040) * 0.15,
            "final audio must fade smoothly toward silence"
        );
        assert!(
            power(10000..11600) < 0.01,
            "second cut should contain original silence"
        );
        let frame = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-ss", "1.25", "-i"])
            .arg(&asset.path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=1:1",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(frame.status.success());
        assert_eq!(frame.stdout.len(), 3);
        assert!(frame.stdout.iter().all(|v| *v < 8), "gap must render black");
        // A cue starting inside the gap plays at the correct source offset after a seek.
        let wav = root.join("music.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for n in 0..16000 {
            writer
                .write_sample(((n as f64 * 0.47).sin() * 8000.) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
        let music = store.import_audio(&wav).unwrap();
        store
            .save_audio_cue(
                &crate::sound::AudioCue {
                    id: uuid::Uuid::new_v4().to_string(),
                    episode_id: episode.clone(),
                    asset_id: music.id,
                    name: "Score".into(),
                    kind: "music".into(),
                    start: 1.,
                    source_start: 0.,
                    duration: 1.5,
                    gain_db: -6.,
                    fade_in: 0.25,
                    fade_out: 0.25,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            store.cut_export_request(&episode, &cut).unwrap().cues.len(),
            1
        );
        let seek = store
            .preview_cut_at(&episode, &cut.playback_from(1.5), 1.5)
            .unwrap();
        crate::worker::run(store.path.clone(), true).unwrap();
        let rendered = store.job(&seek.id).unwrap();
        assert_eq!(rendered.status, "completed", "{}", rendered.error);
        let mixed = store
            .assets()
            .unwrap()
            .into_iter()
            .find(|a| Some(&a.id) == rendered.output_id.as_ref())
            .unwrap();
        let pcm = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(mixed.path)
            .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "f32le", "pipe:1"])
            .output()
            .unwrap();
        assert!(pcm.status.success());
        let samples: Vec<_> = pcm
            .stdout
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let power = |from: usize, to: usize| samples[from..to].iter().map(|v| v * v).sum::<f32>();
        assert!(
            power(3000, 3800) > 0.05,
            "score missing over silent picture"
        );
        assert!(
            power(7600, 7950) < power(4000, 4350) * 0.1,
            "score fade shifted after seek"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
