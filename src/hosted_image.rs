//! Hosted Grok portraits: explicit paid submission, retained original bytes, manual review.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Serialize, Deserialize)]
pub struct PortraitRequest {
    pub prompt: String,
    #[serde(default)]
    pub source_prompt: Option<String>,
    #[serde(default)]
    pub style: Option<crate::portrait_style::PortraitStyle>,
    #[serde(default)]
    pub character_profile: Option<crate::character_profile::CharacterProfile>,
    pub library_id: String,
    pub library_reference_before: Option<String>,
    pub library_context_before: Value,
}
impl Store {
    pub(crate) fn portrait_jobs(&self, entry: &str) -> Result<Vec<crate::store::Job>> {
        let mut query=self.conn.prepare("SELECT id FROM jobs WHERE kind='openrouter_image' AND CASE WHEN json_valid(request) THEN json_extract(request,'$.library_id') END=?1 ORDER BY rowid DESC LIMIT 5")?;
        let ids = query
            .query_map([entry], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter().map(|id| self.job(id)).collect()
    }
    pub fn enqueue_grok_portrait(&self, entry: &str, prompt: &str) -> Result<crate::store::Job> {
        self.enqueue_grok_portrait_with_profile(entry, prompt, None)
    }
    pub(crate) fn enqueue_grok_portrait_with_profile(
        &self,
        entry: &str,
        prompt: &str,
        expected: Option<&crate::character_profile::ProfileDraft>,
    ) -> Result<crate::store::Job> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(expected) = expected
            && (expected.dirty() || expected != &self.character_profile(entry)?)
        {
            bail!(
                "Save or reload the character profile before generating an image. Your draft is preserved."
            );
        }
        let request = self.prepare_grok_portrait(entry, prompt)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO jobs(id,prompt,status,error,request,kind,created_at,updated_at) VALUES(?1,?2,'queued','',?3,'openrouter_image',datetime('now'),datetime('now'))",rusqlite::params![id,prompt,serde_json::to_string(&request)?])?;
        tx.commit()?;
        self.job(&id)
    }
    /// Read-only preview using the same composition as the immutable queued request.
    pub(crate) fn grok_portrait_request(
        &self,
        entry: &str,
        prompt: &str,
    ) -> Result<PortraitRequest> {
        let tx = self.conn.unchecked_transaction()?;
        let request = self.prepare_grok_portrait(entry, prompt)?;
        tx.commit()?;
        Ok(request)
    }
    fn prepare_grok_portrait(&self, entry: &str, prompt: &str) -> Result<PortraitRequest> {
        self.ensure_character_not_deleted(entry)?;
        if prompt.trim().is_empty() || prompt.len() > 65536 {
            bail!("Portrait prompt must contain 1–65536 bytes");
        }
        let (name, description, reference, kind): (String, String, Option<String>, String) =
            self.conn.query_row(
                "SELECT name,description,reference,kind FROM library WHERE id=?1",
                [entry],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        let character_profile = if kind == "character" {
            self.saved_character_profile(entry)?
        } else {
            None
        };
        if let Some(profile) = &character_profile {
            profile.validate()?;
            if profile.brief() != description {
                bail!(
                    "The character form and saved brief differ. Reconcile and save Character profile before generating an image."
                );
            }
        }
        let hash = reference
            .as_ref()
            .map(|path| crate::worker::hash_file(std::path::Path::new(path)))
            .transpose()?;
        let style = self.portrait_style(entry)?;
        let subject = if kind == "character" {
            format!(
                "ADDITIONAL PORTRAIT DIRECTION:\n{prompt}\n\nCOMPLETE SAVED PROFILE — {name}:\n{description}\n\nUse the entire saved profile as context for this subject. Its specified identity, age, measurements, build, facial features, hair, eyes, wardrobe and other traits take precedence over conflicting details in the additional portrait direction. Convey personality, background, goals and relationships through appropriate expression, posture, styling and setting; do not add other people or written labels to illustrate them. Episode-specific exceptions apply only when the portrait direction explicitly requests that episode or scene; do not combine conflicting episode outfits or states. Unspecified traits remain open creative choices."
            )
        } else {
            prompt.to_owned()
        };
        let full_prompt = style.prompt(&subject);
        if full_prompt.len() > 65536 {
            bail!(
                "The combined profile and portrait direction exceed 65536 bytes. Shorten the portrait direction; no profile details were truncated."
            );
        }
        let request = PortraitRequest {
            prompt: full_prompt,
            source_prompt: Some(prompt.into()),
            style: Some(style),
            character_profile,
            library_id: entry.into(),
            library_reference_before: reference,
            library_context_before: json!({"name":name,"description":description,"reference_hash":hash}),
        };
        Ok(request)
    }
}
fn decode(value: &Value) -> Result<(Vec<u8>, u32, u32, &'static str)> {
    let items = value["data"]
        .as_array()
        .context("Image response has no data array")?;
    if items.len() != 1 {
        bail!("Expected one image; retained response needs manual inspection");
    }
    let encoded = items[0]["b64_json"]
        .as_str()
        .context("Image response has no encoded image")?;
    if encoded.len() > 32 * 1024 * 1024 {
        bail!("Encoded image exceeds the 32 MiB limit");
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("Invalid encoded image")?;
    let format = image::guess_format(&bytes)?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        _ => bail!("Unsupported image format"),
    };
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
    let (width, height) = reader.into_dimensions()?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        bail!("Image dimensions exceed the supported bounds");
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode()?;
    Ok((bytes, width, height, extension))
}
pub(crate) fn execute(store: &Store, id: &str) -> Result<()> {
    let key = crate::credentials::read()?
        .context("Save an OpenRouter key in Settings before generating portraits")?;
    execute_with_transport(store, id, "https://openrouter.ai/api/v1/images", &key)
}
// Private injection point for loopback tests; production never accepts a custom host.
fn execute_with_transport(store: &Store, id: &str, endpoint: &str, key: &str) -> Result<()> {
    let (request, attempt): (String, u32) = store.conn.query_row(
        "SELECT request,attempts FROM jobs WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let request: PortraitRequest = serde_json::from_str(&request)?;
    let directory = store
        .root()
        .join("jobs")
        .join(id)
        .join(format!("attempt-{attempt}"));
    std::fs::create_dir_all(&directory)?;
    let canceled: bool = store.conn.query_row(
        "SELECT cancel_requested FROM jobs WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    if canceled {
        bail!("Canceled before submission");
    }
    let payload = json!({"model":"x-ai/grok-imagine-image-2.0","prompt":request.prompt,"n":1,"aspect_ratio":"3:4","resolution":"1K","quality":"medium","provider":{"allow_fallbacks":false}});
    std::fs::write(
        directory.join("request.json"),
        serde_json::to_vec_pretty(&payload)?,
    )?;
    // Marker survives a crash. No retry path can silently submit this attempt again.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("submission-started"))?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(180)))
        .max_redirects(0)
        .build();
    let agent: ureq::Agent = config.into();
    store.conn.execute(
        "UPDATE jobs SET phase='Grok generating portrait' WHERE id=?1",
        [id],
    )?;
    let mut response=agent.post(endpoint).header("Authorization",&format!("Bearer {key}")).send_json(payload).map_err(|_|anyhow::anyhow!("OpenRouter image submission did not complete locally. Billing may have occurred; inspect provider activity. This job will not be resubmitted automatically."))?;
    let raw=response.body_mut().with_config().limit(34*1024*1024).read_to_vec().map_err(|_|anyhow::anyhow!("Image response interrupted or too large; billing may have occurred. No automatic retry."))?;
    std::fs::write(directory.join("response.json"), &raw)?;
    let value: Value = serde_json::from_slice(&raw)?;
    store.conn.execute(
        "UPDATE jobs SET phase='Validating portrait' WHERE id=?1",
        [id],
    )?;
    let (bytes, width, height, extension) = decode(&value)?;
    let output = directory.join(format!("portrait.{extension}"));
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    let hash = crate::worker::hash_file(&output)?;
    let asset = uuid::Uuid::new_v4().to_string();
    let metadata = json!({"kind":"image","provider":"openrouter","model":"x-ai/grok-imagine-image-2.0","request":request,"usage":value.get("usage"),"selection":"manual_review_required"});
    let tx = store.conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO assets VALUES(?1,?2,?3,?4,?5,?6,0,?7,'unreviewed')",
        rusqlite::params![
            asset,
            id,
            output.to_string_lossy(),
            hash,
            width,
            height,
            metadata.to_string()
        ],
    )?;
    crate::completion::finish(&tx, id, &asset, "Grok portrait ready for review")?;
    tx.commit()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_images_and_decodes_real_raster_bytes() {
        assert!(decode(&json!({"data":[{"b64_json":"invalid"}]})).is_err());
        assert!(decode(&json!({"data":[]})).is_err());
        let image = image::RgbaImage::new(8, 12);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.get_ref());
        let (original, w, h, ext) = decode(&json!({"data":[{"b64_json":encoded}]})).unwrap();
        assert_eq!(original, *bytes.get_ref());
        assert_eq!((w, h, ext), (8, 12, "png"));
    }
    #[test]
    fn queued_profiles_are_frozen_and_dirty_stale_or_divergent_forms_cannot_queue() {
        let root =
            std::env::temp_dir().join(format!("df-portrait-profile-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let profile = crate::character_profile::CharacterProfile {
            summary: "A shy herbalist".into(),
            fields: std::collections::BTreeMap::from([
                ("hair_color".into(), "Auburn".into()),
                ("weight".into(), "64 kg".into()),
            ]),
        };
        let entry = store
            .create_profile_character(&series.id, "Ari", &profile, None, Default::default())
            .unwrap();
        let saved = store.character_profile(&entry.id).unwrap();
        let mut dirty = saved.clone();
        dirty.profile.fields.insert("weight".into(), "70 kg".into());
        assert!(
            store
                .enqueue_grok_portrait_with_profile(&entry.id, "Portrait", Some(&dirty))
                .is_err()
        );
        assert!(store.jobs().unwrap().is_empty());
        let job = store
            .enqueue_grok_portrait_with_profile(
                &entry.id,
                "Old direction: black hair",
                Some(&saved),
            )
            .unwrap();
        let current = store.save_character_profile(&dirty).unwrap();
        assert!(
            store
                .enqueue_grok_portrait_with_profile(&entry.id, "Portrait", Some(&saved))
                .is_err()
        );
        let next = store.grok_portrait_request(&entry.id, "Portrait").unwrap();
        assert!(next.prompt.contains("Weight: 70 kg"));
        drop(store);
        let store = Store::open(path).unwrap();
        let raw: String = store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [&job.id], |row| {
                row.get(0)
            })
            .unwrap();
        let request: PortraitRequest = serde_json::from_str(&raw).unwrap();
        assert_eq!(request.character_profile, Some(profile));
        assert!(request.prompt.contains("Weight: 64 kg"));
        assert!(!request.prompt.contains("Weight: 70 kg"));
        assert!(
            request
                .prompt
                .contains("take precedence over conflicting details")
        );
        assert_eq!(
            request.source_prompt.as_deref(),
            Some("Old direction: black hair")
        );
        // Plain-text edits cannot silently bypass the structured form's newer traits.
        let mut edit = current.brief;
        edit.description = "Changed outside the form".into();
        store.update_library_entry(&edit).unwrap();
        assert!(store.enqueue_grok_portrait(&entry.id, "Portrait").is_err());
        let legacy = serde_json::json!({"prompt":"An old immutable prompt","library_id":entry.id,"library_reference_before":null,"library_context_before":{}});
        let legacy: PortraitRequest = serde_json::from_value(legacy).unwrap();
        assert!(legacy.character_profile.is_none());
        assert_eq!(legacy.prompt, "An old immutable prompt");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn fixture_server(body: String, status: &str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut block = [0; 4096];
            loop {
                let n = stream.read(&mut block).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&block[..n]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: http://{address}/credential-trap\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (format!("http://{address}/images"), thread)
    }
    #[test]
    fn actual_http_execution_preserves_bytes_and_late_cancellation_without_selecting_reference() {
        let root = std::env::temp_dir().join(format!("df-grok-http-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store
            .create_series("Fixture", "Not generated content")
            .unwrap();
        let entry = store
            .add_library(
                &series.id,
                "character",
                "Fixture actor",
                "Adult portrait fixture",
                None,
            )
            .unwrap();
        let mut profile = store.character_profile(&entry.id).unwrap();
        for section in crate::character_profile::SECTIONS {
            for (key, _, _) in section.fields {
                profile
                    .profile
                    .fields
                    .insert((*key).into(), format!("fixture-value-{key}"));
            }
        }
        store.save_character_profile(&profile).unwrap();
        let preview = store
            .grok_portrait_request(&entry.id, "Explicit local HTTP fixture")
            .unwrap();
        let image = image::RgbaImage::new(8, 12);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        for canceled in [false, true] {
            let job = store
                .enqueue_grok_portrait(&entry.id, "Explicit local HTTP fixture")
                .unwrap();
            store
                .conn
                .execute(
                    "UPDATE jobs SET status='running',attempts=1 WHERE id=?1",
                    [&job.id],
                )
                .unwrap();
            if canceled {
                store.conn.execute_batch("CREATE TEMP TRIGGER cancel_grok_after_insert AFTER INSERT ON assets BEGIN UPDATE jobs SET cancel_requested=1 WHERE id=NEW.job_id; END;").unwrap();
            }
            let body=json!({"data":[{"b64_json":base64::engine::general_purpose::STANDARD.encode(bytes.get_ref())}],"usage":{"cost":0}}).to_string();
            let (endpoint, thread) = fixture_server(body, "200 OK");
            execute_with_transport(&store, &job.id, &endpoint, "local-fixture-key").unwrap();
            let sent = thread.join().unwrap();
            assert!(sent.starts_with("POST /images "));
            assert!(sent.contains("local-fixture-key"));
            assert!(sent.contains("x-ai/grok-imagine-image-2.0"));
            let payload: Value =
                serde_json::from_str(sent.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(payload["prompt"], preview.prompt);
            let submitted = payload["prompt"].as_str().unwrap();
            assert!(submitted.contains(&profile.profile.summary));
            for value in profile.profile.fields.values() {
                assert!(submitted.contains(value));
            }
            let saved = store.job(&job.id).unwrap();
            assert_eq!(
                saved.status,
                if canceled { "canceled" } else { "completed" }
            );
            let assets = store.assets().unwrap();
            let asset = assets
                .iter()
                .find(|asset| Some(&asset.id) == saved.output_id.as_ref())
                .unwrap();
            assert_eq!(std::fs::read(&asset.path).unwrap(), *bytes.get_ref());
            assert_eq!(asset.review, "unreviewed");
            assert_eq!(
                asset.metadata["request"]["character_profile"],
                serde_json::to_value(&profile.profile).unwrap()
            );
            assert!(
                store.library(&series.id, "character").unwrap()[0]
                    .reference
                    .is_none()
            );
            assert!(store.retry(&job.id).is_err());
            if canceled {
                store
                    .conn
                    .execute_batch("DROP TRIGGER cancel_grok_after_insert;")
                    .unwrap();
            }
        }
        for (status, body) in [
            ("200 OK", r#"{"data":[{"b64_json":"not valid"}]}"#),
            ("302 Found", ""),
            (
                "401 Unauthorized",
                r#"{"error":"fixture credential must not appear in error"}"#,
            ),
        ] {
            let job = store
                .enqueue_grok_portrait(&entry.id, "Failure fixture")
                .unwrap();
            store
                .conn
                .execute(
                    "UPDATE jobs SET status='running',attempts=1 WHERE id=?1",
                    [&job.id],
                )
                .unwrap();
            let (endpoint, thread) = fixture_server(body.into(), status);
            let error = execute_with_transport(&store, &job.id, &endpoint, "local-fixture-key")
                .unwrap_err()
                .to_string();
            assert!(!error.contains("local-fixture-key"));
            thread.join().unwrap();
            assert!(store.retry(&job.id).is_err());
            assert!(store.job(&job.id).unwrap().output_id.is_none());
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
