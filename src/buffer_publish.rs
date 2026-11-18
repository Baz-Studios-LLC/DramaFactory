use crate::{
    credentials::Provider,
    production::Asset,
    publish::{PLATFORMS, ReleaseDraft},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Stdio},
};

fn graphql(key: &str, query: &str, variables: Value) -> Result<Value> {
    let mut response: Value = ureq::post("https://api.buffer.com")
        .header("Authorization", format!("Bearer {key}"))
        .send_json(json!({ "query":query,"variables":variables }))?
        .body_mut()
        .read_json()?;
    if let Some(message) = response
        .pointer("/errors/0/message")
        .and_then(Value::as_str)
    {
        anyhow::bail!("Buffer: {message}");
    }
    Ok(response["data"].take())
}

fn host(asset: &Asset, cloud: &str, preset: &str) -> Result<String> {
    ensure!(
        !cloud.is_empty()
            && cloud
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "Enter your Cloudinary cloud name."
    );
    ensure!(
        !preset.trim().is_empty(),
        "Enter a Cloudinary unsigned video upload preset."
    );
    ensure!(
        crate::worker::hash_file(&asset.path)? == asset.hash,
        "Export changed. Export the episode again."
    );
    let result = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "900",
            "--form-string",
            &format!("upload_preset={preset}"),
            "--form",
            "file=@-;filename=episode.mp4;type=video/mp4",
            &format!("https://api.cloudinary.com/v1_1/{cloud}/video/upload"),
        ])
        .stdin(Stdio::from(std::fs::File::open(&asset.path)?))
        .output()?;
    ensure!(
        result.status.success(),
        "Video upload failed. Check the connection and try again."
    );
    let reply: Value = serde_json::from_slice(&result.stdout)?;
    reply["secure_url"]
        .as_str()
        .filter(|url| url.starts_with("https://"))
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cloudinary: {}",
                reply
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Missing video URL")
            )
        })
}

pub fn queue(
    db: &Path,
    episode: &str,
    asset: &Asset,
    draft: &ReleaseDraft,
    cloud: &str,
    preset: &str,
) -> Result<String> {
    let key = Provider::Buffer
        .read()?
        .context("Add your Buffer key in Settings.")?;
    let account = graphql(&key, "{account{organizations{id}}}", json!({}))?;
    let mut channels = Vec::new();
    for org in account
        .pointer("/account/organizations")
        .and_then(Value::as_array)
        .context("Buffer returned no organizations.")?
    {
        let data = graphql(
            &key,
            "query($id:OrganizationId!){channels(input:{organizationId:$id}){id name service}}",
            json!({"id":org["id"]}),
        )?;
        channels.extend(
            data["channels"]
                .as_array()
                .context("Missing Buffer channels")?
                .iter()
                .cloned(),
        );
    }
    let mut selected = Vec::new();
    for (i, service) in ["tiktok", "youtube", "instagram"].iter().enumerate() {
        if !draft.destinations[i] {
            continue;
        }
        let matches: Vec<_> = channels
            .iter()
            .filter(|c| c["service"].as_str() == Some(service))
            .collect();
        ensure!(
            matches.len() == 1,
            "Expected one connected {} channel in Buffer, found {}.",
            PLATFORMS[i],
            matches.len()
        );
        selected.push((i, matches[0]["id"].as_str().context("Missing channel ID")?));
    }
    let conn = Connection::open(db)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS buffer_uploads(hash TEXT PRIMARY KEY,url TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS buffer_submissions(episode TEXT,hash TEXT,channel TEXT,receipt TEXT,PRIMARY KEY(episode,hash,channel));")?;
    let url = match conn
        .query_row(
            "SELECT url FROM buffer_uploads WHERE hash=?1",
            [&asset.hash],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        Some(url) => url,
        None => {
            let url = host(asset, cloud, preset)?;
            conn.execute(
                "INSERT OR REPLACE INTO buffer_uploads VALUES(?1,?2)",
                params![asset.hash, url],
            )?;
            url
        }
    };
    let mut statuses = Vec::new();
    for (i, id) in selected {
        let key_fields = params![episode, asset.hash, id];
        if conn.execute(
            "INSERT OR IGNORE INTO buffer_submissions VALUES(?1,?2,?3,NULL)",
            key_fields,
        )? == 0
        {
            let receipt:Option<String>=conn.query_row("SELECT receipt FROM buffer_submissions WHERE episode=?1 AND hash=?2 AND channel=?3",key_fields,|r|r.get(0))?;
            statuses.push(format!(
                "{} {}",
                PLATFORMS[i],
                if receipt.is_some() {
                    "already queued"
                } else {
                    "unconfirmed - check Buffer"
                }
            ));
            continue;
        }
        let metadata = match i {
            0 => json!({"tiktok":{"isAiGenerated":true}}),
            1 => json!({"youtube":{"title":draft.title,"categoryId":"1","isAiGenerated":true}}),
            _ => json!({"instagram":{"type":"reel","shouldShareToFeed":true,"isAiGenerated":true}}),
        };
        let input = json!({"text":draft.captions[0],"channelId":id,"mode":"addToQueue","schedulingType":"automatic","assets":[{"video":{"url":url}}],"metadata":metadata});
        match graphql(
            &key,
            "mutation($input:CreatePostInput!){createPost(input:$input){... on PostActionSuccess{post{id}} ... on MutationError{message}}}",
            json!({"input":input}),
        ) {
            Ok(reply)
                if reply
                    .pointer("/createPost/post/id")
                    .and_then(Value::as_str)
                    .is_some() =>
            {
                conn.execute("UPDATE buffer_submissions SET receipt=?4 WHERE episode=?1 AND hash=?2 AND channel=?3",params![episode,asset.hash,id,reply["createPost"]["post"]["id"].as_str()])?;
                statuses.push(format!("{} queued", PLATFORMS[i]));
            }
            Ok(reply)
                if reply
                    .pointer("/createPost/message")
                    .and_then(Value::as_str)
                    .is_some() =>
            {
                conn.execute(
                    "DELETE FROM buffer_submissions WHERE episode=?1 AND hash=?2 AND channel=?3",
                    key_fields,
                )?;
                statuses.push(format!(
                    "{}: {}",
                    PLATFORMS[i], reply["createPost"]["message"]
                ));
            }
            other => statuses.push(format!(
                "{} unconfirmed - check Buffer. {}",
                PLATFORMS[i],
                other.err().map(|e| e.to_string()).unwrap_or_default()
            )),
        }
    }
    Ok(statuses.join(" · "))
}
