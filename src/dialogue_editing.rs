//! Move authored speech between coverage shots without moving its timeline audio.
use crate::{production::Dialogue, store::Store};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MoveDialogue {
    pub line_id: String,
    pub target_shot_id: String,
    pub expected_shot_id: String,
    pub expected_revision: u64,
}
impl Store {
    pub fn move_dialogue(&self, edit: &MoveDialogue) -> Result<Dialogue> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let source_id: String = self
            .conn
            .query_row(
                "SELECT shot_id FROM dialogue WHERE id=?1",
                [&edit.line_id],
                |r| r.get(0),
            )
            .context("Dialogue line not found")?;
        let source = self.shot(&source_id)?;
        let target = self.shot(&edit.target_shot_id)?;
        if source.episode_id != target.episode_id {
            bail!(
                "Move dialogue only within the same episode; copy authored words explicitly for another episode"
            );
        }
        let line = self
            .dialogue(&source_id)?
            .into_iter()
            .find(|l| l.id == edit.line_id)
            .context("Dialogue line not found")?;
        if source_id == edit.target_shot_id {
            tx.commit()?;
            return Ok(line);
        }
        if source_id != edit.expected_shot_id || line.revision != edit.expected_revision {
            bail!(
                "Dialogue changed in another session; keep your draft and reload before moving it"
            );
        }
        let start = if let Some(start) = line.timeline_start {
            start
        } else {
            let assets = self.assets()?;
            let mut shot_start = 0.;
            let mut found = None;
            for shot in self.shots(&source.episode_id)? {
                if shot.id == source_id {
                    let mut offset = Some(0.);
                    for prior in self.dialogue(&source_id)? {
                        if prior.id == line.id {
                            found = offset.map(|o| shot_start + o);
                            break;
                        }
                        if prior.timeline_start.is_none() {
                            let duration = assets
                                .iter()
                                .find(|a| Some(&a.id) == prior.asset_id.as_ref())
                                .filter(|a| {
                                    a.width == 0
                                        && a.duration.is_finite()
                                        && a.duration > 0.
                                        && a.review != "rejected"
                                        && a.path.is_file()
                                })
                                .map(|a| a.duration);
                            offset = offset.zip(duration).map(|(a, b)| a + b);
                        }
                    }
                    break;
                }
                shot_start += shot.duration;
            }
            found.context("Earlier speech has unknown timing. Set an explicit episode start for this line before moving it")?
        };
        self.conn.execute(
            "UPDATE dialogue SET shot_id=?2,timeline_start=?3,revision=revision+1 WHERE id=?1",
            rusqlite::params![line.id, target.id, start],
        )?;
        let saved = self
            .dialogue(&target.id)?
            .into_iter()
            .find(|l| l.id == line.id)
            .context("Moved dialogue not found")?;
        tx.commit()?;
        Ok(saved)
    }
}
impl crate::Studio {
    pub(crate) fn dialogue_move_control(
        &mut self,
        ui: &mut eframe::egui::Ui,
        line: &Dialogue,
        shots: &[crate::production::Shot],
    ) {
        ui.push_id((&line.id,"move"),|ui|ui.collapsing("Move to another shot",|ui| {
            let mut target=self.dialogue_move_targets.get(&line.id).cloned().unwrap_or(line.shot_id.clone());
            let label=shots.iter().find(|s|s.id==target).map(|s|s.title.as_str()).unwrap_or("Choose a shot");
            eframe::egui::ComboBox::from_id_salt("target").selected_text(label).show_ui(ui,|ui|{for shot in shots{ui.selectable_value(&mut target,shot.id.clone(),&shot.title);}});
            self.dialogue_move_targets.insert(line.id.clone(),target.clone());
            if ui.add_enabled(target!=line.shot_id,eframe::egui::Button::new("Move line; keep episode timing")).clicked(){let result=self.store.move_dialogue(&MoveDialogue{line_id:line.id.clone(),target_shot_id:target,expected_shot_id:line.shot_id.clone(),expected_revision:line.revision});if result.is_ok(){self.dialogue_move_targets.remove(&line.id);}self.result(result.map(|_|()),"Dialogue moved; voice and episode timing preserved");}
            ui.small("The generated voice and on/off-screen direction stay with the line. Review coverage and lip sync after moving it.");
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_preserves_voice_visibility_and_known_start_but_rejects_unknown_or_stale_edits() {
        let root = std::env::temp_dir().join(format!("df-dialogue-move-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let episode = store.create_episode(&series.id, "Fixture").unwrap();
        let other = store.create_episode(&series.id, "Other").unwrap();
        let a = store.add_shot(&episode.id, "A", "Fixture", "", 3.).unwrap();
        let b = store.add_shot(&episode.id, "B", "Fixture", "", 3.).unwrap();
        let foreign = store
            .add_shot(&other.id, "Foreign", "Fixture", "", 3.)
            .unwrap();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let line = Dialogue {
                id: uuid::Uuid::new_v4().to_string(),
                shot_id: a.id.clone(),
                text: "Fixture speech".into(),
                voice: "af_heart".into(),
                speed: 1.,
                asset_id: None,
                timeline_start: None,
                revision: 0,
            };
            store.save_dialogue_draft(&line).unwrap();
            ids.push(line.id);
        }
        let path = root.join("metadata-fixture");
        std::fs::write(&path, b"explicit duration fixture").unwrap();
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('voice','fixture',?1,'hash',0,0,1,'{}','unreviewed')",
                [path.to_string_lossy().to_string()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE dialogue SET asset_id='voice' WHERE id=?1",
                [&ids[1]],
            )
            .unwrap();
        let mut edit = MoveDialogue {
            line_id: ids[1].clone(),
            target_shot_id: b.id.clone(),
            expected_shot_id: a.id.clone(),
            expected_revision: 1,
        };
        assert!(
            store
                .move_dialogue(&edit)
                .err()
                .unwrap()
                .to_string()
                .contains("unknown timing")
        );
        store
            .conn
            .execute(
                "UPDATE dialogue SET asset_id='voice' WHERE id=?1",
                [&ids[0]],
            )
            .unwrap();
        store.set_dialogue_visibility(&ids[1], false, true).unwrap();
        edit.expected_revision = 2;
        let before: u64 = store
            .conn
            .query_row(
                "SELECT voice_revision FROM dialogue WHERE id=?1",
                [&ids[1]],
                |r| r.get(0),
            )
            .unwrap();
        let moved = store.move_dialogue(&edit).unwrap();
        assert_eq!(moved.timeline_start, Some(1.));
        assert_eq!(moved.asset_id.as_deref(), Some("voice"));
        assert_eq!(moved.revision, 3);
        assert!(!store.dialogue_direction(&ids[1]).unwrap().speaker_visible);
        assert_eq!(store.move_dialogue(&edit).unwrap().revision, 3);
        let after: u64 = store
            .conn
            .query_row(
                "SELECT voice_revision FROM dialogue WHERE id=?1",
                [&ids[1]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        edit.target_shot_id = a.id;
        assert!(store.move_dialogue(&edit).is_err());
        edit.target_shot_id = foreign.id;
        assert!(
            store
                .move_dialogue(&edit)
                .err()
                .unwrap()
                .to_string()
                .contains("same episode")
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
