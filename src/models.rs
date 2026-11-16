//! Shared file-presence diagnostics. Never equate installed files with runtime or quality approval.
use crate::store::Store;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
#[derive(Serialize)]
pub struct ModelFiles {
    pub id: &'static str,
    pub name: &'static str,
    pub configured: bool,
    pub path: PathBuf,
    pub operations: Vec<&'static str>,
    pub experimental: bool,
    pub missing_files: Vec<PathBuf>,
    pub readiness_error: Option<String>,
    pub readiness_scope: &'static str,
}
impl Store {
    pub fn model_files(&self) -> Result<Vec<ModelFiles>> {
        let c = self.config()?;
        let specs = [
            (
                "wan22-ti2v-5b-q8",
                "Wan 2.2 · TI2V 5B",
                c.model.clone(),
                vec![c.python.clone(), c.ffmpeg.clone()],
                vec![
                    "config.json",
                    "model.safetensors",
                    "t5_encoder.safetensors",
                    "vae.safetensors",
                ],
                vec!["text_to_video", "image_to_video"],
                false,
            ),
            (
                "flux2-klein-4b-q4",
                "FLUX.2 Klein · Q4",
                self.image_root()?,
                vec![c.image_python.clone()],
                vec![
                    "config.json",
                    "transformer/0.safetensors",
                    "text_encoder/0.safetensors",
                    "vae/0.safetensors",
                ],
                vec!["image_generation", "reference_editing"],
                false,
            ),
            (
                "kokoro-int8",
                "Kokoro · INT8",
                self.speech_root()?,
                vec![c.python.clone()],
                vec!["kokoro-v1.0.int8.onnx", "voices-v1.0.bin"],
                vec!["dialogue"],
                false,
            ),
            (
                "musetalk-q4-experimental",
                "MuseTalk · Q4 · Experimental",
                self.lipsync_root()?,
                vec![c.lipsync_python.clone(), c.ffmpeg.clone()],
                vec![
                    "config.json",
                    "unet.safetensors",
                    "vae.safetensors",
                    "whisper_encoder.safetensors",
                    "face_landmarker.task",
                ],
                vec!["lip_sync"],
                true,
            ),
        ];
        let mut result = Vec::new();
        for (id, name, path, mut files, components, operations, experimental) in specs {
            files.extend(components.into_iter().map(|file| path.join(file)));
            let missing_files: Vec<_> = files.into_iter().filter(|p| !p.is_file()).collect();
            let readiness_error = (!missing_files.is_empty()).then(|| {
                format!(
                    "{} required file(s) missing; configure the worker and model paths",
                    missing_files.len()
                )
            });
            result.push(ModelFiles{id,name,configured:missing_files.is_empty(),path,operations,experimental,missing_files,readiness_error,readiness_scope:"file_presence_only; Python imports, executable health, checkpoint integrity and generation quality are not verified"});
        }
        let readiness = crate::performance::validate_setup(&c);
        result.push(ModelFiles{id:"ltx-2.3-q8-performance-experimental",name:"LTX 2.3 · native acting · Experimental",configured:readiness.is_ok(),path:c.performance_model,operations:vec!["image_audio_to_video"],experimental:true,missing_files:vec![],readiness_error:readiness.err().map(|e|e.to_string()),readiness_scope:"local component structure only; runtime and acting/synchronization quality require separate tests"});
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_directories_do_not_report_models_configured() {
        let root = std::env::temp_dir().join(format!("df-model-files-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let python = root.join("python-fixture");
        std::fs::write(&python, b"not a runtime").unwrap();
        let mut c = store.config().unwrap();
        c.python = python.clone();
        c.image_python = python.clone();
        c.lipsync_python = python.clone();
        c.ffmpeg = python;
        store.save_config(&c).unwrap();
        for dir in [
            store.image_root().unwrap(),
            store.speech_root().unwrap(),
            store.lipsync_root().unwrap(),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let status = store.model_files().unwrap();
        assert!(status.iter().all(|s| !s.configured));
        let image = status.iter().find(|s| s.id == "flux2-klein-4b-q4").unwrap();
        assert_eq!(image.missing_files.len(), 4);
        for path in &image.missing_files {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"explicit file-presence fixture").unwrap();
        }
        let status = store.model_files().unwrap();
        let image = status.iter().find(|s| s.id == "flux2-klein-4b-q4").unwrap();
        assert!(image.configured && image.readiness_scope.contains("not verified"));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
