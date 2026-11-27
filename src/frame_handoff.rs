//! Last-frame handoff prepares an editable input. It never queues video generation.
use crate::{
    generation_plan::{SavedPlan, SegmentDraft},
    store::Store,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::mpsc,
};

type Key = (String, String);
#[derive(Default)]
pub(crate) struct State {
    pub queue: VecDeque<Key>,
    pending: Option<Pending>,
    status: HashMap<Key, String>,
    undo: HashMap<Key, (SegmentDraft, SegmentDraft)>,
    previews: HashMap<Key, (SegmentDraft, PathBuf)>,
    open_preview: Option<PathBuf>,
}
struct Pending {
    key: Key,
    expected: SegmentDraft,
    revision: u64,
    saved_revision: u64,
    asset: String,
    receiver: mpsc::Receiver<Result<(String, PathBuf), String>>,
    preview_receiver: Option<mpsc::Receiver<PathBuf>>,
}

pub(crate) fn set_source(segment: &mut SegmentDraft, source: Option<String>) {
    segment.last_frame_from = source;
    segment.frame_take = None;
    segment.frame_source_asset = None;
    segment.frame_preview.clear();
    segment.first.clear();
    segment.mode = if segment.last_frame_from.is_some() {
        2
    } else if segment.cast.is_empty() {
        0
    } else {
        1
    };
}
fn earlier(plan: &SavedPlan, target: &str, source: &str) -> Result<()> {
    let target = plan
        .segments
        .iter()
        .position(|s| s.id == target)
        .context("Destination segment no longer exists")?;
    ensure!(
        plan.segments[..target].iter().any(|s| s.id == source),
        "Choose an earlier segment from this episode"
    );
    Ok(())
}
impl Store {
    pub(crate) fn validate_frame_handoff(
        &self,
        episode: &str,
        segment: &SegmentDraft,
    ) -> Result<()> {
        if let Some(source) = &segment.last_frame_from {
            earlier(&self.generation_plan(episode)?, &segment.id, source)?;
            ensure!(
                segment.mode == 2
                    && !segment.first.is_empty()
                    && segment.frame_source_asset.is_some(),
                "Prepare the source frame before generating"
            );
            ensure!(
                chosen_take(self, episode, segment)? == segment.frame_source_asset,
                "The source take changed. Refresh its last frame before generating."
            );
            if let Some(take) = &segment.frame_take {
                validate_take(self, episode, source, take)?;
            }
        }
        Ok(())
    }
}
fn chosen_take(store: &Store, episode: &str, segment: &SegmentDraft) -> Result<Option<String>> {
    match &segment.frame_take {
        Some(take) => Ok(Some(take.clone())),
        None => store.selected_segment_take(
            episode,
            segment
                .last_frame_from
                .as_deref()
                .context("Choose a source segment")?,
        ),
    }
}
fn validate_take(store: &Store, episode: &str, source: &str, selected: &str) -> Result<()> {
    let owned: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM segment_generations g JOIN jobs j ON j.id=g.job_id JOIN assets a ON a.job_id=j.id WHERE g.episode_id=?1 AND g.segment_id=?2 AND a.id=?3 AND j.status='completed')", rusqlite::params![episode,source,selected], |r|r.get(0))?;
    ensure!(
        owned,
        "This take is no longer available for the source segment. Choose another take."
    );
    Ok(())
}
fn source_asset(
    store: &Store,
    plan: &SavedPlan,
    segment: &SegmentDraft,
) -> Result<crate::production::Asset> {
    let source = segment
        .last_frame_from
        .as_deref()
        .context("Choose a source segment")?;
    earlier(plan, &segment.id, source)?;
    let selected = chosen_take(store, &plan.episode_id, segment)?
        .context("Choose a source take in the Take dropdown.")?;
    validate_take(store, &plan.episode_id, source, &selected)?;
    store
        .assets()?
        .into_iter()
        .find(|a| a.id == selected)
        .context("The source take is unavailable")
}

