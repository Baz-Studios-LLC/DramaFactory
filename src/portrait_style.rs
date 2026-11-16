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
    Cgi,
    Animation,
    Cartoon,
    CelShaded,
    Illustration,
}
impl PortraitStyle {
    pub const ALL: [Self; 7] = [
        Self::Photo,
        Self::Cinematic,
        Self::Cgi,
        Self::Animation,
        Self::Cartoon,
        Self::CelShaded,
        Self::Illustration,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photorealistic",
            Self::Cinematic => "Cinematic",
            Self::Cgi => "3D / CGI",
            Self::Animation => "Animated film",
            Self::Cartoon => "Cartoon",
            Self::CelShaded => "Cel-shaded",
            Self::Illustration => "Illustrated",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Photo => "A natural camera photograph with realistic skin and materials.",
            Self::Cinematic => "A live-action film still with dramatic, motivated lighting.",
            Self::Cgi => {
                "A polished computer-generated 3D render with sculpted forms and rendered materials."
            }
            Self::Animation => {
                "An expressive animated-film design with stylized shapes and lighting."
            }
            Self::Cartoon => "A 2D cartoon with clean outlines, simplified shapes and flat colors.",
            Self::CelShaded => {
                "Crisp contours and flat color areas with distinct, hard-edged shadow bands."
            }
            Self::Illustration => "Drawn or painted artwork with visible artistic texture.",
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
            Self::Cgi => {
                "A polished 3D computer-graphics render of the described subject. Deliberately sculpted forms, modeled geometry, physically based materials, controlled studio illumination and rendered depth. Clearly intentional CGI rather than a camera photograph or a painted illustration. Preserve the described anatomy, proportions, recognizable features and material colors."
            }
            Self::Cartoon => {
                "A 2D cartoon character design with confident clean outlines, expressive simplified shapes, flat colors and restrained graphic shading. Use drawn, readable silhouettes rather than photographic texture or a volumetric 3D-render finish. Stylize while preserving specified age, body type, distinctive features, hair and eye colors, wardrobe and personality."
            }
            Self::CelShaded => {
                "Cel-shaded character artwork with crisp contours, flat color fills and sharply separated two- or three-tone shadow bands. Hard-edged graphic shadows and deliberate highlight shapes; avoid smooth airbrushed gradients, painterly blending, photographic skin texture and physically shaded CGI lighting. Preserve specified anatomy, age, proportions, colors, distinctive features and wardrobe within this stylized treatment."
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
pub(crate) fn picker(ui: &mut eframe::egui::Ui, style: &mut PortraitStyle) -> bool {
    let before = *style;
    ui.label("IMAGE STYLE");
    ui.horizontal_wrapped(|ui| {
        for option in PortraitStyle::ALL {
            if ui
                .selectable_label(*style == option, option.label())
                .on_hover_text(option.description())
                .clicked()
            {
                *style = option;
            }
        }
    });
    ui.small(style.description());
    *style != before
}
impl Store {
    pub(crate) fn series_portrait_style(&self, series: &str) -> Result<Option<PortraitStyle>> {
        use rusqlite::OptionalExtension;
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("series:portrait-style:{series}")],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }
    pub(crate) fn save_series_portrait_style(
        &self,
        series: &str,
        style: PortraitStyle,
    ) -> Result<()> {
        if !self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM series WHERE id=?1)",
            [series],
            |r| r.get::<_, bool>(0),
        )? {
            bail!("Series no longer exists");
        }
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![format!("series:portrait-style:{series}"), serde_json::to_string(&style)?])?;
        Ok(())
    }

    pub(crate) fn portrait_style(&self, entry: &str) -> Result<PortraitStyle> {
        use rusqlite::OptionalExtension;
        let series: Option<String> = self
            .conn
            .query_row(
                "SELECT series_id FROM library WHERE id=?1 AND kind='character'",
                [entry],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(series) = series
            && let Some(style) = self.series_portrait_style(&series)?
        {
            return Ok(style);
        }
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
    fn series_style_is_durable_and_rejects_missing_series() {
        let root = std::env::temp_dir().join(format!("df-series-style-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("test.sqlite");
        let store = Store::open(path.clone()).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO series(id,name,premise) VALUES('s','Style test','')",
                [],
            )
            .unwrap();
        assert!(store.series_portrait_style("s").unwrap().is_none());
        store
            .save_series_portrait_style("s", PortraitStyle::CelShaded)
            .unwrap();
        drop(store);
        let store = Store::open(path.clone()).unwrap();
        assert!(store.series_portrait_style("s").unwrap() == Some(PortraitStyle::CelShaded));
        assert!(
            store
                .save_series_portrait_style("missing", PortraitStyle::Photo)
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
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
