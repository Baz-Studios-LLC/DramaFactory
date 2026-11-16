//! Explicit import/executable diagnostics, separate from checkpoint and quality checks.
use crate::production::WorkerConfig;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Deserialize)]
pub struct ImportCheck {
    pub module: String,
    pub version: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PythonProbe {
    pub python_version: String,
    pub architecture: String,
    pub checks: Vec<ImportCheck>,
}
#[derive(Clone, Serialize)]
pub struct RuntimeReport {
    pub model_id: String,
    pub python: PathBuf,
    pub passed: bool,
    pub elapsed_seconds: f64,
    pub imports: Option<PythonProbe>,
    pub ffmpeg: Option<String>,
    pub errors: Vec<String>,
    pub scope: &'static str,
}
fn bounded_output(mut command: Command, timeout: Duration) -> Result<Vec<u8>> {
    // A temporary output file avoids waiting forever on an inherited stdout
    // pipe if an unhealthy executable leaves a descendant running.
    struct Capture(PathBuf);
    impl Drop for Capture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let capture =
        Capture(std::env::temp_dir().join(format!("df-runtime-{}.log", uuid::Uuid::new_v4())));
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&capture.0)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
        let oversized = std::fs::metadata(&capture.0).is_ok_and(|meta| meta.len() > 65536);
        if oversized || started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            if oversized {
                bail!("Runtime diagnostic output exceeded 64 KiB");
            }
            bail!(
                "Runtime check timed out after {} seconds",
                timeout.as_secs_f64()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        bail!("Diagnostic process exited with {status}; check the executable and its environment");
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&capture.0)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        bail!("Runtime diagnostic output exceeded 64 KiB");
    }
    Ok(bytes)
}

pub fn check(config: &WorkerConfig, id: &str) -> Result<RuntimeReport> {
    let (python, specs, needs_ffmpeg) = match id {
        "wan22-ti2v-5b-q8" => (
            &config.python,
            vec![
                ("mlx.core", "mlx", "array"),
                (
                    "mlx_video.models.wan_2.generate",
                    "mlx-video",
                    "generate_video",
                ),
            ],
            true,
        ),
        "kokoro-int8" => (
            &config.python,
            vec![
                ("kokoro_onnx", "kokoro-onnx", "Kokoro"),
                ("soundfile", "soundfile", "write"),
                ("onnxruntime", "onnxruntime", "InferenceSession"),
            ],
            false,
        ),
        "flux2-klein-4b-q4" => (
            &config.image_python,
            vec![
                ("mlx.core", "mlx", "array"),
                ("mflux.models.flux2.variants", "mflux", "Flux2Klein"),
                ("mflux.models.flux2.variants", "mflux", "Flux2KleinEdit"),
            ],
            false,
        ),
        "musetalk-q4-experimental" => (
            &config.lipsync_python,
            vec![
                ("mlx.core", "mlx", "array"),
                (
                    "musetalk_mlx.pipeline_mlx",
                    "musetalk-mlx",
                    "MuseTalkPipeline",
                ),
                ("mediapipe", "mediapipe", "tasks"),
                ("cv2", "opencv-python", "VideoCapture"),
                ("soundfile", "soundfile", "info"),
            ],
            true,
        ),
        "ltx-2.3-q8-performance-experimental" => (
            &config.performance_python,
            vec![
                ("mlx.core", "mlx", "array"),
                (
                    "ltx_pipelines_mlx",
                    "ltx-pipelines-mlx",
                    "A2VidDistilledPipeline",
                ),
            ],
            true,
        ),
        _ => bail!("Unknown model adapter: {id}"),
    };
    let start = Instant::now();
    let mut report = RuntimeReport {
        model_id: id.into(),
        python: python.clone(),
        passed: false,
        elapsed_seconds: 0.,
        imports: None,
        ffmpeg: None,
        errors: vec![],
        scope: "Import/executable check only. No checkpoint loaded, integrity check, generation, download, or acting/synchronization approval. Versions are reported, not certified compatible.",
    };
    let mut command = Command::new(python);
    command
        .args([
            "-I",
            "-c",
            include_str!("../workers/diagnostics.py"),
            &serde_json::to_string(&specs)?,
        ])
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("TOKENIZERS_PARALLELISM", "false");
    match bounded_output(command, Duration::from_secs(20))
        .and_then(|raw| Ok(serde_json::from_slice::<PythonProbe>(&raw)?))
    {
        Ok(probe) => {
            for item in &probe.checks {
                if let Some(error) = &item.error {
                    report.errors.push(format!("{}: {error}", item.module));
                }
            }
            if probe.checks.len() != specs.len() {
                report.errors.push("Incomplete Python import report".into());
            }
            report.imports = Some(probe);
        }
        Err(error) => report.errors.push(format!("Python check: {error}")),
    }
    if needs_ffmpeg {
        let mut command = Command::new(&config.ffmpeg);
        command.arg("-version");
        match bounded_output(command, Duration::from_secs(5)) {
            Ok(raw) => {
                let text = String::from_utf8_lossy(&raw);
                let first = text.lines().next().unwrap_or("");
                if first.starts_with("ffmpeg version ") {
                    report.ffmpeg = Some(first.chars().take(300).collect());
                } else {
                    report
                        .errors
                        .push("Executable did not identify itself as FFmpeg".into());
                }
            }
            Err(error) => report.errors.push(format!("FFmpeg check: {error}")),
        }
    }
    report.passed = report.errors.is_empty();
    report.elapsed_seconds = start.elapsed().as_secs_f64();
    Ok(report)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn unhealthy_executables_cannot_hang_or_flood_diagnostics() {
        let start = Instant::now();
        let mut slow = Command::new("/bin/sh");
        slow.args(["-c", "exec sleep 5"]);
        assert!(
            bounded_output(slow, Duration::from_millis(80))
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut noisy = Command::new("/bin/sh");
        noisy.args(["-c","while :; do printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'; done"]);
        assert!(
            bounded_output(noisy, Duration::from_secs(2))
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );
        let mut failed = Command::new("/bin/sh");
        failed.args(["-c", "exit 7"]);
        assert!(
            bounded_output(failed, Duration::from_secs(1))
                .unwrap_err()
                .to_string()
                .contains("exit status: 7")
        );
        assert!(check(&WorkerConfig::default(), "not-an-adapter").is_err());
    }
}
