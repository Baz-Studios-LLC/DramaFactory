//! Scope the generation page to the current series/episode; historical rows remain intact.
use crate::store::{Job, Store};
use anyhow::Result;
impl Store {
    pub(crate) fn has_newer_segment_job(&self, id: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT EXISTS(SELECT 1 FROM segment_generations old JOIN jobs j ON j.id=old.job_id JOIN segment_generations newer ON newer.episode_id=old.episode_id AND newer.segment_id=old.segment_id JOIN jobs n ON n.id=newer.job_id WHERE j.id=?1 AND n.rowid>j.rowid)",[id],|row| row.get(0))?)
    }

    pub fn workspace_jobs(&self, series: &str, episode: Option<&str>) -> Result<Vec<Job>> {
        let mut query=self.conn.prepare("SELECT j.id,j.prompt,j.status,j.error,j.progress,j.attempts,j.output_id,j.phase FROM jobs j LEFT JOIN segment_generations g ON g.job_id=j.id WHERE (g.episode_id=?2 AND EXISTS(SELECT 1 FROM episodes e WHERE e.id=g.episode_id AND e.series_id=?1)) OR (j.kind='openrouter_image' AND EXISTS(SELECT 1 FROM library l WHERE l.series_id=?1 AND l.id=CASE WHEN json_valid(j.request) THEN json_extract(j.request,'$.library_id') END)) OR (j.kind='export' AND EXISTS(SELECT 1 FROM episodes e WHERE e.series_id=?1 AND e.id=?2 AND e.id=CASE WHEN json_valid(j.request) THEN json_extract(j.request,'$.episode_id') END)) ORDER BY j.rowid DESC LIMIT 50")?;
        Ok(query
            .query_map(rusqlite::params![series, episode], |row| {
                Ok(Job {
                    id: row.get(0)?,
                    prompt: row.get(1)?,
                    status: row.get(2)?,
                    error: row.get(3)?,
                    progress: row.get(4)?,
                    attempts: row.get(5)?,
                    output_id: row.get(6)?,
                    phase: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}
