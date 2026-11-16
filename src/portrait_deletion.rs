//! Confirmed permanent deletion of unused managed portraits; legacy trash remains accessible.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use rusqlite::params;
use std::collections::HashSet;
impl Store {
    pub(crate) fn deleted_portraits(&self) -> Result<HashSet<String>> {
        Ok(self
            .conn
            .prepare("SELECT substr(key,18) FROM settings WHERE key LIKE 'portrait:deleted:%'")?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub(crate) fn restore_portrait(&self, asset: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM settings WHERE key=?1",
            [format!("portrait:deleted:{asset}")],
        )?;
        Ok(())
    }
    pub(crate) fn permanently_delete_portrait(&self, entry: &str, asset: &str) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let (path, job, metadata): (String, String, String) = tx.query_row(
            "SELECT path,job_id,metadata FROM assets WHERE id=?1",
            [asset],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        if metadata["provider"] != "openrouter" || metadata["request"]["library_id"] != entry {
            bail!("This portrait does not belong to this character or location");
        }
        let selected: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM library WHERE reference=?1)",
            [&path],
            |r| r.get(0),
        )?;
        if selected {
            bail!("Choose another portrait before deleting the selected reference");
        }
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE id<>?1 AND kind<>'openrouter_image' AND instr(request,?2)>0) OR EXISTS(SELECT 1 FROM settings WHERE instr(value,?2)>0) OR EXISTS(SELECT 1 FROM assets WHERE id<>?3 AND (path=?2 OR (COALESCE(json_extract(metadata,'$.kind'),'')<>'image' AND instr(metadata,?2)>0))) OR EXISTS(SELECT 1 FROM shots WHERE asset_id=?3) OR EXISTS(SELECT 1 FROM segment_takes WHERE asset_id=?3)",
            params![job,path,asset], |r| r.get(0))?;
        if used {
            bail!(
                "This portrait is referenced by a saved plan or production job and cannot be permanently deleted"
            );
        }
        let file = std::path::PathBuf::from(&path);
        let managed = self
            .root()
            .join("jobs")
            .join(&job)
            .canonicalize()
            .context("Portrait job folder is unavailable")?;
        let original = file
            .canonicalize()
            .context("Portrait file is unavailable")?;
        if !original.starts_with(&managed)
            || std::fs::symlink_metadata(&file)?.file_type().is_symlink()
        {
            bail!("Only generated files inside their managed job folder can be deleted");
        }
        // The provider response includes another base64 copy of the same image.
        let response = file
            .parent()
            .context("Portrait folder missing")?
            .join("response.json");
        let mut files = vec![file];
        if response.is_file() {
            if !response.canonicalize()?.starts_with(&managed)
                || std::fs::symlink_metadata(&response)?
                    .file_type()
                    .is_symlink()
            {
                bail!("Portrait response is outside its managed job folder");
            }
            files.push(response);
        }
        tx.execute("DELETE FROM asset_review_notes WHERE asset_id=?1", [asset])?;
        tx.execute(
            "DELETE FROM asset_review_history WHERE asset_id=?1",
            [asset],
        )?;
        tx.execute("UPDATE jobs SET output_id=NULL WHERE output_id=?1", [asset])?;
        tx.execute(
            "DELETE FROM settings WHERE key=?1",
            [format!("portrait:deleted:{asset}")],
        )?;
        tx.execute("DELETE FROM assets WHERE id=?1", [asset])?;
        // Stage renames first, so a database failure can restore the original files.
        let mut staged: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
        for source in files {
            let target = source.with_file_name(format!("delete-{}", uuid::Uuid::new_v4()));
            if let Err(error) = std::fs::rename(&source, &target) {
                for (source, target) in staged.iter().rev() {
                    let _ = std::fs::rename(target, source);
                }
                return Err(error.into());
            }
            staged.push((source, target));
        }
        if let Err(error) = tx.commit() {
            for (source, target) in staged.iter().rev() {
                let _ = std::fs::rename(target, source);
            }
            return Err(error.into());
        }
        for (_, target) in staged {
            std::fs::remove_file(&target).with_context(|| {
                format!(
                    "Gallery entry removed, but could not erase {}",
                    target.display()
                )
            })?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permanent_deletion_checks_ownership_selection_and_usage_then_erases_media() {
        let root =
            std::env::temp_dir().join(format!("df-portrait-delete-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Fixture", "").unwrap();
        let entry = store
            .add_library(&series.id, "character", "Actor", "", None)
            .unwrap();
        let folder = root.join("jobs/fixture/attempt-1");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("portrait.png");
        std::fs::write(&path, b"original bytes").unwrap();
        std::fs::write(folder.join("response.json"), b"encoded image").unwrap();
        let metadata =
            serde_json::json!({"provider":"openrouter","request":{"library_id":entry.id}});
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES('portrait','fixture',?1,'hash',1,1,0,?2,'unreviewed')",
                params![path.to_string_lossy(), metadata.to_string()],
            )
            .unwrap();
        assert!(
            store
                .permanently_delete_portrait("wrong", "portrait")
                .is_err()
        );
        store
            .conn
            .execute(
                "UPDATE library SET reference=?1 WHERE id=?2",
                params![path.to_string_lossy(), entry.id],
            )
            .unwrap();
        assert!(
            store
                .permanently_delete_portrait(&entry.id, "portrait")
                .is_err()
        );
        store
            .conn
            .execute("UPDATE library SET reference=NULL WHERE id=?1", [&entry.id])
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO settings VALUES('plan',?1)",
                [path.to_string_lossy()],
            )
            .unwrap();
        assert!(
            store
                .permanently_delete_portrait(&entry.id, "portrait")
                .is_err()
        );
        assert!(path.exists());
        store
            .conn
            .execute("DELETE FROM settings WHERE key='plan'", [])
            .unwrap();
        store
            .permanently_delete_portrait(&entry.id, "portrait")
            .unwrap();
        assert!(!path.exists());
        assert!(!folder.join("response.json").exists());
        assert!(store.assets().unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
