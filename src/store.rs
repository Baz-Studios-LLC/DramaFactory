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
}
pub struct Store {
    conn: Connection,
    pub path: PathBuf,
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS series(id TEXT PRIMARY KEY,name TEXT NOT NULL,premise TEXT NOT NULL); CREATE TABLE IF NOT EXISTS episodes(id TEXT PRIMARY KEY,series_id TEXT NOT NULL REFERENCES series(id),title TEXT NOT NULL,screenplay TEXT NOT NULL DEFAULT ''); CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,prompt TEXT NOT NULL,status TEXT NOT NULL,error TEXT NOT NULL DEFAULT '');")?;
        Ok(Self { conn, path })
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
        Ok(self.conn.prepare("SELECT id,series_id,title,screenplay FROM episodes WHERE series_id=?1 ORDER BY rowid")?.query_map([id],|r|Ok(Episode{id:r.get(0)?,series_id:r.get(1)?,title:r.get(2)?,screenplay:r.get(3)?}))?.collect::<Result<_,_>>()?)
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
            .prepare("SELECT id,prompt,status,error FROM jobs ORDER BY rowid DESC")?
            .query_map([], |r| {
                Ok(Job {
                    id: r.get(0)?,
                    prompt: r.get(1)?,
                    status: r.get(2)?,
                    error: r.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }
    pub fn queue(&self, prompt: &str) -> Result<Job> {
        if prompt.trim().is_empty() {
            bail!("A visual prompt is required");
        }
        let j = Job {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            status: "blocked".into(),
            error: "No verified video worker configured. No model has been downloaded.".into(),
        };
        self.conn.execute(
            "INSERT INTO jobs VALUES (?1,?2,?3,?4)",
            params![j.id, j.prompt, j.status, j.error],
        )?;
        Ok(j)
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        if self.conn.execute("UPDATE jobs SET status='canceled',error='' WHERE id=?1 AND status IN ('queued','blocked')",[id])?==0{bail!("Job not found or cannot be canceled");}
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
#[cfg(test)]
mod tests {
    use super::*;
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