pub(crate) fn selector(
    ui: &mut eframe::egui::Ui,
    store: &Store,
    plan: &mut SavedPlan,
    index: usize,
    state: &mut State,
) {
    let choices: Vec<_> = plan.segments[..index]
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), format!("Segment {:02}", i + 1)))
        .collect();
    let segment = &mut plan.segments[index];
    let key = (plan.episode_id.clone(), segment.id.clone());
    let before = segment.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label("Use last frame from:");
        let label = segment
            .last_frame_from
            .as_ref()
            .map(|id| {
                choices
                    .iter()
                    .find(|(s, _)| s == id)
                    .map(|(_, label)| label.as_str())
                    .unwrap_or("Source unavailable")
            })
            .unwrap_or("None");
        let mut source = segment.last_frame_from.clone();
        eframe::egui::ComboBox::from_id_salt(("last-frame-source", &key))
            .selected_text(label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut source, None, "None");
                for (id, label) in &choices {
                    ui.selectable_value(&mut source, Some(id.clone()), label);
                }
            });
        let source_changed = source != segment.last_frame_from;
        if source_changed {
            set_source(segment, source);
            segment.frame_take = segment.last_frame_from.as_ref().and_then(|source| {
                store
                    .selected_segment_take(&plan.episode_id, source)
                    .ok()
                    .flatten()
            });
        }
        let mut take = segment
            .frame_take
            .clone()
            .or_else(|| segment.frame_source_asset.clone());
        if let Some(source) = &segment.last_frame_from {
            let jobs = store.segment_generation_jobs(&plan.episode_id, source);
            match jobs {
                Ok(jobs) => {
                    let takes: Vec<_> = jobs
                        .iter()
                        .rev()
                        .enumerate()
                        .filter(|(_, job)| job["status"] == "completed")
                        .filter_map(|(n, job)| {
                            job["asset_id"]
                                .as_str()
                                .map(|id| (id.to_owned(), format!("Take {:02}", n + 1)))
                        })
                        .collect();
                    let label = take
                        .as_ref()
                        .and_then(|id| takes.iter().find(|(asset, _)| asset == id))
                        .map(|(_, label)| label.as_str())
                        .unwrap_or("Choose take");
                    eframe::egui::ComboBox::from_id_salt(("last-frame-take", &key))
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (id, label) in &takes {
                                ui.selectable_value(&mut take, Some(id.clone()), label);
                            }
                            if takes.is_empty() {
                                ui.label("No completed takes available");
                            }
                        });
                }
                Err(error) => {
                    ui.label(error.to_string());
                }
            }
        }
        let take_changed = take != segment.frame_take;
        let refresh = segment.last_frame_from.is_some()
            && ui
                .add_enabled(take.is_some(), eframe::egui::Button::new("Refresh frame"))
                .clicked();
        if source_changed || take_changed || refresh {
            let source = segment.last_frame_from.clone();
            set_source(segment, source);
            segment.frame_take = take;
            state.status.remove(&key);
            state.previews.remove(&key);
            state
                .undo
                .insert(key.clone(), (before.clone(), segment.clone()));
            state.queue.retain(|queued| queued != &key);
            if segment.last_frame_from.is_some() && segment.frame_take.is_some() {
                state.queue.push_back(key.clone());
            }
        }
        if let Some((previous, expected)) = state.undo.get(&key).cloned()
            && ui
                .add_enabled(
                    *segment == expected,
                    eframe::egui::Button::new("Undo frame change"),
                )
                .clicked()
        {
            *segment = previous;
            state.undo.remove(&key);
            state.status.remove(&key);
            state.queue.retain(|queued| queued != &key);
        }
    });
    if segment.last_frame_from.is_some() {
        ui.weak("Choose the exact take to use. This does not change the take selected for your episode.");
        if segment.frame_take.is_none() && segment.frame_source_asset.is_none() {
            ui.label("Choose a take above to see its last frame.");
        }
        let preview = state
            .previews
            .get(&key)
            .filter(|(expected, _)| expected == segment)
            .map(|(_, path)| path.clone())
            .or_else(|| {
                (!segment.frame_preview.is_empty()).then(|| PathBuf::from(&segment.frame_preview))
            });
        if let Some(path) = preview {
            ui.label(if segment.first.is_empty() {
                "Last frame preview"
            } else {
                "Starting frame ready"
            });
            let response = ui.add(
                eframe::egui::Image::from_uri(format!("file://{}", path.display()))
                    .max_height(340.)
                    .max_width(480.)
                    .sense(eframe::egui::Sense::click()),
            );
            if response.on_hover_text("Click to enlarge").clicked() {
                state.open_preview = Some(path);
            }
        }
    }
    if let Some(path) = state.open_preview.clone() {
        let mut open = true;
        eframe::egui::Window::new("Starting frame preview")
            .open(&mut open)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.add(
                    eframe::egui::Image::from_uri(format!("file://{}", path.display()))
                        .max_height(760.)
                        .max_width(1000.),
                );
            });
        if !open {
            state.open_preview = None;
        }
    }
    if state.pending.as_ref().is_some_and(|task| task.key == key) || state.queue.contains(&key) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Preparing starting frame...");
        });
    }
    if let Some(message) = state.status.get(&key)
        && !message.starts_with("Starting frame ready.")
    {
        ui.label(message);
    }
}

