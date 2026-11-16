//! Prepare inspectable hosted requests without submitting or billing anything.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    pub fn preview(&self, catalog: &crate::openrouter::Catalog) -> Result<Value> {
        if self.prompt.trim().is_empty() || self.prompt.len() > 64 * 1024 {
            bail!("Write a prompt of at most 64 KiB");
        }
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
        let mut request = json!({"model":self.model,"prompt":self.prompt,"duration":self.duration,"resolution":self.resolution,"aspect_ratio":self.aspect_ratio,"generate_audio":self.generate_audio});
        if self.automatic_duration {
            request
                .as_object_mut()
                .expect("request object")
                .remove("duration");
        }
        match &self.inputs {
            Inputs::Prompt => {}
            Inputs::Portraits { portraits, images } => {
                if self.model != "bytedance/seedance-2.5"
                    || portraits.len() + images.len() == 0
                    || portraits.len() + images.len() > 12
                {
                    bail!("Choose 1–12 reference images for Seedance");
                }
                let mut references = Vec::new();
                let mut guide = String::from(
                    "\n\nCHARACTER REFERENCE GUIDE\nUse the numbered images for character identity. Match named actions and dialogue to these characters; do not swap their faces. Profiles describe the characters, not extra dialogue to speak.\n",
                );
                for (index, portrait) in portraits.iter().enumerate() {
                    if portrait.name.trim().is_empty() || portrait.brief.len() > 64 * 1024 {
                        bail!("Invalid portrait identity information");
                    }
                    guide.push_str(&format!(
                        "Image {}: {}\n{}\n",
                        index + 1,
                        portrait.name,
                        portrait.brief
                    ));
                    references.push(json!({"type":"image_url","image_url":{"url":format!("local-portrait:{}",portrait.sha256)}}));
                }
                for image in images {
                    references.push(image_url(image)?);
                }
                if !portraits.is_empty() {
                    request["prompt"] = json!(format!("{}{}", self.prompt, guide));
                }
                request["input_references"] = json!(references);
            }
            Inputs::References { images } => {
                if images.is_empty() || images.len() > 12 {
                    bail!(
                        "Choose 1–12 reference images for this planner; provider limits still require verification"
                    );
                }
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
                model: "test/video".into(),
                prompt: "An adult speaks while walking.".into(),
                duration: 10,
                automatic_duration: false,
                resolution: "720p".into(),
                aspect_ratio: "9:16".into(),
                generate_audio: true,
                inputs: Inputs::Prompt,
            },
            crate::openrouter::Catalog {
                videos: vec![
                    json!({"id":"test/video","supported_durations":[10],"supported_resolutions":["720p"],"supported_aspect_ratios":["9:16"],"generate_audio":true,"supported_frame_images":["first_frame","last_frame"]}),
                ],
                images: vec![],
                fetched_unix_seconds: 1,
            },
        )
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
