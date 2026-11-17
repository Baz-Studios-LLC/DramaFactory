//! Keep list markers and wrapped text in separate, top-aligned columns.
use eframe::egui;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::ops::Range;
struct List {
    range: Range<usize>,
    items: Vec<(String, Range<usize>)>,
}
fn lists(text: &str) -> Vec<List> {
    let mut lists = Vec::<List>::new();
    let mut depth = 0;
    let mut number = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::List(start)) => {
                if depth == 0 {
                    number = start;
                    lists.push(List {
                        range,
                        items: Vec::new(),
                    });
                }
                depth += 1;
            }
            Event::End(TagEnd::List(_)) => depth -= 1,
            Event::Start(Tag::Item) if depth == 1 => {
                let marker = if let Some(value) = number {
                    number = Some(value + 1);
                    format!("{value}.")
                } else {
                    "•".into()
                };
                lists
                    .last_mut()
                    .expect("inside list")
                    .items
                    .push((marker, range));
            }
            _ => {}
        }
    }
    lists
}
fn body(text: &str) -> &str {
    let text = text.trim_start();
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let marker = if text.starts_with(['-', '*', '+']) {
        1
    } else if digits > 0 && text[digits..].starts_with(['.', ')']) {
        digits + 1
    } else {
        0
    };
    if marker > 0 && (text.len() == marker || text[marker..].starts_with(char::is_whitespace)) {
        return text[marker..].trim();
    }
    text.trim_end()
}
pub fn show(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, text: &str) {
    // Raw provider output can contain a whole edit envelope on one line.
    // Lay those lines out with egui's character-level fallback wrapping.
    if text.split_whitespace().any(|token| token.len() > 1024)
        || text.contains("The returned text is retained below for review;")
    {
        ui.add(egui::Label::new(text).wrap().selectable(true));
        return;
    }
    let mut cursor = 0;
    for list in lists(text) {
        ordinary(ui, cache, text[cursor..list.range.start].trim());
        for (marker, range) in list.items {
            ui.horizontal_top(|ui| {
                ui.add_sized(
                    [22., ui.text_style_height(&egui::TextStyle::Body)],
                    egui::Label::new(marker),
                );
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    let source = &text[range];
                    let content = body(source);
                    if content.len() < source.len() {
                        show(ui, cache, content);
                    } else {
                        ordinary(ui, cache, content);
                    }
                });
            });
            ui.add_space(4.);
        }
        cursor = list.range.end;
    }
    ordinary(ui, cache, text[cursor..].trim());
}
fn ordinary(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, text: &str) {
    if text.is_empty() {
        return;
    }
    ui.scope(|ui| {
        // Line wrapping should use text leading, not the studio's control spacing.
        ui.spacing_mut().item_spacing.y = 0.;
        egui_commonmark::CommonMarkViewer::new().show(ui, cache, text);
    });
}
