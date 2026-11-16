//! Derived thumbnails and compact take cards. Original videos are never modified.
use crate::production::Asset;
use eframe::egui;
use std::{collections::HashMap, path::PathBuf, sync::mpsc};
#[derive(Default)]
pub struct Thumbnails {
    paths: HashMap<String, Option<PathBuf>>,
    pending: Option<mpsc::Receiver<(String, Option<PathBuf>)>>,
}
impl Thumbnails {
    pub fn get(
        &mut self,
        ui: &egui::Ui,
        asset: &Asset,
        root: &std::path::Path,
        ffmpeg: &std::path::Path,
    ) -> Option<PathBuf> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((key, path)) => {
                    self.paths.insert(key, path);
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let key = format!("{}-{}", asset.id, asset.hash);
        if !self.paths.contains_key(&key) && self.pending.is_none() {
            let output = root
                .join("cache/take-thumbnails")
                .join(format!("{}.jpg", asset.id));
            let source = asset.path.clone();
            let ffmpeg = ffmpeg.to_path_buf();
            let key = key.clone();
            let ctx = ui.ctx().clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<PathBuf> {
                    if output.is_file() {
                        return Ok(output);
                    }
                    std::fs::create_dir_all(output.parent().unwrap())?;
                    let mut child = std::process::Command::new(ffmpeg)
                        .args(["-v", "error", "-nostdin", "-i"])
                        .arg(source)
                        .args(["-frames:v", "1", "-vf", "scale=320:-2", "-y"])
                        .arg(&output)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()?;
                    let started = std::time::Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(status)) if status.success() && output.is_file() => {
                                return Ok(output);
                            }
                            Ok(Some(_)) => {
                                let _ = std::fs::remove_file(&output);
                                anyhow::bail!("Thumbnail unavailable");
                            }
                            Ok(None) if started.elapsed().as_secs() < 15 => {
                                std::thread::sleep(std::time::Duration::from_millis(30))
                            }
                            _ => {
                                let _ = child.kill();
                                let _ = child.wait();
                                let _ = std::fs::remove_file(&output);
                                anyhow::bail!("Thumbnail timed out");
                            }
                        }
                    }
                })()
                .ok();
                let _ = tx.send((key, result));
                ctx.request_repaint();
            });
        }
        if self.pending.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.paths.get(&key).cloned().flatten()
    }
}
pub fn card(
    ui: &mut egui::Ui,
    asset: &Asset,
    title: &str,
    thumbnail: Option<PathBuf>,
    width: f32,
    selected: bool,
    open: bool,
) -> (bool, bool) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, width * 1.2 + 52.), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let painter = ui.painter();
    painter.rect_filled(rect, 10, crate::BG);
    let image_rect = egui::Rect::from_min_size(
        rect.min + egui::vec2(6., 6.),
        egui::vec2(width - 12., width * 1.2 - 6.),
    );
    let missing = thumbnail.is_none();
    if let Some(path) = thumbnail {
        egui::Image::from_uri(format!("file://{}", path.display()))
            .corner_radius(6)
            .paint_at(ui, {
                let aspect = asset.width as f32 / asset.height.max(1) as f32;
                let width = image_rect.width().min(image_rect.height() * aspect);
                egui::Rect::from_center_size(
                    image_rect.center(),
                    egui::vec2(width, width / aspect.max(0.01)),
                )
            });
    }
    if response.hovered() || missing {
        let c = image_rect.center();
        painter.circle_filled(c, 18., egui::Color32::from_black_alpha(170));
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(-5., -9.),
                c + egui::vec2(-5., 9.),
                c + egui::vec2(9., 0.),
            ],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
    painter.text(
        image_rect.right_bottom() - egui::vec2(5., 5.),
        egui::Align2::RIGHT_BOTTOM,
        format!("{:.1}s", asset.duration),
        egui::FontId::proportional(12.),
        egui::Color32::WHITE,
    );
    let text = painter.layout(
        title.into(),
        egui::FontId::proportional(12.),
        ui.visuals().text_color(),
        width - 12.,
    );
    painter.galley(
        egui::pos2(rect.left() + 6., image_rect.bottom() + 8.),
        text,
        ui.visuals().text_color(),
    );
    if selected {
        painter.text(
            image_rect.left_top() + egui::vec2(5., 5.),
            egui::Align2::LEFT_TOP,
            "In episode",
            egui::FontId::proportional(12.),
            crate::GOLD,
        );
    }
    painter.rect_stroke(
        rect,
        10,
        egui::Stroke::new(
            if open || selected { 2. } else { 1. },
            if open || selected || response.hovered() {
                crate::GOLD
            } else {
                crate::theme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    let button = egui::Rect::from_min_size(
        rect.right_top() + egui::vec2(-38., 8.),
        egui::vec2(30., 30.),
    );
    let delete = ui.interact(
        button,
        ui.make_persistent_id(("delete-take", &asset.id)),
        egui::Sense::click(),
    );
    delete.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Delete clip"));
    if ui.rect_contains_pointer(rect) || response.has_focus() || delete.has_focus() {
        painter.rect_filled(
            button,
            7,
            if delete.hovered() {
                egui::Color32::from_rgb(85, 40, 35)
            } else {
                egui::Color32::from_black_alpha(210)
            },
        );
        crate::portrait_gallery::draw_icon(
            ui,
            button.center(),
            2,
            egui::Color32::from_rgb(255, 169, 149),
        );
    }
    let over_delete = ui.rect_contains_pointer(button);
    let deleting = delete
        .on_hover_text("Delete clip")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked();
    let playing = response.clicked() && !over_delete && !deleting;
    if !over_delete {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Play clip");
    }
    (playing, deleting)
}
