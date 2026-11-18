//! fal WAN 3 queue transport. One submission per durable job; retries resume polling.
use crate::{
    store::Store,
    video_plan::{Inputs, VideoPlan},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

impl Store {
    pub(crate) fn wan_uses_fal(&self) -> bool {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key='video:provider'",
                [],
                |r| r.get::<_, String>(0),
            )
            .is_ok_and(|v| v == "fal")
    }
    pub(crate) fn set_wan_provider(&self, fal: bool) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key,value) VALUES('video:provider',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [if fal {"fal"} else {"openrouter"}])?;
        Ok(())
    }
    pub(crate) fn route_video_plan(&self, mut plan: VideoPlan) -> VideoPlan {
        plan.fal = plan.model == "alibaba/wan-3.0" && self.wan_uses_fal();
        plan
    }
}
// Documented fal WAN capabilities, independent of the OpenRouter catalog.
pub(crate) fn catalog() -> crate::openrouter::Catalog {
    crate::openrouter::Catalog {
        videos: vec![json!({"id":"alibaba/wan-3.0", "name":"Wan 3.0 · fal.ai",
        "supported_durations":(2..=30).collect::<Vec<_>>(), "supported_resolutions":["480p","720p","1080p"],
        "supported_aspect_ratios":["16:9","9:16","1:1","4:3","3:4"],
        "supported_frame_images":["first_frame","last_frame"], "generate_audio":true})],
        images: vec![],
        fetched_unix_seconds: 0,
    }
}
fn source_request(plan: &VideoPlan) -> Result<Value> {
    ensure!(
        plan.model == "alibaba/wan-3.0",
        "fal routing supports WAN 3.0 only"
    );
    let mut legacy = plan.clone();
    legacy.fal = false;
    Ok(legacy.preview(&catalog())?["request"].clone())
}
fn translate(plan: &VideoPlan, source: &Value) -> (&'static str, Value) {
    let mut payload = json!({"prompt":source["prompt"],"resolution":plan.resolution,"aspect_ratio":plan.aspect_ratio,
        "duration": if plan.automatic_duration { Value::Null } else {json!(plan.duration)},
        "audio":plan.generate_audio, "enable_prompt_expansion":false, "enable_safety_checker":false});
    let endpoint = match &plan.inputs {
        Inputs::Prompt => "text-to-video",
        Inputs::Frames { .. } => {
            payload["start_image_url"] = source["frame_images"][0]["image_url"]["url"].clone();
            if let Some(last) = source["frame_images"].get(1) {
                payload["end_image_url"] = last["image_url"]["url"].clone();
            }
            "image-to-video"
        }
        Inputs::Portraits { .. } | Inputs::References { .. } => {
            payload["reference_image_urls"] = source["input_references"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|v| v["image_url"]["url"].clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
                .into();
            "reference-to-video"
        }
    };
    (endpoint, payload)
}
pub(crate) fn preview(plan: &VideoPlan) -> Result<Value> {
    let source = source_request(plan)?;
    let (endpoint, request) = translate(plan, &source);
    Ok(
        json!({"provider":"fal", "endpoint":format!("alibaba/wan-3.0/{endpoint}"), "request":request,
        "submitted":false,"notes":["fal WAN 3.0. Prompt rewriting is off to preserve authored direction. Provider moderation remains enabled."]}),
    )
}
fn payload(plan: &VideoPlan, directory: &Path) -> Result<(&'static str, Value)> {
    let mut source = source_request(plan)?;
    crate::portrait_delivery::embed(plan, &mut source, directory)?;
    Ok(translate(plan, &source))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn media_url(raw: &str) -> Result<url::Url> {
    let url = url::Url::parse(raw).context("fal returned an invalid media URL")?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url
                .host_str()
                .is_some_and(|h| h == "fal.media" || h.ends_with(".fal.media")),
        "fal output is not on its expected media host; saved request retained for inspection"
    );
    Ok(url)
}
fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}
fn read_response(mut response: ureq::http::Response<ureq::Body>) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "fal HTTP {}. Check your fal dashboard for this request; no automatic resubmission.",
        response.status().as_u16()
    );
    response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_json()
        .map_err(|_| {
            anyhow::anyhow!("fal returned an unreadable response; no automatic resubmission")
        })
}
fn canceled(store: &Store, id: &str) -> Result<bool> {
    Ok(store
        .conn
        .query_row("SELECT cancel_requested FROM jobs WHERE id=?1", [id], |r| {
            r.get(0)
        })?)
}
fn check_cancel(
    store: &Store,
    id: &str,
    agent: &ureq::Agent,
    remote_url: Option<&str>,
    authorization: &str,
) -> Result<()> {
    if !canceled(store, id)? {
        return Ok(());
    }
    if let Some(url) = remote_url {
        let response = agent
            .put(&format!("{url}/cancel"))
            .header("Authorization", authorization)
            .send_empty();
        let status = response
            .ok()
            .and_then(|r| read_response(r).ok())
            .and_then(|v| v["status"].as_str().map(str::to_owned));
        let note = match status.as_deref() {
            Some("OK") => "fal removed the queued request. Billing is determined by fal.",
            Some("CANCELLATION_REQUESTED") => {
                "Cancellation sent to fal; a running generation may still finish and incur charges."
            }
            Some("ALREADY_COMPLETED") => {
                "fal already completed this request. Local monitoring stopped; charges may apply."
            }
            _ => {
                "Local monitoring stopped; remote cancellation could not be confirmed. Check fal before generating again."
            }
        };
        bail!("{note}");
    }
    bail!("Canceled before fal submission")
}
pub(crate) fn execute(store: &Store, id: &str, plan: &VideoPlan) -> Result<()> {
    let key = crate::credentials::Provider::Fal
        .read()?
        .context("Save your fal API key in Settings → Connections")?;
    execute_transport(
        store,
        id,
        plan,
        "https://queue.fal.run",
        &key,
        Duration::from_secs(1),
    )
}
fn execute_transport(
    store: &Store,
    id: &str,
    plan: &VideoPlan,
    base: &str,
    key: &str,
    tick: Duration,
) -> Result<()> {
    let agent = client();
    let authorization = format!("Key {key}");
    let directory = store.root().join("jobs").join(id).join("remote");
    std::fs::create_dir_all(&directory)?;
    let remote: Option<String> = store.conn.query_row(
        "SELECT remote_id FROM remote_video_jobs WHERE job_id=?1",
        [id],
        |r| r.get(0),
    )?;
    let remote = match remote {
        Some(remote) => {
            ensure!(valid_id(&remote), "Invalid saved fal request ID");
            remote
        }
        None => {
            check_cancel(store, id, &agent, None, &authorization)?;
            let (endpoint, request) = payload(plan, &directory)?;
            // Retain only the preview, never the embedded image bytes or credentials.
            std::fs::write(
                directory.join("request.json"),
                serde_json::to_vec_pretty(&preview(plan)?)?,
            )?;
            check_cancel(store, id, &agent, None, &authorization)?;
            store.begin_video_submission(id)?;
            store.conn.execute(
                "UPDATE jobs SET phase='Submitting to fal' WHERE id=?1",
                [id],
            )?;
            let response = agent.post(&format!("{base}/alibaba/wan-3.0/{endpoint}"))
                .header("Authorization",&authorization)
                .header("X-Fal-No-Retry","1")
                .send_json(request).map_err(|_|anyhow::anyhow!("fal submission could not be confirmed. Check your fal dashboard; this job will not submit again automatically."))?;
            let response = read_response(response)?;
            let remote = response["request_id"]
                .as_str()
                .context("fal returned no request ID; submission may have been billed")?;
            store.bind_video_remote(id, remote)?;
            remote.to_owned()
        }
    };
    let remote_url = format!("{base}/alibaba/wan-3.0/requests/{remote}");
    let start = Instant::now();
    loop {
        check_cancel(store, id, &agent, Some(&remote_url), &authorization)?;
        let response = agent.get(&format!("{remote_url}/status")).header("Authorization",&authorization).call()
            .map_err(|_|anyhow::anyhow!("Could not poll fal. Retry this job to resume monitoring without a new generation."))?;
        let result = read_response(response)?;
        match result["status"].as_str() {
            Some("COMPLETED") => {
                if result.get("error").is_some_and(|e| !e.is_null()) {
                    store.conn.execute(
                        "UPDATE remote_video_jobs SET state='terminal_failed' WHERE job_id=?1",
                        [id],
                    )?;
                    bail!(
                        "fal completed with a provider error. Check this request in your fal dashboard; no automatic resubmission."
                    );
                }
                break;
            }
            Some("IN_QUEUE" | "IN_PROGRESS") => {
                store.conn.execute(
                    "UPDATE jobs SET phase=?2 WHERE id=?1",
                    rusqlite::params![
                        id,
                        if result["status"] == "IN_QUEUE" {
                            "Queued at fal"
                        } else {
                            "Generating at fal"
                        }
                    ],
                )?;
            }
            Some("FAILED" | "CANCELED" | "CANCELLED") => {
                store.conn.execute(
                    "UPDATE remote_video_jobs SET state='terminal_failed' WHERE job_id=?1",
                    [id],
                )?;
                bail!(
                    "fal generation failed or was canceled. Check its request in your fal dashboard."
                );
            }
            _ => bail!("fal returned an unknown state. Saved request retained; no new submission."),
        }
        ensure!(
            start.elapsed() < Duration::from_secs(1800),
            "fal monitoring timed out. Retry to resume the same request."
        );
        for _ in 0..5 {
            std::thread::sleep(tick);
            check_cancel(store, id, &agent, Some(&remote_url), &authorization)?;
        }
    }
    check_cancel(store, id, &agent, Some(&remote_url), &authorization)?;
    let response = agent
        .get(&remote_url)
        .header("Authorization", &authorization)
        .call()
        .map_err(|_| anyhow::anyhow!("fal result unavailable. Retry monitoring the saved job."))?;
    if matches!(response.status().as_u16(), 400 | 422) {
        store.conn.execute(
            "UPDATE remote_video_jobs SET state='terminal_failed' WHERE job_id=?1",
            [id],
        )?;
    }
    let result = read_response(response)?;
    let raw_url = result["video"]["url"]
        .as_str()
        .context("fal completed without a video URL; saved request retained")?;
    // Production downloads only fal media, without the API Authorization header.
    let download_url = if cfg!(test) && raw_url.starts_with(&format!("{base}/fixture/")) {
        raw_url.to_owned()
    } else {
        media_url(raw_url)?.to_string()
    };
    let completed = json!({"id":remote,"status":"completed","provider":"fal"});
    std::fs::write(
        directory.join("poll-completed.json"),
        serde_json::to_vec(&completed)?,
    )?;
    store.conn.execute(
        "UPDATE jobs SET phase='Downloading fal video' WHERE id=?1",
        [id],
    )?;
    let mut response = agent
        .get(&download_url)
        .call()
        .map_err(|_| anyhow::anyhow!("fal video download failed; retry the same job"))?;
    ensure!(
        response.status().is_success(),
        "fal video download failed; retry the same job"
    );
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024 * 1024)
        .reader();
    let mut buffer = [0u8; 65536];
    use std::io::{Read, Write};
    loop {
        check_cancel(store, id, &agent, Some(&remote_url), &authorization)?;
        let n = reader
            .read(&mut buffer)
            .map_err(|_| anyhow::anyhow!("fal download interrupted; retry the saved job"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
    }
    file.as_file().sync_all()?;
    let output = directory.join(format!("take-{}.mp4", uuid::Uuid::new_v4()));
    file.persist(&output)
        .map_err(|_| anyhow::anyhow!("Could not retain downloaded fal video"))?;
    crate::hosted_video::finish_download(store, id, plan, &output, &completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> VideoPlan {
        VideoPlan {
            fal: true,
            model: "alibaba/wan-3.0".into(),
            prompt: "Two adults talk in a shop.".into(),
            duration: 2,
            automatic_duration: false,
            resolution: "720p".into(),
            aspect_ratio: "16:9".into(),
            generate_audio: true,
            no_background_music: true,
            ending_hold: false,
            inputs: Inputs::Prompt,
        }
    }
    #[test]
    fn maps_each_wan_mode_and_legacy_requests_remain_openrouter() {
        let mut p = plan();
        let v = preview(&p).unwrap();
        assert_eq!(v["endpoint"], "alibaba/wan-3.0/text-to-video");
        assert_eq!(v["request"]["audio"], true);
        assert_eq!(v["request"]["enable_prompt_expansion"], false);
        assert_eq!(v["request"]["enable_safety_checker"], false);
        assert_eq!(v["request"]["aspect_ratio"], "16:9");
        p.automatic_duration = true;
        assert!(preview(&p).unwrap()["request"]["duration"].is_null());
        p.inputs = Inputs::Frames {
            first: "https://example.com/first.png".into(),
            last: Some("https://example.com/last.png".into()),
        };
        let v = preview(&p).unwrap();
        assert_eq!(v["endpoint"], "alibaba/wan-3.0/image-to-video");
        assert_eq!(
            v["request"]["end_image_url"],
            "https://example.com/last.png"
        );
        p.inputs = Inputs::References {
            images: vec![
                "https://example.com/one.png".into(),
                "https://example.com/two.png".into(),
            ],
        };
        let v = preview(&p).unwrap();
        assert_eq!(
            v["request"]["reference_image_urls"][1],
            "https://example.com/two.png"
        );
        p.inputs = Inputs::References {
            images: vec!["https://example.com/x.png".into(); 11],
        };
        assert!(preview(&p).is_err());
        p = plan();
        p.duration = 31;
        assert!(preview(&p).is_err());
        p = plan();
        p.fal = false;
        let serialized = serde_json::to_string(&p).unwrap();
        assert!(!serialized.contains("\"fal\""));
        assert!(!serde_json::from_str::<VideoPlan>(&serialized).unwrap().fal);
        assert!(media_url("https://v3b.fal.media/files/video.mp4?token=fixture").is_ok());
        for url in [
            "http://v3.fal.media/a",
            "https://fal.media.evil.test/a",
            "https://user:pass@fal.media/a",
            "https://127.0.0.1/a",
        ] {
            assert!(media_url(url).is_err());
        }
    }
    #[test]
    fn portraits_keep_order_and_reject_changed_bytes_before_submission() {
        use base64::Engine;
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("portrait.png");
        image::RgbImage::from_pixel(32, 48, image::Rgb([90, 70, 50]))
            .save(&image)
            .unwrap();
        let bytes = std::fs::read(&image).unwrap();
        let reference = crate::cast_reference::CastReference {
            character_id: "lead".into(),
            name: "Lead".into(),
            brief: String::new(),
            path: image.clone(),
            sha256: crate::worker::hash_file(&image).unwrap(),
        };
        let mut p = plan();
        p.inputs = Inputs::Portraits {
            portraits: vec![reference],
            images: vec!["https://example.com/location.png".into()],
        };
        let (endpoint, request) = payload(&p, temp.path()).unwrap();
        assert_eq!(endpoint, "reference-to-video");
        let encoded = request["reference_image_urls"][0]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            bytes
        );
        assert_eq!(
            request["reference_image_urls"][1],
            "https://example.com/location.png"
        );
        assert!(
            request["prompt"]
                .as_str()
                .unwrap()
                .contains("Image 1: Lead")
        );
        std::fs::write(image, b"changed").unwrap();
        assert!(payload(&p, temp.path()).is_err());
    }

    #[test]
    fn provider_choice_is_frozen_and_switching_does_not_deduplicate_other_route() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
        let series = store.create_series("Series", "").unwrap();
        let ep = store.create_episode(&series.id, "Pilot").unwrap();
        let mut draft = store.generation_plan(&ep.id).unwrap();
        draft.segments[0].prompt = "Scene".into();
        let saved = store.save_generation_plan(&draft).unwrap();
        let first = store
            .enqueue_episode_segment(&ep.id, saved.revision, &saved.segments[0])
            .unwrap();
        store.set_wan_provider(true).unwrap();
        let second = store
            .enqueue_episode_segment(&ep.id, saved.revision, &saved.segments[0])
            .unwrap();
        assert_ne!(first.id, second.id);
        let third = store
            .enqueue_episode_segment(&ep.id, saved.revision, &saved.segments[0])
            .unwrap();
        assert_eq!(second.id, third.id);
        for (id, expected) in [(&first.id, false), (&second.id, true)] {
            let request: String = store
                .conn
                .query_row("SELECT request FROM jobs WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            assert_eq!(
                serde_json::from_str::<VideoPlan>(&request).unwrap().fal,
                expected
            );
        }
        store.set_wan_provider(false).unwrap();
        let mut p = plan();
        p.model = "bytedance/seedance-2.5".into();
        store.set_wan_provider(true).unwrap();
        assert!(!store.route_video_plan(p).fal);
    }
    // Real HTTP framing and authorization, but only disposable loopback requests.
    fn mock(steps: Vec<(&'static str, u16, Vec<u8>)>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let origin = base.clone();
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || {
            for (expected, code, body) in steps {
                let deadline = Instant::now() + Duration::from_secs(20);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "Mock timed out");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let len = headers
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if headers.contains("transfer-encoding: chunked") {
                            if bytes[end + 4..].ends_with(b"0\r\n\r\n") {
                                break;
                            }
                        } else if bytes.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(
                    request.starts_with(expected),
                    "Unexpected mock request path"
                );
                let lower = request.to_lowercase();
                if expected.contains("/fixture/") {
                    assert!(!lower.contains("authorization:"));
                } else {
                    assert!(lower.contains("authorization: key fixture-only"));
                }
                if expected.starts_with("POST") {
                    assert!(lower.contains("x-fal-no-retry: 1"));
                    let raw = request.split_once("\r\n\r\n").unwrap().1;
                    let decoded = if lower.contains("transfer-encoding: chunked") {
                        let mut remaining = raw;
                        let mut result = String::new();
                        loop {
                            let (size, rest) = remaining.split_once("\r\n").unwrap();
                            let n = usize::from_str_radix(size, 16).unwrap();
                            if n == 0 {
                                break;
                            }
                            result.push_str(&rest[..n]);
                            remaining = &rest[n + 2..];
                        }
                        result
                    } else {
                        raw.to_owned()
                    };
                    let payload: Value = serde_json::from_str(&decoded).unwrap();
                    assert_eq!(payload["audio"], true);
                }
                if code == 0 {
                    continue;
                } // Simulate lost submission response.
                let body = if body == b"RESULT" {
                    json!({"video":{"url":format!("{origin}/fixture/video.mp4")}})
                        .to_string()
                        .into_bytes()
                } else {
                    body
                };
                write!(
                    stream,
                    "HTTP/1.1 {code} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        (base, handle)
    }
    #[test]
    fn uncertain_submission_is_not_reposted_and_remote_cancel_is_sent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
        store.set_wan_provider(true).unwrap();
        let p = plan();
        let job = store.enqueue_seedance(&p).unwrap();
        let (base, h) = mock(vec![("POST /alibaba/wan-3.0/text-to-video ", 0, vec![])]);
        assert!(
            execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO).is_err()
        );
        h.join().unwrap();
        assert!(!store.video_retry_available(&job.id).unwrap());
        assert!(
            execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO).is_err()
        );
        let job = store.enqueue_seedance(&p).unwrap();
        store.begin_video_submission(&job.id).unwrap();
        store.bind_video_remote(&job.id, "fixture-id").unwrap();
        store
            .conn
            .execute("UPDATE jobs SET cancel_requested=1 WHERE id=?1", [&job.id])
            .unwrap();
        let (base, h) = mock(vec![(
            "PUT /alibaba/wan-3.0/requests/fixture-id/cancel ",
            202,
            br#"{"status":"CANCELLATION_REQUESTED"}"#.to_vec(),
        )]);
        let error = execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO)
            .unwrap_err();
        assert!(error.to_string().contains("Cancellation sent"));
        h.join().unwrap();
    }
    #[test]
    fn provider_failure_never_becomes_a_download_or_a_new_submission() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
        store.set_wan_provider(true).unwrap();
        let p = plan();
        let job = store.enqueue_seedance(&p).unwrap();
        store.begin_video_submission(&job.id).unwrap();
        store.bind_video_remote(&job.id, "fixture-id").unwrap();
        let (base, h) = mock(vec![(
            "GET /alibaba/wan-3.0/requests/fixture-id/status ",
            200,
            br#"{"status":"COMPLETED","error":"fixture provider rejection"}"#.to_vec(),
        )]);
        assert!(
            execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO).is_err()
        );
        h.join().unwrap();
        assert!(!store.video_retry_available(&job.id).unwrap());
        assert!(store.assets().unwrap().is_empty());
    }

    #[test]
    fn actual_video_download_and_resume_never_repost_or_send_key_to_media() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
        store.set_wan_provider(true).unwrap();
        let mut config = store.config().unwrap();
        config.ffmpeg = ffmpeg.into();
        store.save_config(&config).unwrap();
        let file = tmp.path().join("fixture.mp4");
        assert!(
            std::process::Command::new(&config.ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:s=1280x720:r=24",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
                    "-t",
                    "2",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "aac"
                ])
                .arg(&file)
                .status()
                .unwrap()
                .success()
        );
        let video = std::fs::read(file).unwrap();
        let p = plan();
        for resume in [false, true] {
            let job = store.enqueue_seedance(&p).unwrap();
            store
                .conn
                .execute(
                    "UPDATE jobs SET status='running',attempts=1 WHERE id=?1",
                    [&job.id],
                )
                .unwrap();
            let mut steps = vec![];
            if resume {
                store.begin_video_submission(&job.id).unwrap();
                store.bind_video_remote(&job.id, "fixture-id").unwrap();
            } else {
                steps.push((
                    "POST /alibaba/wan-3.0/text-to-video ",
                    200,
                    br#"{"request_id":"fixture-id","status":"IN_QUEUE"}"#.to_vec(),
                ));
            }
            steps.extend([
                (
                    "GET /alibaba/wan-3.0/requests/fixture-id/status ",
                    200,
                    br#"{"status":"COMPLETED"}"#.to_vec(),
                ),
                (
                    "GET /alibaba/wan-3.0/requests/fixture-id ",
                    200,
                    b"RESULT".to_vec(),
                ),
                ("GET /fixture/video.mp4 ", 200, video.clone()),
            ]);
            let (base, h) = mock(steps);
            execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO).unwrap();
            h.join().unwrap();
            let saved = store.job(&job.id).unwrap();
            assert_eq!(saved.status, "completed");
            let asset = store
                .assets()
                .unwrap()
                .into_iter()
                .find(|a| Some(&a.id) == saved.output_id.as_ref())
                .unwrap();
            assert_eq!(std::fs::read(&asset.path).unwrap(), video);
            assert_eq!(asset.review, "unreviewed");
        }
    }
}
