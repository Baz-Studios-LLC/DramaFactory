//! Hosted-provider setup. Credentials never enter SQLite, drafts, or diagnostics.
use anyhow::{Result, bail};
#[derive(Default)]
pub struct ProviderSettings {
    key_input: String,
    status: String,
    catalog: Option<crate::openrouter::Catalog>,
    catalog_pending: Option<std::sync::mpsc::Receiver<Result<crate::openrouter::Catalog, String>>>,
    catalog_error: String,
    key_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
}
fn validate_key(key: &str) -> Result<&str> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        bail!("Enter an OpenRouter API key without spaces.");
    }
    Ok(key)
}
impl crate::Studio {
    pub(crate) fn provider_settings_panel(&mut self, ui: &mut eframe::egui::Ui) {
        if let Some(receiver) = &self.provider_settings.key_pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.provider_settings.key_pending = None;
                    self.provider_settings.status = result.unwrap_or_else(|error| error);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.provider_settings.key_pending = None;
                    self.provider_settings.status = "Key check stopped. Try again.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(receiver) = &self.provider_settings.catalog_pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.provider_settings.catalog_pending = None;
                    match result {
                        Ok(catalog) => {
                            self.provider_settings.catalog = Some(catalog);
                            self.provider_settings.catalog_error.clear();
                        }
                        Err(error) => self.provider_settings.catalog_error = error,
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.provider_settings.catalog_pending = None;
                    self.provider_settings.catalog_error =
                        "Catalog check stopped. Try again.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        Self::card(ui, |ui| {
            ui.heading("OpenRouter");
            ui.label("One connection for writing, images, and video with sound.");
            ui.add_space(10.);
            ui.label("OPENROUTER API KEY");
            ui.add(
                eframe::egui::TextEdit::singleline(&mut self.provider_settings.key_input)
                    .password(true)
                    .hint_text("Paste your OpenRouter key")
                    .desired_width(ui.available_width().min(560.)),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.provider_settings.key_input.trim().is_empty()
                            && self.provider_settings.key_pending.is_none(),
                        eframe::egui::Button::new("Save key securely"),
                    )
                    .clicked()
                {
                    match validate_key(&self.provider_settings.key_input)
                        .and_then(crate::credentials::save)
                    {
                        Ok(()) => {
                            self.provider_settings.key_input.clear();
                            self.provider_settings.status =
                                "Key saved in macOS Keychain. Connection has not been verified."
                                    .into();
                        }
                        Err(error) => self.provider_settings.status = error.to_string(),
                    }
                }
                ui.hyperlink_to(
                    "Get an OpenRouter key",
                    "https://openrouter.ai/settings/keys",
                );
            });
            ui.horizontal_wrapped(|ui| {
                for (label, action) in [("Check saved key",0),("Test connection",1),("Remove saved key",2)] {
                    if ui.add_enabled(self.provider_settings.key_pending.is_none(),eframe::egui::Button::new(label)).clicked() {
                        let (sender,receiver)=std::sync::mpsc::channel();
                        self.provider_settings.key_pending=Some(receiver);
                        self.provider_settings.status="Checking Keychain…".into();
                        std::thread::spawn(move || {
                            let result = match action {
                                0 => crate::credentials::read().map(|key|if key.is_some(){"An OpenRouter key is saved in Keychain. Authentication has not been checked.".into()}else{"No OpenRouter key saved.".into()}),
                                1 => crate::credentials::test_connection(),
                                _ => crate::credentials::remove().map(|_|"Saved key removed from this Mac. This does not revoke it at OpenRouter.".into()),
                            };
                            let _=sender.send(result.map_err(|error|error.to_string()));
                        });
                    }
                }
            });
            if !self.provider_settings.status.is_empty() {
                ui.label(&self.provider_settings.status);
            }
            ui.small("Stored in macOS Keychain, separate from projects and exports. Saving does not make an API request or start a paid generation.");
            ui.add_space(10.);
            ui.label("Video & sound: Seedance · Images: Grok Imagine");
            if ui
                .add_enabled(
                    self.provider_settings.catalog_pending.is_none(),
                    eframe::egui::Button::new("Refresh available models"),
                )
                .clicked()
            {
                let (sender, receiver) = std::sync::mpsc::channel();
                self.provider_settings.catalog_pending = Some(receiver);
                std::thread::spawn(move || {
                    let _ = sender
                        .send(crate::openrouter::discover().map_err(|error| format!("{error:#}")));
                });
            }
            if self.provider_settings.catalog_pending.is_some() {
                ui.label("Reading OpenRouter catalogs…");
            }
            if !self.provider_settings.catalog_error.is_empty() {
                ui.label(&self.provider_settings.catalog_error);
            }
            if let Some(catalog) = &self.provider_settings.catalog {
                ui.small(format!("Last successful catalog: {} video models · {} image models. This does not verify your API key.",catalog.videos.len(),catalog.images.len()));
                for (label, id, models) in [
                    ("Video", "bytedance/seedance-2.5", &catalog.videos),
                    ("Images", "x-ai/grok-imagine-image-2.0", &catalog.images),
                ] {
                    if let Some(model) = models.iter().find(|model| model["id"] == id) {
                        ui.label(format!("{label}: {}", model["name"].as_str().unwrap_or(id)));
                        ui.collapsing(format!("{label} supported inputs & settings"), |ui| {
                            ui.code(serde_json::to_string_pretty(model).unwrap_or_default());
                        });
                    } else {
                        ui.label(format!(
                            "{label}: preferred model is not in the current catalog."
                        ));
                    }
                }
            }
            ui.small("Catalog checks are public and read-only. They send no project content or API key and do not start generation.");
            ui.label("Generation starts only when you explicitly request it from a workspace. Your original media remains on this Mac.");
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_reject_empty_and_embedded_whitespace_without_echoing_secret() {
        assert!(validate_key("  ").is_err());
        let invalid = "secret\nkey";
        assert!(
            !validate_key(invalid)
                .unwrap_err()
                .to_string()
                .contains(invalid)
        );
        assert_eq!(validate_key(" token-value ").unwrap(), "token-value");
    }
}
