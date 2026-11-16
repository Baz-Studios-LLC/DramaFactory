//! Creative preferences and persistent shot-to-cast/location/camera links.
use crate::{production::RenderRequest, store::Store};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CreativeProfile {
    pub style: String,
    pub casting_brief: String,
    pub min_shot_seconds: f64,
    pub max_shot_seconds: f64,
    pub episode_seconds: f64,
    pub target_episodes: u32,
}
impl Default for CreativeProfile {
    fn default() -> Self {
        Self { style:"character_drama".into(), casting_brief:"Exceptionally attractive soap-opera adult leads and supporting cast, usually around age 25. Use script-appropriate ages for parents, children and other roles. Cast a believable social circle through relationships, chemistry and family resemblance; avoid a demographic-checklist look. Preserve distinct faces, screen presence and expressive acting.".into(), min_shot_seconds:1.,max_shot_seconds:4.,episode_seconds:90.,target_episodes:67 }
    }
}
impl CreativeProfile {
    pub fn validate(&self) -> Result<()> {
        if ![
            "character_drama",
            "slice_of_life",
            "romance",
            "thriller",
            "melodrama",
            "custom",
        ]
        .contains(&self.style.as_str())
            || [
                self.min_shot_seconds,
                self.max_shot_seconds,
                self.episode_seconds,
            ]
            .iter()
            .any(|v| !v.is_finite())
            || self.min_shot_seconds < 0.5
            || self.max_shot_seconds > 30.
            || self.min_shot_seconds > self.max_shot_seconds
            || !(15. ..=600.).contains(&self.episode_seconds)
            || !(1..=150).contains(&self.target_episodes)
        {
            bail!("Invalid style, cut range, or episode target");
        }
        Ok(())
    }
    pub fn performance_note(&self) -> &'static str {
        match self.style.as_str() {
            "slice_of_life" => {
                "Recognizable everyday behavior and small emotional stakes. Understated reactions, awkward pauses and warm or tense relationship subtext. Avoid exaggerated melodramatic gestures."
            }
            "thriller" => {
                "Grounded tension, deliberate action, watchful eyes and controlled urgency."
            }
            "romance" => {
                "Play attraction and relationship subtext through eyelines, hesitation and responsive expressions."
            }
            "melodrama" => {
                "Clear heightened emotion with motivated reactions and readable reversals."
            }
            _ => {
                "Believable character-driven acting, motivated movement and responsive expressions."
            }
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraSetup {
    pub revision: u64,
    pub id: String,
    pub location_id: String,
    pub name: String,
    pub framing: String,
    pub placement: String,
    pub eyeline: String,
    pub screen_axis: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShotDirection {
    pub revision: u64,
    /// Ordered cast: first is the primary reference for the single-image adapter.
    pub character_ids: Vec<String>,
    pub location_id: Option<String>,
    pub camera_setup_id: Option<String>,
    pub dramatic_beat: String,
    pub performance: String,
    pub wardrobe: String,
}
#[derive(Serialize)]
pub struct PreparedShot {
    pub request: RenderRequest,
    pub notes: Vec<String>,
}
impl Store {
    pub fn shot_direction(&self, shot: &str) -> Result<ShotDirection> {
        let value = self
            .conn
            .query_row(
                "SELECT data FROM shot_directions WHERE shot_id=?1",
                [shot],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(match value {
            Some(s) => serde_json::from_str(&s)?,
            None => ShotDirection::default(),
        })
    }
    pub fn camera_setups(&self, location: &str) -> Result<Vec<CameraSetup>> {
        self.conn
            .prepare("SELECT data FROM camera_setups WHERE location_id=?1 ORDER BY rowid")?
            .query_map([location], |r| r.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect()
    }
    pub fn save_camera_setup(&self, camera: &CameraSetup) -> Result<CameraSetup> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let prior = self
            .camera_setups(&camera.location_id)?
            .into_iter()
            .find(|c| c.id == camera.id);
        if prior.as_ref().map(|c| c.revision).unwrap_or(0) != camera.revision {
            bail!("Camera setup changed in another session; reload before saving");
        }
        uuid::Uuid::parse_str(&camera.id)?;
        let kind: String = self
            .conn
            .query_row(
                "SELECT kind FROM library WHERE id=?1",
                [&camera.location_id],
                |r| r.get(0),
            )
            .context("Location not found")?;
        if kind != "location"
            || [
                &camera.name,
                &camera.framing,
                &camera.placement,
                &camera.eyeline,
                &camera.screen_axis,
            ]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            bail!(
                "Camera setup needs a location, name, framing, placement, eyeline and screen-axis convention"
            );
        }
        let previous = self
            .conn
            .query_row(
                "SELECT location_id FROM camera_setups WHERE id=?1",
                [&camera.id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        if previous.is_some_and(|id| id != camera.location_id) {
            bail!("A saved camera cannot move to a different location");
        }
        let mut saved = camera.clone();
        saved.revision += 1;
        self.conn.execute("INSERT INTO camera_setups(id,location_id,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![camera.id,camera.location_id,serde_json::to_string(&saved)?])?;
        tx.commit()?;
        Ok(saved)
    }
    pub fn save_shot_direction(
        &self,
        shot: &str,
        direction: &ShotDirection,
    ) -> Result<ShotDirection> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if self.shot_direction(shot)?.revision != direction.revision {
            bail!("Shot direction changed in another session; reload before saving");
        }
        let shot = self.shot(shot)?;
        let episode = self.get_episode(&shot.episode_id)?;
        let cast = self.library(&episode.series_id, "character")?;
        let mut distinct = BTreeSet::new();
        for id in &direction.character_ids {
            if !distinct.insert(id) || !cast.iter().any(|c| &c.id == id) {
                bail!("Shot cast must use distinct characters from the episode's series");
            }
        }
        if let Some(id) = &direction.location_id
            && !self
                .library(&episode.series_id, "location")?
                .iter()
                .any(|l| &l.id == id)
        {
            bail!("Shot location belongs to another series or is missing");
        }
        if let Some(id) = &direction.camera_setup_id {
            let location = direction
                .location_id
                .as_deref()
                .context("Select a location for this camera setup")?;
            if !self.camera_setups(location)?.iter().any(|c| &c.id == id) {
                bail!("Camera setup belongs to another location or is missing");
            }
        }
        let mut saved = direction.clone();
        saved.revision += 1;
        self.conn.execute("INSERT INTO shot_directions(shot_id,data) VALUES(?1,?2) ON CONFLICT(shot_id) DO UPDATE SET data=excluded.data",params![shot.id,serde_json::to_string(&saved)?])?;
        tx.commit()?;
        Ok(saved)
    }
    pub fn shot_context(&self, id: &str) -> Result<serde_json::Value> {
        let shot = self.shot(id)?;
        let episode = self.get_episode(&shot.episode_id)?;
        let direction = self.shot_direction(id)?;
        let plan = self.series_plan(&episode.series_id)?;
        let profile = plan.creative;
        let outline = plan.episodes.iter().find(|e| e.episode_id == episode.id);
        let library = self.library(&episode.series_id, "character")?;
        let mut cast = Vec::new();
        for character in &direction.character_ids {
            let entry = library
                .iter()
                .find(|e| &e.id == character)
                .context("Shot character is missing")?;
            let hash = entry
                .reference
                .as_ref()
                .map(|p| crate::worker::hash_file(p))
                .transpose()?;
            cast.push(serde_json::json!({"entry":entry,"reference_hash":hash}));
        }
        let location = direction
            .location_id
            .as_ref()
            .map(|id| {
                self.library(&episode.series_id, "location")?
                    .into_iter()
                    .find(|l| &l.id == id)
                    .context("Shot location is missing")
            })
            .transpose()?;
        let camera = direction
            .camera_setup_id
            .as_ref()
            .map(|id| {
                self.camera_setups(
                    direction
                        .location_id
                        .as_deref()
                        .context("Shot camera needs a location")?,
                )?
                .into_iter()
                .find(|c| &c.id == id)
                .context("Shot camera is missing")
            })
            .transpose()?;
        let mut context = serde_json::json!({"shot_id":id,"shot_plan":{"prompt":shot.prompt,"camera":shot.camera,"duration":shot.duration},"story_revision":plan.revision,"episode_outline":outline,"direction":direction,"creative_profile":profile,"cast":cast,"location":location,"camera":camera});
        if shot.source_start != 0. {
            context["shot_plan"]["source_start"] = serde_json::json!(shot.source_start);
        }
        Ok(context)
    }
    pub fn prepare_shot_render(&self, id: &str) -> Result<PreparedShot> {
        let shot = self.shot(id)?;
        if shot.source_start + shot.duration > 121. / 24. {
            bail!("Split this shot into takes of at most 5 seconds for the current Wan adapter");
        }
        let context = self.shot_context(id)?;
        let direction: ShotDirection = serde_json::from_value(context["direction"].clone())?;
        let profile: CreativeProfile = serde_json::from_value(context["creative_profile"].clone())?;
        let mut lines = vec![shot.prompt, shot.camera, profile.performance_note().into()];
        let mut image = None;
        let mut notes = Vec::new();
        if let Some(cast) = context["cast"].as_array() {
            for (i, character) in cast.iter().enumerate() {
                let entry: &serde_json::Value = &character["entry"];
                lines.push(format!(
                    "Character {}: {}. Preserve the established face, age and identity.",
                    entry["name"].as_str().unwrap_or(""),
                    entry["description"].as_str().unwrap_or("")
                ));
                if i == 0 {
                    image=Some(std::path::PathBuf::from(entry["reference"].as_str().context("Generate and approve a reference for the primary character before preparing their performance")?));
                }
            }
            if cast.len() > 1 {
                notes.push("This adapter conditions on the first cast member's image only. Other identities are described in text; review every face or use separate single-character coverage.".into());
            }
        }
        if let Some(name) = context["location"]["name"].as_str() {
            lines.push(format!(
                "Location: {}. {}",
                name,
                context["location"]["description"].as_str().unwrap_or("")
            ));
        }
        if !context["camera"].is_null() {
            let camera: CameraSetup = serde_json::from_value(context["camera"].clone())?;
            lines.push(format!("Camera {}: {}. Placement: {}. Eyeline: {}. Screen axis: {}. Keep this geometry consistent with other shots in this setup.",camera.name,camera.framing,camera.placement,camera.eyeline,camera.screen_axis));
        }
        if direction.character_ids.len() == 1 {
            let words = direction
                .wardrobe
                .split(|c: char| !c.is_alphanumeric())
                .collect::<std::collections::HashSet<_>>();
            let episode = self.get_episode(&shot.episode_id)?;
            for character in self.library(&episode.series_id, "character")? {
                if !direction.character_ids.contains(&character.id)
                    && character
                        .name
                        .split_whitespace()
                        .next()
                        .is_some_and(|name| name.len() > 2 && words.contains(name))
                {
                    notes.push(format!("Wardrobe names {}, who is not linked to this single-character shot. Check that this clothing detail is intentional; off-screen costumes can contaminate the visible performance.", character.name));
                }
            }
        }
        for (label, text) in [
            ("Dramatic beat", direction.dramatic_beat),
            ("Performance", direction.performance),
            ("Wardrobe continuity", direction.wardrobe),
        ] {
            if !text.trim().is_empty() {
                lines.push(format!("{label}: {text}"));
            }
        }
        if shot.duration < profile.min_shot_seconds || shot.duration > profile.max_shot_seconds {
            notes.push(format!("This {:.2}s hold is outside the series' {:.1}–{:.1}s cut guide. Keep it if the performance needs it.",shot.duration,profile.min_shot_seconds,profile.max_shot_seconds));
        }
        let request = RenderRequest {
            prompt: lines
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            image,
            shot_id: Some(id.into()),
            frames: frames_for_duration(shot.source_start + shot.duration)?,
            direction_context: context,
            ..Default::default()
        };
        request.validate()?;
        Ok(PreparedShot { request, notes })
    }
}
/// Round upward to the adapter's 4n+1 constraint, never shorter than the edit.
pub fn frames_for_duration(seconds: f64) -> Result<u32> {
    if !seconds.is_finite() || seconds <= 0. || seconds > 121. / 24. {
        bail!("Unsupported take duration");
    }
    Ok((((seconds * 24. - 1.) / 4.).ceil().max(1.) as u32 * 4 + 1).min(121))
}
#[cfg(test)]
mod tests {
    #[test]
    fn fractional_edits_never_outlast_generated_frames() {
        for milliseconds in 100..=5041 {
            let duration = milliseconds as f64 / 1000.;
            let frames = super::frames_for_duration(duration).unwrap();
            assert!(frames as f64 / 24. >= duration);
            assert_eq!((frames - 1) % 4, 0);
        }
        assert!(super::frames_for_duration(f64::NAN).is_err());
        assert!(super::frames_for_duration(5.1).is_err());
    }
    #[test]
    fn linked_direction_prepares_identity_and_camera_and_rejects_stale_edits() {
        use super::*;
        let root = std::env::temp_dir().join(format!("df-direction-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store
            .create_series("Everyday lives", "Workplace friendships")
            .unwrap();
        let episode = store
            .create_episode(&series.id, "The missed coffee")
            .unwrap();
        let reference = root.join("fixture.png");
        image::RgbImage::new(32, 32).save(&reference).unwrap();
        let actor = store
            .add_library(
                &series.id,
                "character",
                "Mara",
                "Adult woman, dark hair, navy blazer",
                Some(&reference),
            )
            .unwrap();
        let other = store
            .add_library(
                &series.id,
                "character",
                "Eli",
                "Adult man, work jacket",
                None,
            )
            .unwrap();
        let location = store
            .add_library(
                &series.id,
                "location",
                "Cafe",
                "Corner table, window on camera left",
                None,
            )
            .unwrap();
        let camera = store
            .save_camera_setup(&CameraSetup {
                id: uuid::Uuid::new_v4().to_string(),
                location_id: location.id.clone(),
                name: "A / Mara".into(),
                framing: "Medium close-up".into(),
                placement: "Across the cafe table".into(),
                eyeline: "Mara looks just camera-right toward Eli".into(),
                screen_axis: "Mara left, Eli right; stay window side".into(),
                ..Default::default()
            })
            .unwrap();
        let shot = store
            .add_shot(
                &episode.id,
                "A hesitant invitation",
                "Mara slides a coffee across the table",
                "Hold the reaction",
                3.1,
            )
            .unwrap();
        let mut plan = store.series_plan(&series.id).unwrap();
        plan.creative.style = "slice_of_life".into();
        store.save_series_plan(&plan).unwrap();
        let draft = ShotDirection {
            character_ids: vec![actor.id.clone(), other.id],
            location_id: Some(location.id.clone()),
            camera_setup_id: Some(camera.id.clone()),
            performance: "A small tentative smile, then wait for the response".into(),
            dramatic_beat: "Mara risks admitting she wants company".into(),
            wardrobe: "Navy blazer and ivory blouse".into(),
            ..Default::default()
        };
        let saved = store.save_shot_direction(&shot.id, &draft).unwrap();
        assert!(store.save_shot_direction(&shot.id, &draft).is_err());
        let prepared = store.prepare_shot_render(&shot.id).unwrap();
        assert_eq!(prepared.request.image, actor.reference);
        assert_eq!(prepared.request.frames, 77);
        assert!(prepared.request.prompt.contains("small emotional stakes"));
        assert!(prepared.request.prompt.contains("Mara left, Eli right"));
        assert!(
            prepared
                .request
                .prompt
                .contains("Navy blazer and ivory blouse")
        );
        assert_eq!(prepared.notes.len(), 1); // explicit multi-character conditioning limit
        let job = store.enqueue(&prepared.request).unwrap();
        assert_eq!(job.status, "blocked");
        let request: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [job.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<RenderRequest>(&request)
                .unwrap()
                .direction_context["direction"]["revision"],
            saved.revision
        );
        let mut revised = camera.clone();
        revised.eyeline = "Mara looks camera-left".into();
        store.save_camera_setup(&revised).unwrap();
        assert!(store.save_camera_setup(&camera).is_err());
        assert!(store.enqueue(&prepared.request).is_err());
        let foreign = store.create_series("Other series", "").unwrap();
        let stranger = store
            .add_library(&foreign.id, "character", "Different person", "Adult", None)
            .unwrap();
        let mut wrong = saved;
        wrong.character_ids = vec![stranger.id];
        assert!(store.save_shot_direction(&shot.id, &wrong).is_err());
        let mut single = store.shot_direction(&shot.id).unwrap();
        single.character_ids = vec![actor.id.clone()];
        single.wardrobe = "Mara keeps Eli's charcoal jacket".into();
        let mut single = store.save_shot_direction(&shot.id, &single).unwrap();
        let warned = store.prepare_shot_render(&shot.id).unwrap();
        assert!(
            warned
                .notes
                .iter()
                .any(|n| n.contains("Wardrobe names Eli"))
        );
        assert!(warned.request.prompt.contains("Eli's charcoal jacket")); // Warn; never rewrite an intentional costume.
        single.wardrobe = "Mara's navy blazer only".into();
        store.save_shot_direction(&shot.id, &single).unwrap();
        assert!(
            !store
                .prepare_shot_render(&shot.id)
                .unwrap()
                .notes
                .iter()
                .any(|n| n.contains("Wardrobe names"))
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
