//! Hosted-provider setup. Credentials never enter SQLite, drafts, or diagnostics.
use anyhow::{Result, bail};
#[derive(Default)]
pub struct ProviderSettings {
    key_input: String,
    openai_key: String,
    music_key: String,
    music_status: String,
    sidekick_connection: Option<crate::sidekick_provider::Connection>,
    sidekick_status: String,
    openai_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    status: String,
    catalog: Option<crate::openrouter::Catalog>,
    catalog_pending: Option<std::sync::mpsc::Receiver<Result<crate::openrouter::Catalog, String>>>,
    catalog_error: String,
    key_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
}
fn validate_key(key: &str) -> Result<&str> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        bail!("Enter an API key without spaces.");
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
        if let Some(receiver) = &self.provider_settings.openai_pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.provider_settings.openai_pending = None;
                    self.provider_settings.sidekick_status = result.unwrap_or_else(|e| e);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.provider_settings.openai_pending = None;
                    self.provider_settings.sidekick_status = "Connection check stopped.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if self.provider_settings.sidekick_connection.is_none() {
            match crate::sidekick_provider::Connection::load() {
                Ok(connection) => self.provider_settings.sidekick_connection = Some(connection),
                Err(error) => {
                    self.provider_settings.sidekick_status = error.to_string();
                    self.provider_settings.sidekick_connection = Some(Default::default());
                }
            }
        }
        Self::card(ui, |ui| {
            ui.heading("ElevenLabs Music");
            let settings = &mut self.provider_settings;
            ui.add(
                eframe::egui::TextEdit::singleline(&mut settings.music_key)
                    .password(true)
                    .hint_text("ElevenLabs API key"),
            );
            ui.horizontal(|ui| {
                let provider = crate::credentials::Provider::ElevenLabs;
                if ui.button("Save key").clicked() {
                    settings.music_status = match validate_key(&settings.music_key)
                        .and_then(|key| provider.save(key))
                    {
                        Ok(()) => {
                            settings.music_key.clear();
                            "Saved to Keychain".into()
                        }
                        Err(e) => e.to_string(),
                    };
                }
                if ui.button("Remove key").clicked() {
                    settings.music_status = match provider.remove() {
                        Ok(()) => {
                            settings.music_key.clear();
                            "Key removed".into()
                        }
                        Err(e) => e.to_string(),
                    };
                }
                ui.weak(&settings.music_status);
            });
        });
        Self::card(ui, |ui| {
            ui.heading("Sidekick");
            let connection = self.provider_settings.sidekick_connection.as_mut().unwrap();
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut connection.provider,
                    crate::sidekick_provider::Provider::OpenRouter,
                    "OpenRouter",
                );
                ui.selectable_value(
                    &mut connection.provider,
                    crate::sidekick_provider::Provider::OpenAi,
                    "OpenAI direct",
                );
            });
            if connection.provider == crate::sidekick_provider::Provider::OpenAi {
                ui.label("OpenAI API key");
                ui.add(
                    eframe::egui::TextEdit::singleline(&mut self.provider_settings.openai_key)
                        .password(true)
                        .hint_text("Paste your OpenAI API key")
                        .desired_width(460.),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.provider_settings.openai_key.trim().is_empty()
                                && self.provider_settings.openai_pending.is_none(),
                            eframe::egui::Button::new("Save key & use OpenAI"),
                        )
                        .clicked()
                    {
                        match validate_key(&self.provider_settings.openai_key)
                            .and_then(|key| crate::credentials::Provider::OpenAi.save(key))
                        {
                            Ok(()) => {
                                self.provider_settings.openai_key.clear();
                                self.provider_settings.sidekick_status = match connection.save() {
                                    Ok(()) => {
                                        "OpenAI key saved. Sidekick now uses OpenAI direct.".into()
                                    }
                                    Err(e) => {
                                        format!("Key saved, but Sidekick was not switched: {e}")
                                    }
                                };
                            }
                            Err(e) => self.provider_settings.sidekick_status = e.to_string(),
                        }
                    }
                    ui.hyperlink_to("Get API key", "https://platform.openai.com/api-keys");
                    for (label, remove) in [("Test connection", false), ("Remove key", true)] {
                        if ui
                            .add_enabled(
                                self.provider_settings.openai_pending.is_none(),
                                eframe::egui::Button::new(label),
                            )
                            .clicked()
                        {
                            let (sender, receiver) = std::sync::mpsc::channel();
                            self.provider_settings.openai_pending = Some(receiver);
                            self.provider_settings.sidekick_status = "Checking Keychain…".into();
                            std::thread::spawn(move || {
                                let result = if remove {
                                    crate::credentials::Provider::OpenAi
                                        .remove()
                                        .map(|_| "OpenAI key removed from this Mac.".into())
                                } else {
                                    crate::sidekick_provider::test_openai_connection()
                                };
                                let _ = sender.send(result.map_err(|e| e.to_string()));
                            });
                        }
                    }
                });
                ui.label("GPT-6 Astra");
                connection.openai_model = "gpt-6-astra".into();
                eframe::egui::ComboBox::from_id_salt("sidekick_effort")
                    .selected_text(format!("Effort: {}", connection.effort))
                    .show_ui(ui, |ui| {
                        for effort in ["low", "medium", "high", "xhigh", "max"] {
                            ui.selectable_value(&mut connection.effort, effort.into(), effort);
                        }
                    });
            }
            if ui.button("Save Sidekick settings").clicked() {
                self.provider_settings.sidekick_status = match connection.save() {
                    Ok(()) => "Sidekick settings saved. Applies to your next message.".into(),
                    Err(e) => e.to_string(),
                };
            }
            ui.label(&self.provider_settings.sidekick_status);
            ui.small(
                "Video and image generation use OpenRouter. Keys stay in this Mac’s Keychain.",
            );
        });
        ui.add_space(12.);
        Self::card(ui, |ui| {
            ui.heading("OpenRouter");
            ui.label("WAN video and image generation. Also used for Sidekick when selected above.");
            ui.add_space(10.);
            ui.label("OPENROUTER API KEY");
            ui.add(
                eframe::egui::TextEdit::singleline(&mut self.provider_settings.key_input)
                    .password(true)
                    .hint_text(crate::theme::placeholder("Paste your OpenRouter key"))
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
            ui.label("Video & sound: WAN · Images: Grok Imagine");
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
                    ("Video", "alibaba/wan-3.0", &catalog.videos),
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
