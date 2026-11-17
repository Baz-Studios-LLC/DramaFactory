//! Structured character details and their shared, readable production brief.
use crate::{library_editing::LibraryEdit, production::LibraryEntry, store::Store};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub struct Section {
    pub title: &'static str,
    pub fields: &'static [(&'static str, &'static str, &'static str)],
}
pub const SECTIONS: &[Section] = &[
    Section {
        title: "Identity & background",
        fields: &[
            ("age", "Age", "25, or the age this role calls for"),
            ("pronouns", "Pronouns", "She/her, he/him, they/them…"),
            ("role", "Story role", "Lead, rival, mentor, sibling…"),
            ("species", "Species / ancestry", "Human, half-elf…"),
            ("occupation", "Occupation", "Work, specialty, social role"),
            (
                "origin",
                "Hometown / upbringing",
                "Place and formative environment",
            ),
            ("culture", "Cultural background", "Author-defined; optional"),
            (
                "education",
                "Education / training",
                "Formal study, self-taught skills…",
            ),
        ],
    },
    Section {
        title: "Physical appearance",
        fields: &[
            ("height", "Height", "Include units: 5 ft 7 in / 170 cm"),
            ("weight", "Weight", "Include units: 140 lb / 64 kg"),
            (
                "build",
                "Build / proportions",
                "Slender, athletic, broad shoulders…",
            ),
            (
                "complexion",
                "Skin tone / complexion",
                "Tone, undertone, texture, freckles…",
            ),
            (
                "face",
                "Face shape / features",
                "Jawline, cheekbones, nose, lips…",
            ),
            ("eye_color", "Eye color", "Emerald green, dark brown…"),
            ("eye_shape", "Eyes / brows", "Shape, expression, brows…"),
            ("hair_color", "Hair color", "Color, highlights, roots…"),
            (
                "hair_texture",
                "Hair texture / length",
                "Wavy, shoulder-length…",
            ),
            ("hairstyle", "Hairstyle", "Braid, loose curls, cropped…"),
            (
                "facial_hair",
                "Facial hair",
                "Clean-shaven, beard, stubble…",
            ),
            (
                "marks",
                "Distinctive features",
                "Scars, birthmarks, tattoos, pointed ears…",
            ),
        ],
    },
    Section {
        title: "Style, voice & mannerisms",
        fields: &[
            (
                "wardrobe",
                "Signature wardrobe",
                "Usual silhouettes, colors, fabrics",
            ),
            (
                "accessories",
                "Accessories / props",
                "Jewelry, glasses, tools, keepsakes…",
            ),
            ("grooming", "Grooming / makeup", "Everyday presentation"),
            (
                "posture",
                "Posture / movement",
                "Gait, gestures, physical presence",
            ),
            ("voice", "Voice / accent", "Pitch, texture, accent, cadence"),
            (
                "speech",
                "Speaking style",
                "Vocabulary, rhythm, favorite expressions",
            ),
            (
                "mannerisms",
                "Habits / mannerisms",
                "Tells, rituals, nervous habits",
            ),
        ],
    },
    Section {
        title: "Personality",
        fields: &[
            (
                "temperament",
                "Temperament",
                "How they usually meet the world",
            ),
            (
                "strengths",
                "Strengths",
                "What makes them capable or compelling",
            ),
            (
                "flaws",
                "Flaws / blind spots",
                "Patterns that create problems",
            ),
            (
                "values",
                "Values / boundaries",
                "Beliefs and lines they will not cross",
            ),
            ("fears", "Fears / insecurities", "What they hide or avoid"),
            ("humor", "Sense of humor", "Dry, playful, dark, earnest…"),
            (
                "pressure",
                "Under pressure",
                "How they react to conflict or stress",
            ),
            (
                "interests",
                "Interests / skills",
                "Hobbies, talents, expertise",
            ),
        ],
    },
    Section {
        title: "Relationships & story arc",
        fields: &[
            ("family", "Family", "Kinship and family resemblance"),
            (
                "relationships",
                "Important relationships",
                "Allies, rivals, romantic chemistry",
            ),
            ("desire", "External goal", "What they want"),
            ("need", "Inner need", "What they need to learn or accept"),
            (
                "secret",
                "Secret / contradiction",
                "Private truth versus public image",
            ),
            (
                "history",
                "Formative history",
                "Events that explain who they are",
            ),
            (
                "arc",
                "Character arc",
                "Starting point, turning points, destination",
            ),
        ],
    },
    Section {
        title: "Episode-specific continuity",
        fields: &[(
            "continuity",
            "Episode / scene exceptions",
            "Label the episode: outfit, injury, disguise, emotional state…",
        )],
    },
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterProfile {
    pub summary: String,
    #[serde(default, deserialize_with = "profile_fields")]
    pub fields: BTreeMap<String, String>,
}
// Providers sometimes append null placeholders despite the string-only schema.
// Treat null as absent; validate_complete still rejects missing required traits.
fn profile_fields<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let fields =
        <BTreeMap<String, Option<String>> as serde::Deserialize>::deserialize(deserializer)?;
    Ok(fields
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect())
}

