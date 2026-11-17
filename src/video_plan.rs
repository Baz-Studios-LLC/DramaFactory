//! Prepare inspectable hosted requests without submitting or billing anything.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
// Curated reference-capable models. Runtime catalog validation remains authoritative.
pub struct ModelChoice {
    pub id: &'static str,
    pub name: &'static str,
    pub duration: u32,
    pub references: usize,
    pub minimum_resolution: &'static str,
}
pub const MODEL_CHOICES: &[ModelChoice] = &[
    ModelChoice {
        id: "bytedance/seedance-2.5",
        name: "Seedance 2.5",
        duration: 30,
        references: 12,
        minimum_resolution: "480p",
    },
    ModelChoice {
        id: "bytedance/seedance-2.0",
        name: "Seedance 2.0",
        duration: 15,
        references: 9,
        minimum_resolution: "480p",
    },
    ModelChoice {
        id: "alibaba/wan-3.0",
        name: "Wan 3.0",
        duration: 30,
        references: 10,
        minimum_resolution: "480p",
    },
    ModelChoice {
        id: "google/veo-3.1",
        name: "Google Veo 3.1",
        duration: 8,
        references: 3,
        minimum_resolution: "720p",
    },
];
pub fn model_choice(id: &str) -> Option<&'static ModelChoice> {
    MODEL_CHOICES.iter().find(|choice| choice.id == id)
}
fn validate_references(model: &str, count: usize) -> Result<()> {
    let limit = model_choice(model)
        .context("Choose a supported reference-image model")?
        .references;
    if count == 0 || count > limit {
        bail!("Choose 1–{limit} character references for this model");
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Inputs {
    Prompt,
    Portraits {
        portraits: Vec<crate::cast_reference::CastReference>,
        images: Vec<String>,
    },
    References {
        images: Vec<String>,
    },
    Frames {
        first: String,
        last: Option<String>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoPlan {
    pub model: String,
    pub prompt: String,
    pub duration: u32,
    #[serde(default)]
    pub automatic_duration: bool,
    pub resolution: String,
    pub aspect_ratio: String,
    pub generate_audio: bool,
    #[serde(default)]
    pub no_background_music: bool,
    #[serde(default)]
    pub ending_hold: bool,
    pub inputs: Inputs,
}
fn image_url(value: &str) -> Result<Value> {
    let url = url::Url::parse(value).context("Reference must have a valid HTTPS URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("References require HTTPS URLs without embedded credentials or fragments");
    }
    Ok(json!({"type":"image_url","image_url":{"url":value}}))
}
fn supported(model: &Value, field: &str, value: Value) -> Result<()> {
    if !model
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|values| values.contains(&value))
    {
        bail!("Selected model does not advertise this {field} value");
    }
    Ok(())
}
impl VideoPlan {
    pub fn prompt_limit(&self) -> Option<usize> {
        // Observed provider rejection: Wan 3.0 permits 20,000 characters.
        (self.model == "alibaba/wan-3.0").then_some(20_000)
    }
    pub fn assembled_prompt(&self) -> String {
        let mut prompt = self.prompt.clone();
        if let Inputs::Portraits { portraits, .. } = &self.inputs
            && !portraits.is_empty()
        {
            prompt.push_str("\n\nCHARACTER REFERENCE GUIDE\nUse the numbered images for character identity. Match named actions and dialogue to these characters; do not swap their faces.\n");
            for (index, portrait) in portraits.iter().enumerate() {
                prompt.push_str(&format!("Image {}: {}\n", index + 1, portrait.name));
            }
        }
        if self.no_background_music {
            prompt.push_str("\n\nAUDIO DIRECTION: Dialogue, natural environmental ambience, and story-relevant sound effects only. No background music, score, soundtrack, musical drones, ominous tonal beds, rhythmic underscore, stingers, or musical transitions. Do not turn ambience into music. This overrides any earlier request for background music or a score. Music will be added separately in editing. Preserve natural speech and breathing.");
        }
        if self.ending_hold {
            prompt.push_str("\n\nENDING HANDLE: Reserve the final one second of the clip for a natural, continuous hold after all dialogue and essential action finish. Let the characters settle or react subtly, maintaining the final framing, lighting and ambient sound. No new speech, action beat, camera cut, fade to black or end card during this second. Keep speech unhurried; this hold is included within the requested clip duration, not extra time after it.");
        }
        prompt
    }
    fn validate_prompt(&self) -> Result<()> {
        let count = self.assembled_prompt().chars().count();
        if let Some(limit) = self.prompt_limit()
            && count > limit
        {
            bail!(
                "Prompt is {count} characters; this model allows {limit}. Shorten the scene or character descriptions by {} characters.",
                count - limit
            );
        }
        Ok(())
    }

    pub fn preview(&self, catalog: &crate::openrouter::Catalog) -> Result<Value> {
        if self.prompt.trim().is_empty() || self.prompt.len() > 64 * 1024 {
            bail!("Write a prompt of at most 64 KiB");
        }
        self.validate_prompt()?;
        let model = catalog
            .videos
            .iter()
            .find(|model| model["id"] == self.model)
            .context("Video model is not in the current OpenRouter catalog")?;
        if !self.automatic_duration {
            supported(model, "supported_durations", json!(self.duration))?;
        }
        supported(model, "supported_resolutions", json!(self.resolution))?;
        supported(model, "supported_aspect_ratios", json!(self.aspect_ratio))?;
        if self.generate_audio && model["generate_audio"] != true {
            bail!("Selected model does not advertise generated audio");
        }
        let mut request = json!({"model":self.model,"prompt":self.assembled_prompt(),"duration":self.duration,"resolution":self.resolution,"aspect_ratio":self.aspect_ratio,"generate_audio":self.generate_audio});
        if self.automatic_duration {
            request
                .as_object_mut()
                .expect("request object")
                .remove("duration");
        }
        match &self.inputs {
            Inputs::Prompt => {}
            Inputs::Portraits { portraits, images } => {
                validate_references(&self.model, portraits.len() + images.len())?;
                let mut references = Vec::new();
                for portrait in portraits {
                    if portrait.name.trim().is_empty() || portrait.brief.len() > 64 * 1024 {
                        bail!("Invalid portrait identity information");
                    }
                    references.push(json!({"type":"image_url","image_url":{"url":format!("local-portrait:{}",portrait.sha256)}}));
                }
                for image in images {
                    references.push(image_url(image)?);
                }
                request["input_references"] = json!(references);
            }
            Inputs::References { images } => {
                validate_references(&self.model, images.len())?;
                request["input_references"] = Value::Array(
                    images
                        .iter()
                        .map(|image| image_url(image))
                        .collect::<Result<Vec<_>>>()?,
                );
            }
            Inputs::Frames { first, last } => {
                supported(model, "supported_frame_images", json!("first_frame"))?;
                let mut first = image_url(first)?;
                first["frame_type"] = json!("first_frame");
                let mut frames = vec![first];
                if let Some(last) = last {
                    supported(model, "supported_frame_images", json!("last_frame"))?;
                    let mut last = image_url(last)?;
                    last["frame_type"] = json!("last_frame");
                    frames.push(last);
                }
                request["frame_images"] = json!(frames);
            }
        }
        Ok(
            json!({"request":request,"catalog_fetched_unix_seconds":catalog.fetched_unix_seconds,"submitted":false,"provider_ready":false,"notes":["Preview only: no generation or billing request was made.","Provider-specific reference limits, media accessibility and input combinations still require endpoint validation.","Frame control and reference guidance are separate modes; no references are silently discarded.","No cost estimate is inferred from token prices without a verified usage formula."]}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (VideoPlan, crate::openrouter::Catalog) {
        (
            VideoPlan {
                model: "bytedance/seedance-2.5".into(),
                prompt: "An adult speaks while walking.".into(),
                duration: 10,
                automatic_duration: false,
                resolution: "720p".into(),
                aspect_ratio: "9:16".into(),
                generate_audio: true,
                no_background_music: false,
                ending_hold: false,
                inputs: Inputs::Prompt,
            },
            crate::openrouter::Catalog {
                videos: vec![
                    json!({"id":"bytedance/seedance-2.5","supported_durations":[10],"supported_resolutions":["720p"],"supported_aspect_ratios":["9:16"],"generate_audio":true,"supported_frame_images":["first_frame","last_frame"]}),
                ],
                images: vec![],
                fetched_unix_seconds: 1,
            },
        )
    }
    #[test]
    fn ending_handle_is_sent_but_legacy_requests_are_preserved() {
        let (mut plan, catalog) = fixture();
        let legacy = serde_json::to_value(&plan).unwrap();
        plan.ending_hold = true;
        let payload = plan.preview(&catalog).unwrap();
        assert!(
            payload["request"]["prompt"]
                .as_str()
                .unwrap()
                .contains("final one second")
        );
        assert_eq!(plan.duration, 10);
        let mut legacy = legacy;
        legacy.as_object_mut().unwrap().remove("ending_hold");
        let restored: VideoPlan = serde_json::from_value(legacy).unwrap();
        assert!(!restored.ending_hold);
        assert_eq!(restored.assembled_prompt(), restored.prompt);
    }
    #[test]
    fn music_rule_is_sent_and_counted_without_changing_legacy_requests() {
        let (mut plan, catalog) = fixture();
        let legacy = plan.assembled_prompt();
        plan.no_background_music = true;
        let text = plan.assembled_prompt();
        assert!(text.contains("No background music"));
        assert!(text.contains("Dialogue, natural environmental ambience"));
        assert_eq!(plan.preview(&catalog).unwrap()["request"]["prompt"], text);
        assert!(text.chars().count() > legacy.chars().count());
        plan.model = "alibaba/wan-3.0".into();
        plan.prompt = "a".repeat(20_000);
        assert!(plan.validate_prompt().is_err());
        let mut old = serde_json::to_value(&plan).unwrap();
        old.as_object_mut().unwrap().remove("no_background_music");
        let old: VideoPlan = serde_json::from_value(old).unwrap();
        assert!(!old.no_background_music);
        assert_eq!(old.assembled_prompt(), old.prompt);
    }
    #[test]
    fn wan_prompt_limit_counts_characters_and_reference_guide() {
        let (mut plan, _) = fixture();
        plan.model = "alibaba/wan-3.0".into();
        plan.prompt = "é".repeat(20_000);
        assert!(plan.validate_prompt().is_ok());
        plan.prompt.push('x');
        assert!(plan.validate_prompt().is_err());
        assert_eq!(plan.assembled_prompt().chars().count(), 20_001);
        plan.prompt = "Scene".into();
        plan.inputs = Inputs::Portraits {
            portraits: vec![crate::cast_reference::CastReference {
                character_id: "character".into(),
                name: "Sylvi".into(),
                brief: "a".repeat(20_000),
                path: "unused.png".into(),
                sha256: "unused".into(),
            }],
            images: vec![],
        };
        assert!(plan.assembled_prompt().contains("Image 1: Sylvi"));
        assert!(plan.validate_prompt().is_ok());
        plan.prompt = "é".repeat(19_999);
        assert!(plan.validate_prompt().is_err());
        plan.model = "google/veo-3.1".into();
        assert_eq!(plan.prompt_limit(), None);
    }
    #[test]
    fn automatic_duration_omits_parameter_and_legacy_keeps_it() {
        let (mut plan, catalog) = fixture();
        let mut legacy = serde_json::to_value(&plan).unwrap();
        legacy.as_object_mut().unwrap().remove("automatic_duration");
        let legacy: VideoPlan = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.preview(&catalog).unwrap()["request"]["duration"], 10);
        plan.automatic_duration = true;
        plan.duration = 19;
        assert!(
            plan.preview(&catalog).unwrap()["request"]
                .get("duration")
                .is_none()
        );
    }
    #[test]
    fn validates_capabilities_and_keeps_reference_modes_exclusive() {
        let (mut plan, catalog) = fixture();
        plan.inputs = Inputs::Frames {
            first: "https://example.com/start.png".into(),
            last: Some("https://example.com/end.png".into()),
        };
        let result = plan.preview(&catalog).unwrap();
        assert_eq!(
            result["request"]["frame_images"].as_array().unwrap().len(),
            2
        );
        assert!(result["request"].get("input_references").is_none());
        plan.inputs = Inputs::References {
            images: vec!["https://example.com/character.png".into()],
        };
        let result = plan.preview(&catalog).unwrap();
        assert!(result["request"].get("frame_images").is_none());
        plan.duration = 90;
        assert!(plan.preview(&catalog).is_err());
        plan.duration = 10;
        plan.inputs = Inputs::References {
            images: vec!["file:///private/image.png".into()],
        };
        assert!(plan.preview(&catalog).is_err());
        assert!(image_url("https://user:secret@example.com/image").is_err());
        assert!(serde_json::from_value::<Inputs>(json!({"mode":"frames","first":"https://example.com/a","last":null,"images":["ignored"]})).is_err());
        let mut unsupported = catalog.clone();
        unsupported.videos[0]["generate_audio"] = json!(false);
        plan.inputs = Inputs::Prompt;
        assert!(plan.preview(&unsupported).is_err());
    }
}
