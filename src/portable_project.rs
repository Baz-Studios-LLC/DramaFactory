//! Immutable folder snapshots for Finder/iCloud Drive. Never open a cloud SQLite file live.
use crate::store::Store;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Link {
    pub folder: PathBuf,
    pub revision: String,
}
#[derive(Serialize, Deserialize)]
struct Revision {
    format: u32,
    revision: String,
    parent: Option<String>,
    series: String,
    name: String,
    database_hash: String,
    media: BTreeMap<String, String>,
}
const TABLES: &[&str] = &[
    "series",
    "episodes",
    "library",
    "shots",
    "dialogue",
    "story_plans",
    "story_history",
    "canon_commits",
    "character_profiles",
    "deleted_characters",
    "portrait_prompts",
    "camera_setups",
    "shot_directions",
    "audio_cues",
    "generation_plans",
    "segment_takes",
    "segment_generations",
    "job_display_context",
    "sidekick_messages",
    "sidekick_images",
    "assets",
    "jobs",
    "job_timing",
    "remote_video_jobs",
    "asset_review_notes",
    "asset_review_history",
    "settings",
    "buffer_uploads",
    "buffer_submissions",
];
const SETTING_PREFIXES: &[&str] = &[
    "episode-cut:",
    "episode-publish:",
    "episode-publish-result:",
    "captions:",
    "speaker_visible:",
    "picture_fades:",
    "gui:episode:",
    "series:portrait-style:",
    "portrait:style:",
    "sidekick:draft:",
    "sidekick:images:",
];
const ROOT: &str = "__DRAMAFACTORY_PROJECT__/";

pub(crate) fn link(store: &Store, series: &str) -> Result<Option<Link>> {
    use rusqlite::OptionalExtension;
    let value: Option<String> = store
        .conn
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [format!("portable:{series}")],
            |r| r.get(0),
        )
        .optional()?;
    value
        .map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
