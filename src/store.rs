use anyhow::{Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: String,
    pub name: String,
    pub premise: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub series_id: String,
    pub title: String,
    pub screenplay: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub prompt: String,
    pub status: String,
    pub error: String,
    pub progress: f32,
    pub attempts: u32,
    pub output_id: Option<String>,
    pub phase: String,
}
pub struct Store {
    pub(crate) conn: Connection,
    pub path: PathBuf,
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS series(id TEXT PRIMARY KEY,name TEXT NOT NULL,premise TEXT NOT NULL); CREATE TABLE IF NOT EXISTS episodes(id TEXT PRIMARY KEY,series_id TEXT NOT NULL REFERENCES series(id),title TEXT NOT NULL,screenplay TEXT NOT NULL DEFAULT ''); CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,prompt TEXT NOT NULL,status TEXT NOT NULL,error TEXT NOT NULL DEFAULT '');")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let store = Self { conn, path };
        store.migrate_production()?;
        Ok(store)
    }
    pub fn series(&self) -> Result<Vec<Series>> {
        Ok(self
            .conn
            .prepare("SELECT id,name,premise FROM series ORDER BY rowid DESC")?
            .query_map([], |r| {
                Ok(Series {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    premise: r.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }
    pub fn create_series(&self, name: &str, premise: &str) -> Result<Series> {
        if name.trim().is_empty() {
            bail!("Give your series a title");
        }
        let s = Series {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().into(),
            premise: premise.into(),
        };
        self.conn.execute(
            "INSERT INTO series VALUES (?1,?2,?3)",
            params![s.id, s.name, s.premise],
        )?;
        Ok(s)
    }
    pub fn episodes(&self, id: &str) -> Result<Vec<Episode>> {
        let mut episodes: Vec<Episode> = self.conn.prepare("SELECT id,series_id,title,screenplay FROM episodes WHERE series_id=?1 ORDER BY rowid")?.query_map([id],|r|Ok(Episode{id:r.get(0)?,series_id:r.get(1)?,title:r.get(2)?,screenplay:r.get(3)?}))?.collect::<Result<_,_>>()?;
        // The story outline owns playback order. Keep unplanned drafts after it,
        // in creation order, including when a plan is expanded between milestones.
        let plan = self.series_plan(id)?;
        let positions: std::collections::HashMap<_, _> = plan
            .episodes
            .iter()
            .enumerate()
            .map(|(index, outline)| (outline.episode_id.as_str(), index))
            .collect();
        episodes.sort_by_key(|episode| {
            positions
                .get(episode.id.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        Ok(episodes)
    }
    pub fn create_episode(&self, id: &str, title: &str) -> Result<Episode> {
        if title.trim().is_empty() {
            bail!("Episode title is required");
        }
        let e = Episode {
            id: uuid::Uuid::new_v4().to_string(),
            series_id: id.into(),
            title: title.trim().into(),
            screenplay: String::new(),
        };
        self.conn.execute(
            "INSERT INTO episodes VALUES (?1,?2,?3,?4)",
            params![e.id, e.series_id, e.title, e.screenplay],
        )?;
        Ok(e)
    }
    pub fn save_screenplay(&self, id: &str, text: &str) -> Result<()> {
        if self.conn.execute(
            "UPDATE episodes SET screenplay=?1 WHERE id=?2",
            params![text, id],
        )? == 0
        {
            bail!("Episode not found");
        }
        Ok(())
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        Ok(self
            .conn
            .prepare("SELECT id,prompt,status,error,progress,attempts,output_id,phase FROM jobs ORDER BY rowid DESC")?
            .query_map([], |r| {
                Ok(Job {
                    id: r.get(0)?,
                    prompt: r.get(1)?,
                    status: r.get(2)?,
                    error: r.get(3)?,
                    progress: r.get(4)?,
                    attempts: r.get(5)?,
                    output_id: r.get(6)?,
                    phase:r.get(7)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }
    #[cfg(test)]
    pub fn queue(&self, prompt: &str) -> Result<Job> {
        self.enqueue(&crate::production::RenderRequest {
            prompt: prompt.into(),
            ..Default::default()
        })
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        if self.conn.execute("UPDATE jobs SET cancel_requested=1,status=CASE WHEN status='running' THEN status ELSE 'canceled' END,error='',updated_at=datetime('now') WHERE id=?1 AND status IN ('queued','blocked','running')",[id])?==0{bail!("Job not found or cannot be canceled");}
        Ok(())
    }
}
pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("DRAMAFACTORY_DATABASE") {
        return PathBuf::from(path);
    }
    directories::ProjectDirs::from("studio", "DramaFactory", "DramaFactory")
        .expect("User data directory unavailable")
        .data_dir()
        .join("studio.sqlite")
}

impl Store {
    pub fn save_screenplay_if_unchanged(&self, id: &str, original: &str, text: &str) -> Result<()> {
        if self.conn.execute(
            "UPDATE episodes SET screenplay=?3 WHERE id=?1 AND screenplay=?2",
            params![id, original, text],
        )? == 0
        {
            bail!(
                "The screenplay changed in another session. Your draft is still in the editor; copy it before reloading."
            );
        }
        Ok(())
    }
    pub fn get_episode(&self, id: &str) -> Result<Episode> {
        Ok(self.conn.query_row(
            "SELECT id,series_id,title,screenplay FROM episodes WHERE id=?1",
            [id],
            |r| {
                Ok(Episode {
                    id: r.get(0)?,
                    series_id: r.get(1)?,
                    title: r.get(2)?,
                    screenplay: r.get(3)?,
                })
            },
        )?)
    }
    pub fn update_series(&self, id: &str, name: &str, premise: &str) -> Result<()> {
        if name.trim().is_empty() {
            bail!("Series title is required");
        }
        if self.conn.execute(
            "UPDATE series SET name=?2,premise=?3 WHERE id=?1",
            params![id, name, premise],
        )? == 0
        {
            bail!("Series not found");
        }
        Ok(())
    }
    pub fn rename_episode(&self, id: &str, title: &str) -> Result<()> {
        if title.trim().is_empty() {
            bail!("Episode title is required");
        }
        if self.conn.execute(
            "UPDATE episodes SET title=?2 WHERE id=?1",
            params![id, title],
        )? == 0
        {
            bail!("Episode not found");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_editor_cannot_overwrite_mcp_changes() {
        let p = std::env::temp_dir().join(format!("df-{}.sqlite", uuid::Uuid::new_v4()));
        let s = Store::open(p).unwrap();
        let series = s.create_series("Series", "").unwrap();
        let e = s.create_episode(&series.id, "Pilot").unwrap();
        s.save_screenplay(&e.id, "Written via MCP").unwrap();
        assert!(
            s.save_screenplay_if_unchanged(&e.id, "", "Stale desktop draft")
                .is_err()
        );
        assert_eq!(s.get_episode(&e.id).unwrap().screenplay, "Written via MCP");
    }
    #[test]
    fn persistence_and_relationships() {
        let p = std::env::temp_dir().join(format!("df-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let s = Store::open(p.clone()).unwrap();
            assert!(s.create_series(" ", "").is_err());
            let a = s.create_series("Night Shift", "A secret identity").unwrap();
            let e = s.create_episode(&a.id, "Pilot").unwrap();
            s.save_screenplay(&e.id, "INT. LOBBY — NIGHT").unwrap();
            assert!(s.create_episode("missing", "Pilot").is_err());
            let j = s.queue("Rain over Manhattan").unwrap();
            assert_eq!(j.status, "blocked");
            s.cancel(&j.id).unwrap();
        }
        let s = Store::open(p.clone()).unwrap();
        assert_eq!(s.series().unwrap().len(), 1);
        assert_eq!(s.jobs().unwrap()[0].status, "canceled");
        drop(s);
        std::fs::remove_file(p).unwrap();
    }
}
