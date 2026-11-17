//! Clipboard images remain local until the user sends their chat message.
use anyhow::{Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatImage {
    pub id: String,
    pub data_url: String,
}
impl ChatImage {
    pub fn from_rgba(width: usize, height: usize, bytes: Vec<u8>) -> Result<Self> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(bytes.len())
            || bytes.len() > 128 * 1024 * 1024
        {
            bail!("Clipboard image is too large or invalid");
        }
        let image = image::RgbaImage::from_raw(width as u32, height as u32, bytes)
            .ok_or_else(|| anyhow::anyhow!("Invalid clipboard pixels"))?;
        let mut image = image::DynamicImage::ImageRgba8(image);
        if image.width() > 2048 || image.height() > 2048 {
            image = image.thumbnail(2048, 2048);
        }
        let png = loop {
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png)?;
            if png.get_ref().len() <= 4 * 1024 * 1024 {
                break png;
            }
            // Keep the attachment within the request budget instead of silently rejecting photos.
            image = image.thumbnail(
                (image.width() * 3 / 4).max(1),
                (image.height() * 3 / 4).max(1),
            );
        };
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png.into_inner())
            ),
        })
    }
    pub fn from_encoded(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 32 * 1024 * 1024 {
            bail!("Image exceeds the 32 MiB import limit");
        }
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let rgba = reader.decode()?.into_rgba8();
        Self::from_rgba(
            rgba.width() as usize,
            rgba.height() as usize,
            rgba.into_raw(),
        )
    }
    pub fn clipboard() -> Result<Self> {
        // Browsers and screenshot tools can publish PNG without a TIFF representation.
        #[cfg(target_os = "macos")]
        if let Some(bytes) = objc2::rc::autoreleasepool(|_| {
            let board = objc2_app_kit::NSPasteboard::generalPasteboard();
            board
                .dataForType(objc2_foundation::ns_string!("public.png"))
                .map(|data| data.to_vec())
        }) {
            return Self::from_encoded(&bytes);
        }
        let mut clipboard = arboard::Clipboard::new()?;
        if let Ok(pixels) = clipboard.get_image() {
            return Self::from_rgba(pixels.width, pixels.height, pixels.bytes.into_owned());
        }
        // Finder copies file URLs, not image pixels. Read locally; never follow web URLs.
        if let Ok(paths) = clipboard.get().file_list() {
            for path in paths {
                if matches!(
                    path.extension()
                        .and_then(|s| s.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref(),
                    Some("png" | "jpg" | "jpeg" | "webp" | "tif" | "tiff")
                ) {
                    if std::fs::metadata(&path)?.len() > 32 * 1024 * 1024 {
                        bail!("Image exceeds the 32 MiB import limit");
                    }
                    return Self::from_encoded(&std::fs::read(path)?);
                }
            }
        }
        bail!(
            "No supported image on the clipboard. Copy an image, screenshot, or image file first."
        )
    }
    pub fn thumbnail(&self, ui: &mut eframe::egui::Ui) {
        if let Some(data) = self.data_url.strip_prefix("data:image/png;base64,")
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
        {
            ui.add(
                eframe::egui::Image::from_bytes(format!("bytes://chat-{}.png", self.id), bytes)
                    .max_size(eframe::egui::vec2(120., 90.)),
            );
        }
    }
}
pub fn validate(images: &[ChatImage]) -> Result<()> {
    if images.len() > 4 {
        bail!("Attach up to four images per message");
    }
    for image in images {
        if image.data_url.len() > 6 * 1024 * 1024
            || !image.data_url.starts_with("data:image/png;base64,")
        {
            bail!("Invalid chat image");
        }
        let encoded = image
            .data_url
            .strip_prefix("data:image/png;base64,")
            .expect("checked prefix");
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let dimensions =
            image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png)
                .into_dimensions()?;
        if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 2048 || dimensions.1 > 2048 {
            bail!("Chat image exceeds 2048px");
        }
    }
    Ok(())
}
impl crate::store::Store {
    pub fn chat_image_draft(&self, series: &str) -> Result<Vec<ChatImage>> {
        use rusqlite::OptionalExtension;
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("sidekick:images:{series}")],
                |r| r.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(value.as_deref().unwrap_or("[]"))?)
    }
    pub fn save_chat_image_draft(
        &self,
        series: &str,
        expected: &[ChatImage],
        images: &[ChatImage],
    ) -> Result<()> {
        validate(images)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.query_row("SELECT id FROM series WHERE id=?1", [series], |r| {
            r.get::<_, String>(0)
        })?;
        let current = self.chat_image_draft(series)?;
        if current != expected && current != images {
            bail!("Image draft changed in another window; attachments were preserved");
        }
        tx.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![format!("sidekick:images:{series}"),serde_json::to_string(images)?])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pasted_image_drafts_and_messages_survive_restart_and_keep_series_scope() {
        let root = std::env::temp_dir().join(format!("df-chat-image-{}", uuid::Uuid::new_v4()));
        let path = root.join("studio.sqlite");
        let store = crate::store::Store::open(path.clone()).unwrap();
        let a = store.create_series("A", "").unwrap();
        let b = store.create_series("B", "").unwrap();
        let image = ChatImage::from_rgba(32, 32, vec![255; 32 * 32 * 4]).unwrap();
        let images = vec![image.clone()];
        store.save_chat_image_draft(&a.id, &[], &images).unwrap();
        assert!(store.chat_image_draft(&b.id).unwrap().is_empty());
        assert!(store.save_chat_image_draft(&a.id, &[], &[]).is_err());
        let message = crate::sidekick::Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: "user".into(),
            text: "Use this costume".into(),
            context: "Characters".into(),
            images: images.clone(),
        };
        store.append_sidekick_message(&a.id, &message).unwrap();
        store.append_sidekick_message(&a.id, &message).unwrap();
        let mut changed = message.clone();
        changed.images.clear();
        assert!(store.append_sidekick_message(&a.id, &changed).is_err());
        drop(store);
        let store = crate::store::Store::open(path).unwrap();
        assert_eq!(
            store.chat_image_draft(&a.id).unwrap()[0].data_url,
            image.data_url
        );
        let loaded = store.sidekick_messages(&a.id).unwrap();
        assert_eq!(loaded.len(), 1);
        let content = crate::sidekick::message_content(&loaded[0]);
        assert_eq!(content[0]["text"], "Use this costume");
        assert_eq!(content[1]["image_url"]["url"], image.data_url);
        store
            .change_user_message(&a.id, &loaded[0], Some("Use the coat"))
            .unwrap();
        assert_eq!(store.sidekick_messages(&a.id).unwrap()[0].images.len(), 1);
        let updated = store.sidekick_messages(&a.id).unwrap().remove(0);
        store.change_user_message(&a.id, &updated, None).unwrap();
        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM sidekick_images", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(store.jobs().unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn encoded_png_and_jpeg_normalize_and_reject_invalid_data() {
        let source = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            3000,
            12,
            image::Rgb([42, 85, 170]),
        ));
        for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg] {
            let mut encoded = std::io::Cursor::new(Vec::new());
            source.write_to(&mut encoded, format).unwrap();
            let attachment = ChatImage::from_encoded(encoded.get_ref()).unwrap();
            validate(std::slice::from_ref(&attachment)).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(
                    attachment
                        .data_url
                        .strip_prefix("data:image/png;base64,")
                        .unwrap(),
                )
                .unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!(decoded.width(), 2048);
            assert!(decoded.height() > 0);
        }
        assert!(ChatImage::from_encoded(b"not an image").is_err());
        let oversized = image::DynamicImage::ImageRgb8(image::RgbImage::new(8193, 1));
        let mut encoded = std::io::Cursor::new(Vec::new());
        oversized
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(ChatImage::from_encoded(encoded.get_ref()).is_err());
    }
    #[test]
    fn rejects_invalid_pixels_and_excess_attachments() {
        assert!(ChatImage::from_rgba(2, 2, vec![0; 4]).is_err());
        let image = ChatImage::from_rgba(2, 2, vec![0; 16]).unwrap();
        assert!(validate(&vec![image; 5]).is_err());
        assert!(
            validate(&[ChatImage {
                id: "bad".into(),
                data_url: "data:image/png;base64,not-png".into()
            }])
            .is_err()
        );
    }
}
