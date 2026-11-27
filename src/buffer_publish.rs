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
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(45)))
        .max_redirects(0)
        .build()
        .into();
    let mut response: Value = agent
        .post("https://api.buffer.com")
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

fn host(asset: &Asset) -> Result<String> {
    use sha2::{Digest, Sha256};
    let credentials = crate::credentials::CloudinaryCredentials::load()?;
    ensure!(
        crate::worker::hash_file(&asset.path)? == asset.hash,
        "Export changed. Export the episode again."
    );
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let signature: String =
        Sha256::digest(format!("timestamp={timestamp}{}", credentials.api_secret))
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
    let result = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "900",
            "--write-out",
            "\n%{http_code}",
            "--form-string",
            &format!("api_key={}", credentials.api_key),
            "--form-string",
            &format!("timestamp={timestamp}"),
            "--form-string",
            &format!("signature={signature}"),
            "--form",
            "file=@-;filename=episode.mp4;type=video/mp4",
            &format!(
                "https://api.cloudinary.com/v1_1/{}/video/upload",
                credentials.cloud_name
            ),
        ])
        .stdin(Stdio::from(std::fs::File::open(&asset.path)?))
        .output()?;
    ensure!(
        result.status.success(),
        "Video upload could not finish. Check your connection and try again."
    );
    let body = String::from_utf8(result.stdout).context("Invalid Cloudinary response")?;
    let (body, status) = body
        .rsplit_once('\n')
        .context("Missing Cloudinary status")?;
    let status: u16 = status.trim().parse().context("Invalid Cloudinary status")?;
    let reply: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    ensure!(
        (200..300).contains(&status),
        "{}",
        crate::frame_handoff::upload_error(status, &reply)
    );
    reply["secure_url"]
        .as_str()
        .filter(|url| url.starts_with("https://res.cloudinary.com/"))
        .map(str::to_owned)
        .context("Cloudinary did not return a secure video URL")
}

fn cover_video(ffmpeg: &Path, source: &Asset, cover: &[u8], directory: &Path) -> Result<Asset> {
    ensure!(
        crate::worker::hash_file(&source.path)? == source.hash,
        "Export changed. Export the episode again."
    );
    let info = crate::media::probe(ffmpeg, &source.path)?;
    ensure!(
        info.width > 0 && info.height > 0 && info.width % 2 == 0 && info.height % 2 == 0,
        "Video dimensions must be even"
    );
    let image_path = directory.join("cover.png");
    std::fs::write(&image_path, cover)?;
    let output = directory.join("tiktok-with-cover.mp4");
    let (w, h) = (info.width, info.height);
    let mut filter = format!(
        "[1:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,fps=30,trim=duration=0.2,setpts=PTS-STARTPTS[c];[0:v]setsar=1,fps=30,setpts=PTS-STARTPTS[v];[c][v]concat=n=2:v=1:a=0[out]"
    );
    if info.audio {
        filter.push_str(";[0:a]asetpts=PTS-STARTPTS,adelay=200:all=1[a]");
    }
    let mut command = Command::new(ffmpeg);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&source.path)
        .args(["-loop", "1", "-framerate", "30", "-t", "0.2", "-i"])
        .arg(&image_path)
        .args(["-filter_complex", &filter, "-map", "[out]"]);
    if info.audio {
        command.args(["-map", "[a]", "-c:a", "aac", "-b:a", "192k"]);
    }
    let mut child = command
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            "-t",
            &(info.duration + 0.2).to_string(),
        ])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "Could not prepare TikTok cover video; original episode is unchanged"
            );
            break;
        }
        if started.elapsed().as_secs() > 900 {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("TikTok cover preparation timed out; original episode is unchanged");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let result = crate::media::probe(ffmpeg, &output)?;
    ensure!(
        (result.duration - info.duration - 0.2).abs() < 0.15 && result.audio == info.audio,
        "TikTok cover video validation failed"
    );
    let mut asset = source.clone();
    asset.hash = crate::worker::hash_file(&output)?;
    asset.path = output;
    asset.duration = result.duration;
    Ok(asset)
}

