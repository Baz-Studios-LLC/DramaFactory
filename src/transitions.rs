//! Non-overlapping picture fades preserve the episode and audio timebase.
use crate::store::Store;
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureFades {
    pub fade_in: f64,
    pub fade_out: f64,
}
impl PictureFades {
    pub fn is_empty(&self) -> bool {
        self.fade_in == 0. && self.fade_out == 0.
    }
    pub fn validate(&self, duration: f64) -> Result<()> {
        if !self.fade_in.is_finite()
            || !self.fade_out.is_finite()
            || self.fade_in < 0.
            || self.fade_out < 0.
            || self.fade_in + self.fade_out > duration + 0.000001
        {
            bail!(
                "Picture fades must be finite, nonnegative and fit inside the shot; reduce fades before shortening it"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FadeEdit {
    pub shot_id: String,
    pub fades: PictureFades,
    pub expected: PictureFades,
}
impl Store {
    pub fn picture_fades(&self, shot: &str) -> Result<PictureFades> {
        self.shot(shot)?;
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("picture_fades:{shot}")],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default())
    }
    pub fn set_picture_fades(&self, edit: &FadeEdit) -> Result<PictureFades> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = self.picture_fades(&edit.shot_id)?;
        let shot = self.shot(&edit.shot_id)?;
        edit.fades.validate(shot.duration)?;
        if current != edit.fades {
            if current != edit.expected {
                bail!(
                    "Picture fades changed in another session; keep your draft and reload before saving"
                );
            }
            self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("picture_fades:{}",edit.shot_id),serde_json::to_string(&edit.fades)?])?;
        }
        tx.commit()?;
        Ok(edit.fades.clone())
    }
}
impl crate::Studio {
    pub(crate) fn picture_fade_controls(
        &mut self,
        ui: &mut eframe::egui::Ui,
        shot: &crate::production::Shot,
    ) {
        ui.push_id((&shot.id,"fades"),|ui|ui.collapsing("Picture fades",|ui| {
            let Ok(current)=self.store.picture_fades(&shot.id) else{return;};
            let mut edit=self.fade_drafts.get(&shot.id).cloned().unwrap_or(FadeEdit{shot_id:shot.id.clone(),fades:current.clone(),expected:current.clone()});
            if current!=edit.expected {ui.label(eframe::egui::RichText::new("Newer fades are saved; your draft is preserved.").color(crate::GOLD));}
            let mut changed=false;
            for (label,value) in [("From black",&mut edit.fades.fade_in),("To black",&mut edit.fades.fade_out)] {
                ui.horizontal(|ui|{ui.label(label);changed|=ui.add(eframe::egui::DragValue::new(value).range(0. ..=shot.duration).speed(1./24.).suffix(" s")).changed();});
            }
            if changed {self.fade_drafts.insert(shot.id.clone(),edit.clone());}
            if ui.button("Apply fades").clicked() {let result=self.store.set_picture_fades(&edit);if result.is_ok(){self.fade_drafts.remove(&shot.id);}self.result(result.map(|_|()),"Picture fades saved; audio timing preserved");}
            if ui.small_button("Reload saved fades").clicked(){self.fade_drafts.remove(&shot.id);}
            ui.small("Fades use the existing shot time. Dialogue and music are unaffected. These are fades through black, not overlapping dissolves.");
        }));
    }
}