impl CharacterProfile {
    pub fn brief(&self) -> String {
        let mut parts = vec![self.summary.clone()];
        for section in SECTIONS {
            let lines: Vec<_> = section
                .fields
                .iter()
                .filter_map(|(key, label, _)| {
                    self.fields
                        .get(*key)
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| format!("{label}: {}", value.trim()))
                })
                .collect();
            if !lines.is_empty() {
                parts.push(format!("{}\n{}", section.title, lines.join("\n")));
            }
        }
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn validate_complete(&self) -> Result<()> {
        self.validate()?;
        if self.summary.trim().is_empty() {
            bail!("Sidekick profile is missing its summary");
        }
        for (key, label, _) in SECTIONS.iter().flat_map(|section| section.fields.iter()) {
            if self
                .fields
                .get(*key)
                .is_none_or(|value| value.trim().is_empty())
            {
                bail!("Sidekick profile is missing {label}; every field must be filled");
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        for (key, value) in &self.fields {
            if !SECTIONS
                .iter()
                .any(|section| section.fields.iter().any(|field| field.0 == key))
            {
                bail!("Unknown character profile field: {key}");
            }
            if value.len() > 2000 {
                bail!("Character profile fields must be at most 2000 bytes each");
            }
        }
        if self.brief().len() > 12000 {
            bail!("The complete character brief must be at most 12000 bytes");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDraft {
    pub brief: LibraryEdit,
    pub profile: CharacterProfile,
    pub expected_profile: Option<CharacterProfile>,
}
impl ProfileDraft {
    pub fn refresh_brief(&mut self) {
        self.brief.description = self.profile.brief();
    }
    pub fn dirty(&self) -> bool {
        self.brief.name != self.brief.expected_name
            || self.profile.brief() != self.brief.expected_description
            || self
                .expected_profile
                .as_ref()
                .is_some_and(|saved| saved != &self.profile)
    }
}
impl Store {
    pub fn create_profile_character(
        &self,
        series: &str,
        name: &str,
        profile: &CharacterProfile,
        reference: Option<&std::path::Path>,
        style: crate::portrait_style::PortraitStyle,
    ) -> Result<LibraryEntry> {
        profile.validate()?;
        if name.trim().is_empty() || name.len() > 200 {
            bail!("Character name needs 1–200 bytes");
        }
        let tx = self.conn.unchecked_transaction()?;
        let entry = self.add_library(series, "character", name, &profile.brief(), reference)?;
        self.save_portrait_style(&entry.id, style)?;
        tx.execute(
            "INSERT INTO character_profiles(library_id,document) VALUES(?1,?2)",
            params![entry.id, serde_json::to_string(profile)?],
        )?;
        tx.commit()?;
        Ok(entry)
    }
    pub fn saved_character_profile(&self, id: &str) -> Result<Option<CharacterProfile>> {
        let data: Option<String> = self
            .conn
            .query_row(
                "SELECT document FROM character_profiles WHERE library_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn character_profile(&self, id: &str) -> Result<ProfileDraft> {
        let (name, description): (String, String) = self
            .conn
            .query_row(
                "SELECT name,description FROM library WHERE id=?1 AND kind='character'",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("Character not found")?;
        let saved = self.saved_character_profile(id)?;
        let profile = saved.clone().unwrap_or_else(|| CharacterProfile {
            summary: description.clone(),
            ..Default::default()
        });
        Ok(ProfileDraft {
            brief: LibraryEdit {
                id: id.into(),
                name: name.clone(),
                description: profile.brief(),
                expected_name: name,
                // A plain-text writer must not silently be overwritten by an older form.
                expected_description: saved
                    .as_ref()
                    .map(CharacterProfile::brief)
                    .unwrap_or(description),
            },
            profile,
            expected_profile: saved,
        })
    }
    pub fn save_character_profile(&self, draft: &ProfileDraft) -> Result<ProfileDraft> {
        draft.profile.validate()?;
        if draft.brief.name.trim().is_empty() || draft.brief.name.len() > 200 {
            bail!("Character name needs 1–200 bytes");
        }
        let description = draft.profile.brief();
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let (name, current): (String, String) = tx
            .query_row(
                "SELECT name,description FROM library WHERE id=?1 AND kind='character'",
                [&draft.brief.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("Character not found")?;
        let saved = self.saved_character_profile(&draft.brief.id)?;
        let identical = name == draft.brief.name
            && current == description
            && saved.as_ref() == Some(&draft.profile);
        if !identical {
            if name != draft.brief.expected_name
                || current != draft.brief.expected_description
                || saved != draft.expected_profile
            {
                bail!(
                    "Character changed elsewhere. Your form is preserved; reload and reconcile before saving."
                );
            }
            tx.execute(
                "UPDATE library SET name=?2,description=?3 WHERE id=?1",
                params![draft.brief.id, draft.brief.name, description],
            )?;
            tx.execute("INSERT INTO character_profiles(library_id,document) VALUES(?1,?2) ON CONFLICT(library_id) DO UPDATE SET document=excluded.document",
                params![draft.brief.id,serde_json::to_string(&draft.profile)?])?;
        }
        tx.commit()?;
        self.character_profile(&draft.brief.id)
    }
}

pub fn response_schema() -> serde_json::Value {
    let keys: Vec<_> = SECTIONS
        .iter()
        .flat_map(|section| section.fields.iter().map(|field| field.0))
        .collect();
    let properties: serde_json::Map<_, _> = keys
        .iter()
        .map(|key| {
            (
                (*key).into(),
                serde_json::json!({"type":"string","minLength":1,"pattern":r"\S"}),
            )
        })
        .collect();
    serde_json::json!({"type":"object","additionalProperties":false,"required":["summary","fields"],"properties":{
        "summary":{"type":"string","minLength":1,"pattern":r"\S"},"fields":{"type":"object","additionalProperties":false,"required":keys,"properties":properties}
    }})
}

impl crate::Studio {
    pub(crate) fn character_profile_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        entry: &LibraryEntry,
    ) {
        use eframe::egui;
        let mut draft = match self
            .profile_drafts
            .get(&entry.id)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.store.character_profile(&entry.id))
        {
            Ok(draft) => draft,
            Err(error) => {
                ui.label(error.to_string());
                return;
            }
        };
        self.profile_save_controls(ui, &mut draft);
        egui::CollapsingHeader::new("Character profile").id_salt((&entry.id,"character-profile-detail")).default_open(true)
            .show(ui, |ui| {
                ui.small("Optional details build the full brief. Include units for height and weight. Leave unknown traits blank.");
                if draft.brief.expected_name != entry.name || draft.brief.expected_description != entry.description {
                    ui.colored_label(crate::GOLD,"The saved name or brief changed. Reload to reconcile; your form has not been overwritten.");
                }
                ui.label("NAME");
                let mut output = egui::TextEdit::singleline(&mut draft.brief.name).desired_width(f32::INFINITY).show(ui);
                crate::context_menus::text_menu(&mut output, &mut draft.brief.name, false);
                ui.label("STORY NOTES / SUMMARY");
                let mut output = egui::TextEdit::multiline(&mut draft.profile.summary).desired_rows(4).desired_width(f32::INFINITY).show(ui);
                crate::context_menus::text_menu(&mut output, &mut draft.profile.summary, false);
                fields_ui(ui, &entry.id, &mut draft.profile);
                draft.profile.fields.retain(|_,value| !value.trim().is_empty());
                draft.refresh_brief();
                ui.collapsing("Full character brief",|ui| {
                    ui.add(egui::Label::new(&draft.brief.description).wrap().selectable(true));
                    if ui.button("Copy brief").clicked(){ui.ctx().copy_text(draft.brief.description.clone());}
                });
                if let Err(error) = draft.profile.validate() {ui.colored_label(crate::GOLD,error.to_string());}
                self.profile_save_controls(ui, &mut draft);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Reload saved profile").clicked() {
                        match self.store.character_profile(&entry.id) {
                            Ok(saved)=>draft=saved,Err(error)=>self.message=error.to_string(),
                        }
                    }
                    if self.sidekick.character_undo.contains_key(&entry.id) && ui.button("Undo sidekick edit").clicked() {
                        // Put current inputs into the shared snapshot before checking Undo.
                        self.profile_drafts.insert(entry.id.clone(),draft.clone());
                        let result=self.undo_character_sidekick(&entry.series_id,&entry.id);
                        if result.is_ok() {draft=self.profile_drafts[&entry.id].clone();}
                        self.result(result,"Character draft restored; not saved");
                    }
                });
                if draft.brief.expected_description != entry.description {
                    ui.collapsing("Reconcile changed brief",|ui| {
                        ui.label(&entry.description);
                        if ui.button("Keep latest saved brief as story notes").clicked() {
                            draft.profile.summary=entry.description.clone();
                            draft.brief.expected_name=entry.name.clone();
                            draft.brief.expected_description=entry.description.clone();
                            draft.refresh_brief();
                        }
                        ui.small("Keeps the form's attributes. Review the notes and full brief for duplication before saving.");
                    });
                }
                ui.small("Save before generating: the whole profile informs your next image, alongside portrait direction. Existing images stay unchanged.");
            });
        // Merely displaying the collapsed card must not pin a clean saved snapshot.
        if draft.dirty() || self.profile_drafts.contains_key(&entry.id) {
            self.profile_drafts.insert(entry.id.clone(), draft);
        }
    }

    fn profile_save_controls(&mut self, ui: &mut eframe::egui::Ui, draft: &mut ProfileDraft) {
        ui.horizontal_wrapped(|ui| {
            let valid = !draft.brief.name.trim().is_empty()
                && draft.brief.name.len() <= 200
                && draft.profile.validate().is_ok();
            if ui
                .add_enabled(
                    draft.dirty() && valid,
                    crate::theme::primary("Save profile"),
                )
                .clicked()
            {
                match self.store.save_character_profile(draft) {
                    Ok(saved) => {
                        self.library_drafts.remove(&draft.brief.id);
                        *draft = saved;
                        self.message = "Character profile and full brief saved".into();
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
            if draft.dirty() {
                ui.colored_label(
                    crate::GOLD,
                    if valid {
                        "Unsaved changes"
                    } else {
                        "Open Character profile to fix invalid fields"
                    },
                );
            } else {
                ui.small("Saved");
            }
        });
    }
}

pub(crate) fn fields_ui(ui: &mut eframe::egui::Ui, id: &str, profile: &mut CharacterProfile) {
    use eframe::egui::{self, RichText};
    for section in SECTIONS {
        egui::CollapsingHeader::new(section.title)
            .id_salt((id, section.title))
            .default_open(matches!(
                section.title,
                "Identity & background" | "Physical appearance"
            ))
            .show(ui, |ui| {
                let columns = if ui.available_width() >= 420. { 2 } else { 1 };
                for fields in section.fields.chunks(columns) {
                    ui.columns(columns, |uis| {
                        for (ui, (key, label, hint)) in uis.iter_mut().zip(fields) {
                            ui.label(RichText::new(*label).small().color(crate::MUTED));
                            let value = profile.fields.entry((*key).into()).or_default();
                            let mut output = egui::TextEdit::multiline(&mut *value)
                                .id_salt((id, key))
                                .desired_rows(1)
                                .desired_width(f32::INFINITY)
                                .hint_text(crate::theme::placeholder(*hint))
                                .show(ui);
                            crate::context_menus::text_menu(&mut output, value, false);
                        }
                    });
                }
            });
    }
    profile.fields.retain(|_, value| !value.trim().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_save_preserves_identity_and_prompt_and_survives_restart() {
        let root = std::env::temp_dir().join(format!("df-profile-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let entry = store
            .add_library(
                &series.id,
                "character",
                "Ari",
                "Original authored biography.\nKeep every word.",
                None,
            )
            .unwrap();
        store.conn.execute("UPDATE library SET reference='approved-face.png',voice='approved-voice' WHERE id=?1",[&entry.id]).unwrap();
        let mut prompt = store.portrait_prompt(&entry).unwrap();
        prompt.prompt = "An independently authored portrait prompt".into();
        store.save_portrait_prompt(&prompt).unwrap();
        let mut draft = store.character_profile(&entry.id).unwrap();
        assert_eq!(draft.profile.summary, entry.description);
        assert!(draft.profile.fields.is_empty());
        draft
            .profile
            .fields
            .insert("height".into(), "170 cm".into());
        draft.profile.fields.insert("weight".into(), "64 kg".into());
        draft
            .profile
            .fields
            .insert("eye_color".into(), "Green".into());
        draft
            .profile
            .fields
            .insert("flaws".into(), "Too proud to ask for help".into());
        let saved = store.save_character_profile(&draft).unwrap();
        assert_eq!(store.save_character_profile(&draft).unwrap(), saved);
        assert!(!saved.dirty());
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(store.character_profile(&entry.id).unwrap(), saved);
        let current = &store.library(&series.id, "character").unwrap()[0];
        assert_eq!(
            current.reference.as_deref(),
            Some(std::path::Path::new("approved-face.png"))
        );
        assert_eq!(current.voice, "approved-voice");
        assert_eq!(
            store.portrait_prompt(current).unwrap().prompt,
            prompt.prompt
        );
        assert!(current.description.starts_with(&entry.description));
        assert!(current.description.contains("Height: 170 cm"));
        assert!(current.description.contains("Weight: 64 kg"));
        assert!(
            current
                .description
                .contains("Flaws / blind spots: Too proud")
        );
        assert!(!current.description.contains("Age:"));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_form_and_plain_text_edits_never_clobber_each_other() {
        let root =
            std::env::temp_dir().join(format!("df-profile-conflict-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let a = Store::open(path.clone()).unwrap();
        let b = Store::open(path).unwrap();
        let series = a.create_series("Fixture", "").unwrap();
        let entry = a
            .create_profile_character(
                &series.id,
                "Ari",
                &CharacterProfile {
                    summary: "Biography".into(),
                    ..Default::default()
                },
                None,
                Default::default(),
            )
            .unwrap();
        let mut first = a.character_profile(&entry.id).unwrap();
        let mut stale = b.character_profile(&entry.id).unwrap();
        first
            .profile
            .fields
            .insert("hair_color".into(), "Auburn".into());
        a.save_character_profile(&first).unwrap();
        stale
            .profile
            .fields
            .insert("hair_color".into(), "Black".into());
        assert!(b.save_character_profile(&stale).is_err());
        let mut plain = LibraryEdit::from_entry(&a.library(&series.id, "character").unwrap()[0]);
        plain.description = "A newer explicitly authored brief".into();
        b.update_library_entry(&plain).unwrap();
        let reloaded = a.character_profile(&entry.id).unwrap();
        assert!(a.save_character_profile(&reloaded).is_err());
        assert_eq!(
            a.library(&series.id, "character").unwrap()[0].description,
            plain.description
        );
        assert_eq!(reloaded.profile.fields["hair_color"], "Auburn");
        drop(a);
        drop(b);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_profiles_and_noncharacters_are_rejected_without_partial_writes() {
        let root =
            std::env::temp_dir().join(format!("df-profile-validation-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let mut profile = CharacterProfile::default();
        profile
            .fields
            .insert("unrecognized".into(), "Do not silently lose this".into());
        assert!(
            store
                .create_profile_character(&series.id, "Ari", &profile, None, Default::default())
                .is_err()
        );
        assert!(store.library(&series.id, "character").unwrap().is_empty());
        profile.fields.clear();
        profile.fields.insert("height".into(), "x".repeat(2001));
        assert!(profile.validate().is_err());
        let location = store
            .add_library(&series.id, "location", "Market", "A busy square", None)
            .unwrap();
        assert!(store.character_profile(&location.id).is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod null_field_tests {
    use super::*;
    #[test]
    fn null_placeholders_are_absent_but_invalid_values_still_fail() {
        let profile: CharacterProfile = serde_json::from_str(
            r#"{"summary":"Example","fields":{"fears":"Failure","fears_and_insecurities":null,"height":null}}"#,
        ).unwrap();
        assert_eq!(profile.fields.get("fears").unwrap(), "Failure");
        assert!(!profile.fields.contains_key("fears_and_insecurities"));
        assert!(profile.validate_complete().is_err());
        assert!(
            serde_json::from_str::<CharacterProfile>(
                r#"{"summary":"Example","fields":{"age":23}}"#
            )
            .is_err()
        );
    }
}
