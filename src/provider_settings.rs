//! Hosted-provider setup. Credentials never enter SQLite, drafts, or diagnostics.
use anyhow::{Result, bail};
#[derive(Default)]
pub struct ProviderSettings {
    pub(crate) tab: usize,
    fal_key: String,
    fal_status: String,
    key_input: String,
    openai_key: String,
    buffer_key: String,
    buffer_status: String,
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
fn key_status(ui: &mut eframe::egui::Ui, provider: crate::credentials::Provider) {
    use eframe::egui;
    let saved = provider.saved_locally();
    let color = if saved {
        egui::Color32::from_rgb(97, 206, 139)
    } else {
        egui::Color32::from_rgb(242, 115, 115)
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12., 16.), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4., color);
        ui.label(egui::RichText::new(if saved { "Key saved" } else { "No saved key" }).size(12.).color(color))
            .on_hover_text("Saved on this Mac only. This indicator does not test authentication or account credits.");
    });
}
fn provider_heading(
    ui: &mut eframe::egui::Ui,
    name: &str,
    description: &str,
    provider: crate::credentials::Provider,
) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(name);
        ui.add_space(8.);
        key_status(ui, provider);
    });
    ui.weak(description);
    ui.add_space(12.);
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
        if self.provider_settings.tab == 1 {
            Self::card(ui, |ui| {
                ui.heading("Sidekick");
                let connection = self.provider_settings.sidekick_connection.as_mut().unwrap();
                ui.horizontal_wrapped(|ui| {
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
                    ui.horizontal_wrapped(|ui| {
                        ui.label("OpenAI API key");
                        key_status(ui, crate::credentials::Provider::OpenAi);
                    });
                    ui.add(
                        eframe::egui::TextEdit::singleline(&mut self.provider_settings.openai_key)
                            .password(true)
                            .hint_text("Paste your OpenAI API key")
                            .desired_width(ui.available_width()),
                    );
                    ui.horizontal_wrapped(|ui| {
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
                                    self.provider_settings.sidekick_status = match connection.save()
                                    {
                                        Ok(()) => {
                                            "OpenAI key saved. Sidekick now uses OpenAI direct."
                                                .into()
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
                                self.provider_settings.sidekick_status =
                                    "Checking saved key…".into();
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
                "WAN uses the provider selected in Connections. Image generation uses OpenRouter. Keys stay on this Mac.",
            );
            });
            return;
        }
        if self.provider_settings.tab != 0 {
            return;
        }
        Self::card(ui, |ui| {
            ui.heading("WAN video provider");
            ui.weak("Applies to new WAN jobs in the current project. Queued jobs keep their original provider.");
            let before = self.store.wan_uses_fal();
            let mut fal = before;
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut fal, false, "OpenRouter");
                ui.selectable_value(&mut fal, true, "fal.ai");
            });
            if fal != before {
                match self.store.set_wan_provider(fal) {
                    Ok(()) => {
                        self.video_planner.reset_provider_catalog();
                        self.provider_settings.fal_status.clear();
                    }
                    Err(error) => self.provider_settings.fal_status = error.to_string(),
                }
            }
            if fal {
                ui.small("WAN 3.0 · prompt, references and first/last frames · project aspect ratio · generated audio");
            }
        });
        ui.add_space(12.);
        ui.label("Connect your providers");
        ui.weak("Keys are stored only on this Mac, outside iCloud projects and Git. Saving a key never starts generation.");
        ui.add_space(12.);
        Self::card(ui, |ui| {
            provider_heading(
                ui,
                "OpenRouter",
                "Video, image generation, and optional Sidekick access.",
                crate::credentials::Provider::OpenRouter,
            );
            ui.add_space(10.);
            ui.label("OPENROUTER API KEY");
            ui.add(
                eframe::egui::TextEdit::singleline(&mut self.provider_settings.key_input)
                    .password(true)
                    .hint_text(crate::theme::placeholder("Paste your OpenRouter key"))
                    .desired_width(ui.available_width()),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.provider_settings.key_input.trim().is_empty()
                            && self.provider_settings.key_pending.is_none(),
                        crate::theme::primary("Save key"),
                    )
                    .clicked()
                {
                    match validate_key(&self.provider_settings.key_input)
                        .and_then(crate::credentials::save)
                    {
                        Ok(()) => {
                            self.provider_settings.key_input.clear();
                            self.provider_settings.status =
                                "Key saved privately on this Mac. Connection has not been verified."
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
                        self.provider_settings.status="Checking saved key…".into();
                        std::thread::spawn(move || {
                            let result = match action {
                                0 => crate::credentials::read().map(|key|if key.is_some(){"An OpenRouter key is saved on this Mac. Authentication has not been checked.".into()}else{"No OpenRouter key saved.".into()}),
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
            ui.small("Stored in owner-only local files, outside iCloud, projects and exports. Saving does not make an API request or start a paid generation.");
            ui.add_space(10.);
            ui.collapsing("Available models & diagnostics", |ui| {
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
            });
        });
        ui.add_space(12.);
        Self::card(ui, |ui| {
            provider_heading(
                ui,
                "fal.ai",
                "WAN 3.0 video generation, using the provider selected above.",
                crate::credentials::Provider::Fal,
            );
            let settings = &mut self.provider_settings;
            ui.add(
                eframe::egui::TextEdit::singleline(&mut settings.fal_key)
                    .password(true)
                    .hint_text("Paste your fal API key")
                    .desired_width(ui.available_width()),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !settings.fal_key.trim().is_empty(),
                        crate::theme::primary("Save key"),
                    )
                    .clicked()
                {
                    settings.fal_status = match validate_key(&settings.fal_key)
                        .and_then(|key| crate::credentials::Provider::Fal.save(key))
                    {
                        Ok(()) => {
                            settings.fal_key.clear();
                            "Key saved on this Mac. No request was sent to fal.".into()
                        }
                        Err(error) => error.to_string(),
                    };
                }
                ui.hyperlink_to("Get API key", "https://fal.ai/dashboard/keys");
                if ui.button("Remove key").clicked() {
                    settings.fal_status = match crate::credentials::Provider::Fal.remove() {
                        Ok(()) => {
                            settings.fal_key.clear();
                            "Key removed from this Mac.".into()
                        }
                        Err(error) => error.to_string(),
                    };
                }
            });
            if !settings.fal_status.is_empty() {
                ui.label(&settings.fal_status);
            }
        });
        ui.add_space(12.);
        Self::card(ui, |ui| {
            provider_heading(
                ui,
                "ElevenLabs",
                "Instrumental background music for your episodes.",
                crate::credentials::Provider::ElevenLabs,
            );
            let settings = &mut self.provider_settings;
            ui.add(
                eframe::egui::TextEdit::singleline(&mut settings.music_key)
                    .password(true)
                    .hint_text("Paste your ElevenLabs API key")
                    .desired_width(ui.available_width()),
            );
            ui.horizontal_wrapped(|ui| {
                let provider = crate::credentials::Provider::ElevenLabs;
                if ui
                    .add_enabled(
                        !settings.music_key.trim().is_empty(),
                        crate::theme::primary("Save key"),
                    )
                    .clicked()
                {
                    settings.music_status = match validate_key(&settings.music_key)
                        .and_then(|key| provider.save(key))
                    {
                        Ok(()) => {
                            settings.music_key.clear();
                            "Saved privately on this Mac".into()
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
        ui.add_space(12.);
        Self::card(ui, |ui| {
            provider_heading(
                ui,
                "Buffer",
                "Publishing account for TikTok, Instagram Reels and YouTube Shorts.",
                crate::credentials::Provider::Buffer,
            );
            let settings = &mut self.provider_settings;
            ui.add(
                eframe::egui::TextEdit::singleline(&mut settings.buffer_key)
                    .password(true)
                    .hint_text("Paste your Buffer API key")
                    .desired_width(ui.available_width()),
            );
            ui.horizontal_wrapped(|ui| {
                let provider = crate::credentials::Provider::Buffer;
                if ui
                    .add_enabled(
                        !settings.buffer_key.trim().is_empty(),
                        crate::theme::primary("Save key"),
                    )
                    .clicked()
                {
                    settings.buffer_status = match validate_key(&settings.buffer_key)
                        .and_then(|key| provider.save(key))
                    {
                        Ok(()) => {
                            settings.buffer_key.clear();
                            "Saved privately on this Mac".into()
                        }
                        Err(e) => e.to_string(),
                    };
                }
                if ui.button("Remove key").clicked() {
                    settings.buffer_status = match provider.remove() {
                        Ok(()) => {
                            settings.buffer_key.clear();
                            "Key removed".into()
                        }
                        Err(e) => e.to_string(),
                    };
                }
                ui.weak(&settings.buffer_status);
            });
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
