use eframe::egui;
pub fn name(id: &str) -> String {
    let raw = id.rsplit('_').next().unwrap_or(id);
    let mut chars = raw.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
pub fn picker(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    voice: &mut String,
    assigned: &[String],
) -> bool {
    let before = voice.clone();
    let mut choices = assigned.to_vec();
    choices.extend(
        [
            "af_heart",
            "af_bella",
            "af_sarah",
            "am_adam",
            "am_michael",
            "bf_emma",
            "bm_george",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    if !choices.contains(voice) {
        choices.push(voice.clone());
    }
    choices.sort();
    choices.dedup();
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name(voice))
        .show_ui(ui, |ui| {
            for choice in choices {
                let label = name(&choice);
                ui.selectable_value(voice, choice.clone(), label)
                    .on_hover_text(choice);
            }
        });
    *voice != before
}
