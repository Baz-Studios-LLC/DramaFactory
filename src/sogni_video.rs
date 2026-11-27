//! Sogni durable workflows. Saved model IDs select the provider without mutable routing.
//! Contracts: docs.sogni.ai/api-reference/{workflows,direct-generation}/ and models/wan-3/.
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

pub(crate) fn is_model(model: &str) -> bool {
    matches!(model, "sogni/wan3.0-video" | "sogni/wan3.0-spicy-video")
}
pub(crate) fn append_catalog(catalog: &mut crate::openrouter::Catalog) {
    for choice in crate::video_plan::MODEL_CHOICES
        .iter()
        .filter(|c| is_model(c.id))
    {
        if !catalog.videos.iter().any(|v| v["id"] == choice.id) {
            let mut model = crate::fal_video::catalog().videos.remove(0);
            model["id"] = json!(choice.id);
            model["name"] = json!(choice.name);
            catalog.videos.push(model);
        }
    }
}
fn source(plan: &VideoPlan) -> Result<Value> {
    ensure!(is_model(&plan.model), "Choose a supported Sogni model");
    ensure!(
        !plan.automatic_duration,
        "Sogni requires an explicit duration"
    );
    let mut validated = plan.clone();
    validated.model = "alibaba/wan-3.0".into();
    validated.fal = false;
    Ok(validated.preview(&crate::fal_video::catalog())?["request"].clone())
}
fn translate(plan: &VideoPlan, source: &Value) -> Value {
    let mut args = json!({"prompt": source["prompt"], "videoModel":plan.model.trim_start_matches("sogni/"),
        "duration":plan.duration, "ratio":plan.aspect_ratio,
        "targetResolution":plan.resolution.trim_end_matches('p').parse::<u32>().unwrap_or(0),
        "generateAudio":plan.generate_audio, "skipPromptProcessing":true, "expandPrompt":false});
    let mut media = Vec::new();
    let tool = match &plan.inputs {
        Inputs::Prompt => "generate_video",
        Inputs::Frames { last, .. } => {
            media
                .push(json!({"kind":"image", "url":source["frame_images"][0]["image_url"]["url"]}));
            args["sourceImageIndex"] = json!(-1);
            args["frameRole"] = json!(if last.is_some() { "both" } else { "start" });
            if last.is_some() {
                media.push(
                    json!({"kind":"image", "url":source["frame_images"][1]["image_url"]["url"]}),
                );
                args["endImageIndex"] = json!(-2);
            }
            "animate_photo"
        }
        Inputs::Portraits { .. } | Inputs::References { .. } => {
            for reference in source["input_references"].as_array().into_iter().flatten() {
                media.push(json!({"kind":"image", "url":reference["image_url"]["url"]}));
            }
            args["referenceImageIndices"] =
                json!((1..=media.len()).map(|n| -(n as i32)).collect::<Vec<_>>());
            "generate_video"
        }
    };
    json!({"input":{"title":"DramaFactory episode segment","steps":[{"id":"clip","toolName":tool,"arguments":args}]},
        "media_references":media,"app_source":"DramaFactory","safe_content_filter":!plan.model.ends_with("spicy-video"),
        "billing_mode":"auto","token_type":"spark"})
}
pub(crate) fn preview(plan: &VideoPlan) -> Result<Value> {
    Ok(
        json!({"provider":"sogni","request":translate(plan, &source(plan)?),"submitted":false,
        "notes":["Explicit model selection; no automatic provider fallback.","Local portraits upload to Cloudinary at submission. Starting frames stay separate from loose references."]}),
    )
}
fn payload(plan: &VideoPlan, directory: &Path) -> Result<Value> {
    let mut input = source(plan)?;
    // Reuse byte/hash validation and retained request-local portrait copies.
    crate::portrait_delivery::embed(plan, &mut input, directory)?;
    if let Inputs::Portraits { portraits, .. } = &plan.inputs {
        for (index, _) in portraits.iter().enumerate() {
            use base64::Engine;
            let data = input["input_references"][index]["image_url"]["url"]
                .as_str()
                .context("Missing portrait")?;
            let (_, encoded) = data.split_once(',').context("Invalid embedded portrait")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
            let path = directory.join(format!("sogni-portrait-{index}.input"));
            std::fs::write(&path, bytes)?;
            let url = crate::frame_handoff::upload_frame(&path)?;
            input["input_references"][index]["image_url"]["url"] = json!(url);
        }
    }
    Ok(translate(plan, &input))
}
fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}
fn read(mut response: ureq::http::Response<ureq::Body>) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "Sogni HTTP {}. Check your key, balance and workflow dashboard. No automatic resubmission.",
        response.status().as_u16()
    );
    response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_json()
        .map_err(|_| anyhow::anyhow!("Sogni returned an unreadable response; request retained"))
}
fn waiting_message(workflow: &Value) -> String {
    let reason = if workflow["awaitingCostApproval"].as_bool() == Some(true) {
        "cost approval required"
    } else {
        match workflow["waitingReason"].as_str() {
            Some("permission_required") => "permission required",
            Some("cost_approval_required") => "cost approval required",
            Some("insufficient_credit") => "insufficient credit",
            Some("safety_review_required") => "content review required",
            _ => "provider review required (Sogni did not give a specific reason)",
        }
    };
    format!(
        "Sogni paused: {reason}. Open this workflow in Sogni and resolve it, then Retry here to retrieve the same workflow. No replacement video will be generated."
    )
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn canceled(
    store: &Store,
    id: &str,
    agent: &ureq::Agent,
    remote: Option<&str>,
    auth: &str,
) -> Result<()> {
    let cancel: bool =
        store
            .conn
            .query_row("SELECT cancel_requested FROM jobs WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
    if !cancel {
        return Ok(());
    }
    if let Some(remote) = remote {
        let result = agent
            .post(format!("{remote}/cancel"))
            .header("Authorization", auth)
            .send_empty();
        ensure!(
            result.is_ok_and(|r| r.status().is_success()),
            "Local cancellation requested; Sogni cancellation could not be confirmed. Check the workflow dashboard."
        );
        bail!("Sogni cancellation requested; in-flight provider work may still be billed");
    }
    bail!("Canceled before Sogni submission")
}
fn media_url(raw: &str) -> Result<url::Url> {
    let url = url::Url::parse(raw).context("Invalid Sogni artifact URL")?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.domain().is_some_and(|h| h.contains('.')
                && !h.ends_with(".localhost")
                && !h.ends_with(".local")),
        "Sogni artifact must use a public HTTPS storage URL"
    );
    Ok(url)
}
pub(crate) fn execute(store: &Store, id: &str, plan: &VideoPlan) -> Result<()> {
    let key = crate::credentials::Provider::Sogni
        .read()?
        .context("Save a Sogni key in Settings → Connections")?;
    execute_transport(
        store,
        id,
        plan,
        "https://api.sogni.ai",
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
    let auth = format!("Bearer {key}");
    let directory = store.root().join("jobs").join(id).join("remote");
    std::fs::create_dir_all(&directory)?;
    let remote: Option<String> = store.conn.query_row(
        "SELECT remote_id FROM remote_video_jobs WHERE job_id=?1",
        [id],
        |r| r.get(0),
    )?;
    let remote = match remote {
        Some(remote) => {
            ensure!(valid_id(&remote), "Invalid stored Sogni workflow ID");
            remote
        }
        None => {
            canceled(store, id, &agent, None, &auth)?;
            let request = payload(plan, &directory)?;
            std::fs::write(
                directory.join("request.json"),
                serde_json::to_vec_pretty(&preview(plan)?)?,
            )?;
            canceled(store, id, &agent, None, &auth)?;
            store.begin_video_submission(id)?;
            store.conn.execute(
                "UPDATE jobs SET phase='Submitting to Sogni' WHERE id=?1",
                [id],
            )?;
            let response = agent.post(format!("{base}/v1/creative-agent/workflows"))
                .header("Authorization",&auth).header("Idempotency-Key",id).send_json(request)
                .map_err(|_|anyhow::anyhow!("Sogni submission unconfirmed. Check the dashboard; this job will not automatically resubmit."))?;
            let result = read(response)?;
            let remote = result["data"]["workflow"]["workflowId"]
                .as_str()
                .context("Sogni returned no workflow ID; submission may have been billed")?;
            store.bind_video_remote(id, remote)?;
            remote.to_owned()
        }
    };
    let remote_url = format!("{base}/v1/creative-agent/workflows/{remote}");
    let started = Instant::now();
    let workflow = loop {
        canceled(store, id, &agent, Some(&remote_url), &auth)?;
        // Polling is read-only. A transient connection failure must not abandon
        // a paid workflow or cause a second submission.
        let mut poll_attempt = 0;
        let result = loop {
            canceled(store, id, &agent, Some(&remote_url), &auth)?;
            let response = client()
                .get(&remote_url)
                .header("Authorization", &auth)
                .call();
            let retryable = match &response {
                Err(_) => true,
                Ok(r) => r.status().as_u16() == 429 || r.status().is_server_error(),
            };
            if retryable && poll_attempt < 5 {
                poll_attempt += 1;
                store.conn.execute(
                    "UPDATE jobs SET phase='Reconnecting to Sogni - existing workflow retained' WHERE id=?1",
                    [id],
                )?;
                for _ in 0..(poll_attempt * 2) {
                    std::thread::sleep(tick);
                    canceled(store, id, &agent, Some(&remote_url), &auth)?;
                }
                continue;
            }
            break read(response.map_err(|error| anyhow::anyhow!(
                "Could not retrieve Sogni status after reconnecting: {error}. The video may still be complete on Sogni. Retry retrieves the same workflow without generating again."
            ))?)?;
        };
        let workflow = &result["data"]["workflow"];
        let status = workflow["status"]
            .as_str()
            .context("Sogni returned no workflow status")?;
        match status {
            "completed" => break workflow.clone(),
            "failed" | "partial_failure" | "cancelled" => {
                store.conn.execute(
                    "UPDATE remote_video_jobs SET state='terminal_failed' WHERE job_id=?1",
                    [id],
                )?;
                bail!(
                    "Sogni workflow {status}. Inspect it in the Sogni dashboard; no automatic replacement."
                );
            }
            "waiting_for_user" => bail!("{}", waiting_message(workflow)),
            "queued" | "running" => {
                store.conn.execute(
                    "UPDATE jobs SET phase=?2 WHERE id=?1",
                    rusqlite::params![id, format!("Sogni: {status}")],
                )?;
            }
            _ => bail!("Unknown Sogni workflow state; saved workflow retained for retry"),
        }
        ensure!(
            started.elapsed() < Duration::from_secs(3600),
            "Sogni monitoring timed out. Retry this job to monitor the same workflow."
        );
        for _ in 0..10 {
            std::thread::sleep(tick);
            canceled(store, id, &agent, Some(&remote_url), &auth)?;
        }
    };
    let raw = workflow["steps"][0]["artifacts"][0]["url"]
        .as_str()
        .context("Sogni completed without a video artifact URL")?;
    let url = if cfg!(test) && raw.starts_with(&format!("{base}/fixture/")) {
        raw.to_owned()
    } else {
        media_url(raw)?.to_string()
    };
    let completed = json!({"id":remote,"provider":"sogni","status":"completed"});
    std::fs::write(
        directory.join("poll-completed.json"),
        serde_json::to_vec(&completed)?,
    )?;
    canceled(store, id, &agent, Some(&remote_url), &auth)?;
    store.conn.execute(
        "UPDATE jobs SET phase='Downloading Sogni video' WHERE id=?1",
        [id],
    )?;
    // Storage URLs are opaque and short-lived. No API credential goes to storage.
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|_| anyhow::anyhow!("Sogni download failed; retry the saved workflow"))?;
    ensure!(
        response.status().is_success(),
        "Sogni download failed; retry the saved workflow"
    );
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    use std::io::{Read, Write};
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024 * 1024)
        .reader();
    let mut buffer = [0u8; 65536];
    loop {
        canceled(store, id, &agent, Some(&remote_url), &auth)?;
        let n = reader
            .read(&mut buffer)
            .map_err(|_| anyhow::anyhow!("Sogni download interrupted; retry the saved workflow"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
    }
    file.as_file().sync_all()?;
    let output = directory.join(format!("take-{}.mp4", uuid::Uuid::new_v4()));
    file.persist(&output)
        .map_err(|_| anyhow::anyhow!("Could not retain Sogni download"))?;
    crate::hosted_video::finish_download(store, id, plan, &output, &completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> VideoPlan {
        VideoPlan {
            fal: false,
            model: "sogni/wan3.0-spicy-video".into(),
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
    fn waiting_reasons_are_specific_without_guessing() {
        for (reason, expected) in [
            ("permission_required", "permission required"),
            ("insufficient_credit", "insufficient credit"),
            ("safety_review_required", "content review required"),
            ("cost_approval_required", "cost approval required"),
        ] {
            assert!(waiting_message(&json!({"waitingReason":reason})).contains(expected));
        }
        assert!(waiting_message(&json!({})).contains("did not give a specific reason"));
        assert!(
            waiting_message(&json!({"awaitingCostApproval":true}))
                .contains("cost approval required")
        );
    }
    #[test]
    fn maps_models_references_and_frame_anchors_without_dropping_inputs() {
        let mut p = plan();
        let request = preview(&p).unwrap()["request"].clone();
        assert_eq!(request["safe_content_filter"], false);
        assert_eq!(
            request["input"]["steps"][0]["arguments"]["videoModel"],
            "wan3.0-spicy-video"
        );
        p.model = "sogni/wan3.0-video".into();
        assert_eq!(preview(&p).unwrap()["request"]["safe_content_filter"], true);
        p.inputs = Inputs::References {
            images: vec![
                "https://example.com/a.png".into(),
                "https://example.com/b.png".into(),
            ],
        };
        let request = preview(&p).unwrap()["request"].clone();
        assert_eq!(request["media_references"].as_array().unwrap().len(), 2);
        assert_eq!(
            request["input"]["steps"][0]["arguments"]["referenceImageIndices"],
            json!([-1, -2])
        );
        p.inputs = Inputs::Frames {
            first: "https://example.com/a.png".into(),
            last: Some("https://example.com/b.png".into()),
        };
        let request = preview(&p).unwrap()["request"].clone();
        assert_eq!(request["input"]["steps"][0]["toolName"], "animate_photo");
        assert_eq!(
            request["input"]["steps"][0]["arguments"]["endImageIndex"],
            -2
        );
        assert_eq!(
            request["input"]["steps"][0]["arguments"]["frameRole"],
            "both"
        );
        p.duration = 31;
        assert!(preview(&p).is_err());
        for invalid in [
            "http://example.com/a",
            "https://127.0.0.1/a",
            "https://user:secret@example.com/a",
            "https://example.local/a",
        ] {
            assert!(media_url(invalid).is_err());
        }
    }
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
                    assert!(lower.contains("authorization: bearer fixture-only"));
                }
                if expected == "POST /v1/creative-agent/workflows " {
                    assert!(lower.contains("idempotency-key:"));
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
                    assert_eq!(
                        payload["input"]["steps"][0]["arguments"]["generateAudio"],
                        true
                    );
                }
                if code == 0 {
                    continue;
                } // Simulate lost submission response.
                let body = if body == b"RESULT" {
                    json!({"data":{"workflow":{"workflowId":"fixture-id","status":"completed","steps":[{"artifacts":[{"url":format!("{origin}/fixture/video.mp4")}]}]}}})
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
    fn actual_video_download_and_resume_never_repost_or_send_key_to_media() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
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
                    "POST /v1/creative-agent/workflows ",
                    200,
                    br#"{"data":{"workflow":{"workflowId":"fixture-id","status":"queued"}}}"#
                        .to_vec(),
                ));
            }
            steps.extend([
                (
                    "GET /v1/creative-agent/workflows/fixture-id ",
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
    #[test]
    fn uncertain_submission_and_cancellation_never_start_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
        let p = plan();
        let job = store.enqueue_seedance(&p).unwrap();
        let (base, h) = mock(vec![("POST /v1/creative-agent/workflows ", 0, vec![])]);
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
            "POST /v1/creative-agent/workflows/fixture-id/cancel ",
            200,
            b"{}".to_vec(),
        )]);
        assert!(
            execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO)
                .unwrap_err()
                .to_string()
                .contains("cancellation requested")
        );
        h.join().unwrap();
    }
    #[test]
    fn failed_and_paused_workflows_do_not_become_takes() {
        for status in [
            "failed",
            "partial_failure",
            "cancelled",
            "waiting_for_user",
            "unexpected",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let store = Store::open(tmp.path().join("studio.sqlite")).unwrap();
            let p = plan();
            let job = store.enqueue_seedance(&p).unwrap();
            store.begin_video_submission(&job.id).unwrap();
            store.bind_video_remote(&job.id, "fixture-id").unwrap();
            let (base, h) = mock(vec![
                ("GET /v1/creative-agent/workflows/fixture-id ", 0, vec![]),
                ("GET /v1/creative-agent/workflows/fixture-id ", 503, vec![]),
                (
                    "GET /v1/creative-agent/workflows/fixture-id ",
                    200,
                    json!({"data":{"workflow":{"status":status}}})
                        .to_string()
                        .into_bytes(),
                ),
            ]);
            assert!(
                execute_transport(&store, &job.id, &p, &base, "fixture-only", Duration::ZERO)
                    .is_err()
            );
            h.join().unwrap();
            assert!(store.assets().unwrap().is_empty());
            assert_eq!(
                store.video_retry_available(&job.id).unwrap(),
                matches!(status, "waiting_for_user" | "unexpected")
            );
        }
    }
}
