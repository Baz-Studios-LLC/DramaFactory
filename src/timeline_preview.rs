//! Source-backed macOS timeline playback. No preview export or generated media.
use anyhow::{Context, Result, ensure};
use eframe::egui;
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
};

pub enum Player {
    Source(crate::media::Preview),
    Native(Native),
}
impl Player {
    pub fn playback_position(&self) -> f64 {
        match self {
            Self::Source(p) => p.playback_position(),
            Self::Native(p) => p.shared.lock().map_or(0., |s| s.time),
        }
    }
    pub fn is_paused(&self) -> bool {
        match self {
            Self::Source(p) => p.is_paused(),
            Self::Native(p) => p.paused,
        }
    }
    pub fn finished(&self) -> bool {
        match self {
            Self::Source(p) => p.finished(),
            Self::Native(p) => p.shared.lock().is_ok_and(|s| s.ended),
        }
    }
    pub fn toggle_playback(&mut self) -> Result<()> {
        match self {
            Self::Source(p) => p.toggle_playback(),
            Self::Native(p) => {
                p.send(serde_json::json!({"command":if p.paused {"play"} else {"pause"}}))?;
                p.paused = !p.paused;
                Ok(())
            }
        }
    }
    pub fn timeline_viewer(&mut self, ui: &mut egui::Ui, height: f32) -> (bool, bool) {
        match self {
            Self::Source(p) => p.timeline_viewer(ui, height),
            Self::Native(p) => p.ui(ui, height),
        }
    }
    pub fn update_mix(&mut self, manifest: serde_json::Value) -> Result<()> {
        if let Self::Native(p) = self {
            p.send(serde_json::json!({"command":"update", "manifest":manifest}))?;
        }
        Ok(())
    }
}
#[derive(Default)]
struct Frame {
    pixels: Option<(usize, usize, Vec<u8>)>,
    time: f64,
    ended: bool,
    error: String,
}
pub struct Native {
    child: Child,
    input: ChildStdin,
    shared: Arc<Mutex<Frame>>,
    texture: Option<egui::TextureHandle>,
    paused: bool,
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Native {
    #[cfg(target_os = "macos")]
    pub fn open(manifest: serde_json::Value) -> Result<Player> {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/timeline-player"));
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let directory = directories::ProjectDirs::from("studio", "BazStudios", "DramaFactory")
            .context("Application cache unavailable")?
            .cache_dir()
            .join("native-playback");
        std::fs::create_dir_all(&directory)?;
        let executable = directory.join(format!("timeline-{hash}"));
        if !executable.exists() {
            let mut temp = tempfile::NamedTempFile::new_in(&directory)?;
            temp.write_all(bytes)?;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o700))?;
            temp.persist(&executable)?;
        }
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child.stdin.take().context("Native player input missing")?;
        let mut output = child
            .stdout
            .take()
            .context("Native player output missing")?;
        let shared = Arc::new(Mutex::new(Frame::default()));
        let reader = shared.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                loop {
                    let mut header = [0u8; 24];
                    output.read_exact(&mut header)?;
                    let u32_at = |offset| {
                        u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap()) as usize
                    };
                    let (tag, count, width, height) = (u32_at(0), u32_at(4), u32_at(8), u32_at(12));
                    ensure!(count <= 16_000_000, "Native preview packet too large");
                    let time = f64::from_le_bytes(header[16..24].try_into().unwrap());
                    let mut payload = vec![0; count];
                    output.read_exact(&mut payload)?;
                    let mut frame = reader
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Preview lock unavailable"))?;
                    match tag {
                        1 => {
                            ensure!(
                                width > 0
                                    && height > 0
                                    && width <= 2048
                                    && height <= 2048
                                    && width * height * 4 == count,
                                "Invalid preview frame"
                            );
                            frame.pixels = Some((width, height, payload));
                            frame.time = time;
                        }
                        2 => {
                            frame.time = time;
                            frame.ended = true;
                        }
                        3 => frame.error = String::from_utf8_lossy(&payload).into_owned(),
                        4 => {
                            if time.is_finite() {
                                frame.time = time;
                            }
                        }
                        _ => anyhow::bail!("Unknown native preview packet"),
                    }
                }
            })();
            if let Err(error) = result
                && let Ok(mut frame) = reader.lock()
            {
                frame.error = format!("Native playback stopped: {error}");
            }
        });
        let mut player = Self {
            child,
            input,
            shared,
            texture: None,
            paused: false,
        };
        player.send(serde_json::json!({"command":"load", "manifest":manifest}))?;
        Ok(Player::Native(player))
    }
    #[cfg(not(target_os = "macos"))]
    pub fn open(_manifest: serde_json::Value) -> Result<Player> {
        anyhow::bail!("Direct timeline playback currently requires macOS")
    }
    fn send(&mut self, message: serde_json::Value) -> Result<()> {
        serde_json::to_writer(&mut self.input, &message)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        Ok(())
    }
    fn ui(&mut self, ui: &mut egui::Ui, height: f32) -> (bool, bool) {
        let (pixels, _time, _ended, error) = self
            .shared
            .lock()
            .map(|mut s| (s.pixels.take(), s.time, s.ended, s.error.clone()))
            .unwrap_or_default();
        if let Some((w, h, pixels)) = pixels {
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
            if let Some(texture) = &mut self.texture {
                texture.set(image, Default::default());
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "native-timeline",
                    image,
                    Default::default(),
                ));
            }
        }
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 4, egui::Color32::BLACK);
        if let Some(texture) = &self.texture {
            let aspect = texture.size()[0] as f32 / texture.size()[1] as f32;
            let h = rect.height().min(rect.width() / aspect);
            ui.painter().image(
                texture.id(),
                egui::Rect::from_center_size(rect.center(), egui::vec2(h * aspect, h)),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                egui::Color32::WHITE,
            );
        }
        if self.texture.is_none() && error.is_empty() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Opening source clips…",
                egui::FontId::proportional(13.),
                egui::Color32::GRAY,
            );
        }
        if !error.is_empty() {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
        (false, false)
    }
}
pub fn manifest(
    cut: &crate::editing_room::Cut,
    cues: &[crate::sound::AudioCue],
    assets: &[crate::production::Asset],
    aspect: &str,
    origin: f64,
) -> Result<serde_json::Value> {
    let path = |id: &str| -> Result<&std::path::Path> {
        Ok(&assets
            .iter()
            .find(|a| a.id == id)
            .context("Timeline source missing")?
            .path)
    };
    let clips = cut
        .clips
        .iter()
        .map(|clip| {
            let mut value = serde_json::to_value(clip)?;
            value["path"] = serde_json::to_value(path(&clip.asset_id)?)?;
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()?;
    let cues = cues
        .iter()
        .map(|cue| {
            let mut value = serde_json::to_value(cue)?;
            value["path"] = serde_json::to_value(path(&cue.asset_id)?)?;
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()?;
    let (width, height) = if aspect == "16:9" {
        (854, 480)
    } else {
        (270, 480)
    };
    Ok(serde_json::json!({"clips":clips,"cues":cues,"width":width,"height":height,"origin":origin}))
}
