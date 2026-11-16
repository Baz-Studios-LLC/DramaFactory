//! Shared visual language for every studio workspace.
use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, Vec2};
pub const SURFACE: Color32 = Color32::from_rgb(25, 27, 32);
pub const BORDER: Color32 = Color32::from_rgb(47, 49, 56);
pub const TEXT: Color32 = Color32::from_rgb(233, 232, 228);
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Use the system's own face on Mac, without redistributing Apple font assets.
    #[cfg(target_os = "macos")]
    for (name, path, family) in [
        (
            "studio-sans",
            "/System/Library/Fonts/SFNS.ttf",
            FontFamily::Proportional,
        ),
        (
            "studio-mono",
            "/System/Library/Fonts/SFNSMono.ttf",
            FontFamily::Monospace,
        ),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.into());
        }
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = crate::BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = Color32::from_rgb(19, 21, 25);
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(64, 54, 38);
    style.visuals.selection.stroke = Stroke::new(1_f32, crate::GOLD);
    style.visuals.window_stroke = Stroke::new(1_f32, BORDER);
    style.visuals.window_corner_radius = 14.into();
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.noninteractive,
    ] {
        widget.bg_fill = SURFACE;
        widget.weak_bg_fill = Color32::from_rgb(32, 34, 40);
        widget.bg_stroke = Stroke::new(1_f32, BORDER);
        widget.fg_stroke = Stroke::new(1_f32, TEXT);
        widget.corner_radius = 7.into();
    }
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(49, 48, 44);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(49, 48, 44);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1_f32, Color32::from_rgb(111, 96, 69));
    style.visuals.widgets.hovered.corner_radius = 7.into();
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(67, 56, 39);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(67, 56, 39);
    style.visuals.widgets.active.corner_radius = 7.into();
    style.spacing.item_spacing = Vec2::new(10., 10.);
    style.spacing.button_padding = Vec2::new(13., 8.);
    style.spacing.interact_size = Vec2::new(36., 34.);
    style.spacing.text_edit_width = 260.;
    style.visuals.text_cursor.stroke = Stroke::new(2_f32, crate::GOLD);
    for (kind, size) in [
        (egui::TextStyle::Body, 14.),
        (egui::TextStyle::Button, 13.),
        (egui::TextStyle::Small, 11.),
        (egui::TextStyle::Heading, 21.),
    ] {
        style.text_styles.insert(kind, FontId::proportional(size));
    }
    ctx.set_style(style);
}
pub fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .color(Color32::from_rgb(25, 23, 19))
            .strong(),
    )
    .fill(crate::GOLD)
    .stroke(Stroke::NONE)
}

/// Explicit color keeps placeholders subdued even with the global text override.
pub fn placeholder(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).color(Color32::from_rgb(125, 128, 138))
}
