//! Scope the generation page to the current series/episode; historical rows remain intact.
use crate::store::{Job, Store};
use anyhow::Result;
impl Store {
    pub(crate) fn segment_job_context(&self, job: &str) -> Result<Option<(String, String)>> {
        use rusqlite::OptionalExtension;
        let saved: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT heading,episode_title FROM job_display_context WHERE job_id=?1",
                [job],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let binding: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT episode_id,segment_id FROM segment_generations WHERE job_id=?1",
                [job],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((episode, segment)) = binding else {
            return Ok(saved);
        };
        let (heading, title) = match saved {
            Some(context) => context,
            None => self.segment_display_context(&episode, &segment)?,
        };
        // Number generation requests in queue order, not completion order or worker
        // retry count. Failed/canceled requests keep their number permanently.
        let take: usize = self.conn.query_row(
            "SELECT count(*) FROM segment_generations g JOIN jobs j ON j.id=g.job_id WHERE g.episode_id=?1 AND g.segment_id=?2 AND j.rowid <= (SELECT rowid FROM jobs WHERE id=?3)",
            rusqlite::params![episode, segment, job],
            |r| r.get(0),
        )?;
        Ok(Some((
            format!("{} - Take {take:02}", heading.replace(" · ", " - ")),
            title,
        )))
    }
    pub(crate) fn segment_display_context(
        &self,
        episode: &str,
        segment: &str,
    ) -> Result<(String, String)> {
        let saved = self.get_episode(episode)?;
        let episode_number = self
            .ordered_gui_episodes(&saved.series_id)?
            .iter()
            .position(|item| item.id == episode)
            .map(|i| i + 1);
        let segment_number = self
            .generation_plan(episode)?
            .segments
            .iter()
            .position(|item| item.id == segment)
            .map(|i| i + 1);
        let episode_label = episode_number
            .map(|n| format!("Episode {n:02}"))
            .unwrap_or_else(|| "Episode unavailable".into());
        let segment_label = segment_number
            .map(|n| format!("Segment {n:02}"))
            .unwrap_or_else(|| "Removed segment".into());
        Ok((format!("{episode_label} · {segment_label}"), saved.title))
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_labels_keep_queue_order_across_completion_and_retries() {
        let temp = std::env::temp_dir().join(format!("df-take-labels-{}", uuid::Uuid::new_v4()));
        let store = Store::open(temp.join("studio.sqlite")).unwrap();
        let series = store.create_series("Series", "").unwrap();
        let episode = store
            .create_episode(&series.id, "Episode 2: Storage Fees")
            .unwrap();
        for (id, segment, status) in [
            ("first", "one", "failed"),
            ("other", "two", "completed"),
            ("second", "one", "running"),
            ("third", "one", "queued"),
        ] {
            store
                .conn
                .execute(
                    "INSERT INTO jobs(id,prompt,status) VALUES(?1,'',?2)",
                    rusqlite::params![id, status],
                )
                .unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO segment_generations VALUES(?1,?2,?3,1,'{}')",
                    rusqlite::params![id, episode.id, segment],
                )
                .unwrap();
            store.conn.execute("INSERT INTO job_display_context VALUES(?1,'Episode 02 · Segment 01','Episode 2: Storage Fees')", [id]).unwrap();
        }
        let heading = |id| store.segment_job_context(id).unwrap().unwrap().0;
        assert_eq!(heading("first"), "Episode 02 - Segment 01 - Take 01");
        assert_eq!(heading("second"), "Episode 02 - Segment 01 - Take 02");
        assert_eq!(heading("third"), "Episode 02 - Segment 01 - Take 03");
        store
            .conn
            .execute(
                "UPDATE jobs SET status='completed',attempts=3 WHERE id='second'",
                [],
            )
            .unwrap();
        assert_eq!(heading("second"), "Episode 02 - Segment 01 - Take 02");
        assert_eq!(heading("other"), "Episode 02 - Segment 01 - Take 01");
        drop(store);
        std::fs::remove_dir_all(temp).unwrap();
    }
}
