//! Machine-local GUI command mailbox. Never included in portable project snapshots.
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let c = Connection::open(path.with_extension("control.sqlite"))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS live_state(id INTEGER PRIMARY KEY CHECK(id=1), instance TEXT NOT NULL, touched INTEGER NOT NULL, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS commands(id TEXT PRIMARY KEY, instance TEXT NOT NULL, action TEXT NOT NULL, payload TEXT NOT NULL, result TEXT, created INTEGER NOT NULL DEFAULT(unixepoch()));")?;
    Ok(c)
}
pub fn state(path: &Path) -> Result<Value> {
    let c = open(path)?;
    let raw: Option<String> = c
        .query_row(
            "SELECT data FROM live_state WHERE id=1 AND touched >= unixepoch()-10",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let mut v: Value = serde_json::from_str(&raw.ok_or_else(|| {
        anyhow::anyhow!("Open DramaFactory with this project database. Live workspace is offline.")
    })?)?;
    v["reply_schema"] = crate::sidekick_edit::response_format()["json_schema"]["schema"].clone();
    v["actions"] = json!({
        "read_episode_batch":{"series_id":"required; returns numbered episodes with live screenplay and plot drafts, independent of visible tab"},
        "apply_episode_batch":{"series_id":"required","expected":"exact read_episode_batch result","start_episode":"one-based number","episodes":"1-25 consecutive NewEpisode objects: title, screenplay, goal, conflict, turn, payoff, hook, ending_resolution, requires (Fact[]), changes (Change[]). Creates missing episodes and updates existing ones atomically. Saves targeted episodes, preserves unrelated drafts and retains recovery backup. No media generation; no tab or episode switch."},
        "take_over":{},"capture":{},"release":{},
        "reply":{"request_id":"from capture or pending GUI Send", "reply":"object matching reply_schema; read messages for actual applied/conflict result"},
        "navigate":{"page":["Studio","Synopsis","Story","Series","Cast","Locations","Scenes","Assets","Timeline","Dialogue","Images","Queue","Settings","EditingRoom","Publish"],"series_id":"optional","episode_id":"optional"},
        "save_drafts":{"expected_workspace":"exact workspace from get_live_workspace; saves local drafts, not portable folder"},
        "undo":{"target":["synopsis","plot","episode","generation","publishing","character"],"character_id":"required for character"},
        "edit_timeline":{"expected":"exact workspace.timeline snapshot","cut":"complete Cut object; draft only, UI Undo available"}
    });
    Ok(v)
}
pub fn submit(
    path: &Path,
    id: &str,
    instance: &str,
    action: &str,
    payload: &Value,
) -> Result<Value> {
    ensure!(
        !id.is_empty() && id.len() <= 128,
        "Provide a stable request_id (up to 128 bytes)"
    );
    ensure!(
        [
            "take_over",
            "capture",
            "reply",
            "release",
            "navigate",
            "save_drafts",
            "edit_timeline",
            "undo",
            "read_episode_batch",
            "apply_episode_batch"
        ]
        .contains(&action),
        "Unknown live action"
    );
    ensure!(
        payload.to_string().len() <= 2 * 1024 * 1024,
        "Command too large"
    );
    let c = open(path)?;
    let tx = c.unchecked_transaction()?;
    let active: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM live_state WHERE instance=?1 AND touched>=unixepoch()-10)",
        [instance],
        |r| r.get(0),
    )?;
    ensure!(
        active,
        "GUI session changed or is offline. Read live workspace again."
    );
    let body = payload.to_string();
    tx.execute(
        "INSERT OR IGNORE INTO commands(id,instance,action,payload) VALUES(?1,?2,?3,?4)",
        params![id, instance, action, body],
    )?;
    let same: bool = tx.query_row(
        "SELECT instance=?2 AND action=?3 AND payload=?4 FROM commands WHERE id=?1",
        params![id, instance, action, body],
        |r| r.get(0),
    )?;
    ensure!(same, "request_id already used for different content");
    tx.commit()?;
    status(path, id)
}
pub fn status(path: &Path, id: &str) -> Result<Value> {
    let c = open(path)?;
    let (result, fresh): (Option<String>, bool) = c.query_row(
        "SELECT result,created>=unixepoch()-30 FROM commands WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(match result {
        Some(raw) => serde_json::from_str(&raw)?,
        None => json!({"request_id":id,"status":if fresh {"pending"} else {"expired"}}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mailbox_rejects_offline_stale_and_reused_commands() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("studio.sqlite");
        assert!(state(&p).is_err());
        let c = open(&p).unwrap();
        c.execute(
            "INSERT INTO live_state VALUES(1,'session',unixepoch(),'{\"instance\":\"session\"}')",
            [],
        )
        .unwrap();
        assert!(submit(&p, "one", "old", "take_over", &json!({})).is_err());
        assert_eq!(
            submit(&p, "one", "session", "take_over", &json!({})).unwrap()["status"],
            "pending"
        );
        assert!(submit(&p, "one", "session", "release", &json!({})).is_err());
        assert!(submit(&p, "one", "session", "take_over", &json!({})).is_ok());
    }
}

#[cfg(test)]
mod batch_contract_tests {
    use super::*;
    #[test]
    fn advertised_batch_actions_can_be_submitted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("studio.sqlite");
        let c = open(&path).unwrap();
        c.execute(
            "INSERT INTO live_state VALUES(1,'test',unixepoch(),'{}')",
            [],
        )
        .unwrap();
        for action in ["read_episode_batch", "apply_episode_batch"] {
            let receipt = submit(&path, action, "test", action, &json!({"series_id":"s"})).unwrap();
            assert_eq!(receipt["status"], "pending");
        }
    }
}
