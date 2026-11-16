//! Resolve hash-bound local portraits into embedded image data only at submission.
//! Seedance documents data URIs; OpenRouter route acceptance still needs a live test.
use anyhow::{Context, Result, bail};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
pub(crate) fn embed(
    plan: &crate::video_plan::VideoPlan,
    payload: &mut serde_json::Value,
    directory: &Path,
) -> Result<()> {
    let crate::video_plan::Inputs::Portraits { portraits, .. } = &plan.inputs else {
        return Ok(());
    };
    let mut total = 0usize;
    for (index, portrait) in portraits.iter().enumerate() {
        let mut bytes = Vec::new();
        std::fs::File::open(&portrait.path)?
            .take(24 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > 24 * 1024 * 1024 {
            bail!("Portrait is empty or exceeds 24 MiB");
        }
        if Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            != portrait.sha256
        {
            bail!(
                "Portrait changed since it was linked; relink the selected image before generating"
            );
        }
        let format = image::guess_format(&bytes)?;
        let mime = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::WebP => "image/webp",
            _ => bail!("Portrait must be PNG, JPEG or WebP"),
        };
        let dimensions = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
            .into_dimensions()?;
        if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 4096 || dimensions.1 > 4096 {
            bail!("Portrait dimensions exceed supported bounds");
        }
        let data = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        total += data.len();
        if total > 48 * 1024 * 1024 {
            bail!("Combined portraits exceed the 48 MiB request budget");
        }
        let reference = payload
            .get_mut("input_references")
            .and_then(|v| v.get_mut(index))
            .context("Portrait reference order changed")?;
        if reference["image_url"]["url"] != format!("local-portrait:{}", portrait.sha256) {
            bail!("Portrait request mapping changed");
        }
        reference["image_url"]["url"] = data.into();
        std::fs::write(
            directory.join(format!(
                "portrait-{}.{}",
                index + 1,
                format.extensions_str()[0]
            )),
            &bytes,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generation_plan::SegmentDraft, store::Store};
    #[test]
    fn portraits_keep_exact_bytes_names_bios_and_reference_order() {
        let root =
            std::env::temp_dir().join(format!("df-portrait-delivery-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Series", "Story").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let path = root.join("portrait.png");
        image::RgbImage::from_pixel(320, 480, image::Rgb([100, 60, 30]))
            .save(&path)
            .unwrap();
        let one = store
            .add_library(
                &series.id,
                "character",
                "Sylvi",
                "An alchemist with auburn hair.",
                Some(&path),
            )
            .unwrap();
        let two = store
            .add_library(
                &series.id,
                "character",
                "Bram",
                "Sylvi's father, a weathered craftsman.",
                Some(&path),
            )
            .unwrap();
        let mut draft = store.generation_plan(&episode.id).unwrap();
        draft.segments[0] = SegmentDraft {
            prompt: "Sylvi greets Bram.".into(),
            mode: 1,
            cast: vec![
                store.prepare_cast_reference(&episode.id, &one.id).unwrap(),
                store.prepare_cast_reference(&episode.id, &two.id).unwrap(),
            ],
            references: "https://example.com/workshop.png".into(),
            ..Default::default()
        };
        let saved = store.save_generation_plan(&draft).unwrap();
        let job = store
            .enqueue_episode_segment(&episode.id, saved.revision, &saved.segments[0])
            .unwrap();
        let request: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [&job.id], |r| {
                r.get(0)
            })
            .unwrap();
        let plan: crate::video_plan::VideoPlan = serde_json::from_str(&request).unwrap();
        let catalog = crate::openrouter::Catalog {
            videos: vec![
                serde_json::json!({"id":"bytedance/seedance-2.5","supported_durations":[10,30],"supported_resolutions":["480p"],"supported_aspect_ratios":["9:16"],"generate_audio":true}),
            ],
            images: vec![],
            fetched_unix_seconds: 0,
        };
        let mut payload = plan.preview(&catalog).unwrap()["request"].clone();
        let prompt = payload["prompt"].as_str().unwrap();
        assert!(prompt.contains("Image 1: Sylvi\nAn alchemist with auburn hair."));
        assert!(prompt.contains("Image 2: Bram\nSylvi's father"));
        let directory = root.join("delivery");
        std::fs::create_dir_all(&directory).unwrap();
        embed(&plan, &mut payload, &directory).unwrap();
        for index in 0..2 {
            let data = payload["input_references"][index]["image_url"]["url"]
                .as_str()
                .unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.strip_prefix("data:image/png;base64,").unwrap())
                .unwrap();
            assert_eq!(bytes, std::fs::read(&path).unwrap());
        }
        assert_eq!(
            payload["input_references"][2]["image_url"]["url"],
            "https://example.com/workshop.png"
        );
        assert!(payload.get("frame_images").is_none());
        image::RgbImage::new(320, 480)
            .save(&saved.segments[0].cast[0].path)
            .unwrap();
        let mut preview = plan.preview(&catalog).unwrap()["request"].clone();
        assert!(embed(&plan, &mut preview, &directory).is_err());
        assert_eq!(store.job(&job.id).unwrap().status, "queued");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