pub(crate) fn extract_last_frame(ffmpeg: &Path, source: &Path, destination: &Path) -> Result<()> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // Decode in presentation order and retain the final actual frame, not duration
    // minus an estimated frame interval (which breaks with variable frame rate).
    let mut child = Command::new(ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-fps_mode",
            "passthrough",
            "-update",
            "1",
            "-y",
        ])
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not start frame extraction")?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                ensure!(
                    status.success() && destination.is_file(),
                    "Could not extract the final video frame"
                );
                image::open(destination).context("Extracted frame is invalid")?;
                return Ok(());
            }
            Ok(None) if start.elapsed() < Duration::from_secs(90) => {
                std::thread::sleep(Duration::from_millis(30))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Frame extraction timed out");
            }
        }
    }
}
pub(crate) fn upload_error(status: u16, reply: &serde_json::Value) -> String {
    // Provider messages can echo signatures, API keys or uploaded input. Classify
    // the reason without placing raw response bodies in UI, drafts or logs.
    let message = reply
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let reason = if message.contains("cloud_name") || message.contains("cloud name") {
        "Cloudinary rejected the cloud name. In Settings, use the exact Cloud name from your Cloudinary dashboard, not the account or application name."
    } else if message.contains("api_key") || message.contains("api key") {
        "Cloudinary rejected the API key. Check that the key belongs to this cloud name in Settings."
    } else if message.contains("signature") {
        "Cloudinary rejected the upload signature. Check that the API secret matches the API key in Settings."
    } else if message.contains("timestamp") || message.contains("stale request") {
        "Cloudinary rejected the upload time. Check this Mac's date and time, then Refresh frame."
    } else if status == 413 || message.contains("file size") || message.contains("too large") {
        "Cloudinary rejected the image size. Check your account's upload limits."
    } else if status == 429 || message.contains("quota") || message.contains("limit exceeded") {
        "Cloudinary's account or rate limit was reached. Check account usage before retrying."
    } else if status == 401 || status == 403 {
        "Cloudinary did not authorize the upload. Check your saved credentials and key permissions in Settings."
    } else if status >= 500 {
        "Cloudinary is having a server problem. Try Refresh frame again later."
    } else {
        "Cloudinary rejected the image upload. Check your account's upload settings."
    };
    format!("{reason} (HTTP {status})")
}
pub(crate) fn upload_frame(path: &Path) -> Result<String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let credentials = crate::credentials::CloudinaryCredentials::load()?;
    let bytes = std::fs::read(path)?;
    ensure!(
        bytes.len() <= 20 * 1024 * 1024,
        "Frame exceeds the 20 MB upload limit"
    );
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let signature: String =
        Sha256::digest(format!("timestamp={timestamp}{}", credentials.api_secret))
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
    let mime = match image::guess_format(&bytes)? {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => bail!("Upload requires PNG, JPEG or WebP"),
    };
    let file = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent.post(format!("https://api.cloudinary.com/v1_1/{}/image/upload",credentials.cloud_name))
        .send_form([("file",file.as_str()),("api_key",credentials.api_key.as_str()),("timestamp",timestamp.as_str()),("signature",signature.as_str())])
        .map_err(|_|anyhow::anyhow!("Could not connect to Cloudinary or finish sending the frame. Check your internet connection, then Refresh frame."))?;
    let status = response.status().as_u16();
    let reply = response.body_mut().read_json::<serde_json::Value>();
    if !(200..300).contains(&status) {
        bail!(
            "{}",
            upload_error(status, &reply.unwrap_or(serde_json::Value::Null))
        );
    }
    let reply =
        reply.map_err(|_| anyhow::anyhow!("Cloudinary returned an invalid upload response"))?;
    let url = reply["secure_url"]
        .as_str()
        .context("Cloudinary did not return an image URL")?;
    ensure!(
        url.starts_with("https://res.cloudinary.com/"),
        "Cloudinary returned an unexpected delivery URL"
    );
    Ok(url.into())
}
fn prepare(
    store: &Store,
    asset: &crate::production::Asset,
    preview: &mpsc::Sender<PathBuf>,
    ctx: &eframe::egui::Context,
) -> Result<(String, PathBuf)> {
    ensure!(
        crate::worker::hash_file(&asset.path)? == asset.hash,
        "Source video changed; select an intact take"
    );
    let folder = store
        .root()
        .join("frame-handoffs")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&folder)?;
    let frame = folder.join("last-frame.png");
    extract_last_frame(&store.config()?.ffmpeg, &asset.path, &frame)?;
    ensure!(
        crate::worker::hash_file(&asset.path)? == asset.hash,
        "Source changed during extraction; frame was not uploaded"
    );
    let _ = preview.send(frame.clone());
    ctx.request_repaint();
    let url = upload_frame(&frame)?;
    Ok((url, frame))
}

