//! Bounded latest-frame preview with explicit decoder lifetime and terminal states.
//! System player remains the full-resolution synchronized review path.
use anyhow::{Context, Result, bail};
use std::{
    io::{BufReader, Read},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone, Debug, PartialEq)]
enum State {
    Loading,
    Playing,
    Ended,
    Failed(String),
}
struct Frames {
    latest: Option<Vec<u8>>,
    count: u64,
    last: Instant,
    state: State,
}
pub struct Preview {
    ffmpeg: PathBuf,
    path: PathBuf,
    aspect: f32,
    with_audio: bool,
    paused: Arc<AtomicBool>,
    width: usize,
    height: usize,
    texture: Option<eframe::egui::TextureHandle>,
    shared: Arc<Mutex<Frames>>,
    decoder: Arc<Mutex<Child>>,
    stop: Arc<AtomicBool>,
    audio: Option<Child>,
    audio_path: Option<PathBuf>,
}
fn terminate(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        terminate(&self.decoder);
        self.stop_audio();
    }
}
impl Preview {
    pub fn open(ffmpeg: PathBuf, path: PathBuf, aspect: f32, with_audio: bool) -> Result<Self> {
        if !path.is_file() {
            bail!("Preview file is missing");
        }
        if !aspect.is_finite() || aspect <= 0. {
            bail!("Preview aspect ratio must be positive and finite");
        }
        let height = 480;
        let width = ((height as f32 * aspect / 2.).round() as usize)
            .saturating_mul(2)
            .clamp(2, 854);
        let mut child = Command::new(&ffmpeg)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&path)
            .args([
                "-an",
                "-vf",
                &format!("scale={width}:{height},fps=24"),
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let pipe = match child.stdout.take().context("Preview stream missing") {
            Ok(pipe) => pipe,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let decoder = Arc::new(Mutex::new(child));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Frames {
            latest: None,
            count: 0,
            last: Instant::now(),
            state: State::Loading,
        }));
        let (frames, stopped) = (shared.clone(), stop.clone());
        let paused = Arc::new(AtomicBool::new(false));
        let reader_paused = paused.clone();
        let monitor_paused = paused.clone();
        let reader_done = Arc::new(AtomicBool::new(false));
        let done = reader_done.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut clock = None;
            let mut index = 0u64;
            let mut pause_time = Duration::ZERO;
            while !stopped.load(Ordering::Relaxed) {
                let mut bytes = vec![0; width * height * 3];
                if reader.read_exact(&mut bytes).is_err() {
                    break;
                }
                let started = *clock.get_or_insert_with(Instant::now);
                loop {
                    if stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    if reader_paused.load(Ordering::Relaxed) {
                        let pause_start = Instant::now();
                        while reader_paused.load(Ordering::Relaxed)
                            && !stopped.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        pause_time += pause_start.elapsed();
                    }
                    let due = started + pause_time + Duration::from_secs_f64(index as f64 / 24.);
                    if Instant::now() >= due {
                        break;
                    }
                    std::thread::sleep(
                        due.saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(10)),
                    );
                }
                if stopped.load(Ordering::Relaxed) {
                    break;
                }
                index += 1;
                if let Ok(mut frames) = frames.lock() {
                    frames.latest = Some(bytes);
                    frames.count += 1;
                    frames.last = Instant::now();
                    if frames.state == State::Loading {
                        frames.state = State::Playing;
                    }
                }
            }
            done.store(true, Ordering::Release);
        });
        let (process, frames, stopped) = (decoder.clone(), shared.clone(), stop.clone());
        std::thread::spawn(move || {
            loop {
                if stopped.load(Ordering::Relaxed) {
                    break;
                }
                let result = process
                    .lock()
                    .map_err(|_| "Preview process lock failed".to_string())
                    .and_then(|mut process| process.try_wait().map_err(|e| e.to_string()));
                match result {
                    Ok(Some(status)) => {
                        // The pipe can still contain the final decoded frame after process exit.
                        while !reader_done.load(Ordering::Acquire)
                            && !stopped.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        if let Ok(mut frames) = frames.lock() {
                            frames.state = if status.success() && frames.count > 0 {
                                State::Ended
                            } else {
                                State::Failed(format!(
                                    "Preview decoder exited with {status}. Open the original in the system player or check the media and FFmpeg configuration."
                                ))
                            };
                        }
                        break;
                    }
                    Err(error) => {
                        if let Ok(mut frames) = frames.lock() {
                            frames.state = State::Failed(error);
                        }
                        terminate(&process);
                        break;
                    }
                    Ok(None) => {}
                }
                let timed_out = frames
                    .lock()
                    .is_ok_and(|frames| frames.last.elapsed() > Duration::from_secs(15));
                if timed_out && !monitor_paused.load(Ordering::Relaxed) {
                    terminate(&process);
                    if let Ok(mut frames) = frames.lock() {
                        frames.state=State::Failed("Preview decoder stopped delivering frames. Check the media or open it in the system player.".into());
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Ok(Self {
            ffmpeg,
            path: path.clone(),
            aspect,
            with_audio,
            paused,
            width,
            height,
            texture: None,
            shared,
            decoder,
            stop,
            audio: None,
            audio_path: with_audio.then_some(path),
        })
    }
    pub fn playback_position(&self) -> f64 {
        self.shared
            .lock()
            .map_or(0., |frames| frames.count as f64 / 24.)
    }
    pub fn finished(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|frames| frames.state == State::Ended)
    }
    pub fn toggle_playback(&mut self) -> Result<()> {
        let ended = self
            .shared
            .lock()
            .is_ok_and(|frames| frames.state == State::Ended);
        if ended {
            *self = Self::open(
                self.ffmpeg.clone(),
                self.path.clone(),
                self.aspect,
                self.with_audio,
            )?;
            return Ok(());
        }
        let was_paused = self.paused.load(Ordering::Relaxed);
        self.paused.store(!was_paused, Ordering::Relaxed);
        if let Ok(mut frames) = self.shared.lock() {
            frames.last = Instant::now();
        }
        #[cfg(unix)]
        if let Some(audio) = &self.audio {
            unsafe {
                libc::kill(
                    audio.id() as i32,
                    if was_paused {
                        libc::SIGCONT
                    } else {
                        libc::SIGSTOP
                    },
                );
            }
        }
        Ok(())
    }
    fn stop_audio(&mut self) {
        if let Some(mut audio) = self.audio.take() {
            let _ = audio.kill();
            let _ = audio.wait();
        }
    }
    fn poll(&self) -> (Option<Vec<u8>>, State) {
        match self.shared.lock() {
            Ok(mut frames) => (frames.latest.take(), frames.state.clone()),
            Err(_) => (
                None,
                State::Failed("Preview state unavailable; reopen the take.".into()),
            ),
        }
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        self.draw(ui, None);
    }
    pub fn viewer(&mut self, ui: &mut eframe::egui::Ui, height: f32) {
        self.draw(ui, Some(height));
    }
    fn draw(&mut self, ui: &mut eframe::egui::Ui, viewer_height: Option<f32>) {
        let (latest, state) = self.poll();
        if let Some(frame) = latest {
            let image = eframe::egui::ColorImage::from_rgb([self.width, self.height], &frame);
            if let Some(texture) = &mut self.texture {
                texture.set(image, Default::default());
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "video-preview",
                    image,
                    Default::default(),
                ));
            }
            // Start sound only when a first decoded frame can be shown. These remain
            // separate clocks; this is not a frame-accurate lip-sync review instrument.
            if let Some(path) = self.audio_path.take() {
                #[cfg(target_os = "macos")]
                if state == State::Playing {
                    match Command::new("/usr/bin/afplay")
                        .arg(path)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(audio) => self.audio = Some(audio),
                        Err(error) => {
                            ui.label(format!("Preview sound unavailable: {error}"));
                        }
                    }
                }
                #[cfg(not(target_os = "macos"))]
                let _ = path;
            }
        }
        if let Some(texture) = &self.texture {
            let response = if let Some(height) = viewer_height {
                let (rect, response) = ui.allocate_exact_size(
                    eframe::egui::vec2(ui.available_width(), height),
                    eframe::egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(rect, 4, eframe::egui::Color32::BLACK);
                let aspect = self.width as f32 / self.height as f32;
                let height = height.min(rect.width() / aspect);
                let image_rect = eframe::egui::Rect::from_center_size(
                    rect.center(),
                    eframe::egui::vec2(height * aspect, height),
                );
                ui.painter().image(
                    texture.id(),
                    image_rect,
                    eframe::egui::Rect::from_min_max(
                        eframe::egui::pos2(0., 0.),
                        eframe::egui::pos2(1., 1.),
                    ),
                    eframe::egui::Color32::WHITE,
                );
                response
            } else {
                let width = (self.width as f32).min(ui.available_width());
                ui.image((
                    texture.id(),
                    eframe::egui::Vec2::new(width, width * self.height as f32 / self.width as f32),
                ))
            };
            let rect = response.rect;
            let paused = self.paused.load(Ordering::Relaxed);
            if response.hovered()
                || paused
                || state == State::Ended
                || ui.rect_contains_pointer(rect)
            {
                let bar = eframe::egui::Rect::from_min_max(
                    eframe::egui::pos2(rect.left(), rect.bottom() - 42.),
                    rect.max,
                );
                ui.painter()
                    .rect_filled(bar, 0, eframe::egui::Color32::from_black_alpha(210));
                let mut replay = false;
                // Independent overlay: controls must never advance or expand the viewer layout.
                {
                    let mut overlay = ui.new_child(
                        eframe::egui::UiBuilder::new()
                            .id_salt("playback-overlay")
                            .max_rect(bar.shrink(5.)),
                    );
                    overlay.set_clip_rect(bar.intersect(ui.clip_rect()));
                    let ui = &mut overlay;
                    ui.spacing_mut().button_padding = eframe::egui::vec2(6., 4.);
                    ui.horizontal(|ui| {
                        if state == State::Ended {
                            replay = ui.button("▶").on_hover_text("Replay").clicked();
                        } else if ui
                            .button(if paused { "▶" } else { "⏸" })
                            .on_hover_text(if paused { "Play" } else { "Pause" })
                            .clicked()
                            && let Err(error) = self.toggle_playback()
                        {
                            ui.label(error.to_string());
                        }
                        if state != State::Ended {
                            replay = ui.button("↺").on_hover_text("Replay").clicked();
                        }
                        let seconds = self.shared.lock().map_or(0, |f| f.count / 24);
                        ui.label(format!("{}:{:02}", seconds / 60, seconds % 60));
                        if ui
                            .button("↗")
                            .on_hover_text("Open in system player")
                            .clicked()
                        {
                            let _ = Command::new("/usr/bin/open").arg(&self.path).spawn();
                        }
                    });
                }
                if replay {
                    match Self::open(
                        self.ffmpeg.clone(),
                        self.path.clone(),
                        self.aspect,
                        self.with_audio,
                    ) {
                        Ok(player) => {
                            *self = player;
                            return;
                        }
                        Err(error) => {
                            ui.label(error.to_string());
                        }
                    }
                }
            }
        }
        match state {
            State::Loading => {
                ui.spinner();
            }
            State::Playing => {}
            State::Ended => {
                self.stop_audio();
            }
            State::Failed(error) => {
                self.stop_audio();
                ui.label(eframe::egui::RichText::new(error).color(crate::GOLD));
            }
        }

        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn failure_and_close_do_not_leave_spinners_or_blocked_decoders() {
        let root = std::env::temp_dir().join(format!("df-preview-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("fixture.media");
        std::fs::write(&source, b"explicit lifecycle fixture").unwrap();
        assert!(Preview::open("/usr/bin/false".into(), source.clone(), f32::NAN, false).is_err());
        let failed = Preview::open("/usr/bin/false".into(), source.clone(), 1., false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(failed.poll().1, State::Failed(_)) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(failed);
        let stalled = root.join("stalled-decoder");
        std::fs::write(&stalled, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&stalled, std::fs::Permissions::from_mode(0o700)).unwrap();
        let preview = Preview::open(stalled, source, 1., false).unwrap();
        let pid = preview.decoder.lock().unwrap().id();
        let start = Instant::now();
        drop(preview);
        assert!(start.elapsed() < Duration::from_secs(1));
        // The actual blocked child has been reaped, not merely signaled via a flag.
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn actual_ffmpeg_delivers_final_frame_and_end_state() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            eprintln!("Set DRAMAFACTORY_TEST_FFMPEG for actual decoder integration");
            return;
        };
        let path = std::env::temp_dir().join(format!("df-preview-{}.mp4", uuid::Uuid::new_v4()));
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=size=64x64:rate=24:duration=0.25",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let started = Instant::now();
        let mut preview = Preview::open(ffmpeg.into(), path.clone(), 1., false).unwrap();
        preview.toggle_playback().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let count = preview.shared.lock().unwrap().count;
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(preview.shared.lock().unwrap().count, count);
        assert_ne!(preview.poll().1, State::Ended);
        preview.toggle_playback().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = preview.shared.lock().unwrap().state.clone();
            match state {
                State::Ended => break,
                State::Failed(error) => panic!("{error}"),
                _ => {}
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started.elapsed() >= Duration::from_millis(180));
        let (frame, state) = preview.poll();
        assert_eq!(state, State::Ended);
        assert_eq!(preview.shared.lock().unwrap().count, 6);
        assert_eq!(frame.unwrap().len(), 480 * 480 * 3);
        assert!(preview.poll().0.is_none());
        drop(preview);
        std::fs::remove_file(path).unwrap();
    }
}
