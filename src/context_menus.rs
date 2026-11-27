use crate::{Studio, production::LibraryEntry};
use anyhow::Result;
use eframe::egui;
use std::path::Path;

/// Protect the transcript's selection while a popup takes pointer focus.
pub(crate) struct HistoryCopyMenu {
    pointer: Option<egui::PointerState>,
    copy_event: Option<usize>,
    cache_id: egui::Id,
}
impl HistoryCopyMenu {
    pub(crate) fn finish(self, ui: &mut egui::Ui) {
        let selection = ui
            .ctx()
            .plugin::<egui::text_selection::LabelSelectionState>();
        let saved = selection.lock().clone();
        ui.data_mut(|data| data.insert_temp(self.cache_id, saved));
        if let Some(pointer) = self.pointer {
            ui.input_mut(|input| input.pointer = pointer);
        }
        if let Some(index) = self.copy_event {
            ui.input_mut(|input| {
                input.events.remove(index);
            });
        }
    }
}
pub(crate) fn history_copy_menu(ui: &mut egui::Ui) -> HistoryCopyMenu {
    use egui::{Plugin, text_selection::LabelSelectionState};
    let id = ui.id().with("copy-highlight");
    let cache_id = ui.id().with("transcript-selection");
    let selection = ui.ctx().plugin::<LabelSelectionState>();
    let mut guard = HistoryCopyMenu {
        pointer: None,
        copy_event: None,
        cache_id,
    };
    // Earlier labels can clear the shared selection on a secondary press, before
    // the transcript is rendered. Keep its own previous-frame snapshot.
    if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        && ui.clip_rect().contains(pos)
        && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary))
    {
        let saved = ui
            .data(|d| d.get_temp::<LabelSelectionState>(cache_id))
            .filter(|saved| saved.has_selection())
            .or_else(|| {
                let saved = selection.lock().clone();
                saved.has_selection().then_some(saved)
            });
        if let Some(saved) = saved {
            ui.data_mut(|d| d.insert_temp(id, (pos, saved)));
            egui::Popup::open_id(ui.ctx(), id);
        }
    }
    if !egui::Popup::is_id_open(ui.ctx(), id) {
        ui.data_mut(|d| d.remove::<(egui::Pos2, LabelSelectionState)>(id));
        return guard;
    }
    let Some((pos, mut saved)) = ui.data(|d| d.get_temp::<(egui::Pos2, LabelSelectionState)>(id))
    else {
        return guard;
    };
    let close_behavior = if ui.input(|i| {
        i.pointer.button_down(egui::PointerButton::Secondary)
            || i.pointer.button_released(egui::PointerButton::Secondary)
    }) {
        egui::PopupCloseBehavior::IgnoreClicks
    } else {
        egui::PopupCloseBehavior::CloseOnClickOutside
    };
    let clicked = egui::Popup::new(id, ui.ctx().clone(), pos, ui.layer_id())
        .close_behavior(close_behavior)
        .open_memory(None)
        .show(|ui| {
            let clicked = ui.button("Copy").clicked();
            if clicked {
                ui.close();
            }
            clicked
        })
        .is_some_and(|response| response.inner);
    // Reset per-pass paint/copy bookkeeping, retaining only the highlighted range.
    saved.on_begin_pass(ui.ctx());
    *selection.lock() = saved;
    guard.pointer = Some(ui.input_mut(|input| std::mem::take(&mut input.pointer)));
    if clicked {
        guard.copy_event = Some(ui.input_mut(|input| {
            let index = input.events.len();
            input.events.push(egui::Event::Copy);
            index
        }));
    }
    guard
}

