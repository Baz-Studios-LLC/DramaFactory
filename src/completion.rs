//! Atomic terminal status for already-validated outputs. Cancellation never selects speech.
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, Transaction, params};
pub(crate) fn finish(tx: &Transaction<'_>, job: &str, asset: &str, phase: &str) -> Result<bool> {
    let status:Option<String>=tx.query_row("UPDATE jobs SET status=CASE WHEN cancel_requested=1 THEN 'canceled' ELSE 'completed' END,progress=1,output_id=?2,error=CASE WHEN cancel_requested=1 THEN 'Canceled as processing finished; validated output preserved for review without automatic selection.' ELSE '' END,phase=CASE WHEN cancel_requested=1 THEN 'Canceled · output retained' ELSE ?3 END,updated_at=datetime('now') WHERE id=?1 AND status='running' AND EXISTS(SELECT 1 FROM assets WHERE id=?2 AND job_id=?1) RETURNING status",params![job,asset,phase],|row|row.get(0)).optional()?;
    Ok(status.context("Output no longer belongs to a running job")? == "completed")
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    #[test]
    fn validated_results_respect_cancellation_for_every_shared_adapter() {
        let root = std::env::temp_dir().join(format!("df-completion-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        for kind in [
            "export",
            "rehearsal",
            "dialogue",
            "image",
            "performance",
            "lipsync",
        ] {
            for canceled in [false, true] {
                let job = uuid::Uuid::new_v4().to_string();
                let asset = uuid::Uuid::new_v4().to_string();
                store.conn.execute("INSERT INTO jobs(id,prompt,status,kind) VALUES(?1,'Explicit completion fixture','running',?2)",params![job,kind]).unwrap();
                if canceled {
                    store.cancel(&job).unwrap();
                }
                let tx = store.conn.unchecked_transaction().unwrap();
                tx.execute("INSERT INTO assets VALUES(?1,?2,'explicit-test-fixture','no-media-hash',0,0,1,'{}','unreviewed')",params![asset,job]).unwrap();
                assert_eq!(finish(&tx, &job, &asset, "Ready").unwrap(), !canceled);
                tx.commit().unwrap();
                let saved = store.job(&job).unwrap();
                assert_eq!(
                    saved.status,
                    if canceled { "canceled" } else { "completed" }
                );
                assert_eq!(saved.output_id.as_deref(), Some(asset.as_str()));
                assert_eq!(saved.error.is_empty(), !canceled);
                let tx = store.conn.unchecked_transaction().unwrap();
                assert!(finish(&tx, &job, &asset, "Should not finish twice").is_err());
                drop(tx);
            }
        }
        assert_eq!(store.assets().unwrap().len(), 12);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
