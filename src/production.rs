use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionBefore {
    pub asset_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderRequest {
    pub prompt: String,
    pub negative_prompt: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub steps: u32,
    pub seed: u32,
    pub image: Option<PathBuf>,
    pub image_hash: Option<String>,
    pub shot_id: Option<String>,
    pub direction_context: serde_json::Value,
    pub selected_take_before: Option<SelectionBefore>,
}
impl Default for RenderRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            negative_prompt: String::new(),
            width: 704,
            height: 1280,
            frames: 121,
            steps: 40,
            seed: 42,
            image: None,
            image_hash: None,
            shot_id: None,
            direction_context: serde_json::Value::Null,
            selected_take_before: None,
        }
    }
}
impl RenderRequest {
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            bail!("A visual prompt is required");
        }
        if !(256..=1280).contains(&self.width)
            || !(256..=1280).contains(&self.height)
            || !self.width.is_multiple_of(32)
            || !self.height.is_multiple_of(32)
        {
            bail!("Dimensions must be multiples of 32 between 256 and 1280");
        }
        if self.width as u64 * self.height as u64 > 901120 {
            bail!("The current worker profile supports up to 704 × 1280 worth of pixels per frame");
        }
        if !(5..=121).contains(&self.frames) || !(self.frames - 1).is_multiple_of(4) {
            bail!("Frame count must be 4n+1, between 5 and 121 for this 64 GB worker profile");
        }
        if !(1..=60).contains(&self.steps) {
            bail!("Steps must be between 1 and 60");
        }
        if let Some(p) = &self.image
            && !p.is_file()
        {
            bail!("Reference image not found");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerConfig {
    pub python: PathBuf,
    pub model: PathBuf,
    pub ffmpeg: PathBuf,
    pub speech_model: PathBuf,
    pub image_python: PathBuf,
    pub image_model: PathBuf,
    pub lipsync_python: PathBuf,
    pub lipsync_model: PathBuf,
    pub performance_python: PathBuf,
    pub performance_model: PathBuf,
    pub performance_encoder: PathBuf,
}
impl WorkerConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.python.is_file() {
            bail!("Select the inference Python executable in Preferences");
        }
        if !self.ffmpeg.is_file() {
            bail!("Select FFmpeg in Preferences");
        }
        for f in [
            "config.json",
            "model.safetensors",
            "t5_encoder.safetensors",
            "vae.safetensors",
        ] {
            if !self.model.join(f).is_file() {
                bail!("Model is incomplete: missing {f}");
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Shot {
    pub id: String,
    pub episode_id: String,
    pub position: u32,
    pub title: String,
    pub prompt: String,
    pub camera: String,
    pub duration: f64,
    #[serde(default)]
    pub source_start: f64,
    pub status: String,
    pub asset_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub job_id: String,
    pub path: PathBuf,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub metadata: serde_json::Value,
    pub review: String,
}
pub(crate) fn asset_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    let metadata: String = row.get(7)?;
    Ok(Asset {
        id: row.get(0)?,
        job_id: row.get(1)?,
        path: row.get::<_, String>(2)?.into(),
        hash: row.get(3)?,
        width: row.get(4)?,
        height: row.get(5)?,
        duration: row.get(6)?,
        metadata: serde_json::from_str(&metadata).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?,
        review: row.get(8)?,
    })
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub id: String,
    pub series_id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub reference: Option<PathBuf>,
    pub voice: String,
}
impl Store {
    pub(crate) fn migrate_production(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS deleted_characters(library_id TEXT PRIMARY KEY REFERENCES library(id),deleted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS character_profiles(library_id TEXT PRIMARY KEY REFERENCES library(id),document TEXT NOT NULL);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS segment_takes(episode_id TEXT NOT NULL REFERENCES episodes(id),segment_id TEXT NOT NULL,asset_id TEXT NOT NULL REFERENCES assets(id),PRIMARY KEY(episode_id,segment_id));")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS job_display_context(job_id TEXT PRIMARY KEY,heading TEXT NOT NULL,episode_title TEXT NOT NULL);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS segment_generations(job_id TEXT PRIMARY KEY REFERENCES jobs(id) DEFERRABLE INITIALLY DEFERRED,episode_id TEXT NOT NULL REFERENCES episodes(id),segment_id TEXT NOT NULL,plan_revision INTEGER NOT NULL,snapshot TEXT NOT NULL);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS job_timing(job_id TEXT PRIMARY KEY REFERENCES jobs(id),started_unix INTEGER NOT NULL,finished_unix INTEGER);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS remote_video_jobs(job_id TEXT PRIMARY KEY REFERENCES jobs(id),state TEXT NOT NULL,remote_id TEXT);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS portrait_prompts(library_id TEXT PRIMARY KEY REFERENCES library(id),prompt TEXT NOT NULL);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS sidekick_messages(id TEXT PRIMARY KEY,series_id TEXT NOT NULL REFERENCES series(id),role TEXT NOT NULL,text TEXT NOT NULL,context TEXT NOT NULL);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS sidekick_images(message_id TEXT PRIMARY KEY REFERENCES sidekick_messages(id) ON DELETE CASCADE, data TEXT NOT NULL);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS generation_plans(episode_id TEXT PRIMARY KEY REFERENCES episodes(id),revision INTEGER NOT NULL,data TEXT NOT NULL);")?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let version: u32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 2 {
            bail!("This database was created by a newer DramaFactory version");
        }
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS shots(id TEXT PRIMARY KEY,episode_id TEXT NOT NULL REFERENCES episodes(id),position INTEGER NOT NULL,title TEXT NOT NULL,prompt TEXT NOT NULL,camera TEXT NOT NULL,duration REAL NOT NULL,status TEXT NOT NULL DEFAULT 'planned',asset_id TEXT); CREATE TABLE IF NOT EXISTS assets(id TEXT PRIMARY KEY,job_id TEXT NOT NULL,path TEXT NOT NULL,hash TEXT NOT NULL,width INTEGER NOT NULL,height INTEGER NOT NULL,duration REAL NOT NULL,metadata TEXT NOT NULL,review TEXT NOT NULL DEFAULT 'unreviewed'); CREATE TABLE IF NOT EXISTS library(id TEXT PRIMARY KEY,series_id TEXT NOT NULL REFERENCES series(id),kind TEXT NOT NULL,name TEXT NOT NULL,description TEXT NOT NULL,reference TEXT);")?;
        let shot_columns: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(shots)")?
            .query_map([], |row| row.get(1))?
            .collect::<std::result::Result<_, _>>()?;
        if !shot_columns.iter().any(|column| column == "source_start") {
            self.conn.execute_batch(
                "ALTER TABLE shots ADD COLUMN source_start REAL NOT NULL DEFAULT 0;",
            )?;
        }
        let columns: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(jobs)")?
            .query_map([], |r| r.get(1))?
            .collect::<std::result::Result<_, _>>()?;
        for (name, def) in [
            ("kind", "TEXT NOT NULL DEFAULT 'video'"),
            ("phase", "TEXT NOT NULL DEFAULT ''"),
            ("request", "TEXT NOT NULL DEFAULT '{}'"),
            ("progress", "REAL NOT NULL DEFAULT 0"),
            ("attempts", "INTEGER NOT NULL DEFAULT 0"),
            ("cancel_requested", "INTEGER NOT NULL DEFAULT 0"),
            ("output_id", "TEXT"),
            ("created_at", "TEXT NOT NULL DEFAULT ''"),
            ("updated_at", "TEXT NOT NULL DEFAULT ''"),
        ] {
            if !columns.iter().any(|c| c == name) {
                self.conn
                    .execute_batch(&format!("ALTER TABLE jobs ADD COLUMN {name} {def};"))?;
            }
        }
        let has_voice: bool = self
            .conn
            .prepare("PRAGMA table_info(library)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .iter()
            .any(|c| c == "voice");
        if !has_voice {
            self.conn.execute_batch(
                "ALTER TABLE library ADD COLUMN voice TEXT NOT NULL DEFAULT 'af_heart';",
            )?;
        }
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS dialogue(id TEXT PRIMARY KEY,shot_id TEXT NOT NULL REFERENCES shots(id),text TEXT NOT NULL,voice TEXT NOT NULL,speed REAL NOT NULL,asset_id TEXT);")?;
        let dialogue_columns: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(dialogue)")?
            .query_map([], |r| r.get(1))?
            .collect::<std::result::Result<_, _>>()?;
        if !dialogue_columns.iter().any(|c| c == "timeline_start") {
            self.conn
                .execute_batch("ALTER TABLE dialogue ADD COLUMN timeline_start REAL;")?;
        }
        if !dialogue_columns.iter().any(|c| c == "revision") {
            self.conn.execute_batch(
                "ALTER TABLE dialogue ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !dialogue_columns.iter().any(|c| c == "voice_revision") {
            self.conn.execute_batch(
                "ALTER TABLE dialogue ADD COLUMN voice_revision INTEGER NOT NULL DEFAULT 0; UPDATE dialogue SET voice_revision=revision;",
            )?;
        }
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS shot_directions(shot_id TEXT PRIMARY KEY REFERENCES shots(id),data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS camera_setups(id TEXT PRIMARY KEY,location_id TEXT NOT NULL REFERENCES library(id),data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS story_plans(series_id TEXT PRIMARY KEY REFERENCES series(id),revision INTEGER NOT NULL,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS story_history(series_id TEXT NOT NULL,revision INTEGER NOT NULL,data TEXT NOT NULL,PRIMARY KEY(series_id,revision)); CREATE TABLE IF NOT EXISTS canon_commits(id TEXT PRIMARY KEY,series_id TEXT NOT NULL REFERENCES series(id),episode_id TEXT NOT NULL REFERENCES episodes(id),revision INTEGER NOT NULL,fingerprint TEXT NOT NULL,export_id TEXT NOT NULL,parent TEXT,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP); CREATE TABLE IF NOT EXISTS audio_cues(id TEXT PRIMARY KEY,episode_id TEXT NOT NULL REFERENCES episodes(id),start REAL NOT NULL,data TEXT NOT NULL); PRAGMA user_version=2;")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS asset_review_notes(asset_id TEXT PRIMARY KEY REFERENCES assets(id),revision INTEGER NOT NULL,note TEXT NOT NULL); CREATE TABLE IF NOT EXISTS asset_review_history(asset_id TEXT NOT NULL REFERENCES assets(id),revision INTEGER NOT NULL,decision TEXT NOT NULL,note TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(asset_id,revision));")?;
        tx.commit()?;
        Ok(())
    }
    pub fn root(&self) -> PathBuf {
        self.path.parent().unwrap_or(Path::new(".")).to_path_buf()
    }
    pub fn config(&self) -> Result<WorkerConfig> {
        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key='worker'", [], |r| {
                r.get::<_, String>(0)
            });
        let mut config: WorkerConfig = match value {
            Ok(v) => serde_json::from_str(&v)?,
            Err(rusqlite::Error::QueryReturnedNoRows) => WorkerConfig::default(),
            Err(e) => return Err(e.into()),
        };
        if !config.ffmpeg.is_file() {
            for candidate in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
                if std::path::Path::new(candidate).is_file() {
                    config.ffmpeg = candidate.into();
                    break;
                }
            }
        }
        Ok(config)
    }
    pub fn save_config(&self, c: &WorkerConfig) -> Result<()> {
        self.conn.execute("INSERT INTO settings VALUES('worker',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(c)?])?;
        Ok(())
    }
    pub fn enqueue(&self, r: &RenderRequest) -> Result<crate::store::Job> {
        r.validate()?;
        let mut r = r.clone();
        if let Some(id) = &r.shot_id {
            let context = self.shot_context(id)?;
            if !r.direction_context.is_null() && context != r.direction_context {
                bail!("Shot direction or references changed; prepare the render again");
            }
            r.direction_context = context;
            r.selected_take_before = Some(SelectionBefore {
                asset_id: self.shot(id)?.asset_id,
            });
        }
        r.image_hash = r
            .image
            .as_ref()
            .map(|path| crate::worker::hash_file(path))
            .transpose()?;
        let config = self.config()?;
        let (status, error) = match config.validate() {
            Ok(_) => ("queued", String::new()),
            Err(e) => ("blocked", e.to_string()),
        };
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,datetime('now'),datetime('now'))",params![id,r.prompt,status,error,serde_json::to_string(&r)?])?;
        self.job(&id)
    }
    pub fn job(&self, id: &str) -> Result<crate::store::Job> {
        self.jobs()?
            .into_iter()
            .find(|j| j.id == id)
            .context("Job not found")
    }
    pub fn retry(&self, id: &str) -> Result<()> {
        let (hosted, attempts): (bool, u32) = self.conn.query_row(
            "SELECT kind='openrouter_image',attempts FROM jobs WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if hosted && attempts > 0 {
            bail!(
                "Hosted image attempts cannot be replayed: billing may already have occurred. Inspect provider activity before explicitly creating a new generation."
            );
        }
        let c = self.config()?;
        let kind: String = self
            .conn
            .query_row("SELECT kind FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
        let mut text: String =
            self.conn
                .query_row("SELECT request FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
        if kind == "video" {
            c.validate()?;
            let mut request: RenderRequest = serde_json::from_str(&text)?;
            if request.prompt.is_empty() {
                request.prompt = self.job(id)?.prompt;
            }
            request.validate()?;
            text = serde_json::to_string(&request)?;
        } else if kind == "performance" {
            let request: crate::performance::PerformanceRequest = serde_json::from_str(&text)?;
            request.validate()?;
        } else if kind == "lipsync" {
            if !c.lipsync_python.is_file() {
                bail!("Lip-sync Python is not configured");
            }
            let _: LipSyncRequest = serde_json::from_str(&text)?;
        } else if kind == "image" {
            if !c.image_python.is_file() {
                bail!("Image Python is not configured");
            }
            let r: ImageRequest = serde_json::from_str(&text)?;
            r.validate()?;
        } else if kind == "dialogue" {
            if !c.python.is_file() {
                bail!("Python worker is not configured");
            }
            let _: VoiceRequest = serde_json::from_str(&text)?;
        } else if kind == "openrouter_video" {
            if !self.video_retry_available(id)? {
                bail!(
                    "This remote job cannot be safely resumed. Inspect provider activity; no new submission was queued."
                );
            }
            let _: crate::video_plan::VideoPlan = serde_json::from_str(&text)?;
        } else if kind == "rehearsal" {
            if !c.ffmpeg.is_file() {
                bail!("FFmpeg is not configured");
            }
            let _: crate::rehearsal::RehearsalRequest = serde_json::from_str(&text)?;
        } else if kind == "export" {
            if !c.ffmpeg.is_file() {
                bail!("FFmpeg is not configured");
            }
            let _: ExportRequest = serde_json::from_str(&text)?;
        } else {
            bail!("Unknown job type");
        }
        if self.conn.execute("UPDATE jobs SET status='queued',error='',progress=0,cancel_requested=0,output_id=NULL,phase='Queued for retry',request=?2,updated_at=datetime('now') WHERE id=?1 AND status IN ('blocked','failed','canceled')",params![id,text])?==0{bail!("Only blocked, failed or canceled jobs may be retried");}
        Ok(())
    }
    pub fn add_shot(
        &self,
        episode: &str,
        title: &str,
        prompt: &str,
        camera: &str,
        duration: f64,
    ) -> Result<Shot> {
        if title.trim().is_empty() || prompt.trim().is_empty() {
            bail!("Shot title and visual prompt are required");
        }
        if !duration.is_finite() || duration <= 0. || duration > 30. {
            bail!("Shot duration must be between 0 and 30 seconds");
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO shots(id,episode_id,position,title,prompt,camera,duration) VALUES(?1,?2,(SELECT COALESCE(MAX(position)+1,0) FROM shots WHERE episode_id=?2),?3,?4,?5,?6)",params![id,episode,title,prompt,camera,duration])?;
        self.shot(&id)
    }
    pub fn shots(&self, episode: &str) -> Result<Vec<Shot>> {
        Ok(self.conn.prepare("SELECT id,episode_id,position,title,prompt,camera,duration,status,asset_id,source_start FROM shots WHERE episode_id=?1 ORDER BY position,rowid")?.query_map([episode],|r|Ok(Shot{id:r.get(0)?,episode_id:r.get(1)?,position:r.get(2)?,title:r.get(3)?,prompt:r.get(4)?,camera:r.get(5)?,duration:r.get(6)?,status:r.get(7)?,asset_id:r.get(8)?,source_start:r.get(9)?}))?.collect::<std::result::Result<_,_>>()?)
    }
    pub fn shot(&self, id: &str) -> Result<Shot> {
        let episode: String = self
            .conn
            .query_row("SELECT episode_id FROM shots WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .context("Shot not found")?;
        self.shots(&episode)?
            .into_iter()
            .find(|s| s.id == id)
            .context("Shot not found")
    }
    pub fn assets(&self) -> Result<Vec<Asset>> {
        Ok(self.conn.prepare("SELECT id,job_id,path,hash,width,height,duration,metadata,review FROM assets ORDER BY rowid DESC")?.query_map([],asset_row)?.collect::<std::result::Result<Vec<_>,_>>()?)
    }
    pub fn review(&self, id: &str, review: &str) -> Result<()> {
        if self.deleted_portraits()?.contains(id) {
            bail!("Restore this deleted portrait before reviewing or selecting it");
        }
        self.review_status(id, review)
    }
    pub(crate) fn apply_review_status(&self, id: &str, review: &str) -> Result<()> {
        if !["approved", "rejected", "unreviewed"].contains(&review) {
            bail!("Invalid review status");
        }
        let tx = &self.conn;
        let (metadata, path): (String, String) = tx
            .query_row("SELECT metadata,path FROM assets WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .context("Asset not found")?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        if metadata["kind"] == "image"
            && let Some(entry) = metadata["request"]["library_id"].as_str()
        {
            let original = if metadata["request"]
                .get("library_reference_before")
                .is_some()
            {
                metadata["request"]["library_reference_before"].as_str()
            } else {
                metadata["request"]["reference"].as_str()
            };
            let current: Option<String> =
                tx.query_row("SELECT reference FROM library WHERE id=?1", [entry], |r| {
                    r.get(0)
                })?;
            if review == "approved" {
                let snapshot = &metadata["request"]["library_context_before"];
                if !snapshot.is_null() && current.as_deref() != Some(&path) {
                    let (name, description): (String, String) = tx.query_row(
                        "SELECT name,description FROM library WHERE id=?1",
                        [entry],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    let reference_hash = current
                        .as_ref()
                        .map(|p| crate::worker::hash_file(Path::new(p)))
                        .transpose()?;
                    if *snapshot
                        != serde_json::json!({"name":name,"description":description,"reference_hash":reference_hash})
                    {
                        bail!(
                            "The library brief or reference changed during generation. This image is preserved; regenerate from the current brief before replacing its reference."
                        );
                    }
                }
                if current.as_deref() != original && current.as_deref() != Some(&path) {
                    bail!(
                        "This library reference changed while the image was generating. Select this image explicitly rather than overwriting the newer reference."
                    );
                }
                tx.execute(
                    "UPDATE library SET reference=?2 WHERE id=?1",
                    params![entry, path],
                )?;
            } else if current.as_deref() == Some(&path) {
                tx.execute(
                    "UPDATE library SET reference=?2 WHERE id=?1",
                    params![entry, original],
                )?;
            }
        }
        tx.execute(
            "UPDATE assets SET review=?2 WHERE id=?1",
            params![id, review],
        )?;
        Ok(())
    }

    pub fn library(&self, series: &str, kind: &str) -> Result<Vec<LibraryEntry>> {
        self.library_by_deletion_state(series, kind, false)
    }
    pub fn deleted_characters(&self, series: &str) -> Result<Vec<LibraryEntry>> {
        self.library_by_deletion_state(series, "character", true)
    }
    fn library_by_deletion_state(
        &self,
        series: &str,
        kind: &str,
        deleted: bool,
    ) -> Result<Vec<LibraryEntry>> {
        Ok(self.conn.prepare("SELECT id,series_id,kind,name,description,reference,voice FROM library WHERE series_id=?1 AND kind=?2 AND EXISTS(SELECT 1 FROM deleted_characters d WHERE d.library_id=library.id)=?3 ORDER BY rowid")?.query_map(params![series,kind,deleted],|r|Ok(LibraryEntry{id:r.get(0)?,series_id:r.get(1)?,kind:r.get(2)?,name:r.get(3)?,description:r.get(4)?,reference:r.get::<_,Option<String>>(5)?.map(PathBuf::from),voice:r.get(6)?}))?.collect::<std::result::Result<_,_>>()?)
    }
    pub fn add_library(
        &self,
        series: &str,
        kind: &str,
        name: &str,
        description: &str,
        reference: Option<&Path>,
    ) -> Result<LibraryEntry> {
        if !["character", "location"].contains(&kind) || name.trim().is_empty() {
            bail!("Choose a name and valid library type");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let reference = if let Some(p) = reference {
            if !p.is_file() {
                bail!("Reference file is missing");
            }
            let dir = self.root().join("references");
            std::fs::create_dir_all(&dir)?;
            let dest = dir.join(format!(
                "{id}.{}",
                p.extension().and_then(|s| s.to_str()).unwrap_or("png")
            ));
            std::fs::copy(p, &dest)?;
            Some(dest)
        } else {
            None
        };
        self.conn.execute(
            "INSERT INTO library(id,series_id,kind,name,description,reference) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                series,
                kind,
                name,
                description,
                reference.as_ref().map(|p| p.to_string_lossy())
            ],
        )?;
        Ok(LibraryEntry {
            id,
            series_id: series.into(),
            kind: kind.into(),
            name: name.into(),
            description: description.into(),
            reference,
            voice: "af_heart".into(),
        })
    }
}

impl Store {
    pub fn automatic_updates(&self) -> Result<bool> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key='automatic_updates'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|v| v == "true")
            .unwrap_or(true))
    }
    pub fn set_automatic_updates(&self, enabled: bool) -> Result<()> {
        self.conn.execute("INSERT INTO settings VALUES('automatic_updates',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[if enabled{"true"}else{"false"}])?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ExportClip {
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offscreen_dialogue_ids: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::transitions::PictureFades::is_empty"
    )]
    pub picture_fades: crate::transitions::PictureFades,
    #[serde(default, skip_serializing_if = "zero_source_start")]
    pub source_start: f64,
    #[serde(default)]
    pub direction_context: serde_json::Value,
    pub asset_id: String,
    pub path: PathBuf,
    pub duration: f64,
    pub hash: String,
    #[serde(default)]
    pub audio: Vec<ExportAudio>,
}
fn zero_source_start(value: &f64) -> bool {
    *value == 0.
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExportAudio {
    #[serde(default, skip_serializing_if = "zero_source_start")]
    pub source_start: f64,
    pub path: PathBuf,
    pub hash: String,
    pub duration: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    pub episode_id: String,
    #[serde(default)]
    pub screenplay: String,
    #[serde(default)]
    pub continuity_context: serde_json::Value,
    pub clips: Vec<ExportClip>,
    #[serde(default)]
    pub cues: Vec<crate::sound::ExportCue>,
    #[serde(default)]
    pub captions: Vec<crate::captions::Caption>,
    #[serde(default)]
    pub burn_captions: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl Store {
    pub fn update_shot(
        &self,
        id: &str,
        title: &str,
        prompt: &str,
        camera: &str,
        duration: f64,
    ) -> Result<()> {
        if title.trim().is_empty()
            || prompt.trim().is_empty()
            || !duration.is_finite()
            || duration <= 0.
            || duration > 30.
        {
            bail!("Shot needs a title, prompt and duration between 0 and 30 seconds");
        }
        if self.conn.execute(
            "UPDATE shots SET title=?2,prompt=?3,camera=?4,duration=?5 WHERE id=?1",
            params![id, title, prompt, camera, duration],
        )? == 0
        {
            bail!("Shot not found");
        }
        Ok(())
    }
    pub fn move_shot(&self, id: &str, offset: i32) -> Result<()> {
        if ![-1, 1].contains(&offset) {
            bail!("Move must be -1 or 1");
        }
        let shot = self.shot(id)?;
        let shots = self.shots(&shot.episode_id)?;
        let index = shots
            .iter()
            .position(|s| s.id == id)
            .context("Shot not found")? as i32;
        let target = index + offset;
        if target < 0 || target >= shots.len() as i32 {
            return Ok(());
        }
        let other = &shots[target as usize];
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE shots SET position=?2 WHERE id=?1",
            params![shot.id, other.position],
        )?;
        tx.execute(
            "UPDATE shots SET position=?2 WHERE id=?1",
            params![other.id, shot.position],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn export_request(&self, episode: &str) -> Result<ExportRequest> {
        let c = self.config()?;
        if !c.ffmpeg.is_file() {
            bail!("FFmpeg is not configured");
        }
        let shots = self.shots(episode)?;
        if shots.is_empty() {
            bail!("Episode has no shots");
        }
        let assets = self.assets()?;
        let total: f64 = shots.iter().map(|s| s.duration).sum();
        let mut cues = Vec::new();
        let mut captions = Vec::new();
        let mut shot_start = 0.;
        let mut clips = Vec::new();
        for shot in shots {
            let asset = assets
                .iter()
                .find(|a| Some(&a.id) == shot.asset_id.as_ref())
                .with_context(|| format!("Shot '{}' has no rendered asset", shot.title))?;
            if asset.review != "approved" {
                bail!("Approve the take for '{}' before exporting", shot.title);
            }
            self.validate_performance_take(&shot, asset)?;
            if !shot.source_start.is_finite()
                || shot.source_start < 0.
                || asset.duration + 0.02 < shot.source_start + shot.duration
            {
                bail!(
                    "Shot '{}' extends past its take; adjust the picture in-point or duration",
                    shot.title
                );
            }
            let mut audio = Vec::new();
            let mut offscreen_dialogue_ids = Vec::new();
            let mut speech_duration = 0.;
            for line in self.dialogue(&shot.id)? {
                if !self.dialogue_direction(&line.id)?.speaker_visible {
                    offscreen_dialogue_ids.push(line.id.clone());
                }
                let speech = assets
                    .iter()
                    .find(|a| Some(&a.id) == line.asset_id.as_ref())
                    .with_context(|| {
                        format!("Dialogue for '{}' has not been generated", shot.title)
                    })?;
                if speech.review == "rejected" {
                    bail!(
                        "Dialogue for '{}' was rejected; approve it or regenerate before export",
                        shot.title
                    );
                }
                let start = line.timeline_start.unwrap_or(shot_start + speech_duration);
                if start + speech.duration > total + 0.001 {
                    bail!("Dialogue extends past the episode; move it earlier or extend the cut");
                }
                captions.push(crate::captions::Caption {
                    start,
                    end: start + speech.duration,
                    text: line.text.clone(),
                });
                if line.timeline_start.is_some() {
                    cues.push(crate::sound::ExportCue {
                        skip: 0.,
                        cue: crate::sound::AudioCue {
                            id: line.id.clone(),
                            episode_id: episode.into(),
                            asset_id: speech.id.clone(),
                            name: line.text.clone(),
                            kind: "dialogue".into(),
                            start,
                            source_start: 0.,
                            duration: speech.duration,
                            gain_db: 0.,
                            muted: false,
                            fade_in: 0.,
                            fade_out: 0.,
                        },
                        path: speech.path.clone(),
                        hash: speech.hash.clone(),
                    });
                    continue;
                }
                speech_duration += speech.duration;
                audio.push(ExportAudio {
                    source_start: 0.,
                    path: speech.path.clone(),
                    hash: speech.hash.clone(),
                    duration: speech.duration,
                });
            }
            if speech_duration > shot.duration {
                bail!(
                    "Dialogue for '{}' is longer than the shot; increase shot duration or regenerate faster speech",
                    shot.title
                );
            }
            shot_start += shot.duration;
            let picture_fades = self.picture_fades(&shot.id)?;
            picture_fades.validate(shot.duration)?;
            clips.push(ExportClip {
                gap_before: 0.,
                audio_fade_out: 0.,
                audio_fade_in: 0.,
                audio_gain_db: 0.,
                audio_muted: false,
                offscreen_dialogue_ids,
                picture_fades,
                source_start: shot.source_start,
                direction_context: self.shot_context(&shot.id)?,
                audio,
                asset_id: asset.id.clone(),
                path: asset.path.clone(),
                duration: shot.duration,
                hash: asset.hash.clone(),
            });
        }
        for cue in self.audio_cues(episode)? {
            let asset = assets
                .iter()
                .find(|a| a.id == cue.asset_id)
                .context("Cue audio is missing")?;
            if asset.review != "approved" {
                bail!("Approve audio for '{}' before exporting", cue.name);
            }
            if cue.start + cue.duration > total + 0.001 {
                bail!(
                    "Audio cue '{}' extends beyond the episode; trim or reposition it",
                    cue.name
                );
            }
            cues.push(crate::sound::ExportCue {
                skip: 0.,
                cue,
                path: asset.path.clone(),
                hash: asset.hash.clone(),
            });
        }
        let episode_data = self.get_episode(episode)?;
        let mut library = self.library(&episode_data.series_id, "character")?;
        library.extend(self.library(&episode_data.series_id, "location")?);
        let references: Vec<_> = library
            .iter()
            .filter_map(|entry| {
                entry
                    .reference
                    .as_ref()
                    .map(|path| (entry.id.clone(), path))
            })
            .map(|(id, path)| Ok((id, crate::worker::hash_file(path)?)))
            .collect::<Result<_>>()?;
        captions.sort_by(|a, b| a.start.total_cmp(&b.start));
        let request = ExportRequest {
            captions,
            burn_captions: self.burn_captions(episode)?,
            screenplay: episode_data.screenplay,
            continuity_context: serde_json::json!({"library":library,"references":references}),
            cues,
            episode_id: episode.into(),
            clips,
            width: self.video_dimensions(1080, 1920).0,
            height: self.video_dimensions(1080, 1920).1,
            fps: 24,
        };
        Ok(request)
    }
    pub fn enqueue_export(&self, episode: &str) -> Result<crate::store::Job> {
        let request = self.export_request(episode)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,'Assemble episode · project format / 24 fps','queued','',?2,'export',datetime('now'),datetime('now'))",params![id,serde_json::to_string(&request)?])?;
        self.job(&id)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Dialogue {
    pub id: String,
    pub shot_id: String,
    pub text: String,
    pub voice: String,
    pub speed: f64,
    pub asset_id: Option<String>,
    pub timeline_start: Option<f64>,
    pub revision: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceRequest {
    pub text: String,
    pub voice: String,
    pub speed: f64,
    pub line_id: String,
    #[serde(default)]
    pub line_revision: Option<u64>,
}
impl Store {
    pub fn speech_root(&self) -> Result<PathBuf> {
        let c = self.config()?;
        Ok(if c.speech_model.as_os_str().is_empty() {
            self.root().join("models/kokoro")
        } else {
            c.speech_model
        })
    }
    pub fn set_voice(&self, id: &str, voice: &str) -> Result<()> {
        if voice.is_empty() || voice.len() > 50 {
            bail!("Invalid voice ID");
        }
        if self.conn.execute(
            "UPDATE library SET voice=?2 WHERE id=?1 AND kind='character'",
            params![id, voice],
        )? == 0
        {
            bail!("Character not found");
        }
        Ok(())
    }
    pub fn dialogue(&self, shot: &str) -> Result<Vec<Dialogue>> {
        Ok(self.conn.prepare("SELECT id,shot_id,text,voice,speed,asset_id,timeline_start,revision FROM dialogue WHERE shot_id=?1 ORDER BY rowid")?.query_map([shot],|r|Ok(Dialogue{id:r.get(0)?,shot_id:r.get(1)?,text:r.get(2)?,voice:r.get(3)?,speed:r.get(4)?,asset_id:r.get(5)?,timeline_start:r.get(6)?,revision:r.get(7)?}))?.collect::<std::result::Result<_,_>>()?)
    }
    /// Save authored dialogue without requiring an installed inference model.
    pub fn save_dialogue_draft(&self, draft: &Dialogue) -> Result<Dialogue> {
        validate_dialogue(&draft.text, &draft.voice, draft.speed)?;
        uuid::Uuid::parse_str(&draft.id)?;
        self.shot(&draft.shot_id)?;
        if draft
            .timeline_start
            .is_some_and(|v| !v.is_finite() || !(0. ..=1800.).contains(&v))
        {
            bail!("Invalid dialogue timeline start");
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = self
            .dialogue(&draft.shot_id)?
            .into_iter()
            .find(|line| line.id == draft.id);
        if current.as_ref().map(|line| line.revision).unwrap_or(0) != draft.revision {
            bail!("Dialogue changed in another session; keep your draft and reload before saving");
        }
        if let Some(current) = &current {
            let changed = current.text != draft.text
                || current.voice != draft.voice
                || current.speed != draft.speed;
            if !changed && current.timeline_start == draft.timeline_start {
                return Ok(current.clone());
            }
            tx.execute("UPDATE dialogue SET text=?2,voice=?3,speed=?4,timeline_start=?5,revision=revision+1,voice_revision=voice_revision+CASE WHEN ?6 THEN 1 ELSE 0 END,asset_id=CASE WHEN ?6 THEN NULL ELSE asset_id END WHERE id=?1",params![draft.id,draft.text,draft.voice,draft.speed,draft.timeline_start,changed])?;
        } else {
            tx.execute("INSERT INTO dialogue(id,shot_id,text,voice,speed,timeline_start,revision) VALUES(?1,?2,?3,?4,?5,?6,1)",params![draft.id,draft.shot_id,draft.text,draft.voice,draft.speed,draft.timeline_start])?;
        }
        let saved = self
            .dialogue(&draft.shot_id)?
            .into_iter()
            .find(|line| line.id == draft.id)
            .context("Dialogue not found")?;
        tx.commit()?;
        Ok(saved)
    }
    pub fn set_dialogue_timing(&self, line: &str, start: Option<f64>) -> Result<()> {
        if start.is_some_and(|v| !v.is_finite() || !(0. ..=1800.).contains(&v)) {
            bail!("Dialogue timeline start must be finite and between 0 and 1800 seconds");
        }
        if self.conn.execute(
            "UPDATE dialogue SET revision=revision+CASE WHEN timeline_start IS NOT ?2 THEN 1 ELSE 0 END,timeline_start=?2 WHERE id=?1",
            params![line, start],
        )? == 0
        {
            bail!("Dialogue line not found");
        }
        Ok(())
    }
    pub fn burn_captions(&self, episode: &str) -> Result<bool> {
        let key = format!("captions:{episode}");
        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get::<_, String>(0)
            });
        match value {
            Ok(value) => Ok(value == "burn_in"),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    pub fn set_caption_mode(&self, episode: &str, burn: bool) -> Result<()> {
        self.get_episode(episode)?;
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("captions:{episode}"),if burn {"burn_in"}else{"sidecar"}])?;
        Ok(())
    }
    pub fn enqueue_dialogue(
        &self,
        shot: &str,
        text: &str,
        voice: &str,
        speed: f64,
        line_id: Option<&str>,
    ) -> Result<crate::store::Job> {
        self.shot(shot)?;
        validate_dialogue(text, voice, speed)?;
        let c = self.config()?;
        let root = self.speech_root()?;
        if !c.python.is_file()
            || !root.join("kokoro-v1.0.int8.onnx").is_file()
            || !root.join("voices-v1.0.bin").is_file()
        {
            bail!("Local Kokoro model and Python worker are not configured");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let line_id = line_id
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let tx = self.conn.unchecked_transaction()?;
        if line_id.is_empty() {
            bail!("Invalid dialogue line ID");
        }
        let existing: Option<String> = {
            use rusqlite::OptionalExtension;
            tx.query_row(
                "SELECT shot_id FROM dialogue WHERE id=?1",
                [&line_id],
                |r| r.get(0),
            )
            .optional()?
        };
        if existing.as_deref().is_some_and(|s| s != shot) {
            bail!("Dialogue belongs to another shot");
        }
        tx.execute("INSERT INTO dialogue(id,shot_id,text,voice,speed,revision,voice_revision) VALUES(?1,?2,?3,?4,?5,1,1) ON CONFLICT(id) DO UPDATE SET text=excluded.text,voice=excluded.voice,speed=excluded.speed,asset_id=NULL,revision=revision+1,voice_revision=voice_revision+1",params![line_id,shot,text,voice,speed])?;
        let revision = tx.query_row(
            "SELECT voice_revision FROM dialogue WHERE id=?1",
            [&line_id],
            |row| row.get(0),
        )?;
        let r = VoiceRequest {
            text: text.into(),
            voice: voice.into(),
            speed,
            line_id,
            line_revision: Some(revision),
        };
        tx.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,?2,'queued','',?3,'dialogue',datetime('now'),datetime('now'))",params![id,format!("Dialogue · {text}"),serde_json::to_string(&r)?])?;
        tx.commit()?;
        self.job(&id)
    }
}
pub(crate) fn validate_dialogue(text: &str, voice: &str, speed: f64) -> Result<()> {
    if text.trim().is_empty()
        || text.len() > 5000
        || voice.is_empty()
        || voice.len() > 50
        || !speed.is_finite()
        || !(0.5..=2.).contains(&speed)
    {
        bail!("Dialogue needs text (up to 5000 bytes), a voice (up to 50 bytes), and speed 0.5–2");
    }
    Ok(())
}
impl Store {
    pub fn assign_take(&self, shot: &str, asset: &str) -> Result<()> {
        let planned = self.shot(shot)?;
        let assets = self.assets()?;
        let a = assets
            .iter()
            .find(|a| a.id == asset)
            .context("Asset not found")?;
        self.validate_performance_take(&planned, a)?;
        if a.width == 0 || a.duration + 0.02 < planned.source_start + planned.duration {
            bail!(
                "Take must contain video long enough for this shot's in-point and duration; reset the picture trim if needed"
            );
        }
        self.conn.execute(
            "UPDATE shots SET asset_id=?2,status='rendered' WHERE id=?1",
            params![shot, asset],
        )?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImageRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub seed: u32,
    pub reference: Option<PathBuf>,
    pub library_id: Option<String>,
    pub library_reference_before: Option<PathBuf>,
    pub reference_hash: Option<String>,
    pub library_context_before: serde_json::Value,
}
impl Default for ImageRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            width: 704,
            height: 1280,
            steps: 4,
            seed: 42,
            reference: None,
            library_id: None,
            library_reference_before: None,
            reference_hash: None,
            library_context_before: serde_json::Value::Null,
        }
    }
}
impl ImageRequest {
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty()
            || !(256..=1536).contains(&self.width)
            || !(256..=1536).contains(&self.height)
            || !self.width.is_multiple_of(16)
            || !self.height.is_multiple_of(16)
            || !(1..=20).contains(&self.steps)
        {
            bail!(
                "Image generation requires a prompt, dimensions divisible by 16 (256–1536) and 1–20 steps"
            );
        }
        if let Some(path) = &self.reference
            && !path.is_file()
        {
            bail!("Reference image is missing");
        }
        Ok(())
    }
}
impl Store {
    pub fn image_root(&self) -> Result<PathBuf> {
        let c = self.config()?;
        Ok(if c.image_model.as_os_str().is_empty() {
            self.root().join("models/flux2-klein-4b-q4")
        } else {
            c.image_model
        })
    }
    pub fn enqueue_image(&self, r: &ImageRequest) -> Result<crate::store::Job> {
        let mut r = r.clone();
        r.validate()?;
        r.library_context_before = serde_json::Value::Null;
        if let Some(id) = &r.library_id {
            self.ensure_character_not_deleted(id)?;
            let reference: Option<String> = self
                .conn
                .query_row("SELECT reference FROM library WHERE id=?1", [id], |row| {
                    row.get(0)
                })
                .context("Library entry not found")?;
            let (name, description): (String, String) = self.conn.query_row(
                "SELECT name,description FROM library WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let reference_hash = reference
                .as_ref()
                .map(|path| crate::worker::hash_file(Path::new(path)))
                .transpose()?;
            r.library_context_before = serde_json::json!({"name":name,"description":description,"reference_hash":reference_hash});
            r.library_reference_before = reference.clone().map(PathBuf::from);
            if r.reference.is_none() {
                r.reference = reference.map(PathBuf::from);
            }
        }
        r.validate()?;
        let c = self.config()?;
        r.reference_hash = r
            .reference
            .as_ref()
            .map(|path| crate::worker::hash_file(path))
            .transpose()?;
        let root = self.image_root()?;
        let ready = c.image_python.is_file()
            && root.join("config.json").is_file()
            && root.join("transformer/0.safetensors").is_file()
            && root.join("text_encoder/0.safetensors").is_file()
            && root.join("vae/0.safetensors").is_file();
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'image',datetime('now'),datetime('now'))",params![id,r.prompt,if ready{"queued"}else{"blocked"},if ready{""}else{"Configure the isolated image Python environment and local FLUX.2 checkpoint"},serde_json::to_string(&r)?])?;
        self.job(&id)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LipSyncRequest {
    pub video_asset_id: String,
    pub audio_asset_id: String,
    pub shot_id: Option<String>,
}
impl Store {
    pub fn lipsync_root(&self) -> Result<PathBuf> {
        let c = self.config()?;
        Ok(if c.lipsync_model.as_os_str().is_empty() {
            self.root().join("models/musetalk-q4")
        } else {
            c.lipsync_model
        })
    }
    pub fn enqueue_lipsync(&self, r: &LipSyncRequest) -> Result<crate::store::Job> {
        let assets = self.assets()?;
        let video = assets
            .iter()
            .find(|a| a.id == r.video_asset_id)
            .context("Video asset not found")?;
        let audio = assets
            .iter()
            .find(|a| a.id == r.audio_asset_id)
            .context("Dialogue asset not found")?;
        if video.width == 0
            || video.duration <= 0.
            || audio.width != 0
            || audio.duration <= 0.
            || audio.duration > video.duration
        {
            bail!("Lip sync needs a video take and a dialogue asset that fits its duration");
        }
        if let Some(shot) = &r.shot_id {
            self.shot(shot)?;
            for line in self.dialogue(shot)? {
                if line.asset_id.as_deref() == Some(r.audio_asset_id.as_str()) {
                    self.require_visible_speaker(&line.id)?;
                }
            }
        }
        let c = self.config()?;
        let root = self.lipsync_root()?;
        if !c.lipsync_python.is_file()
            || ![
                "config.json",
                "unet.safetensors",
                "vae.safetensors",
                "whisper_encoder.safetensors",
                "face_landmarker.task",
            ]
            .iter()
            .all(|f| root.join(f).is_file())
        {
            bail!("Local MuseTalk worker is not configured");
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,'Experimental lip synchronization · one visible speaker','queued','',?2,'lipsync',datetime('now'),datetime('now'))",params![id,serde_json::to_string(r)?])?;
        self.job(&id)
    }
}

impl Store {
    pub fn asset_label(&self, asset: &Asset) -> String {
        let request = &asset.metadata["request"];
        match asset.metadata["kind"].as_str() {
            Some("episode_export") => asset.metadata["timeline"]["episode_id"]
                .as_str()
                .and_then(|id| self.get_episode(id).ok())
                .map(|e| format!("{} · export", e.title))
                .unwrap_or("Episode export".into()),
            Some("image") => request["library_id"]
                .as_str()
                .and_then(|id| {
                    self.conn
                        .query_row("SELECT name FROM library WHERE id=?1", [id], |r| {
                            r.get::<_, String>(0)
                        })
                        .ok()
                })
                .map(|name| format!("{name} · reference"))
                .unwrap_or("Generated image".into()),
            Some("imported_audio") => asset.metadata["name"]
                .as_str()
                .unwrap_or("Imported audio")
                .into(),
            Some("dialogue") => "Dialogue line".into(),
            Some("performance") => request["shot_id"]
                .as_str()
                .and_then(|id| self.shot(id).ok())
                .map(|shot| format!("{} · native acting · experimental", shot.title))
                .unwrap_or("Native performance · experimental".into()),
            Some("dialogue_rehearsal") => request["episode_id"]
                .as_str()
                .and_then(|id| self.get_episode(id).ok())
                .map(|episode| format!("{} · dialogue rehearsal", episode.title))
                .unwrap_or("Dialogue rehearsal · audio only".into()),
            kind => request["shot_id"]
                .as_str()
                .and_then(|id| self.shot(id).ok())
                .map(|s| {
                    if kind == Some("lipsync") {
                        format!("{} · lip sync", s.title)
                    } else {
                        s.title
                    }
                })
                .unwrap_or("Video take".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> Store {
        Store::open(std::env::temp_dir().join(format!("df-{}.sqlite", uuid::Uuid::new_v4())))
            .unwrap()
    }
    #[test]
    fn shot_order_and_export_gates() {
        let s = setup();
        let series = s.create_series("Series", "").unwrap();
        let e = s.create_episode(&series.id, "Pilot").unwrap();
        let a = s.add_shot(&e.id, "A", "Rain", "Wide", 5.).unwrap();
        let b = s.add_shot(&e.id, "B", "Sun", "Close", 4.).unwrap();
        s.move_shot(&b.id, -1).unwrap();
        assert_eq!(s.shots(&e.id).unwrap()[0].id, b.id);
        s.update_shot(&a.id, "A revised", "Storm", "Wide", 3.)
            .unwrap();
        assert_eq!(s.shot(&a.id).unwrap().duration, 3.);
        assert!(s.add_shot(&e.id, "bad", "prompt", "", f64::NAN).is_err());
        assert!(s.enqueue_export(&e.id).is_err());
    }
    #[test]
    fn render_validation_and_running_cancellation() {
        let s = setup();
        let mut r = RenderRequest {
            prompt: "Test".into(),
            ..Default::default()
        };
        for invalid in [0, 4, 120, 242] {
            r.frames = invalid;
            assert!(r.validate().is_err());
        }
        r.frames = 121;
        r.width = 701;
        assert!(r.validate().is_err());
        r.width = 704;
        let j = s.enqueue(&r).unwrap();
        assert_eq!(j.status, "blocked");
        s.conn
            .execute("UPDATE jobs SET status='running' WHERE id=?1", [&j.id])
            .unwrap();
        s.cancel(&j.id).unwrap();
        assert_eq!(s.job(&j.id).unwrap().status, "running");
        assert!(
            s.conn
                .query_row(
                    "SELECT cancel_requested FROM jobs WHERE id=?1",
                    [&j.id],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap()
        );
        assert!(s.retry(&j.id).is_err());
    }
    #[test]
    fn migration_preserves_v001_projects() {
        let path = std::env::temp_dir().join(format!("df-{}.sqlite", uuid::Uuid::new_v4()));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE series(id TEXT PRIMARY KEY,name TEXT,premise TEXT); INSERT INTO series VALUES('original','Keep me','Original premise'); CREATE TABLE jobs(id TEXT PRIMARY KEY,prompt TEXT,status TEXT,error TEXT); INSERT INTO jobs VALUES('old','Original request','blocked','Setup needed');").unwrap();
        drop(conn);
        let s = Store::open(path).unwrap();
        assert_eq!(s.series().unwrap()[0].name, "Keep me");
        assert_eq!(s.jobs().unwrap()[0].prompt, "Original request");
        assert_eq!(s.jobs().unwrap()[0].attempts, 0);
    }
}
#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn reference_changes_require_review_and_cannot_clobber_a_newer_choice() {
        let s =
            Store::open(std::env::temp_dir().join(format!("df-{}.sqlite", uuid::Uuid::new_v4())))
                .unwrap();
        let series = s.create_series("Test", "").unwrap();
        let entry = s
            .add_library(&series.id, "character", "Actor", "", None)
            .unwrap();
        let metadata =
            serde_json::json!({"kind":"image","request":{"library_id":entry.id,"reference":null}})
                .to_string();
        s.conn.execute("INSERT INTO assets VALUES('image-fixture','job','/temporary/image.png','hash',704,1280,0,?1,'unreviewed')",[metadata]).unwrap();
        assert!(
            s.library(&series.id, "character").unwrap()[0]
                .reference
                .is_none()
        );
        s.review("image-fixture", "approved").unwrap();
        assert_eq!(
            s.library(&series.id, "character").unwrap()[0].reference,
            Some(PathBuf::from("/temporary/image.png"))
        );
        s.review("image-fixture", "rejected").unwrap();
        assert!(
            s.library(&series.id, "character").unwrap()[0]
                .reference
                .is_none()
        );
        s.conn
            .execute(
                "UPDATE library SET reference='/newer/reference.png' WHERE id=?1",
                [&entry.id],
            )
            .unwrap();
        assert!(s.review("image-fixture", "approved").is_err());
        assert_eq!(s.assets().unwrap()[0].review, "rejected");
    }
    #[test]
    fn externally_conditioned_first_reference_approves_and_restores_correctly() {
        let root = std::env::temp_dir().join(format!("df-reference-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Reference fixture", "").unwrap();
        let character = store
            .add_library(&series.id, "character", "Mara", "Adult woman", None)
            .unwrap();
        let input = root.join("external.png");
        image::RgbImage::new(32, 32).save(&input).unwrap();
        let output = root.join("proposal.png");
        image::RgbImage::new(32, 32).save(&output).unwrap();
        let metadata = serde_json::json!({"kind":"image","request":{"library_id":character.id,"reference":input,"library_reference_before":null}});
        store.conn.execute("INSERT INTO assets VALUES('proposal','fixture',?1,'fixture',32,32,0,?2,'unreviewed')",params![output.to_string_lossy(),metadata.to_string()]).unwrap();
        store.review("proposal", "approved").unwrap();
        assert_eq!(
            store.library(&series.id, "character").unwrap()[0].reference,
            Some(output)
        );
        store.review("proposal", "rejected").unwrap();
        assert!(
            store.library(&series.id, "character").unwrap()[0]
                .reference
                .is_none()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_image_dimensions_are_rejected() {
        let mut r = ImageRequest {
            prompt: "A portrait".into(),
            ..Default::default()
        };
        r.width = 705;
        assert!(r.validate().is_err());
        r.width = 704;
        r.steps = 0;
        assert!(r.validate().is_err());
    }
}
