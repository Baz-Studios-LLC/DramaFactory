//! Durable Seedance jobs. An uncertain POST is never automatically repeated.
use crate::{store::Store, video_plan::VideoPlan};
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde_json::{Value, json};
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}
impl Store {
    pub fn enqueue_seedance(&self, plan: &VideoPlan) -> Result<crate::store::Job> {
        self.enqueue_seedance_bound(plan, None)
    }
    pub fn enqueue_episode_segment(
        &self,
        episode: &str,
        revision: u64,
        segment: &crate::generation_plan::SegmentDraft,
    ) -> Result<crate::store::Job> {
        let saved = self.generation_plan(episode)?;
        if saved.revision != revision || !saved.segments.contains(segment) {
            bail!("Save/reload the current episode plan before generating this segment");
        }
        let plan = segment.video_plan()?;
        self.enqueue_seedance_bound(&plan, Some((episode, revision, segment)))
    }
    fn enqueue_seedance_bound(
        &self,
        plan: &VideoPlan,
        binding: Option<(&str, u64, &crate::generation_plan::SegmentDraft)>,
    ) -> Result<crate::store::Job> {
        let tx = self.conn.unchecked_transaction()?;
        let job = self.enqueue_seedance_in_transaction(plan, binding)?;
        tx.commit()?;
        Ok(job)
    }
    fn enqueue_seedance_in_transaction(
        &self,
        plan: &VideoPlan,
        binding: Option<(&str, u64, &crate::generation_plan::SegmentDraft)>,
    ) -> Result<crate::store::Job> {
        if plan.model != "bytedance/seedance-2.5" || plan.prompt.trim().is_empty() {
            bail!("Choose Seedance 2.5 and provide a prompt");
        }
        if serde_json::to_vec(plan)?.len() > 128 * 1024 {
            bail!("Video request exceeds the 128 KiB authoring limit");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let tx = &self.conn;
        if let Some((episode, revision, segment)) = binding {
            let current: u64 = tx
                .query_row(
                    "SELECT revision FROM generation_plans WHERE episode_id=?1",
                    [episode],
                    |row| row.get(0),
                )
                .context("Save the episode plan first")?;
            if current != revision {
                bail!("Episode plan changed before queueing");
            }
            use rusqlite::OptionalExtension;
            let existing:Option<String>=tx.query_row("SELECT j.id FROM segment_generations g JOIN jobs j ON j.id=g.job_id WHERE g.episode_id=?1 AND g.segment_id=?2 AND g.plan_revision=?3 AND j.status!='completed' ORDER BY j.rowid DESC LIMIT 1",params![episode,segment.id,revision],|row|row.get(0)).optional()?;
            if let Some(existing) = existing {
                return self.job(&existing);
            }
            tx.execute("INSERT INTO segment_generations(job_id,episode_id,segment_id,plan_revision,snapshot) VALUES(?1,?2,?3,?4,?5)",params![id,episode,segment.id,revision,serde_json::to_string(segment)?])?;
        }
        tx.execute("INSERT INTO jobs(id,prompt,status,kind,request,created_at,updated_at) VALUES(?1,?2,'queued','openrouter_video',?3,datetime('now'),datetime('now'))",params![id,plan.prompt,serde_json::to_string(plan)?])?;
        tx.execute(
            "INSERT INTO remote_video_jobs(job_id,state) VALUES(?1,'prepared')",
            [&id],
        )?;
        self.job(&id)
    }
    /// Explicit paid batch action. Save and queue atomically; never retry an existing request.
    pub(crate) fn enqueue_generation_plan(
        &self,
        draft: &crate::generation_plan::SavedPlan,
        catalog: &crate::openrouter::Catalog,
    ) -> Result<(crate::generation_plan::SavedPlan, usize)> {
        use rusqlite::OptionalExtension;
        for segment in &draft.segments {
            segment.video_plan()?.preview(catalog)?;
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let saved = self.save_generation_plan_in_transaction(draft)?;
        let mut queued = 0;
        for segment in &saved.segments {
            let snapshot = serde_json::to_string(segment)?;
            let existing: Option<String> = self.conn.query_row("SELECT job_id FROM segment_generations WHERE episode_id=?1 AND segment_id=?2 AND snapshot=?3 LIMIT 1", params![saved.episode_id, segment.id, snapshot], |row| row.get(0)).optional()?;
            if existing.is_none() {
                self.enqueue_seedance_in_transaction(
                    &segment.video_plan()?,
                    Some((&saved.episode_id, saved.revision, segment)),
                )?;
                queued += 1;
            }
        }
        tx.commit()?;
        Ok((saved, queued))
    }
    pub fn segment_generation_jobs(&self, episode: &str, segment: &str) -> Result<Vec<Value>> {
        self.get_episode(episode)?;
        let mut query=self.conn.prepare("SELECT j.id,j.status,j.phase,j.output_id,g.plan_revision FROM segment_generations g JOIN jobs j ON j.id=g.job_id WHERE g.episode_id=?1 AND g.segment_id=?2 ORDER BY j.rowid DESC")?;
        Ok(query.query_map(params![episode,segment],|row|Ok(json!({"job_id":row.get::<_,String>(0)?,"status":row.get::<_,String>(1)?,"stage":row.get::<_,String>(2)?,"asset_id":row.get::<_,Option<String>>(3)?,"plan_revision":row.get::<_,u64>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    fn begin_video_submission(&self, id: &str) -> Result<()> {
        if self.conn.execute("UPDATE remote_video_jobs SET state='submitting' WHERE job_id=?1 AND state='prepared' AND remote_id IS NULL",[id])?!=1{bail!("Submission is uncertain or already attempted. Inspect OpenRouter activity; this job will not submit again automatically.");}
        Ok(())
    }
    fn bind_video_remote(&self, id: &str, remote: &str) -> Result<()> {
        if !valid_id(remote) {
            bail!("Provider returned an invalid job identifier; submission may have been billed");
        }
        if self.conn.execute("UPDATE remote_video_jobs SET remote_id=?2,state='accepted' WHERE job_id=?1 AND state='submitting' AND remote_id IS NULL",params![id,remote])?!=1{bail!("Remote job binding changed; refusing to overwrite it");}
        Ok(())
    }
}
fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .max_redirects(0)
        .build()
        .into()
}
fn check_cancel(store: &Store, id: &str) -> Result<()> {
    let canceled: bool = store.conn.query_row(
        "SELECT cancel_requested FROM jobs WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    if canceled {
        bail!(
            "Local monitoring canceled. The provider job may continue and incur charges; no remote cancellation is claimed."
        );
    }
    Ok(())
}
fn read_json(mut response: ureq::http::Response<ureq::Body>) -> Result<Value> {
    Ok(response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_json()?)
}
pub(crate) fn execute(store: &Store, id: &str) -> Result<()> {
    let key = crate::credentials::read()?.context("Save an OpenRouter key in Settings first")?;
    execute_transport(
        store,
        id,
        "https://openrouter.ai/api/v1",
        &key,
        None,
        std::time::Duration::from_secs(1),
    )
}
// Private loopback injection only; production endpoint cannot be configured.
fn execute_transport(
    store: &Store,
    id: &str,
    base: &str,
    key: &str,
    catalog: Option<&crate::openrouter::Catalog>,
    poll_tick: std::time::Duration,
) -> Result<()> {
    check_cancel(store, id)?;
    let text: String =
        store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [id], |row| {
                row.get(0)
            })?;
    let plan: VideoPlan = serde_json::from_str(&text)?;
    let agent = client();
    let authorization = format!("Bearer {key}");
    let remote: Option<String> = store.conn.query_row(
        "SELECT remote_id FROM remote_video_jobs WHERE job_id=?1",
        [id],
        |row| row.get(0),
    )?;
    let directory = store.root().join("jobs").join(id).join("remote");
    std::fs::create_dir_all(&directory)?;
    let remote = match remote {
        Some(remote) => {
            if !valid_id(&remote) {
                bail!("Stored remote job identifier is invalid");
            }
            remote
        }
        None => {
            let discovered;
            let catalog = match catalog {
                Some(catalog) => catalog,
                None => {
                    discovered = crate::openrouter::discover()?;
                    &discovered
                }
            };
            let preview = plan.preview(catalog)?;
            let payload = preview["request"].clone();
            std::fs::write(
                directory.join("request.json"),
                serde_json::to_vec_pretty(&payload)?,
            )?;
            check_cancel(store, id)?;
            store.conn.execute(
                "UPDATE jobs SET phase='Submitting to Seedance' WHERE id=?1",
                [id],
            )?;
            store.begin_video_submission(id)?;
            let response=agent.post(&format!("{base}/videos")).header("Authorization",&authorization).send_json(payload).map_err(|_|anyhow::anyhow!("Seedance submission was not confirmed locally; billing may have occurred. No automatic resubmission."))?;
            let response = read_json(response)?;
            std::fs::write(
                directory.join("submit-response.json"),
                serde_json::to_vec_pretty(&response)?,
            )?;
            let remote = response["id"]
                .as_str()
                .context("Provider returned no job ID; submission is uncertain")?;
            store.bind_video_remote(id, remote)?;
            remote.into()
        }
    };
    let start = std::time::Instant::now();
    let result = loop {
        check_cancel(store, id)?;
        let response=agent.get(&format!("{base}/videos/{remote}")).header("Authorization",&authorization).call().map_err(|_|anyhow::anyhow!("Could not poll the saved provider job. Retry monitoring this job; do not generate a replacement."))?;
        let response = read_json(response)?;
        // Retain polls without exposing signed media URLs or response errors in diagnostics.
        let poll = directory.join(format!("poll-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(poll, serde_json::to_vec_pretty(&response)?)?;
        match response["status"].as_str() {
            Some("completed") => break response,
            Some("failed" | "cancelled" | "canceled" | "expired") => {
                store.conn.execute(
                    "UPDATE remote_video_jobs SET state='terminal_failed' WHERE job_id=?1",
                    [id],
                )?;
                bail!(
                    "Provider job ended without usable output. Inspect its retained response before generating again."
                );
            }
            Some("pending" | "in_progress") => {}
            _ => bail!(
                "Provider returned an unknown status. Saved job ID retained; no new generation was submitted."
            ),
        }
        if start.elapsed() > std::time::Duration::from_secs(30 * 60) {
            bail!(
                "Monitoring time limit reached. Saved provider job can be monitored again without resubmitting."
            );
        }
        let phase = if response["status"] == "pending" {
            "Queued at Seedance"
        } else {
            "Seedance generating picture and sound"
        };
        store
            .conn
            .execute("UPDATE jobs SET phase=?2 WHERE id=?1", params![id, phase])?;
        for _ in 0..30 {
            std::thread::sleep(poll_tick);
            check_cancel(store, id)?;
        }
    };
    check_cancel(store, id)?;
    store.conn.execute(
        "UPDATE jobs SET phase='Downloading video' WHERE id=?1",
        [id],
    )?;
    let part = directory.join(format!("download-{}.part", uuid::Uuid::new_v4()));
    let mut response = agent
        .get(&format!("{base}/videos/{remote}/content?index=0"))
        .header("Authorization", &authorization)
        .call()
        .map_err(|_| {
            anyhow::anyhow!("Provider download unavailable; saved remote job retained for retry")
        })?;
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)?;
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024 * 1024)
            .reader();
        std::io::copy(&mut reader, &mut file)?;
        file.flush()?;
        file.sync_all()?;
    }
    let output = directory.join(format!("take-{}.mp4", uuid::Uuid::new_v4()));
    std::fs::rename(&part, &output)?;
    store.conn.execute(
        "UPDATE jobs SET phase='Validating picture and sound' WHERE id=?1",
        [id],
    )?;
    let config = store.config()?;
    crate::worker::validate_video(&config.ffmpeg, &output)?;
    let info = crate::media::probe(&config.ffmpeg, &output)?;
    validate_output(&plan, &info)?;
    let hash = crate::worker::hash_file(&output)?;
    let asset = uuid::Uuid::new_v4().to_string();
    let metadata = json!({"kind":"hosted_video","provider":"openrouter","remote_id":remote,"request":plan,"usage":result.get("usage"),"info":info});
    let tx = store.conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO assets VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'unreviewed')",
        params![
            asset,
            id,
            output.to_string_lossy(),
            hash,
            info.width,
            info.height,
            info.duration,
            metadata.to_string()
        ],
    )?;
    crate::completion::finish(&tx, id, &asset, "Seedance take ready for review")?;
    tx.execute(
        "UPDATE remote_video_jobs SET state='downloaded' WHERE job_id=?1",
        [id],
    )?;
    tx.commit()?;
    Ok(())
}
fn validate_output(plan: &VideoPlan, info: &crate::media::VideoInfo) -> Result<()> {
    let ratio = plan
        .aspect_ratio
        .split_once(':')
        .and_then(|(w, h)| Some(w.parse::<f64>().ok()? / h.parse::<f64>().ok()?))
        .context("Requested aspect ratio is invalid")?;
    let minimum = plan
        .resolution
        .strip_suffix('p')
        .and_then(|value| value.parse::<u32>().ok())
        .context("Requested resolution is unsupported for validation")?;
    if info.width == 0
        || info.height == 0
        || !info.duration.is_finite()
        || (info.duration - plan.duration as f64).abs() > 0.25
        || info.width.min(info.height) < minimum
        || ((info.width as f64 / info.height as f64) - ratio).abs() > 0.02
        || plan.generate_audio && !info.audio
    {
        bail!(
            "Downloaded take does not match requested duration, framing, resolution or sound; original download retained for inspection"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_submission_gate_prevents_reposting_and_preserves_remote_identity() {
        let root = std::env::temp_dir().join(format!("df-remote-video-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let plan = VideoPlan {
            model: "bytedance/seedance-2.5".into(),
            prompt: "Fixture".into(),
            duration: 10,
            resolution: "720p".into(),
            aspect_ratio: "9:16".into(),
            generate_audio: true,
            inputs: crate::video_plan::Inputs::Prompt,
        };
        let job = store.enqueue_seedance(&plan).unwrap();
        store.begin_video_submission(&job.id).unwrap();
        assert!(store.begin_video_submission(&job.id).is_err());
        assert!(
            store
                .bind_video_remote(&job.id, "../credential-leak")
                .is_err()
        );
        store
            .conn
            .execute("UPDATE jobs SET status='failed' WHERE id=?1", [&job.id])
            .unwrap();
        assert!(store.retry(&job.id).is_err());
        store
            .bind_video_remote(&job.id, "remote_fixture-123")
            .unwrap();
        drop(store);
        let store = Store::open(path).unwrap();
        assert!(store.begin_video_submission(&job.id).is_err());
        store
            .conn
            .execute("UPDATE jobs SET status='failed' WHERE id=?1", [&job.id])
            .unwrap();
        store.retry(&job.id).unwrap();
        assert!(store.bind_video_remote(&job.id, "replacement").is_err());
        let remote: String = store
            .conn
            .query_row(
                "SELECT remote_id FROM remote_video_jobs WHERE job_id=?1",
                [&job.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remote, "remote_fixture-123");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn mock_provider(
        video: Vec<u8>,
        resume: bool,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            let mut requests = Vec::new();
            let steps = if resume { 2 } else { 4 };
            while requests.len() < steps {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Mock provider request timeout"
                );
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                let line = request.lines().next().unwrap().to_owned();
                let body = if line.starts_with("POST ") {
                    assert!(!resume);
                    assert!(line.starts_with("POST /api/v1/videos "));
                    assert!(request.contains("bytedance/seedance-2.5"));
                    br#"{"id":"fixture_job-123","status":"pending","polling_url":"https://untrusted.invalid/ignored"}"#.to_vec()
                } else if line.contains("/content?") {
                    assert!(
                        line.starts_with("GET /api/v1/videos/fixture_job-123/content?index=0 ")
                    );
                    video.clone()
                } else {
                    assert!(line.starts_with("GET /api/v1/videos/fixture_job-123 "));
                    if !resume && requests.len() == 1 {
                        br#"{"status":"in_progress"}"#.to_vec()
                    } else {
                        br#"{"status":"completed","unsigned_urls":["https://untrusted.invalid/ignored"],"usage":{"cost":0}}"#.to_vec()
                    }
                };
                requests.push(line);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
                stream.flush().unwrap();
            }
            requests
        });
        (format!("http://{address}/api/v1"), thread)
    }
    #[test]
    fn actual_http_video_roundtrip_and_resumption_use_one_remote_generation() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            eprintln!("Set DRAMAFACTORY_TEST_FFMPEG to run actual video HTTP/FFmpeg integration");
            return;
        };
        let root = std::env::temp_dir().join(format!("df-video-http-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let mut config = store.config().unwrap();
        config.ffmpeg = ffmpeg.into();
        store.save_config(&config).unwrap();
        let fixture = root.join("fixture.mp4");
        assert!(
            std::process::Command::new(&config.ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:s=720x1280:r=24",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
                    "-t",
                    "10",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "aac"
                ])
                .arg(&fixture)
                .status()
                .unwrap()
                .success()
        );
        let original = std::fs::read(&fixture).unwrap();
        let plan = VideoPlan {
            model: "bytedance/seedance-2.5".into(),
            prompt: "Explicit mock provider fixture".into(),
            duration: 10,
            resolution: "720p".into(),
            aspect_ratio: "9:16".into(),
            generate_audio: true,
            inputs: crate::video_plan::Inputs::Prompt,
        };
        let invalid = crate::media::VideoInfo {
            width: 64,
            height: 96,
            duration: 0.5,
            fps: 24.,
            audio: true,
        };
        assert!(validate_output(&plan, &invalid).is_err());
        let catalog = crate::openrouter::Catalog {
            videos: vec![
                json!({"id":plan.model,"supported_durations":[10],"supported_resolutions":["720p"],"supported_aspect_ratios":["9:16"],"generate_audio":true}),
            ],
            images: vec![],
            fetched_unix_seconds: 1,
        };
        for resume in [false, true] {
            let job = store.enqueue_seedance(&plan).unwrap();
            store
                .conn
                .execute(
                    "UPDATE jobs SET status='running',attempts=1 WHERE id=?1",
                    [&job.id],
                )
                .unwrap();
            if resume {
                store.begin_video_submission(&job.id).unwrap();
                store.bind_video_remote(&job.id, "fixture_job-123").unwrap();
            }
            let (base, thread) = mock_provider(original.clone(), resume);
            execute_transport(
                &store,
                &job.id,
                &base,
                "local-fixture-key",
                Some(&catalog),
                std::time::Duration::ZERO,
            )
            .unwrap();
            let requests = thread.join().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|line| line.starts_with("POST "))
                    .count(),
                if resume { 0 } else { 1 }
            );
            let saved = store.job(&job.id).unwrap();
            assert_eq!(saved.status, "completed");
            let asset = store
                .assets()
                .unwrap()
                .into_iter()
                .find(|asset| Some(&asset.id) == saved.output_id.as_ref())
                .unwrap();
            assert_eq!(asset.review, "unreviewed");
            assert_eq!(std::fs::read(asset.path).unwrap(), original);
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saved_segment_binding_reuses_pending_job_and_rejects_unsaved_changes() {
        let root = std::env::temp_dir().join(format!("df-bound-video-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("S", "P").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let mut plan = store.generation_plan(&episode.id).unwrap();
        plan.segments[0].prompt = "Saved segment".into();
        let plan = store.save_generation_plan(&plan).unwrap();
        let segment = &plan.segments[0];
        let first = store
            .enqueue_episode_segment(&episode.id, plan.revision, segment)
            .unwrap();
        let second = store
            .enqueue_episode_segment(&episode.id, plan.revision, segment)
            .unwrap();
        assert_eq!(first.id, second.id);
        let jobs = store
            .segment_generation_jobs(&episode.id, &segment.id)
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["plan_revision"], plan.revision);
        let mut changed = segment.clone();
        changed.prompt = "Unsaved".into();
        assert!(
            store
                .enqueue_episode_segment(&episode.id, plan.revision, &changed)
                .is_err()
        );
        assert!(
            store
                .enqueue_episode_segment(&episode.id, plan.revision + 1, segment)
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