pub(crate) fn text_menu(
    output: &mut egui::text_edit::TextEditOutput,
    value: &mut String,
    images: bool,
) -> bool {
    let mut paste_image = false;
    let context = output.response.ctx.clone();
    let selection_id = output.response.id.with("context-menu-selection");
    let opening = output.response.contains_pointer()
        && context.input(|input| input.pointer.button_pressed(egui::PointerButton::Secondary));
    let menu_open = output.response.context_menu_opened();
    // TextEdit moves the cursor on any pointer press, including secondary.
    // Restore the previous frame's selection before building the context menu.
    if (opening || menu_open)
        && let Some(saved) =
            context.data(|data| data.get_temp::<egui::text::CCursorRange>(selection_id))
    {
        output.state.cursor.set_char_range(Some(saved));
        output.cursor_range = output.state.cursor.range(&output.galley);
        output.state.cursor.set_char_range(output.cursor_range);
        output.state.clone().store(&context, output.response.id);
    } else if let Some(range) = output.state.cursor.range(&output.galley) {
        context.data_mut(|data| data.insert_temp(selection_id, range));
    }
    // State retains the selection even when the menu takes keyboard focus.
    let range = output
        .state
        .cursor
        .range(&output.galley)
        .map(|range| range.as_sorted_char_range());
    output.response.context_menu(|ui| {
        if ui
            .add_enabled(
                range.as_ref().is_some_and(|range| !range.is_empty()),
                egui::Button::new("Copy"),
            )
            .clicked()
        {
            let range = range.clone().unwrap();
            ui.ctx()
                .copy_text(value.chars().skip(range.start).take(range.len()).collect());
            ui.close();
        }
        if ui.button("Paste").clicked() {
            if let Ok(text) =
                arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text())
            {
                let range = range
                    .clone()
                    .unwrap_or(value.chars().count()..value.chars().count());
                let byte_at = |index| {
                    value
                        .char_indices()
                        .nth(index)
                        .map_or(value.len(), |(offset, _)| offset)
                };
                value.replace_range(byte_at(range.start)..byte_at(range.end), &text);
                output
                    .state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(range.start + text.chars().count()),
                    )));
                output.state.clone().store(ui.ctx(), output.response.id);
                output.response.request_focus();
            }
            ui.close();
        }
        if images && ui.button("Paste image").clicked() {
            paste_image = true;
            ui.close();
        }
        if ui.button("Select all").clicked() {
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(value.chars().count()),
                )));
            output.state.clone().store(ui.ctx(), output.response.id);
            output.response.request_focus();
            ui.close();
        }
    });
    paste_image
}

