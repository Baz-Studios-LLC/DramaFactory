//! One bounded source-frame decode at a time; rapid seeks coalesce to the latest cursor.
use eframe::egui;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, PartialEq)]
struct Request {
    path: PathBuf,
    frame: u64,
}
type FrameResult = (Request, Result<egui::ColorImage, String>);
#[derive(Default)]
pub struct Scrubber {
    pending: Option<mpsc::Receiver<FrameResult>>,
    cancel: Option<Arc<AtomicBool>>,
    last: Option<Request>,
    pub texture: Option<egui::TextureHandle>,
    pub error: Option<String>,
}
impl Drop for Scrubber {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
fn decode(
    ffmpeg: &Path,
    request: &Request,
    cancel: &AtomicBool,
) -> anyhow::Result<egui::ColorImage> {
    let output = std::env::temp_dir().join(format!("df-scrub-{}.png", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<egui::ColorImage> {
        let mut child = Command::new(ffmpeg)
            .args(["-v", "error", "-nostdin", "-ss"])
            .arg(format!("{:.6}", request.frame as f64 / 24.))
            .arg("-i")
            .arg(&request.path)
            .args(["-frames:v", "1", "-vf", "scale=288:-2", "-y"])
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) => anyhow::bail!("Frame unavailable"),
                Ok(None)
                    if started.elapsed() < Duration::from_secs(5)
                        && !cancel.load(Ordering::Relaxed) =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("Frame preview interrupted");
                }
            }
        }
        let image = image::open(&output)?.to_rgba8();
        Ok(egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ))
    })();
    let _ = std::fs::remove_file(output);
    result
}
impl Scrubber {
    pub fn request(&mut self, ctx: &egui::Context, ffmpeg: &Path, path: &Path, seconds: f64) {
        let wanted = Request {
            path: path.to_path_buf(),
            frame: (seconds.max(0.) * 24.).floor() as u64,
        };
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok((request, result)) => {
                    self.last = Some(request);
                    self.pending = None;
                    self.cancel = None;
                    match result {
                        Ok(image) => {
                            self.texture =
                                Some(ctx.load_texture("timeline-scrub", image, Default::default()));
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.cancel = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none() && self.last.as_ref() != Some(&wanted) {
            let ffmpeg = ffmpeg.to_path_buf();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            self.cancel = Some(cancel.clone());
            self.pending = Some(rx);
            std::thread::spawn(move || {
                let result = decode(&ffmpeg, &wanted, &cancel).map_err(|error| error.to_string());
                let _ = tx.send((wanted, result));
                ctx.request_repaint();
            });
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeks_real_source_and_cleans_up() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let path = std::env::temp_dir().join(format!("df-scrub-test-{}.mp4", uuid::Uuid::new_v4()));
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=red:s=64x64:d=1:r=24",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=blue:s=64x64:d=1:r=24",
                    "-filter_complex",
                    "[0:v][1:v]concat=n=2:v=1:a=0",
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
        let cancel = AtomicBool::new(false);
        for (frame, red) in [(6, true), (30, false)] {
            let image = decode(
                Path::new(&ffmpeg),
                &Request {
                    path: path.clone(),
                    frame,
                },
                &cancel,
            )
            .unwrap();
            let pixel = image.pixels[image.pixels.len() / 2];
            if red {
                assert!(pixel.r() > 200 && pixel.b() < 40);
            } else {
                assert!(pixel.b() > 200 && pixel.r() < 40);
            }
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(
            decode(
                Path::new(&ffmpeg),
                &Request {
                    path: path.clone(),
                    frame: 0
                },
                &cancel
            )
            .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
}
