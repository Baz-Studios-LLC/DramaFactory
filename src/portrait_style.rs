//! Visual direction is saved per character/location and snapshotted into each new job.
use crate::store::Store;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortraitStyle {
    #[default]
    Photo,
    Cinematic,
    Animation,
    Illustration,
}
impl PortraitStyle {
    pub const ALL: [Self; 4] = [
        Self::Photo,
        Self::Cinematic,
        Self::Animation,
        Self::Illustration,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photo",
            Self::Cinematic => "Cinematic",
            Self::Animation => "Animation",
            Self::Illustration => "Illustration",
        }
    }
    pub fn prompt(self, subject: &str) -> String {
        let direction = match self {
            Self::Photo => {
                "A believable camera photograph of the described subject. Natural daylight, neutral white balance, restrained contrast, real material texture and fine detail. For human subjects: realistic skin texture, subtle pores, natural facial proportions and small asymmetries; avoid porcelain/plastic skin, exaggerated eyes and beauty-filter smoothing. Fantasy subjects should look physically photographed with practical costumes or props. No painting, drawing, CGI render, excessive glow, oversharpening or HDR effect."
            }
            Self::Cinematic => {
                "A live-action cinematic production still, photographed with a real lens. Motivated film lighting, controlled color grading, believable material textures and fine detail. For humans retain natural skin and facial anatomy. Fantasy costumes and environments should feel like physical film sets. Avoid a painted, plastic or game-render finish."
            }
            Self::Animation => {
                "An expressive polished animated-film character design. Stylized shapes, appealing silhouettes, deliberate materials and lighting. Non-human characters, including fruit, may have expressive faces and personalities when described. Preserve the subject's requested design; do not force human anatomy or photographic skin."
            }
            Self::Illustration => {
                "A polished illustrated character portrait with intentional drawing and painterly texture. Expressive composition, harmonious color and readable shapes. Preserve the described subject, including non-human subjects; do not force photographic realism."
            }
        };
        format!(
            "SUBJECT AND CHARACTER DETAILS:\n{subject}\n\nSELECTED VISUAL STYLE: {}\n{direction}\nThe selected visual style takes precedence over conflicting rendering-style adjectives in the subject description. Preserve identity, wardrobe and story details. No text or watermark.",
            self.label()
        )
    }
}
impl Store {
    pub(crate) fn portrait_style(&self, entry: &str) -> Result<PortraitStyle> {
        use rusqlite::OptionalExtension;
        let saved: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("portrait:style:{entry}")],
                |r| r.get(0),
            )
            .optional()?;
        saved
            .map(|text| serde_json::from_str(&text).map_err(Into::into))
            .unwrap_or(Ok(PortraitStyle::default()))
    }
    pub(crate) fn save_portrait_style(&self, entry: &str, style: PortraitStyle) -> Result<()> {
        if !self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM library WHERE id=?1)",
            [entry],
            |r| r.get::<_, bool>(0),
        )? {
            bail!("Character or location no longer exists");
        }
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![format!("portrait:style:{entry}"),serde_json::to_string(&style)?])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn styles_persist_per_character_and_queued_requests_keep_their_original_style() {
        let root = std::env::temp_dir().join(format!("df-styles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("Fruit drama", "").unwrap();
        let fruit = store
            .add_library(
                &series.id,
                "character",
                "Peach",
                "An expressive peach",
                None,
            )
            .unwrap();
        assert!(store.portrait_style(&fruit.id).unwrap() == PortraitStyle::Photo);
        store
            .save_portrait_style(&fruit.id, PortraitStyle::Animation)
            .unwrap();
        let job = store
            .enqueue_grok_portrait(&fruit.id, "A peach detective")
            .unwrap();
        store
            .save_portrait_style(&fruit.id, PortraitStyle::Photo)
            .unwrap();
        let raw: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [job.id], |r| {
                r.get(0)
            })
            .unwrap();
        let request: crate::hosted_image::PortraitRequest = serde_json::from_str(&raw).unwrap();
        assert!(request.style == Some(PortraitStyle::Animation));
        assert!(request.prompt.contains("do not force human anatomy"));
        assert_eq!(request.source_prompt.as_deref(), Some("A peach detective"));
        drop(store);
        let store = Store::open(path).unwrap();
        assert!(store.portrait_style(&fruit.id).unwrap() == PortraitStyle::Photo);
        assert!(
            store
                .save_portrait_style("missing", PortraitStyle::Photo)
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
