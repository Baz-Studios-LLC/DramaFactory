//! Editorial notes are versioned independently from immutable media provenance.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReview {
    pub asset_id: String,
    pub revision: u64,
    pub decision: String,
    pub note: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_portrait_choice_allows_old_images_but_rejects_stale_ui_and_changed_bytes() {
        let root =
            std::env::temp_dir().join(format!("df-select-portrait-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("db.sqlite")).unwrap();
        let series = store.create_series("Series", "").unwrap();
        let entry = store
            .add_library(&series.id, "character", "Actor", "New bio", None)
            .unwrap();
        let file = root.join("portrait.png");
        image::RgbaImage::new(8, 8).save(&file).unwrap();
        let hash = crate::worker::hash_file(&file).unwrap();
        let metadata = serde_json::json!({"kind":"image","request":{"library_id":entry.id,"library_reference_before":null,"library_context_before":{"name":"Old name","description":"Old bio","reference_hash":null}}});
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('old','job',?1,?2,8,8,0,?3,'unreviewed')",
                params![file.to_string_lossy(), hash, metadata.to_string()],
            )
            .unwrap();
        assert!(store.review("old", "approved").is_err());
        store.select_portrait(&entry, "old").unwrap();
        let current = store.library(&series.id, "character").unwrap().remove(0);
        assert_eq!(current.reference.as_ref(), Some(&file));
        assert_eq!(current.description, "New bio");
        assert_eq!(store.asset_review("old").unwrap().decision, "approved");
        assert!(store.select_portrait(&entry, "old").is_err());
        store.select_portrait(&current, "old").unwrap();
        image::RgbaImage::new(10, 10).save(&file).unwrap();
        assert!(store.select_portrait(&current, "old").is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_library_brief_blocks_old_image_reference_approval() {
        let root = std::env::temp_dir().join(format!("df-review-brief-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let entry = store
            .add_library(&series.id, "character", "Mara", "Original brief", None)
            .unwrap();
        let metadata = serde_json::json!({"kind":"image","request":{"library_id":entry.id,"library_reference_before":null,"library_context_before":{"name":"Mara","description":"Original brief","reference_hash":null}}});
        store.conn.execute("INSERT INTO assets VALUES('old','fixture','retained-fixture','fixture',32,32,0,?1,'unreviewed')",[metadata.to_string()]).unwrap();
        let mut edit = crate::library_editing::LibraryEdit {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: "New brief".into(),
            expected_name: entry.name.clone(),
            expected_description: entry.description.clone(),
        };
        store.update_library_entry(&edit).unwrap();
        assert!(
            store
                .review("old", "approved")
                .unwrap_err()
                .to_string()
                .contains("brief or reference changed")
        );
        assert_eq!(store.asset_review("old").unwrap().decision, "unreviewed");
        assert!(
            store.library(&series.id, "character").unwrap()[0]
                .reference
                .is_none()
        );
        edit.expected_description = "New brief".into();
        edit.description = "Original brief".into();
        store.update_library_entry(&edit).unwrap();
        store.review("old", "approved").unwrap();
        assert_eq!(
            store.library(&series.id, "character").unwrap()[0]
                .reference
                .as_ref()
                .unwrap()
                .to_string_lossy(),
            "retained-fixture"
        );
        // A same-path byte replacement is also an intervening reference change.
        let reference = root.join("reference-fixture.dat");
        std::fs::write(&reference, b"original reference fixture").unwrap();
        let hash = crate::worker::hash_file(&reference).unwrap();
        store
            .conn
            .execute(
                "UPDATE library SET reference=?2 WHERE id=?1",
                rusqlite::params![entry.id, reference.to_string_lossy()],
            )
            .unwrap();
        let metadata = serde_json::json!({"kind":"image","request":{"library_id":entry.id,"library_reference_before":reference,"library_context_before":{"name":"Mara","description":"Original brief","reference_hash":hash}}});
        store.conn.execute("INSERT INTO assets VALUES('next','fixture','another-retained-fixture','fixture',32,32,0,?1,'unreviewed')",[metadata.to_string()]).unwrap();
        std::fs::write(&reference, b"changed reference fixture").unwrap();
        assert!(store.review("next", "approved").is_err());
        assert_eq!(store.asset_review("next").unwrap().decision, "unreviewed");
        assert_eq!(
            store.library(&series.id, "character").unwrap()[0]
                .reference
                .as_ref(),
            Some(&reference)
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn notes_on_an_old_approved_image_do_not_reselect_its_reference() {
        let root = std::env::temp_dir().join(format!("df-review-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let cast = store
            .add_library(&series.id, "character", "Actor", "", None)
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE library SET reference='newer-reference' WHERE id=?1",
                [&cast.id],
            )
            .unwrap();
        let metadata = serde_json::json!({"kind":"image","request":{"library_id":cast.id,"library_reference_before":null}});
        store.conn.execute("INSERT INTO assets VALUES('old','fixture','older-reference','fixture-hash',1,1,0,?1,'approved')",[metadata.to_string()]).unwrap();
        let mut review = store.asset_review("old").unwrap();
        review.note = "Keep as an alternate; the newer reference remains selected.".into();
        store.save_asset_review(&review).unwrap();
        assert_eq!(
            store.library(&series.id, "character").unwrap()[0]
                .reference
                .as_ref()
                .unwrap()
                .to_string_lossy(),
            "newer-reference"
        );
        assert!(store.review("old", "approved").is_err());
        assert_eq!(store.review_history("old").unwrap().len(), 1);
    }
}
#[derive(Serialize)]
pub struct ReviewHistory {
    pub revision: u64,
    pub decision: String,
    pub note: String,
    pub created_at: String,
}
impl Store {
    pub(crate) fn select_portrait(
        &self,
        expected: &crate::production::LibraryEntry,
        asset: &str,
    ) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.ensure_character_not_deleted(&expected.id)?;
        if self.deleted_portraits()?.contains(asset) {
            bail!("Restore this portrait before selecting it");
        }
        let (path, hash, metadata): (String, String, String) = self.conn.query_row(
            "SELECT path,hash,metadata FROM assets WHERE id=?1",
            [asset],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        if metadata["kind"] != "image" || metadata["request"]["library_id"] != expected.id {
            bail!("This image does not belong to this character or location");
        }
        if crate::worker::hash_file(std::path::Path::new(&path))? != hash {
            bail!("The portrait file has changed; selection was not saved");
        }
        let current: (String, String, Option<String>) = self.conn.query_row(
            "SELECT name,description,reference FROM library WHERE id=?1",
            [&expected.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if current.0 != expected.name
            || current.1 != expected.description
            || current.2.as_deref() != expected.reference.as_ref().and_then(|p| p.to_str())
        {
            bail!(
                "This character changed in another window. Review the latest card, then select the portrait again"
            );
        }
        // The explicit user selection supersedes generation-time reference/brief snapshots.
        // Ordinary approval still retains its existing stale-result protection.
        self.conn.execute(
            "UPDATE library SET reference=?2 WHERE id=?1",
            params![expected.id, path],
        )?;
        let mut review = self.asset_review(asset)?;
        review.decision = "approved".into();
        self.write_asset_review(&review, true)?;
        tx.commit()?;
        Ok(())
    }
    pub fn asset_review(&self, id: &str) -> Result<AssetReview> {
        self.conn.query_row("SELECT a.review,COALESCE(n.revision,0),COALESCE(n.note,'') FROM assets a LEFT JOIN asset_review_notes n ON n.asset_id=a.id WHERE a.id=?1",[id],|row|Ok(AssetReview {asset_id:id.into(),decision:row.get(0)?,revision:row.get(1)?,note:row.get(2)?})).context("Asset not found")
    }
    pub fn review_history(&self, id: &str) -> Result<Vec<ReviewHistory>> {
        self.asset_review(id)?;
        Ok(self.conn.prepare("SELECT revision,decision,note,created_at FROM asset_review_history WHERE asset_id=?1 ORDER BY revision DESC")?.query_map([id],|row|Ok(ReviewHistory{revision:row.get(0)?,decision:row.get(1)?,note:row.get(2)?,created_at:row.get(3)?}))?.collect::<std::result::Result<_,_>>()?)
    }
    fn write_asset_review(
        &self,
        draft: &AssetReview,
        explicit_status: bool,
    ) -> Result<AssetReview> {
        if draft.note.len() > 12000 {
            bail!("Review note is too long; shorten it before saving");
        }
        let current = self.asset_review(&draft.asset_id)?;
        if current.revision != draft.revision {
            bail!("Review changed in another session; keep your note and reload before saving");
        }
        if !["approved", "rejected", "unreviewed"].contains(&draft.decision.as_str()) {
            bail!("Invalid review status");
        }
        if explicit_status || current.decision != draft.decision {
            self.apply_review_status(&draft.asset_id, &draft.decision)?;
        }
        if current.decision == draft.decision && current.note == draft.note {
            return Ok(current);
        }
        let mut saved = draft.clone();
        saved.revision += 1;
        self.conn.execute("INSERT INTO asset_review_notes(asset_id,revision,note) VALUES(?1,?2,?3) ON CONFLICT(asset_id) DO UPDATE SET revision=excluded.revision,note=excluded.note",params![saved.asset_id,saved.revision,saved.note])?;
        self.conn.execute(
            "INSERT INTO asset_review_history(asset_id,revision,decision,note) VALUES(?1,?2,?3,?4)",
            params![saved.asset_id, saved.revision, saved.decision, saved.note],
        )?;
        Ok(saved)
    }
    pub fn save_asset_review(&self, draft: &AssetReview) -> Result<AssetReview> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let saved = self.write_asset_review(draft, false)?;
        tx.commit()?;
        Ok(saved)
    }
    pub(crate) fn review_status(&self, id: &str, status: &str) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let mut draft = self.asset_review(id)?;
        draft.decision = status.into();
        self.write_asset_review(&draft, true)?;
        tx.commit()?;
        Ok(())
    }
}
impl crate::Studio {
    pub(crate) fn review_choice(&mut self, id: &str, status: &str) {
        let draft = self
            .review_drafts
            .get(id)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.store.asset_review(id));
        let result = draft.and_then(|mut draft| {
            draft.decision = status.into();
            self.store.save_asset_review(&draft)
        });
        if result.is_ok() {
            self.review_drafts.remove(id);
            self.asset_browser.invalidate();
        }
        self.result(result.map(|_| ()), "Review saved; original media preserved");
    }
    pub(crate) fn review_notes(&mut self, ui: &mut eframe::egui::Ui, id: &str) {
        ui.push_id(id,|ui|ui.collapsing("Review notes",|ui| {
            let Ok(current)=self.store.asset_review(id) else{return;};
            let mut draft=self.review_drafts.get(id).cloned().unwrap_or(current.clone());
            if draft.revision!=current.revision {ui.label(eframe::egui::RichText::new("A newer review is saved. Your draft is preserved; reload before applying it.").color(crate::GOLD));}
            ui.label("Acting, synchronization, identity, eyes and hands");
            if ui.add(eframe::egui::TextEdit::multiline(&mut draft.note).desired_rows(3).desired_width(f32::INFINITY)).changed(){self.review_drafts.insert(id.into(),draft.clone());}
            ui.horizontal(|ui| {
                if ui.button("Save note").clicked(){let result=self.store.save_asset_review(&draft);if result.is_ok(){self.review_drafts.remove(id);self.asset_browser.invalidate();}self.result(result.map(|_|()),"Review note saved");}
                if ui.button("Reload saved review").clicked(){self.review_drafts.remove(id);}
            });
            ui.collapsing("Review history",|ui|if let Ok(history)=self.store.review_history(id){for item in history {ui.label(format!("{} UTC · {}",item.created_at,item.decision));if !item.note.is_empty(){ui.label(item.note);}ui.separator();}});
        }));
    }
}