fn copy_image(path: &Path) -> Result<()> {
    crate::reference_drop::validate_reference(path)?;
    let pixels = image::open(path)?.into_rgba8();
    arboard::Clipboard::new()?.set_image(arboard::ImageData {
        width: pixels.width() as usize,
        height: pixels.height() as usize,
        bytes: std::borrow::Cow::Owned(pixels.into_raw()),
    })?;
    Ok(())
}
fn clipboard_png() -> Result<Vec<u8>> {
    #[cfg(target_os = "macos")]
    if let Some(bytes) = objc2::rc::autoreleasepool(|_| {
        objc2_app_kit::NSPasteboard::generalPasteboard()
            .dataForType(objc2_foundation::ns_string!("public.png"))
            .map(|data| data.to_vec())
    }) {
        return Ok(bytes);
    }
    let mut clipboard = arboard::Clipboard::new()?;
    if let Ok(pixels) = clipboard.get_image() {
        if pixels.width > 8192 || pixels.height > 8192 || pixels.bytes.len() > 128 * 1024 * 1024 {
            anyhow::bail!("Clipboard image is too large");
        }
        let image = image::RgbaImage::from_raw(
            pixels.width as u32,
            pixels.height as u32,
            pixels.bytes.into_owned(),
        )
        .ok_or_else(|| anyhow::anyhow!("Invalid clipboard image"))?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        return Ok(bytes.into_inner());
    }
    if let Ok(files) = clipboard.get().file_list()
        && files.len() == 1
    {
        crate::reference_drop::validate_reference(&files[0])?;
        return Ok(std::fs::read(&files[0])?);
    }
    anyhow::bail!("Copy an image or image file first.")
}
impl Studio {
    pub(crate) fn portrait_context_menu(
        &mut self,
        response: &egui::Response,
        entry: &LibraryEntry,
        portrait: Option<&Path>,
    ) {
        response.context_menu(|ui| {
            if ui
                .add_enabled(portrait.is_some(), egui::Button::new("Copy image"))
                .clicked()
            {
                self.result(copy_image(portrait.unwrap()), "Image copied");
                ui.close();
            }
            if ui.button("Paste image").clicked() {
                let result = (|| -> Result<()> {
                    let bytes = clipboard_png()?;
                    if bytes.len() > 24 * 1024 * 1024 {
                        anyhow::bail!("Image exceeds 24 MiB");
                    }
                    let path = std::env::temp_dir()
                        .join(format!("dramafactory-paste-{}.png", uuid::Uuid::new_v4()));
                    use std::io::Write;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    file.write_all(&bytes)?;
                    drop(file);
                    let result = self.store.import_character_portrait(entry, &path);
                    let _ = std::fs::remove_file(&path);
                    result
                })();
                self.result(result, "Pasted image selected as portrait");
                ui.close();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_popup_keeps_highlight_and_copies_selected_text() {
        let ctx = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        let mut popup_id = egui::Id::NULL;
        let mut frame = |events: Vec<egui::Event>| {
            let output = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label("Earlier unrelated label");
                        popup_id = ui.id().with("copy-highlight");
                        let menu = history_copy_menu(ui);
                        rect = ui
                            .add(egui::Label::new("Hello café world").selectable(true))
                            .rect;
                        menu.finish(ui);
                    });
                },
            );
            (rect, popup_id, output)
        };
        let (rect, _, _) = frame(vec![]);
        let start = rect.left_center() + egui::vec2(1., 0.);
        let end = rect.right_center() - egui::vec2(1., 0.);
        let button = |pos, button, pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![
            egui::Event::PointerMoved(start),
            button(start, egui::PointerButton::Primary, true),
        ]);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![button(end, egui::PointerButton::Primary, false)]);
        assert!(
            ctx.plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
        );
        let pos = rect.center();
        let (_, id, _) = frame(vec![
            egui::Event::PointerMoved(pos),
            button(pos, egui::PointerButton::Secondary, true),
        ]);
        assert!(egui::Popup::is_id_open(&ctx, id));
        frame(vec![button(pos, egui::PointerButton::Secondary, false)]);
        assert!(egui::Popup::is_id_open(&ctx, id));
        assert!(
            ctx.plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
        );
        let menu_pos = pos + egui::vec2(20., 15.);
        frame(vec![
            egui::Event::PointerMoved(menu_pos),
            button(menu_pos, egui::PointerButton::Primary, true),
        ]);
        let (_, _, output) = frame(vec![button(menu_pos, egui::PointerButton::Primary, false)]);
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(text) if text.contains("café"))),
            "Expected selected text in clipboard output"
        );
    }

    #[test]
    fn right_click_preserves_text_selection_for_copy() {
        let ctx = egui::Context::default();
        let mut value = "Hello café world".to_owned();
        let mut field = None;
        let mut selected = None;
        let mut frame = |events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut output = egui::TextEdit::singleline(&mut value).show(ui);
                        text_menu(&mut output, &mut value, false);
                        field = Some((output.response.id, output.response.rect));
                        selected = output.state.cursor.char_range();
                    });
                },
            );
            (field.unwrap(), selected)
        };
        let ((id, rect), _) = frame(vec![]);
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
        let selection = egui::text::CCursorRange::two(
            egui::text::CCursor::new(6),
            egui::text::CCursor::new(10),
        );
        state.cursor.set_char_range(Some(selection));
        state.store(&ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
        frame(vec![]);
        let position = rect.center();
        let (_, selected) = frame(vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        assert_eq!(selected.unwrap().as_sorted_char_range(), 6..10);
        let (_, selected) = frame(vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(selected.unwrap().as_sorted_char_range(), 6..10);
        let (_, selected) = frame(vec![]);
        assert_eq!(selected.unwrap().as_sorted_char_range(), 6..10);
    }
}
