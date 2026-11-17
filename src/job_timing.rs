//! Recorded worker timing only. Provider progress and ETA are never inferred.
use crate::store::Store;
use anyhow::Result;
use serde::Serialize;
#[derive(Serialize)]
pub struct JobTiming {
    pub job_id: String,
    pub kind: String,
    pub stage: String,
    pub attempt_elapsed_seconds: Option<u64>,
    pub queued_seconds: Option<u64>,
    pub remaining_seconds: Option<u64>,
    pub estimate_note: String,
}
impl Store {
    pub fn job_created_local(&self, id: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row(
            "SELECT strftime('%m/%d %H:%M:%S', NULLIF(created_at, ''), 'localtime') FROM jobs WHERE id=?1",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn job_timing(&self, id: &str) -> Result<JobTiming> {
        let (kind,status,phase,start,end,created,now):(String,String,String,Option<i64>,Option<i64>,Option<i64>,i64)=self.conn.query_row("SELECT j.kind,j.status,j.phase,t.started_unix,t.finished_unix,unixepoch(NULLIF(j.created_at,'')),unixepoch() FROM jobs j LEFT JOIN job_timing t ON t.job_id=j.id WHERE j.id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)))?;
        let elapsed = match (start, end) {
            (Some(start), Some(end)) if end >= start => Some((end - start) as u64),
            (Some(start), None) if status == "running" && now >= start => {
                Some((now - start) as u64)
            }
            _ => None,
        };
        let queued = if status == "queued" {
            created
                .filter(|created| *created <= now)
                .map(|created| (now - created) as u64)
        } else {
            None
        };
        Ok(JobTiming{job_id:id.into(),kind,stage:if phase.is_empty(){status}else{phase},attempt_elapsed_seconds:elapsed,queued_seconds:queued,remaining_seconds:None,estimate_note:"Time remaining unknown. No verified provider ETA or calibrated history is available.".into()})
    }
}
pub fn duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}h {:02}m {:02}s",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_timing_freezes_and_missing_or_interrupted_timing_stays_unknown() {
        let root = std::env::temp_dir().join(format!("df-timing-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        store.conn.execute("INSERT INTO jobs(id,prompt,status,kind,created_at) VALUES('fixture','Fixture','completed','openrouter_video',datetime('now','-125 seconds'))",[]).unwrap();
        assert!(
            store
                .job_timing("fixture")
                .unwrap()
                .attempt_elapsed_seconds
                .is_none()
        );
        store
            .conn
            .execute(
                "INSERT INTO job_timing VALUES('fixture',unixepoch()-125,unixepoch()-65)",
                [],
            )
            .unwrap();
        let report = store.job_timing("fixture").unwrap();
        assert_eq!(report.attempt_elapsed_seconds, Some(60));
        assert!(report.remaining_seconds.is_none());
        store
            .conn
            .execute("UPDATE jobs SET status='failed' WHERE id='fixture'", [])
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE job_timing SET finished_unix=NULL WHERE job_id='fixture'",
                [],
            )
            .unwrap();
        assert!(
            store
                .job_timing("fixture")
                .unwrap()
                .attempt_elapsed_seconds
                .is_none()
        );
        assert_eq!(duration(3661), "1h 01m 01s");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
