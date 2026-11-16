//! Speaker visibility is editorial direction, independent of voice synthesis.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
#[derive(Serialize)]
pub struct DialogueDirection {
    pub line_id: String,
    pub speaker_visible: bool,
}
impl Store {
    pub fn dialogue_direction(&self, line: &str) -> Result<DialogueDirection> {
        self.conn
            .query_row("SELECT 1 FROM dialogue WHERE id=?1", [line], |_| Ok(()))
            .context("Dialogue line not found")?;
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("speaker_visible:{line}")],
                |row| row.get(0),
            )
            .optional()?;
        Ok(DialogueDirection {
            line_id: line.into(),
            speaker_visible: raw
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or(true),
        })
    }
    pub fn set_dialogue_visibility(
        &self,
        line: &str,
        visible: bool,
        expected: bool,
    ) -> Result<DialogueDirection> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = self.dialogue_direction(line)?;
        if current.speaker_visible != visible {
            if current.speaker_visible != expected {
                bail!("Speaker direction changed in another session; reload before saving");
            }
            self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("speaker_visible:{line}"),serde_json::to_string(&visible)?])?;
            // A stale text editor must reload the authoring revision, but the
            // selected/generated voice remains current and reusable.
            self.conn.execute(
                "UPDATE dialogue SET revision=revision+1 WHERE id=?1",
                [line],
            )?;
        }
        tx.commit()?;
        Ok(DialogueDirection {
            line_id: line.into(),
            speaker_visible: visible,
        })
    }
    pub(crate) fn require_visible_speaker(&self, line: &str) -> Result<()> {
        if !self.dialogue_direction(line)?.speaker_visible {
            bail!(
                "This dialogue is off screen. Keep its audio, but do not animate a visible listener speaking it. Use silent/reaction coverage or change the speaker direction after editorial review."
            );
        }
        Ok(())
    }
}
impl crate::Studio {
    pub(crate) fn speaker_visibility_control(&mut self, ui: &mut eframe::egui::Ui, line: &str) {
        if let Ok(direction) = self.store.dialogue_direction(line) {
            let mut visible = direction.speaker_visible;
            if ui
                .checkbox(&mut visible, "Speaker is visible in this shot")
                .changed()
            {
                let result =
                    self.store
                        .set_dialogue_visibility(line, visible, direction.speaker_visible);
                self.result(
                    result.map(|_| ()),
                    "Speaker direction saved; speech preserved",
                );
            }
            if !visible {
                ui.small("Off-screen voice: keep it over phone, insert or reaction coverage. Native speaking animation is blocked for this line.");
            }
        }
    }
}
