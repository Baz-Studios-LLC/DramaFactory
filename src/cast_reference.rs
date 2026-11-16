//! Bind episode segments to exact selected character images without uploading them.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastReference {
    pub character_id: String,
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
}
impl Store {
    pub fn prepare_cast_reference(&self, episode: &str, character: &str) -> Result<CastReference> {
        let episode = self.get_episode(episode)?;
        let entry = self
            .library(&episode.series_id, "character")?
            .into_iter()
            .find(|entry| entry.id == character)
            .context("Choose a character from this episode's series")?;
        let path = entry
            .reference
            .context("Choose a portrait for this character first")?;
        let size = std::fs::metadata(&path)?.len();
        if size == 0 || size > 24 * 1024 * 1024 {
            bail!("Character portrait must be a nonempty image up to 24 MiB");
        }
        let reader = image::ImageReader::open(&path)?.with_guessed_format()?;
        if !matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
        ) {
            bail!("Character portrait must be PNG, JPEG or WebP");
        }
        let (width, height) = reader.into_dimensions()?;
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            bail!("Character portrait exceeds the current 4096px authoring bounds");
        }
        let sha256 = crate::worker::hash_file(&path)?;
        Ok(CastReference {
            character_id: entry.id,
            name: entry.name,
            path,
            sha256,
        })
    }
    pub fn validate_cast_reference(&self, episode: &str, reference: &CastReference) -> Result<()> {
        let current = self.prepare_cast_reference(episode, &reference.character_id)?;
        if &current != reference {
            bail!(
                "Character name or selected portrait changed. Review and relink the intended character image."
            );
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cast_links_reject_other_series_and_changed_selected_bytes() {
        let root = std::env::temp_dir().join(format!("df-castref-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("S", "P").unwrap();
        let other = store.create_series("Other", "P").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let path = root.join("source.png");
        image::RgbaImage::new(16, 16).save(&path).unwrap();
        let entry = store
            .add_library(&series.id, "character", "Actor", "Adult", Some(&path))
            .unwrap();
        let foreign = store
            .add_library(&other.id, "character", "Other actor", "Adult", Some(&path))
            .unwrap();
        assert!(
            store
                .prepare_cast_reference(&episode.id, &foreign.id)
                .is_err()
        );
        let reference = store
            .prepare_cast_reference(&episode.id, &entry.id)
            .unwrap();
        store
            .validate_cast_reference(&episode.id, &reference)
            .unwrap();
        image::RgbaImage::new(20, 20).save(&reference.path).unwrap();
        assert!(
            store
                .validate_cast_reference(&episode.id, &reference)
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