pub enum Event {
    Progress(String),
    Finished(String),
}
fn post_input(draft: &ReleaseDraft, platform: usize, channel: &str, url: &str) -> Value {
    let metadata = match platform {
        0 => json!({"tiktok":{"isAiGenerated":true}}),
        1 => json!({"youtube":{"title":draft.title,"categoryId":"1","isAiGenerated":true}}),
        _ => json!({"instagram":{"type":"reel","shouldShareToFeed":true,"isAiGenerated":true}}),
    };
    let mut video = json!({"url":url});
    if platform == 0 && !draft.tiktok_cover_png.is_empty() {
        video["metadata"] = json!({"thumbnailOffset":100});
    }
    json!({"text":draft.captions[platform],"channelId":channel,"mode":"shareNow","schedulingType":"automatic","assets":[{"video":video}],"metadata":metadata})
}

pub fn publish(
    db: &Path,
    episode: &str,
    asset: &Asset,
    draft: &ReleaseDraft,
    progress: &std::sync::mpsc::Sender<Event>,
) -> Result<String> {
    ensure!(
        draft.destinations.contains(&true),
        "Choose at least one platform"
    );
    for (index, enabled) in draft.destinations.iter().enumerate() {
        ensure!(
            !enabled || !draft.captions[index].trim().is_empty(),
            "Add text for {}",
            PLATFORMS[index]
        );
    }
    ensure!(
        !draft.destinations[1] || !draft.title.trim().is_empty(),
        "Add a YouTube title"
    );
    let _ = progress.send(Event::Progress("Checking Buffer channels...".into()));
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
    conn.busy_timeout(std::time::Duration::from_secs(15))?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS buffer_uploads(hash TEXT PRIMARY KEY,url TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS buffer_submissions(episode TEXT,hash TEXT,channel TEXT,receipt TEXT,PRIMARY KEY(episode,hash,channel));")?;
    let mut statuses = Vec::new();
    for (i, id) in selected {
        let _ = progress.send(Event::Progress(format!(
            "Sending {} to Buffer for immediate publishing...",
            PLATFORMS[i]
        )));
        let key_fields = params![episode, asset.hash, id];
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM buffer_submissions WHERE episode=?1 AND hash=?2 AND channel=?3)", key_fields, |r| r.get(0))?;
        let staging = tempfile::tempdir()?;
        let url = if exists {
            String::new()
        } else {
            let delivery = if i == 0 && !draft.tiktok_cover_png.is_empty() {
                let _ = progress.send(Event::Progress(
                    "Preparing TikTok video with cover...".into(),
                ));
                let store = crate::Store::open(db.to_path_buf())?;
                cover_video(
                    &store.config()?.ffmpeg,
                    asset,
                    &draft.tiktok_cover_png,
                    staging.path(),
                )?
            } else {
                asset.clone()
            };
            match conn
                .query_row(
                    "SELECT url FROM buffer_uploads WHERE hash=?1",
                    [&delivery.hash],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            {
                Some(url) => url,
                None => {
                    let _ = progress.send(Event::Progress(format!(
                        "Uploading {} video to Cloudinary...",
                        PLATFORMS[i]
                    )));
                    let url = host(&delivery)?;
                    conn.execute(
                        "INSERT OR REPLACE INTO buffer_uploads VALUES(?1,?2)",
                        params![delivery.hash, url],
                    )?;
                    url
                }
            }
        };

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
                    "already submitted to Buffer - check its status there"
                } else {
                    "unconfirmed - check Buffer"
                }
            ));
            continue;
        }
        let input = post_input(draft, i, id, &url);
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
                statuses.push(format!(
                    "{}: Accepted by Buffer for immediate publishing",
                    PLATFORMS[i]
                ));
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
    let report = statuses.join("\n");
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("episode-publish-result:{episode}"),report])?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cover_is_tiktok_only_and_selects_an_opening_frame() {
        let draft = ReleaseDraft {
            tiktok_cover_png: vec![1],
            ..Default::default()
        };
        assert_eq!(
            post_input(&draft, 0, "channel", "url")["assets"][0]["video"]["metadata"]["thumbnailOffset"],
            100
        );
        for platform in [1, 2] {
            assert!(
                post_input(&draft, platform, "channel", "url")["assets"][0]["video"]
                    .get("metadata")
                    .is_none()
            );
        }
        let legacy: ReleaseDraft = serde_json::from_str(r#"{"cover_png":[1]}"#).unwrap();
        assert!(legacy.tiktok_cover_png.is_empty());
    }

    #[test]
    fn real_cover_video_preserves_episode_and_shifts_audio() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let ffmpeg = std::path::PathBuf::from(ffmpeg);
        for audio in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let source = temp.path().join("original.mp4");
            let mut cmd = Command::new(&ffmpeg);
            cmd.args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=120x180:r=30:d=1",
            ]);
            if audio {
                cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1",
                    "-c:a",
                    "aac",
                ]);
            }
            assert!(
                cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
                    .arg(&source)
                    .status()
                    .unwrap()
                    .success()
            );
            let hash = crate::worker::hash_file(&source).unwrap();
            let asset = Asset {
                id: "fixture".into(),
                job_id: "fixture".into(),
                path: source.clone(),
                hash: hash.clone(),
                width: 120,
                height: 180,
                duration: 1.,
                metadata: json!({}),
                review: "unreviewed".into(),
            };
            let picture = image::RgbImage::from_pixel(120, 180, image::Rgb([0, 0, 255]));
            let mut bytes = std::io::Cursor::new(Vec::new());
            picture
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            let result = cover_video(&ffmpeg, &asset, bytes.get_ref(), temp.path()).unwrap();
            assert_eq!(crate::worker::hash_file(&source).unwrap(), hash);
            for (time, channel) in [("0.1", 2usize), ("0.5", 0usize)] {
                let frame = Command::new(&ffmpeg)
                    .args(["-v", "error", "-ss", time, "-i"])
                    .arg(&result.path)
                    .args([
                        "-frames:v",
                        "1",
                        "-vf",
                        "scale=1:1",
                        "-pix_fmt",
                        "rgb24",
                        "-f",
                        "rawvideo",
                        "pipe:1",
                    ])
                    .output()
                    .unwrap();
                assert!(frame.status.success());
                assert!(frame.stdout[channel] > 200);
            }
            if audio {
                let sound = Command::new(&ffmpeg)
                    .args(["-v", "error", "-i"])
                    .arg(&result.path)
                    .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "f32le", "pipe:1"])
                    .output()
                    .unwrap();
                assert!(sound.status.success());
                let samples: Vec<f32> = sound
                    .stdout
                    .chunks_exact(4)
                    .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
                    .collect();
                assert!(samples[0..800].iter().all(|v| v.abs() < 0.005));
                assert!(samples[2400..4000].iter().any(|v| v.abs() > 0.05));
            }
        }
    }
    #[test]
    fn publishes_each_platform_now_with_its_own_text() {
        let draft = ReleaseDraft {
            title: "Episode title".into(),
            captions: [
                "TikTok caption".into(),
                "YouTube description".into(),
                "Instagram caption".into(),
            ],
            ..Default::default()
        };
        for i in 0..3 {
            let request = post_input(&draft, i, "channel", "https://example.com/video.mp4");
            assert_eq!(request["text"], draft.captions[i]);
            assert_eq!(request["mode"], "shareNow");
            assert!(request.get("dueAt").is_none());
            assert_eq!(request["schedulingType"], "automatic");
            assert_eq!(
                request["assets"][0]["video"]["url"],
                "https://example.com/video.mp4"
            );
        }
        assert_eq!(
            post_input(&draft, 1, "channel", "https://example.com/video.mp4")["metadata"]["youtube"]
                ["title"],
            "Episode title"
        );
    }
}