fn remember(store: &Store, series: &str, link: &Link) -> Result<()> {
    store.conn.execute(
        "INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![format!("portable:{series}"), serde_json::to_string(link)?],
    )?;
    Ok(())
}
fn valid_id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}
fn head(folder: &Path) -> Result<Option<Revision>> {
    let revisions = folder.join("revisions");
    if !revisions.exists() {
        return Ok(None);
    }
    let mut all = Vec::new();
    for entry in std::fs::read_dir(revisions)? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "json") {
            let rev: Revision = serde_json::from_slice(&std::fs::read(&path)?)
                .context("Project snapshot is not fully downloaded or is invalid")?;
            ensure!(
                rev.format == 1 && valid_id(&rev.revision),
                "Unsupported project format"
            );
            all.push(rev);
        }
    }
    let parents: HashSet<_> = all.iter().filter_map(|r| r.parent.clone()).collect();
    let mut heads = all.into_iter().filter(|r| !parents.contains(&r.revision));
    let first = heads.next();
    ensure!(
        heads.next().is_none(),
        "This project has conflicting saves from different Macs. Both versions are preserved. Save local work as a new project, or resolve the conflicting revisions before opening."
    );
    Ok(first)
}
struct Row {
    table: String,
    rowid: i64,
    fields: BTreeMap<String, String>,
}
fn rows(conn: &Connection) -> Result<Vec<Row>> {
    let mut result = Vec::new();
    for table in TABLES {
        let mut q = conn.prepare(&format!("SELECT rowid,* FROM {table}"))?;
        let names: Vec<_> = q.column_names().iter().map(|s| s.to_string()).collect();
        let mut records = q.query([])?;
        while let Some(row) = records.next()? {
            let mut fields = BTreeMap::new();
            for (i, name) in names.iter().enumerate().skip(1) {
                if let Ok(value) = row.get::<_, String>(i) {
                    fields.insert(name.clone(), value);
                }
            }
            result.push(Row {
                table: table.to_string(),
                rowid: row.get(0)?,
                fields,
            });
        }
    }
    Ok(result)
}
// Select the series-owned graph, including referenced assets and their jobs. Do not
// export machine settings, API configuration, or unrelated series.
fn isolate(conn: &Connection, series: &str) -> Result<()> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ensure!(
        tables.iter().all(|name| TABLES.contains(&name.as_str())),
        "This project contains a newer database table. Update DramaFactory before transferring it."
    );
    let data = rows(conn)?;
    let mut ids = HashSet::from([series.to_string()]);
    let mut keep = HashSet::new();
    let media_ids: HashSet<_> = data
        .iter()
        .filter(|r| r.table == "assets" || r.table == "jobs")
        .filter_map(|r| r.fields.get("id").cloned())
        .collect();
    loop {
        let before = (ids.len(), keep.len());
        for (index, row) in data.iter().enumerate() {
            let f = &row.fields;
            let owned = if row.table == "settings" {
                f.get("key").is_some_and(|key| {
                    (key == "video:aspect_ratio" || key == "video:provider")
                        || SETTING_PREFIXES.iter().any(|prefix| {
                            key.strip_prefix(prefix).is_some_and(|id| ids.contains(id))
                        })
                })
            } else if row.table == "buffer_submissions" {
                f.get("episode").is_some_and(|id| ids.contains(id))
            } else if row.table == "buffer_uploads" {
                f.get("hash").is_some_and(|hash| {
                    data.iter().any(|asset| {
                        asset.table == "assets"
                            && asset.fields.get("hash") == Some(hash)
                            && asset.fields.get("id").is_some_and(|id| ids.contains(id))
                    })
                })
            } else if row.table == "series" {
                f.get("id").is_some_and(|id| id == series)
            } else if let Some(owner) = f.get("series_id") {
                owner == series
            } else {
                f.iter()
                    .any(|(k, v)| (k == "id" || k.ends_with("_id")) && ids.contains(v))
                    || (row.table == "jobs"
                        && f.get("request")
                            .is_some_and(|v| ids.iter().any(|id| v.contains(id))))
                    || (row.table == "assets"
                        && f.get("metadata")
                            .is_some_and(|v| ids.iter().any(|id| v.contains(id))))
            };
            if owned {
                keep.insert(index);
                if let Some(id) = f.get("id") {
                    ids.insert(id.clone());
                }
                // Link assets named in cuts, selected portraits and request snapshots.
                for candidate in &media_ids {
                    if f.values().any(|v| v.contains(candidate)) {
                        ids.insert(candidate.clone());
                    }
                }
            }
        }
        if before == (ids.len(), keep.len()) {
            break;
        }
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")?;
    for (i, row) in data.iter().enumerate() {
        if !keep.contains(&i) {
            conn.execute(
                &format!("DELETE FROM {} WHERE rowid=?1", row.table),
                [row.rowid],
            )?;
        }
    }
    conn.execute(
        "INSERT INTO settings VALUES('gui:current_series',?1)",
        [series],
    )?;
    conn.execute_batch("COMMIT; PRAGMA foreign_keys=ON;")?;
    let broken: i64 = conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
        r.get(0)
    })?;
    ensure!(
        broken == 0,
        "Project has unresolved links; it was not saved to the folder"
    );
    Ok(())
}
fn rewrite_value(
    value: &mut serde_json::Value,
    transform: &mut impl FnMut(&str) -> Result<String>,
) -> Result<()> {
    match value {
        serde_json::Value::String(s) => *s = transform(s)?,
        serde_json::Value::Array(a) => {
            for v in a {
                rewrite_value(v, transform)?;
            }
        }
        serde_json::Value::Object(o) => {
            for v in o.values_mut() {
                rewrite_value(v, transform)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn rewrite(conn: &Connection, mut transform: impl FnMut(&str) -> Result<String>) -> Result<()> {
    for row in rows(conn)? {
        for (column, text) in row.fields {
            let updated = if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) {
                rewrite_value(&mut json, &mut transform)?;
                serde_json::to_string(&json)?
            } else {
                transform(&text)?
            };
            if updated != text {
                conn.execute(
                    &format!("UPDATE {} SET \"{}\"=?1 WHERE rowid=?2", row.table, column),
                    params![updated, row.rowid],
                )?;
            }
        }
    }
    Ok(())
}
fn copy_checked(source: &Path, target: &Path, hash: &str) -> Result<()> {
    if target.exists() {
        ensure!(
            crate::worker::hash_file(target)? == hash,
            "Project media failed verification: {}",
            target.display()
        );
        return Ok(());
    }
    let temp = target.with_extension(format!("{}.partial", uuid::Uuid::new_v4()));
    std::fs::copy(source, &temp).with_context(|| {
        format!(
            "Could not copy {}. Ensure iCloud has downloaded it.",
            source.display()
        )
    })?;
    ensure!(
        crate::worker::hash_file(&temp)? == hash,
        "File changed while copying: {}",
        source.display()
    );
    std::fs::File::open(&temp)?.sync_all()?;
    std::fs::rename(temp, target)?;
    Ok(())
}
pub(crate) fn save(
    store: &Store,
    series: &str,
    folder: &Path,
    expected: Option<&str>,
) -> Result<Link> {
    if expected.is_none() {
        ensure!(
            !folder.exists(),
            "Choose a new project name; this folder already exists"
        );
        std::fs::create_dir(folder)?;
    }
    ensure!(folder.is_dir(), "Project folder is unavailable");
    let current = head(folder)?;
    ensure!(
        current.as_ref().map(|r| r.revision.as_str()) == expected,
        "The project folder contains a different revision than this working copy. Use Save As to keep your edits separately, or open the folder to use its saved version"
    );
    if let Some(current) = &current {
        ensure!(current.series == series, "This is a different series");
    }
    let revision = uuid::Uuid::new_v4().to_string();
    let temp = store.root().join(format!("portable-{revision}.sqlite"));
    store
        .conn
        .execute("VACUUM INTO ?1", [temp.to_string_lossy().as_ref()])?;
    let result = (|| -> Result<Link> {
        let conn = Connection::open(&temp)?;
        isolate(&conn, series)?;
        std::fs::create_dir_all(folder.join("media"))?;
        std::fs::create_dir_all(folder.join("revisions"))?;
        let required: Vec<String> = conn.prepare("SELECT path FROM assets UNION SELECT reference FROM library WHERE reference IS NOT NULL")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for path in &required {
            ensure!(
                Path::new(path).is_file(),
                "Missing media: {path}. The local project is unchanged."
            );
        }
        let mut media = BTreeMap::new();
        let mut mapped: BTreeMap<String, String> = BTreeMap::new();
        rewrite(&conn, |s| {
            if let Some(path) = mapped.get(s) {
                return Ok(path.clone());
            }
            let path = Path::new(s);
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if path.is_absolute()
                && path.is_file()
                && (required.iter().any(|p| p == s)
                    || [
                        "png", "jpg", "jpeg", "webp", "mp4", "mov", "wav", "mp3", "m4a", "aac",
                        "srt", "ass",
                    ]
                    .contains(&extension.as_str()))
            {
                let hash = crate::worker::hash_file(path)?;
                let name = format!("{hash}.{extension}");
                copy_checked(path, &folder.join("media").join(&name), &hash)?;
                media.insert(name.clone(), hash);
                let portable = format!("{ROOT}media/{name}");
                mapped.insert(s.to_string(), portable.clone());
                Ok(portable)
            } else {
                Ok(s.to_string())
            }
        })?;
        let name: String =
            conn.query_row("SELECT name FROM series WHERE id=?1", [series], |r| {
                r.get(0)
            })?;
        conn.execute_batch("VACUUM;")?;
        drop(conn);
        let hash = crate::worker::hash_file(&temp)?;
        copy_checked(
            &temp,
            &folder.join("revisions").join(format!("{revision}.sqlite")),
            &hash,
        )?;
        // Recheck before publishing. Concurrent cloud saves form retained branches;
        // head() detects those branches instead of silently choosing a winner.
        ensure!(
            head(folder)?.as_ref().map(|r| r.revision.as_str()) == expected,
            "Project changed during save. Your local edit remains safe; save a separate copy."
        );
        let manifest = Revision {
            format: 1,
            revision: revision.clone(),
            parent: expected.map(str::to_string),
            series: series.into(),
            name,
            database_hash: hash,
            media,
        };
        let staged = folder.join("revisions").join(format!("{revision}.partial"));
        std::fs::write(&staged, serde_json::to_vec_pretty(&manifest)?)?;
        std::fs::File::open(&staged)?.sync_all()?;
        std::fs::rename(
            staged,
            folder.join("revisions").join(format!("{revision}.json")),
        )?;
        let link = Link {
            folder: folder.to_path_buf(),
            revision,
        };
        remember(store, series, &link)?;
        Ok(link)
    })();
    let _ = std::fs::remove_file(temp);
    if result.is_err() && expected.is_none() {
        let _ = std::fs::remove_dir(folder);
    }
    result
}
pub(crate) fn open(folder: &Path, local_home: &Path) -> Result<PathBuf> {
    let manifest = head(folder)?.context(
        "No complete project save found in this folder. Wait for iCloud to finish syncing.",
    )?;
    let root = local_home
        .join("projects")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(root.join("media"))?;
    // Validate every byte before opening a local database. Partial cloud sync never
    // replaces an existing working copy.
    for (name, hash) in &manifest.media {
        ensure!(
            Path::new(name).components().count() == 1 && !name.starts_with('.'),
            "Invalid media filename"
        );
        copy_checked(
            &folder.join("media").join(name),
            &root.join("media").join(name),
            hash,
        )?;
    }
    let database = root.join("studio.sqlite");
    copy_checked(
        &folder
            .join("revisions")
            .join(format!("{}.sqlite", manifest.revision)),
        &database,
        &manifest.database_hash,
    )?;
    let store = Store::open(database.clone())?;
    let series = store.series()?;
    ensure!(
        series.len() == 1 && series[0].id == manifest.series,
        "Project series does not match its saved revision"
    );
    // This is a new working copy, not ownership transfer of the source queue.
    // Keep requests and provider IDs, but never automatically execute copied jobs.
    // The original Mac and remote provider continue independently.
    store.conn.execute(
        "UPDATE jobs SET error='Imported snapshot of a ' || status || ' job. Automatic execution is stopped in this copy. The original job may still be running; no remote cancellation was requested.', status='failed', phase='Imported snapshot', updated_at=datetime('now') WHERE status IN ('queued','running','blocked')",
        [],
    )?;
    rewrite(&store.conn, |s| {
        if let Some(relative) = s.strip_prefix(ROOT) {
            ensure!(
                relative.starts_with("media/")
                    && !relative.contains("..")
                    && Path::new(relative).components().count() == 2,
                "Invalid project media path"
            );
            Ok(root.join(relative).to_string_lossy().into_owned())
        } else {
            Ok(s.to_string())
        }
    })?;
    // Configuration is machine-local. Keys remain exclusively in Keychain.
    let local = Store::open(local_home.join("studio.sqlite"))?;
    let mut config = local.config()?;
    if !config.ffmpeg.is_file() {
        for candidate in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
            if Path::new(candidate).is_file() {
                config.ffmpeg = candidate.into();
                break;
            }
        }
    }
    store.save_config(&config)?;
    remember(
        &store,
        &manifest.series,
        &Link {
            folder: folder.to_path_buf(),
            revision: manifest.revision,
        },
    )?;
    Ok(database)
}

pub(crate) fn project_directory() -> Result<PathBuf> {
    let user = directories::UserDirs::new().context("Could not locate your home folder")?;
    let cloud = user
        .home_dir()
        .join("Library/Mobile Documents/com~apple~CloudDocs");
    ensure!(
        cloud.is_dir(),
        "iCloud Drive is unavailable. Enable iCloud Drive; your local work remains safe."
    );
    let folder = cloud.join("DramaFactory");
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}
fn migrate_projects(store: &Store, directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let mut known = HashSet::new();
    for entry in std::fs::read_dir(directory)?.flatten() {
        if entry
            .path()
            .extension()
            .is_some_and(|x| x == "dramafactory")
        {
            // Stop rather than write a duplicate when a cloud save is incomplete/conflicted.
            let revision = head(&entry.path())?
                .context("Wait for iCloud projects to finish syncing before migration")?;
            known.insert(revision.series);
        }
    }
    for series in store.series()? {
        if known.contains(&series.id) || link(store, &series.id)?.is_some() {
            continue;
        }
        let name: String = series
            .name
            .chars()
            .map(|c| if c == '/' || c == ':' { '-' } else { c })
            .collect();
        let folder = directory.join(format!("{}-{}.dramafactory", name, series.id));
        save(store, &series.id, &folder, None)?;
    }
    Ok(())
}

pub(crate) fn default_project_folder(store: &Store, series: &str) -> Result<PathBuf> {
    let name = store
        .series()?
        .into_iter()
        .find(|s| s.id == series)
        .context("Project not found")?
        .name;
    let name: String = name
        .chars()
        .map(|c| if c == '/' || c == ':' { '-' } else { c })
        .collect();
    let directory = project_directory()?;
    for entry in std::fs::read_dir(&directory)?.flatten() {
        if entry
            .path()
            .extension()
            .is_some_and(|x| x == "dramafactory")
        {
            let revision = head(&entry.path())?
                .context("Wait for iCloud to finish syncing before saving a new project")?;
            ensure!(
                revision.series != series,
                "This project already exists in iCloud. Open that project before saving, or use Save project as to preserve this separate revision."
            );
        }
    }
    Ok(directory.join(format!("{}-{}.dramafactory", name, series)))
}

fn remember_folder(folder: &Path) {
    if std::env::var_os("DRAMAFACTORY_DATABASE").is_some() {
        return;
    }
    let Some(parent) = folder.parent() else {
        return;
    };
    let home = crate::store::default_path();
    let registry = home.parent().unwrap().join("project-folders.json");
    let mut folders: Vec<PathBuf> = std::fs::read(&registry)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if !folders.contains(&parent.to_path_buf()) {
        folders.push(parent.to_path_buf());
        if let Ok(bytes) = serde_json::to_vec(&folders) {
            let _ = std::fs::write(registry, bytes);
        }
    }
}
#[derive(Clone)]
pub(crate) struct DiscoveredProject {
    pub name: String,
    pub series: Option<String>,
    pub path: Option<PathBuf>,
    pub issue: Option<String>,
}
#[derive(Default)]
pub(crate) struct Discovery {
    pub projects: Vec<DiscoveredProject>,
    receiver: Option<std::sync::mpsc::Receiver<Vec<DiscoveredProject>>>,
    started: bool,
}
fn discover_projects(home: &Path, folders: &[PathBuf]) -> Vec<DiscoveredProject> {
    let mut projects = Vec::new();
    let mut seen = HashSet::new();
    for root in folders {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let folder = entry.path();
            if folder.extension().is_none_or(|s| s != "dramafactory")
                || !seen.insert(folder.clone())
            {
                continue;
            }
            let mut name = folder
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut series = None;
            let result = (|| -> Result<PathBuf> {
                let revision = head(&folder)?.context("Waiting for a complete iCloud save")?;
                series = Some(revision.series.clone());
                name = revision.name;
                // Cache each immutable cloud revision separately. Never overwrite an edited
                // local working copy, including when another Mac publishes a newer revision.
                let cache = home.join("cloud-projects");
                std::fs::create_dir_all(&cache)?;
                let key = cache.join(format!("{}.json", revision.revision));
                if let Ok(bytes) = std::fs::read(&key)
                    && let Ok(path) = serde_json::from_slice::<PathBuf>(&bytes)
                    && path.starts_with(home.join("projects"))
                    && path.is_file()
                {
                    return Ok(path);
                }
                let path = open(&folder, home)?;
                std::fs::write(key, serde_json::to_vec(&path)?)?;
                Ok(path)
            })();
            let (path, issue) = match result {
                Ok(path) => (Some(path), None),
                Err(error) => (None, Some(error.to_string())),
            };
            projects.push(DiscoveredProject {
                name,
                series,
                path,
                issue,
            });
        }
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    projects
}
impl crate::Studio {
    pub(crate) fn cloud_project_progress(&mut self, ctx: &eframe::egui::Context) {
        if !self.cloud_projects.started {
            self.cloud_projects.started = true;
            if std::env::var_os("DRAMAFACTORY_DATABASE").is_none() {
                let home = crate::store::default_path().parent().unwrap().to_path_buf();
                let mut folders: Vec<PathBuf> = std::fs::read(home.join("project-folders.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
                if let Some(user) = directories::UserDirs::new() {
                    folders.push(
                        user.home_dir()
                            .join("Library/Mobile Documents/com~apple~CloudDocs/DramaFactory"),
                    );
                }
                let (sender, receiver) = std::sync::mpsc::channel();
                self.cloud_projects.receiver = Some(receiver);
                let repaint = ctx.clone();
                let database = self.store.path.clone();
                std::thread::spawn(move || {
                    let migration = (|| -> Result<()> {
                        let directory = project_directory()?;
                        migrate_projects(&Store::open(database.clone())?, &directory)?;
                        let original = home.join("studio.sqlite");
                        if original != database && original.is_file() {
                            migrate_projects(&Store::open(original)?, &directory)?;
                        }
                        Ok(())
                    })();
                    let mut projects = discover_projects(&home, &folders);
                    if let Err(error) = migration {
                        projects.push(DiscoveredProject {
                            name: "Project folder needs attention".into(),
                            series: None,
                            path: None,
                            issue: Some(error.to_string()),
                        });
                    }
                    let _ = sender.send(projects);
                    repaint.request_repaint();
                });
            }
        }
        if let Some(receiver) = &self.cloud_projects.receiver {
            match receiver.try_recv() {
                Ok(projects) => {
                    self.cloud_projects.projects = projects;
                    self.cloud_projects.receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.cloud_projects.receiver = None;
                    self.message = "Project folder scan stopped. Use Project → Refresh synced projects to retry.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    pub(crate) fn open_discovered_project(&mut self, path: PathBuf) {
        if self.portable_task.is_some() || !self.save_before_open() {
            return;
        }
        match Store::open(path.clone()) {
            Ok(store) => {
                self.stop_playback();
                *self = crate::Studio::from_store(store);
                let home = crate::store::default_path();
                if let Err(error) = std::fs::write(
                    home.parent().unwrap().join("last-project.json"),
                    serde_json::to_vec(&path).unwrap(),
                ) {
                    self.portable_error(error);
                } else {
                    self.message =
                        "Synced project opened. Previous local working copies are preserved."
                            .into();
                }
            }
            Err(error) => self.portable_error(error),
        }
    }
}

pub(crate) struct Task {
    receiver: std::sync::mpsc::Receiver<Result<Option<PathBuf>, String>>,
    opening: bool,
}
pub(crate) fn startup_path() -> PathBuf {
    let default = crate::store::default_path();
    if std::env::var_os("DRAMAFACTORY_DATABASE").is_some() {
        return default;
    }
    let remembered = default.parent().unwrap().join("last-project.json");
    std::fs::read(remembered)
        .ok()
        .and_then(|b| serde_json::from_slice::<PathBuf>(&b).ok())
        .filter(|p| p.is_file() && p.starts_with(default.parent().unwrap().join("projects")))
        .unwrap_or(default)
}
impl crate::Studio {
    pub(crate) fn project_file_menu(&mut self, ui: &mut eframe::egui::Ui) {
        ui.menu_button("Project…", |ui| {
            if let Some(series) = &self.selected
                && let Ok(Some(link)) = link(&self.store, series)
            {
                ui.label(
                    link.folder
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
                .on_hover_text(link.folder.display().to_string());
                ui.separator();
            }
            if ui
                .add_enabled(
                    self.selected.is_some(),
                    eframe::egui::Button::new("Save project as…"),
                )
                .clicked()
            {
                ui.close();
                self.save_portable_as();
            }
            if ui.button("Refresh synced projects").clicked() {
                self.cloud_projects = Default::default();
                ui.close();
            }
            if ui.button("Open project…").clicked() {
                ui.close();
                self.open_portable();
            }
            if ui.button("Recover older local projects").clicked() {
                ui.close();
                if self.save_before_open() {
                    let path = crate::store::default_path();
                    match Store::open(path) {
                        Ok(store) => {
                            self.stop_playback();
                            *self = crate::Studio::from_store(store);
                            let _ =
                                std::fs::remove_file(self.store.root().join("last-project.json"));
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
            }
        });
    }
    fn save_before_open(&mut self) -> bool {
        let result = (|| -> Result<()> {
            ensure!(
                !self.sidekick.busy(),
                "Wait for Sidekick to finish before opening a project"
            );
            // Flush every loaded series before replacing the GUI state.
            for series in self.store.series()? {
                let issues = self.save_project_inner(&series.id)?;
                ensure!(
                    issues.is_empty(),
                    "Finish saving before opening a project: {}",
                    issues.join(" · ")
                );
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.portable_error(e);
            return false;
        }
        true
    }
    fn portable_error(&mut self, error: impl std::fmt::Display) {
        self.message = error.to_string();
        self.project_save_notice = Some((false, self.message.clone(), std::time::Instant::now()));
    }
    pub(crate) fn save_portable_as(&mut self) {
        let Some(series) = self.selected.clone() else {
            return;
        };
        let name = self
            .store
            .series()
            .unwrap_or_default()
            .into_iter()
            .find(|s| s.id == series)
            .map(|s| s.name)
            .unwrap_or_else(|| "Series".into());
        let name: String = name
            .chars()
            .map(|c| if c == '/' || c == ':' { '-' } else { c })
            .collect();
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save portable project")
            .set_file_name(format!("{name}.dramafactory"));
        if let Some(home) = directories::UserDirs::new() {
            let cloud = home
                .home_dir()
                .join("Library/Mobile Documents/com~apple~CloudDocs/DramaFactory");
            if cloud.is_dir() {
                dialog = dialog.set_directory(cloud);
            }
        }
        let Some(mut folder) = dialog.save_file() else {
            return;
        };
        if folder.extension().is_none_or(|s| s != "dramafactory") {
            folder.set_extension("dramafactory");
        }
        match self.save_project_inner(&series) {
            Ok(issues) if issues.is_empty() => self.begin_portable_save(folder, None),
            Ok(issues) => self.portable_error(issues.join(" · ")),
            Err(e) => self.portable_error(e),
        }
    }
    pub(crate) fn begin_portable_save(&mut self, folder: PathBuf, expected: Option<String>) {
        if self.portable_task.is_some() {
            return;
        }
        let Some(series) = self.selected.clone() else {
            return;
        };
        remember_folder(&folder);
        let database = self.store.path.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.portable_task = Some(Task {
            receiver,
            opening: false,
        });
        self.message = "Saving project and media…".into();
        std::thread::spawn(move || {
            let result = Store::open(database)
                .and_then(|store| save(&store, &series, &folder, expected.as_deref()))
                .map(|_| None)
                .map_err(|e| {
                    format!("Could not save the project folder: {e}. Local edits are preserved.")
                });
            let _ = sender.send(result);
        });
    }
    fn open_portable(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose a .dramafactory project folder")
            .pick_folder()
        else {
            return;
        };
        if !self.save_before_open() {
            return;
        }
        remember_folder(&folder);
        let local_home = crate::store::default_path().parent().unwrap().to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.portable_task = Some(Task {
            receiver,
            opening: true,
        });
        self.message = "Opening project and media…".into();
        std::thread::spawn(move || {
            let _ = sender.send(open(&folder, &local_home).map(Some).map_err(|e| {
                format!("Could not open project: {e}. Your existing project is unchanged.")
            }));
        });
    }
    pub(crate) fn portable_progress(&mut self, ctx: &eframe::egui::Context) {
        let Some(task) = &self.portable_task else {
            return;
        };
        let result = task.receiver.try_recv();
        match result {
            Ok(result) => {
                self.portable_task = None;
                match result {
                    Ok(Some(path)) => {
                        match Store::open(path.clone()) {
                            Ok(store) => {
                                self.stop_playback();
                                *self = crate::Studio::from_store(store);
                                let home = crate::store::default_path();
                                let pointer = home.parent().unwrap().join("last-project.json");
                                if let Err(e) =
                                    std::fs::write(pointer, serde_json::to_vec(&path).unwrap())
                                {
                                    self.portable_error(format!("Project opened, but its location could not be remembered: {e}"));
                                } else {
                                    self.message = "Project opened from local working copy".into();
                                }
                            }
                            Err(e) => self.portable_error(e),
                        }
                    }
                    Ok(None) => {
                        self.cloud_projects = Default::default();
                        self.message = "Project saved".into();
                        self.project_save_notice =
                            Some((true, self.message.clone(), std::time::Instant::now()));
                    }
                    Err(e) => self.portable_error(e),
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.portable_task = None;
                self.portable_error("Project transfer stopped. Your local work is preserved.");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                eframe::egui::Modal::new(eframe::egui::Id::new("portable-progress")).show(
                    ctx,
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(if task.opening {
                                "Opening project and media…"
                            } else {
                                "Saving project and media…"
                            });
                        });
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_projects_move_to_cloud_without_duplicate_saves_or_data_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("local/studio.sqlite")).unwrap();
        let series = store
            .create_series("Existing project", "Keep this story")
            .unwrap();
        let directory = temp.path().join("cloud");
        migrate_projects(&store, &directory).unwrap();
        let linked = link(&store, &series.id).unwrap().unwrap();
        assert!(linked.folder.starts_with(&directory));
        assert_eq!(head(&linked.folder).unwrap().unwrap().series, series.id);
        migrate_projects(&store, &directory).unwrap();
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), 1);
        assert_eq!(store.series().unwrap()[0].premise, "Keep this story");
        assert_eq!(
            link(&store, &series.id).unwrap().unwrap().revision,
            linked.revision
        );
    }

    #[test]
    fn discovery_reuses_local_edits_and_preserves_them_when_cloud_advances() {
        let temp = tempfile::tempdir().unwrap();
        let cloud = temp.path().join("cloud");
        std::fs::create_dir_all(&cloud).unwrap();
        let source = Store::open(temp.path().join("source/studio.sqlite")).unwrap();
        let series = source.create_series("BlockSide", "Original").unwrap();
        let folder = cloud.join("BlockSide.dramafactory");
        let first = save(&source, &series.id, &folder, None).unwrap();
        let home = temp.path().join("studio");
        let roots = vec![cloud.clone()];
        let discovered = discover_projects(&home, &roots);
        assert_eq!(discovered.len(), 1);
        let old_path = discovered[0].path.clone().unwrap();
        let local = Store::open(old_path.clone()).unwrap();
        local
            .conn
            .execute("UPDATE series SET name='Local edits'", [])
            .unwrap();
        assert_eq!(
            discover_projects(&home, &roots)[0].path.as_ref(),
            Some(&old_path)
        );
        source
            .conn
            .execute("UPDATE series SET name='From MacBook'", [])
            .unwrap();
        save(&source, &series.id, &folder, Some(&first.revision)).unwrap();
        let refreshed = discover_projects(&home, &roots);
        let new_path = refreshed[0].path.clone().unwrap();
        assert_ne!(old_path, new_path);
        assert_eq!(
            Store::open(new_path).unwrap().series().unwrap()[0].name,
            "From MacBook"
        );
        assert_eq!(local.series().unwrap()[0].name, "Local edits");
        // Partial iCloud downloads are visible but cannot be opened.
        let incomplete = cloud.join("Incomplete.dramafactory");
        std::fs::create_dir_all(&incomplete).unwrap();
        let results = discover_projects(&home, &roots);
        assert!(
            results
                .iter()
                .any(|p| p.name == "Incomplete" && p.path.is_none() && p.issue.is_some())
        );
        // Divergent cloud heads are never silently chosen.
        let mut branch: Revision = serde_json::from_slice(
            &std::fs::read(
                folder
                    .join("revisions")
                    .join(format!("{}.json", first.revision)),
            )
            .unwrap(),
        )
        .unwrap();
        branch.revision = uuid::Uuid::new_v4().to_string();
        std::fs::write(
            folder
                .join("revisions")
                .join(format!("{}.json", branch.revision)),
            serde_json::to_vec(&branch).unwrap(),
        )
        .unwrap();
        let results = discover_projects(&home, &roots);
        assert!(results.iter().any(|p| {
            p.path.is_none()
                && p.issue
                    .as_ref()
                    .is_some_and(|s| s.contains("conflicting saves"))
        }));
        assert_eq!(local.series().unwrap()[0].name, "Local edits");
    }

    #[test]
    fn portable_round_trip_keeps_series_media_chat_and_cut_and_detects_conflicts() {
        let temp = std::env::temp_dir().join(format!("df-portable-test-{}", uuid::Uuid::new_v4()));
        let home = temp.join("mac-one");
        let store = Store::open(home.join("studio.sqlite")).unwrap();
        let series = store.create_series("Traveling series", "Story").unwrap();
        let other = store
            .create_series("Private unrelated series", "Do not export")
            .unwrap();
        let episode = store.create_episode(&series.id, "Pilot").unwrap();
        store.create_episode(&other.id, "Unrelated").unwrap();
        let portrait = home.join("portrait.png");
        std::fs::write(&portrait, b"portrait fixture bytes").unwrap();
        let cast = store
            .add_library(&series.id, "character", "Lead", "Profile", Some(&portrait))
            .unwrap();
        let asset = uuid::Uuid::new_v4().to_string();
        let job = uuid::Uuid::new_v4().to_string();
        let clip = home.join("clip.mp4");
        let ffmpeg = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG").map(PathBuf::from);
        if let Some(ffmpeg) = &ffmpeg {
            assert!(
                std::process::Command::new(ffmpeg)
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "lavfi",
                        "-i",
                        "color=c=blue:s=96x170:r=24:d=30",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=frequency=440:duration=30",
                        "-c:v",
                        "libx264",
                        "-c:a",
                        "aac",
                        "-shortest"
                    ])
                    .arg(&clip)
                    .status()
                    .unwrap()
                    .success()
            );
        } else {
            std::fs::write(&clip, b"video fixture bytes").unwrap();
        }
        let source_bytes = std::fs::read(&clip).unwrap();
        store.conn.execute("INSERT INTO jobs(id,prompt,status,error,request) VALUES(?1,'prompt','completed','',?2)", params![job, serde_json::json!({"episode_id":episode.id}).to_string()]).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO assets VALUES(?1,?2,?3,?4,480,854,30,'{}','unreviewed')",
                params![
                    asset,
                    job,
                    clip.to_string_lossy(),
                    crate::worker::hash_file(&clip).unwrap()
                ],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO segment_generations VALUES(?1,?2,'segment',1,'{}')",
                params![job, episode.id],
            )
            .unwrap();
        let cut = serde_json::json!({"clips":[{"id":"cut-clip","asset_id":asset,"title":"Take 1","gap_before":2.0,"audio_fade_out":0.5,"audio_fade_in":0.2,"audio_gain_db":-3.0,"audio_muted":false,"start":1.0,"duration":20.0}]}).to_string();
        store
            .conn
            .execute(
                "INSERT INTO settings VALUES(?1,?2)",
                params![format!("episode-cut:{}", episode.id), cut],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO settings VALUES('private-machine-setting','never transfer')",
                [],
            )
            .unwrap();
        store.conn.execute("INSERT INTO sidekick_messages VALUES('message',?1,'assistant','Remember our story','Synopsis')",[&series.id]).unwrap();
        store.set_video_aspect_ratio("16:9").unwrap();
        store.remember_gui_series(&series.id).unwrap();
        store.remember_gui_episode(&episode.id).unwrap();
        let release = crate::publish::ReleaseDraft {
            title: "Pilot release".into(),
            cover_png: vec![1, 2, 3],
            tiktok_cover_png: vec![4, 5, 6],
            destinations: [true, true, true],
            ..Default::default()
        };
        store
            .save_release_draft(&episode.id, &release, None)
            .unwrap();
        let folder = temp.join("Traveling.dramafactory");
        let first = save(&store, &series.id, &folder, None).unwrap();
        assert_eq!(
            link(&store, &series.id).unwrap().unwrap().revision,
            first.revision
        );
        let snapshot = std::fs::read(
            folder
                .join("revisions")
                .join(format!("{}.sqlite", first.revision)),
        )
        .unwrap();
        assert!(
            !snapshot
                .windows(b"never transfer".len())
                .any(|b| b == b"never transfer")
        );
        assert!(
            !snapshot
                .windows(b"Private unrelated series".len())
                .any(|b| b == b"Private unrelated series")
        );
        let mac_two = temp.join("mac-two");
        if let Some(ffmpeg) = &ffmpeg {
            let machine = Store::open(mac_two.join("studio.sqlite")).unwrap();
            let mut config = machine.config().unwrap();
            config.ffmpeg = ffmpeg.clone();
            machine.save_config(&config).unwrap();
        }
        let opened = open(&folder, &mac_two).unwrap();
        let second = Store::open(opened).unwrap();
        let restored_release = second.release_draft(&episode.id).unwrap().0;
        assert_eq!(restored_release.title, release.title);
        assert_eq!(restored_release.cover_png, release.cover_png);
        assert_eq!(restored_release.destinations, release.destinations);
        assert_eq!(second.video_aspect_ratio(), "16:9");
        assert_eq!(second.series().unwrap().len(), 1);
        assert_eq!(second.series().unwrap()[0].id, series.id);
        assert_eq!(
            second.current_gui_episode(&series.id).unwrap(),
            Some(episode.id.clone())
        );
        let actual: String = second
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [format!("episode-cut:{}", episode.id)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
            serde_json::from_str::<serde_json::Value>(&cut).unwrap()
        );
        assert_eq!(
            second.sidekick_messages(&series.id).unwrap()[0].text,
            "Remember our story"
        );
        let new_portrait: String = second
            .conn
            .query_row(
                "SELECT reference FROM library WHERE id=?1",
                [&cast.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(Path::new(&new_portrait).starts_with(&mac_two));
        assert_eq!(
            std::fs::read(new_portrait).unwrap(),
            b"portrait fixture bytes"
        );
        assert_eq!(
            std::fs::read(&second.assets().unwrap()[0].path).unwrap(),
            source_bytes
        );
        assert_eq!(
            second
                .conn
                .query_row(
                    "SELECT count(*) FROM settings WHERE key='private-machine-setting'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        if let Some(ffmpeg) = &ffmpeg {
            let restored_cut = second.episode_cut(&episode.id).unwrap();
            let export = second
                .preview_episode_cut(&episode.id, &restored_cut)
                .unwrap();
            crate::worker::run(second.path.clone(), true).unwrap();
            let finished = second.job(&export.id).unwrap();
            assert_eq!(finished.status, "completed", "{}", finished.error);
            let output = second
                .assets()
                .unwrap()
                .into_iter()
                .find(|a| Some(&a.id) == finished.output_id.as_ref())
                .unwrap();
            let info = crate::media::probe(ffmpeg, &output.path).unwrap();
            assert!((info.duration - 22.).abs() < 0.15);
            assert!(info.audio);
        }
        let next = save(&second, &series.id, &folder, Some(&first.revision)).unwrap();
        assert!(save(&store, &series.id, &folder, Some(&first.revision)).is_err());
        assert_eq!(head(&folder).unwrap().unwrap().revision, next.revision);
        // Two offline Macs can create sibling revisions before iCloud synchronizes.
        // Keep both and report the branch rather than silently picking one.
        let mut branch = head(&folder).unwrap().unwrap();
        branch.revision = uuid::Uuid::new_v4().to_string();
        branch.parent = Some(first.revision.clone());
        let branch_path = folder
            .join("revisions")
            .join(format!("{}.json", branch.revision));
        std::fs::write(&branch_path, serde_json::to_vec(&branch).unwrap()).unwrap();
        assert!(head(&folder).is_err());
        std::fs::remove_file(branch_path).unwrap();
        // Missing cloud media must never produce an apparently successful open.
        let manifest = head(&folder).unwrap().unwrap();
        let media = manifest.media.keys().next().unwrap();
        std::fs::write(folder.join("media").join(media), b"corrupt").unwrap();
        assert!(open(&folder, &temp.join("mac-three")).is_err());
        // The source media and database are unchanged.
        assert_eq!(std::fs::read(clip).unwrap(), source_bytes);
        assert_eq!(store.series().unwrap().len(), 2);
        drop(second);
        drop(store);
        std::fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn portable_save_preserves_only_this_series_buffer_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("Series", "").unwrap();
        let other = store.create_series("Other", "").unwrap();
        let episode = store.create_episode(&series.id, "Episode").unwrap();
        let other_episode = store.create_episode(&other.id, "Other episode").unwrap();
        for id in [&episode.id, &other_episode.id] {
            store.conn.execute("INSERT INTO buffer_submissions VALUES(?1,'hash','channel','confirmed-post-id')",[id]).unwrap();
        }
        let folder = temp.path().join("Project.dramafactory");
        save(&store, &series.id, &folder, None).unwrap();
        let imported = Store::open(open(&folder, &temp.path().join("other-mac")).unwrap()).unwrap();
        let receipts: Vec<(String, String)> = imported
            .conn
            .prepare("SELECT episode,receipt FROM buffer_submissions")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(receipts, vec![(episode.id, "confirmed-post-id".into())]);
    }
    #[test]
    fn portable_save_snapshots_active_jobs_without_restarting_them() {
        let temp =
            std::env::temp_dir().join(format!("df-portable-active-{}", uuid::Uuid::new_v4()));
        let store = Store::open(temp.join("studio.sqlite")).unwrap();
        let series = store.create_series("Series", "Before completion").unwrap();
        for status in ["queued", "running", "blocked"] {
            store
                .conn
                .execute(
                    "INSERT INTO jobs(id,prompt,status,error,request) VALUES(?1,'prompt',?1,'',?2)",
                    rusqlite::params![
                        status,
                        serde_json::json!({"series_id":series.id}).to_string()
                    ],
                )
                .unwrap();
        }
        store.conn.execute("INSERT INTO remote_video_jobs(job_id,state,remote_id) VALUES('running','accepted','provider-job-123')", []).unwrap();
        let folder = temp.join("Project.dramafactory");
        let first = save(&store, &series.id, &folder, None).unwrap();
        let imported = Store::open(open(&folder, &temp.join("other-mac")).unwrap()).unwrap();
        for status in ["queued", "running", "blocked"] {
            assert_eq!(store.job(status).unwrap().status, status);
            let job = imported.job(status).unwrap();
            assert_eq!(job.status, "failed");
            assert!(job.error.contains("no remote cancellation"));
            let request = |db: &Store| -> String {
                db.conn
                    .query_row("SELECT request FROM jobs WHERE id=?1", [status], |r| {
                        r.get(0)
                    })
                    .unwrap()
            };
            assert_eq!(request(&imported), request(&store));
        }
        let remote: String = imported
            .conn
            .query_row(
                "SELECT remote_id FROM remote_video_jobs WHERE job_id='running'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remote, "provider-job-123");
        // Later saves capture source progress without changing the earlier revision.
        store
            .conn
            .execute("UPDATE jobs SET status='completed' WHERE id='running'", [])
            .unwrap();
        let second = save(&store, &series.id, &folder, Some(&first.revision)).unwrap();
        assert_ne!(first.revision, second.revision);
        let latest = Store::open(open(&folder, &temp.join("third-mac")).unwrap()).unwrap();
        assert_eq!(latest.job("running").unwrap().status, "completed");
        assert_eq!(imported.job("running").unwrap().status, "failed");
        drop(latest);
        drop(imported);
        drop(store);
        std::fs::remove_dir_all(temp).unwrap();
    }
}
