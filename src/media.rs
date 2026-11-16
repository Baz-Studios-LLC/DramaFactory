use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command};
#[derive(Debug, Serialize, Deserialize)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub fps: f64,
    pub audio: bool,
}
pub fn probe(ffmpeg: &Path, path: &Path) -> Result<VideoInfo> {
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .output()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let d = regex::Regex::new(r"Duration: (\d+):(\d+):(\d+(?:\.\d+)?)")?;
    let dur = d
        .captures(&text)
        .context("Media has no readable duration")?;
    let video = text
        .lines()
        .find(|l| l.contains("Video:"))
        .context("No video stream found")?;
    let dims = regex::Regex::new(r"\b(\d{2,5})x(\d{2,5})\b")?;
    let dims = dims.captures(video).context("No video dimensions found")?;
    let fps = regex::Regex::new(r"(\d+(?:\.\d+)?) fps")?;
    let fps = fps.captures(video).context("No frame rate found")?;
    Ok(VideoInfo {
        width: dims[1].parse()?,
        height: dims[2].parse()?,
        duration: dur[1].parse::<f64>()? * 3600.
            + dur[2].parse::<f64>()? * 60.
            + dur[3].parse::<f64>()?,
        fps: fps[1].parse()?,
        audio: text.contains("Audio:"),
    })
}
pub use crate::preview::Preview;
