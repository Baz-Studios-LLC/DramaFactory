//! Non-destructive picture edits. Dialogue retains its own timeline placement.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShotTrim {
    pub shot_id: String,
    pub source_start: f64,
    pub duration: f64,
    pub expected_source_start: f64,
    pub expected_duration: f64,
    pub expected_asset_id: String,
}
impl crate::Studio {
    pub(crate) fn picture_trim_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        shot: &crate::production::Shot,
    ) {
        let Some(asset_id) = shot.asset_id.as_ref() else {
            return;
        };
        ui.label(
            eframe::egui::RichText::new(format!(
                "Source {:.2}–{:.2}s",
                shot.source_start,
                shot.source_start + shot.duration
            ))
            .size(10.)
            .color(crate::MUTED),
        );
        ui.push_id(&shot.id, |ui| {
            ui.collapsing("Trim picture", |ui| {
                let mut draft = self
                    .shot_trim_drafts
                    .get(&shot.id)
                    .cloned()
                    .unwrap_or(ShotTrim {
                        shot_id: shot.id.clone(),
                        source_start: shot.source_start,
                        duration: shot.duration,
                        expected_source_start: shot.source_start,
                        expected_duration: shot.duration,
                        expected_asset_id: asset_id.clone(),
                    });
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("In");
                    changed |= ui
                        .add(
                            eframe::egui::DragValue::new(&mut draft.source_start)
                                .range(0. ..=1800.)
                                .speed(1. / 24.)
                                .suffix(" s"),
                        )
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Length");
                    changed |= ui
                        .add(
                            eframe::egui::DragValue::new(&mut draft.duration)
                                .range(0.04..=30.)
                                .speed(1. / 24.)
                                .suffix(" s"),
                        )
                        .changed();
                });
                if changed {
                    self.shot_trim_drafts.insert(shot.id.clone(), draft.clone());
                }
                if ui.button("Apply picture trim").clicked() {
                    let result = self.store.trim_shot(&draft);
                    if result.is_ok() {
                        self.shot_trim_drafts.remove(&shot.id);
                    }
                    self.result(
                        result.map(|_| ()),
                        "Picture trim saved; original take preserved",
                    );
                }
                if ui.small_button("Reload saved range").clicked() {
                    self.shot_trim_drafts.remove(&shot.id);
                }
                ui.label(
                    eframe::egui::RichText::new(
                        "Dialogue keeps its own timing. Review lip sync after trimming picture.",
                    )
                    .size(10.)
                    .color(crate::MUTED),
                );
            })
        });
    }
}
impl Store {
    pub fn trim_shot(&self, trim: &ShotTrim) -> Result<crate::production::Shot> {
        if !trim.source_start.is_finite()
            || trim.source_start < 0.
            || !trim.duration.is_finite()
            || !(0.04..=30.).contains(&trim.duration)
        {
            bail!("Picture trim needs a finite in-point and a duration from 0.04 to 30 seconds");
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let shot = self.shot(&trim.shot_id)?;
        if shot.source_start != trim.expected_source_start
            || shot.duration != trim.expected_duration
            || shot.asset_id.as_deref() != Some(trim.expected_asset_id.as_str())
        {
            bail!(
                "Shot timing or selected take changed in another session; keep your draft and reload before trimming"
            );
        }
        let assets = self.assets()?;
        let asset = assets
            .iter()
            .find(|asset| asset.id == trim.expected_asset_id)
            .context("Selected take not found")?;
        if asset.width == 0
            || !asset.duration.is_finite()
            || !asset.path.is_file()
            || trim.source_start + trim.duration > asset.duration + 0.001
        {
            bail!("Picture range extends beyond the selected video take");
        }
        self.picture_fades(&trim.shot_id)?.validate(trim.duration)?;
        tx.execute(
            "UPDATE shots SET source_start=?2,duration=?3 WHERE id=?1",
            params![trim.shot_id, trim.source_start, trim.duration],
        )?;
        let saved = self.shot(&trim.shot_id)?;
        tx.commit()?;
        Ok(saved)
    }
}