impl crate::Studio {
    pub(crate) fn frame_handoff_progress(&mut self, ctx: &eframe::egui::Context) {
        let state = &mut self.video_planner.handoff;
        if let Some(task) = &state.pending
            && let Some(receiver) = &task.preview_receiver
            && let Ok(path) = receiver.try_recv()
        {
            state
                .previews
                .insert(task.key.clone(), (task.expected.clone(), path));
        }
        let outcome = state
            .pending
            .as_ref()
            .and_then(|task| match task.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Frame preparation stopped. Choose Refresh frame to retry.".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(outcome) = outcome {
            let task = state.pending.take().unwrap();
            if let Some(receiver) = &task.preview_receiver
                && let Ok(path) = receiver.try_recv()
            {
                state
                    .previews
                    .insert(task.key.clone(), (task.expected.clone(), path));
            }
            let result = (|| -> Result<()> {
                let (url, path) = outcome.map_err(anyhow::Error::msg)?;
                let plan = self
                    .video_planner
                    .drafts
                    .get_mut(&task.key.0)
                    .context("Destination draft closed; uploaded frame was not applied")?;
                earlier(
                    plan,
                    &task.key.1,
                    task.expected.last_frame_from.as_deref().unwrap(),
                )?;
                let segment = plan
                    .segments
                    .iter_mut()
                    .find(|s| s.id == task.key.1)
                    .context("Destination segment removed")?;
                ensure!(
                    *segment == task.expected
                        && plan.revision == task.revision
                        && self.store.generation_plan(&task.key.0)?.revision == task.saved_revision,
                    "Draft changed during frame preparation. Newer work was preserved; Refresh frame to retry."
                );
                ensure!(
                    chosen_take(&self.store, &task.key.0, &task.expected)?.as_ref()
                        == Some(&task.asset),
                    "Source take changed during preparation. Refresh frame to use its selected take."
                );
                if task.expected.frame_take.is_some() {
                    validate_take(
                        &self.store,
                        &task.key.0,
                        task.expected.last_frame_from.as_deref().unwrap(),
                        &task.asset,
                    )?;
                }
                segment.first = url;
                segment.frame_source_asset = Some(task.asset);
                segment.frame_preview = path.to_string_lossy().into_owned();
                if let Some((_, after)) = state.undo.get_mut(&task.key)
                    && *after == task.expected
                {
                    *after = segment.clone();
                }
                // Keep Sidekick's guarded Undo valid after this asynchronous part.
                if let Some((_, after)) = self.sidekick.generation_undo.get_mut(&task.key.0)
                    && let Some(s) = after.plan.segments.iter_mut().find(|s| s.id == task.key.1)
                    && *s == task.expected
                {
                    *s = segment.clone();
                }
                Ok(())
            })();
            let message = match result {
                Ok(()) => {
                    "Starting frame ready. Save the plan when ready; no video was generated.".into()
                }
                Err(e) => e.to_string(),
            };
            self.message = message.clone();
            state.status.insert(task.key, message);
        }
        if state.pending.is_none()
            && let Some(key) = state.queue.pop_front()
        {
            let setup = (|| -> Result<_> {
                let plan = self
                    .video_planner
                    .drafts
                    .get(&key.0)
                    .context("Destination draft unavailable")?;
                let segment = plan
                    .segments
                    .iter()
                    .find(|s| s.id == key.1)
                    .context("Destination segment unavailable")?;
                let asset = source_asset(&self.store, plan, segment)?;
                Ok((
                    segment.clone(),
                    plan.revision,
                    self.store.generation_plan(&key.0)?.revision,
                    asset,
                ))
            })();
            match setup {
                Ok((expected, revision, saved_revision, asset)) => {
                    let database = self.store.path.clone();
                    let (sender, receiver) = mpsc::channel();
                    let (preview_sender, preview_receiver) = mpsc::channel();
                    state.pending = Some(Pending {
                        key,
                        expected,
                        revision,
                        saved_revision,
                        asset: asset.id.clone(),
                        receiver,
                        preview_receiver: Some(preview_receiver),
                    });
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let result = Store::open(database)
                            .and_then(|store| prepare(&store, &asset, &preview_sender, &ctx))
                            .map_err(|e| e.to_string());
                        let _ = sender.send(result);
                        ctx.request_repaint();
                    });
                }
                Err(error) => {
                    self.message = error.to_string();
                    state.status.insert(key, error.to_string());
                }
            }
        }
        if state.pending.is_some() || !state.queue.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_failure_explains_reason_without_echoing_credentials() {
        let invalid_cloud = upload_error(
            401,
            &serde_json::json!({"error":{"message":"Invalid cloud_name DramaFactory"}}),
        );
        assert!(invalid_cloud.contains("cloud name"));
        assert!(invalid_cloud.contains("HTTP 401"));
        let signature = upload_error(
            401,
            &serde_json::json!({"error":{"message":"Invalid Signature secret-fixture, api_key=private-fixture"}}),
        );
        assert!(!signature.contains("fixture"));
        assert!(upload_error(503, &serde_json::Value::Null).contains("server problem"));
    }
    #[test]
    fn source_must_precede_destination_and_unprepared_frames_block_generation() {
        let first = SegmentDraft::default();
        let mut second = SegmentDraft::default();
        let mut plan = SavedPlan {
            episode_id: "episode".into(),
            revision: 0,
            segments: vec![first.clone(), second.clone()],
        };
        assert!(earlier(&plan, &second.id, &first.id).is_ok());
        assert!(earlier(&plan, &first.id, &second.id).is_err());
        assert!(earlier(&plan, &first.id, &first.id).is_err());
        assert!(earlier(&plan, &second.id, "other-episode").is_err());
        set_source(&mut second, Some(first.id.clone()));
        assert_eq!(second.mode, 2);
        assert!(second.video_plan("9:16").is_err());
        second.frame_source_asset = Some("source-take".into());
        second.first = "https://res.cloudinary.com/demo/image/upload/frame.png".into();
        assert!(second.video_plan("9:16").is_ok());
        plan.segments.swap(0, 1);
        assert!(earlier(&plan, &second.id, &first.id).is_err());
        set_source(&mut second, None);
        assert!(second.first.is_empty());
        assert!(second.frame_source_asset.is_none());
        assert!(second.video_plan("9:16").is_ok());
    }
    #[test]
    fn explicit_take_overrides_selected_take_and_rejects_other_segments() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let episode = store.create_episode(&series.id, "Episode").unwrap();
        let first = SegmentDraft::default();
        let mut second = SegmentDraft::default();
        set_source(&mut second, Some(first.id.clone()));
        second.frame_take = Some("take-two".into());
        second.frame_source_asset = second.frame_take.clone();
        second.first = "https://res.cloudinary.com/demo/image/upload/frame.png".into();
        let saved = store
            .save_generation_plan(&SavedPlan {
                episode_id: episode.id.clone(),
                revision: 0,
                segments: vec![first.clone(), second.clone()],
            })
            .unwrap();
        for id in ["take-one", "take-two"] {
            store
                .conn
                .execute(
                    "INSERT INTO jobs(id,prompt,status) VALUES(?1,'','completed')",
                    [id],
                )
                .unwrap();
            store.conn.execute("INSERT INTO assets VALUES(?1,?1,'/fixture.mp4','hash',64,64,1,'{}','unreviewed')",[id]).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO segment_generations VALUES(?1,?2,?3,1,'{}')",
                    rusqlite::params![id, episode.id, first.id],
                )
                .unwrap();
        }
        store
            .conn
            .execute(
                "INSERT INTO segment_takes VALUES(?1,?2,'take-one')",
                rusqlite::params![episode.id, first.id],
            )
            .unwrap();
        assert_eq!(
            source_asset(&store, &saved, &second).unwrap().id,
            "take-two"
        );
        assert!(store.validate_frame_handoff(&episode.id, &second).is_ok());
        store.conn.execute("DELETE FROM segment_takes", []).unwrap();
        assert!(store.validate_frame_handoff(&episode.id, &second).is_ok());
        store
            .conn
            .execute(
                "UPDATE segment_generations SET segment_id=?1 WHERE job_id='take-two'",
                [&second.id],
            )
            .unwrap();
        assert!(source_asset(&store, &saved, &second).is_err());
        assert!(store.validate_frame_handoff(&episode.id, &second).is_err());
    }
    #[test]
    fn late_frame_results_preserve_edits_and_changed_take_selections() {
        for scenario in [
            "ready",
            "edited",
            "take-changed",
            "reordered",
            "saved-elsewhere",
            "upload-failed",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::open(temp.path().join("studio.sqlite")).unwrap();
            let series = store.create_series("Fixture", "").unwrap();
            let episode = store.create_episode(&series.id, "Episode").unwrap();
            let first = SegmentDraft::default();
            let mut second = SegmentDraft::default();
            set_source(&mut second, Some(first.id.clone()));
            let saved = store
                .save_generation_plan(&SavedPlan {
                    episode_id: episode.id.clone(),
                    revision: 0,
                    segments: vec![first.clone(), second.clone()],
                })
                .unwrap();
            store.conn.execute("INSERT INTO assets VALUES('take','job','/fixture.mp4','hash',64,64,1,'{}','unreviewed')", []).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO segment_takes VALUES(?1,?2,'take')",
                    rusqlite::params![episode.id, first.id],
                )
                .unwrap();
            let key = (episode.id.clone(), second.id.clone());
            let mut app = crate::Studio::from_store(store);
            app.video_planner
                .drafts
                .insert(episode.id.clone(), saved.clone());
            let (sender, receiver) = mpsc::channel();
            let (preview_sender, preview_receiver) = mpsc::channel();
            app.video_planner.handoff.pending = Some(Pending {
                key: key.clone(),
                expected: second.clone(),
                revision: saved.revision,
                saved_revision: saved.revision,
                asset: "take".into(),
                receiver,
                preview_receiver: Some(preview_receiver),
            });
            match scenario {
                "edited" => {
                    app.video_planner
                        .drafts
                        .get_mut(&episode.id)
                        .unwrap()
                        .segments[1]
                        .prompt = "Newer user edit".into()
                }
                "take-changed" => {
                    app.store
                        .conn
                        .execute("DELETE FROM segment_takes", [])
                        .unwrap();
                }
                "reordered" => app
                    .video_planner
                    .drafts
                    .get_mut(&episode.id)
                    .unwrap()
                    .segments
                    .swap(0, 1),
                "saved-elsewhere" => {
                    let mut newer = saved.clone();
                    newer.segments[0].prompt = "Concurrent edit".into();
                    app.store.save_generation_plan(&newer).unwrap();
                }
                _ => {}
            }
            preview_sender.send(temp.path().join("frame.png")).unwrap();
            if scenario == "upload-failed" {
                sender.send(Err("Upload failed".into())).unwrap();
            } else {
                sender
                    .send(Ok((
                        "https://res.cloudinary.com/demo/image/upload/frame.png".into(),
                        temp.path().join("frame.png"),
                    )))
                    .unwrap();
            }
            app.frame_handoff_progress(&Default::default());
            let target = app.video_planner.drafts[&episode.id]
                .segments
                .iter()
                .find(|s| s.id == second.id)
                .unwrap();
            assert_eq!(!target.first.is_empty(), scenario == "ready", "{scenario}");
            if scenario == "upload-failed" {
                assert!(app.video_planner.handoff.previews.contains_key(&key));
                assert!(target.first.is_empty());
            }
            if scenario == "edited" {
                assert_eq!(target.prompt, "Newer user edit");
            }
            if scenario == "ready" {
                assert!(
                    app.store
                        .validate_frame_handoff(&episode.id, target)
                        .is_ok()
                );
                app.store
                    .conn
                    .execute("DELETE FROM segment_takes", [])
                    .unwrap();
                assert!(
                    app.store
                        .validate_frame_handoff(&episode.id, target)
                        .is_err()
                );
            }
            let jobs: i64 = app
                .store
                .conn
                .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
                .unwrap();
            assert_eq!(jobs, 0);
        }
    }
    #[test]
    fn extraction_uses_actual_last_frame_and_preserves_video() {
        let Ok(ffmpeg) = std::env::var("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("colors.mkv");
        let status = std::process::Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=64x64:r=5:d=0.4",
                "-f",
                "lavfi",
                "-i",
                "color=blue:s=64x64:r=5:d=0.4",
                "-filter_complex",
                "[0:v][1:v]concat=n=2:v=1:a=0",
                "-c:v",
                "ffv1",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let hash = crate::worker::hash_file(&video).unwrap();
        let frame = temp.path().join("last.png");
        extract_last_frame(Path::new(&ffmpeg), &video, &frame).unwrap();
        let image = image::open(&frame).unwrap().to_rgb8();
        let pixel = image.get_pixel(32, 32);
        assert!(
            pixel[2] > 240 && pixel[0] < 10,
            "Expected final blue frame, got {pixel:?}"
        );
        assert_eq!(crate::worker::hash_file(&video).unwrap(), hash);
    }
}
