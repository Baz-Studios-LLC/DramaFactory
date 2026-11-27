//! Automatic local Codex responder. Uses ChatGPT login, never an API key.
//! Responses enter the normal guarded draft pipeline; no automatic media requests.
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn executable() -> Result<PathBuf> {
    let candidates = [
        "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
        "/Applications/Codex.app/Contents/Resources/codex",
        "/opt/homebrew/bin/codex",
        "/usr/local/bin/codex",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
        .or_else(|| std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|p| p.join("codex")).find(|p| p.is_file())))
        .context("Codex was not found. Install Codex and sign in with ChatGPT, or turn off Automatic answers to use a connected MCP client.")
}
fn command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    command
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("CODEX_API_KEY");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}
struct Running(std::process::Child);
impl Drop for Running {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(
    child: &mut Running,
    canceled: &AtomicBool,
    timeout: Duration,
) -> Result<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        ensure!(!canceled.load(Ordering::Relaxed), "Codex reply stopped");
        if let Some(status) = child.0.try_wait()? {
            return Ok(status);
        }
        ensure!(
            start.elapsed() < timeout,
            "Codex reply timed out. Your message is retained; no automatic retry was made."
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn preflight(exe: &Path, directory: &Path, canceled: &AtomicBool) -> Result<()> {
    let path = directory.join("capabilities.txt");
    let log = std::fs::File::create(&path)?;
    let mut child = Running(
        command(exe)
            .args(["exec", "--help"])
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?,
    );
    let status = wait(&mut child, canceled, Duration::from_secs(15))?;
    let help = std::fs::read_to_string(path)?;
    ensure!(
        status.success()
            && ["--ignore-user-config", "--ephemeral", "--output-schema"]
                .iter()
                .all(|flag| help.contains(flag)),
        "This Codex installation is too old for automatic replies. Update Codex/ChatGPT and reopen DramaFactory."
    );
    Ok(())
}
pub fn reply(
    messages: &[crate::sidekick::Message],
    context: &str,
    canceled: &AtomicBool,
) -> Result<crate::sidekick_edit::Reply> {
    reply_with(&executable()?, messages, context, canceled)
}
fn reply_with(
    exe: &Path,
    messages: &[crate::sidekick::Message],
    context: &str,
    canceled: &AtomicBool,
) -> Result<crate::sidekick_edit::Reply> {
    ensure!(!canceled.load(Ordering::Relaxed), "Codex reply stopped");
    ensure!(
        context.len() + messages.iter().map(|m| m.text.len()).sum::<usize>() <= 1024 * 1024,
        "Conversation is too large for a Codex request"
    );
    let dir = tempfile::tempdir()?;
    preflight(exe, dir.path(), canceled)?;
    let login_log = dir.path().join("login.txt");
    let log = std::fs::File::create(&login_log)?;
    let mut login = Running(
        command(exe)
            .args(["login", "status"])
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .context("Could not start Codex")?,
    );
    let status = wait(&mut login, canceled, Duration::from_secs(15))?;
    let login_text = std::fs::read_to_string(login_log)?;
    ensure!(
        status.success() || login_text.to_lowercase().contains("not logged in"),
        "Codex sign-in check could not run. Update the Codex installation; this is not a sign-in failure."
    );
    ensure!(
        status.success() && login_text.contains("Logged in using ChatGPT"),
        "Sign in to Codex with ChatGPT first. Automatic answers do not use API-key billing."
    );
    let schema =
        crate::sidekick_edit::response_format_for("All workspaces")["json_schema"]["schema"]
            .clone();
    let schema_path = dir.path().join("reply-schema.json");
    std::fs::write(&schema_path, serde_json::to_vec(&schema)?)?;
    let output = dir.path().join("reply.json");
    let mut cmd = command(exe);
    cmd.args([
        "exec",
        "--ignore-user-config",
        "--ephemeral",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--disable",
        "shell_tool",
        "--disable",
        "unified_exec",
        "--color",
        "never",
    ])
    .arg("--cd")
    .arg(dir.path())
    .arg("--output-schema")
    .arg(&schema_path)
    .arg("--output-last-message")
    .arg(&output);
    let workspace: serde_json::Value = serde_json::from_str(context)?;
    if let Some(database) = workspace["database"].as_str() {
        let executable = std::env::current_exe()?;
        let options = [
            (
                "mcp_servers.dramafactory.command",
                serde_json::json!(executable),
            ),
            (
                "mcp_servers.dramafactory.args",
                serde_json::json!(["--mcp"]),
            ),
            (
                "mcp_servers.dramafactory.env.DRAMAFACTORY_DATABASE",
                serde_json::json!(database),
            ),
            (
                "mcp_servers.dramafactory.enabled_tools",
                serde_json::json!(crate::mcp::authoring_tool_names()),
            ),
            (
                "mcp_servers.dramafactory.tool_timeout_sec",
                serde_json::json!(120),
            ),
            (
                "mcp_servers.dramafactory.env.DRAMAFACTORY_MCP_REQUEST",
                serde_json::json!(messages.last().map(|m| m.id.as_str()).unwrap_or_default()),
            ),
            ("approval_policy", serde_json::json!("never")),
        ];
        for (key, value) in options {
            cmd.arg("-c").arg(format!("{key}={value}"));
        }
    }
    // Attach the latest user images with actual pixels, never pretend to see them from paths.
    if let Some(last) = messages.last() {
        crate::chat_images::validate(&last.images)?;
        for (index, image) in last.images.iter().enumerate() {
            let data = image
                .data_url
                .strip_prefix("data:image/png;base64,")
                .context("Unsupported chat image")?;
            let path = dir.path().join(format!("attachment-{index}.png"));
            std::fs::write(
                &path,
                base64::engine::general_purpose::STANDARD.decode(data)?,
            )?;
            cmd.arg("--image").arg(path);
        }
    }
    let history: Vec<_> = messages.iter().map(|m| serde_json::json!({"role":m.role,"text":m.text,"context":m.context,"image_count":m.images.len()})).collect();
    let prompt = serde_json::json!({
        "instructions":crate::sidekick::authoring_instructions(),
        "transport_instructions":"You are the automatic Codex responder inside DramaFactory, not an already-running desktop chat. Answer the latest user message using this shared history and current workspace. Return only the required JSON. Use the connected DramaFactory MCP tools to read or edit any workspace, episode or series even when its tab is not visible. Do not execute shell commands, access credentials, upload, publish or generate media. Read get_live_workspace first for the current instance and action contracts. Do not call take_over, capture, reply, release or navigate: you are already answering and must leave the user on their current tab. For batch episode requests such as fill out episodes 5 through 15, use control_app read_episode_batch with series_id, then poll get_control_result. Read the overall plot and surrounding episodes and author the requested range coherently. Use control_app apply_episode_batch with its exact expected snapshot, start_episode and the complete consecutive authored episode objects. Poll receipts and verify the numbered episodes after success. A queued command is not success. For long ranges, complete successive manageable batches in this response, rereading state between them; do not stop after the first batch. Batch actions save only the targeted episodes and outlines. Do not also return duplicate structured episode/new_episodes/plot edits after successful tool edits. For single edits to captured drafts prefer structured response actions with Undo. Use saved-service tools for other documents only after reading their current values and checking get_live_workspace for unsaved drafts. Never overwrite a dirty GUI draft through saved-service tools. Newer-work conflicts require rereading and reconciliation, not blind retries. Return structured document edits for the host to apply with conflict protection and Undo. Set cast_queue null and generate_portrait false. Use Baz and no em dashes. Only claim edits are saved when a completed MCP receipt confirms it. Only the latest message's images are attached; do not claim to inspect earlier images or local portraits. Music and publishing actions prepare drafts only. The host will apply requested edits after validating the response.",
        "workspace":serde_json::from_str::<serde_json::Value>(context)?,"conversation":history
    }).to_string();
    let mut child = Running(
        cmd.arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not launch Codex responder")?,
    );
    let mut stdin = child.0.stdin.take().context("Codex stdin unavailable")?;
    let writer = std::thread::spawn(move || stdin.write_all(prompt.as_bytes()));
    let status = wait(&mut child, canceled, Duration::from_secs(600))?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("Codex input task stopped"))??;
    ensure!(
        status.success(),
        "Codex could not complete the reply. Check Codex sign-in, connection and usage limits. Your message is retained; no automatic retry was made."
    );
    ensure!(
        std::fs::metadata(&output)?.len() <= 2 * 1024 * 1024,
        "Codex reply exceeds 2 MiB"
    );
    let reply = crate::sidekick_edit::parse_reply(&std::fs::read_to_string(output)?)?;
    validate_reply(&reply)?;
    Ok(reply)
}
fn validate_reply(reply: &crate::sidekick_edit::Reply) -> Result<()> {
    if reply.cast_queue.is_some()
        || reply
            .new_character
            .as_ref()
            .is_some_and(|c| c.generate_portrait)
        || reply
            .new_characters
            .as_ref()
            .is_some_and(|cs| cs.iter().any(|c| c.generate_portrait))
    {
        bail!(
            "Codex returned a media-generation action. Nothing was applied; request draft edits only, then generate through the app."
        );
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fake(dir: &Path, login: &str, body: &str) -> PathBuf {
        let path = dir.join("codex");
        std::fs::write(
            &path,
            format!(
                r#"#!/bin/sh
case "$*" in "exec --help") echo '--ignore-user-config --ephemeral --output-schema'; exit 0;; esac
case "$*" in *"--ignore-user-config login"*) exit 2;; esac
case "$*" in *"login status"*) echo '{login}' >&2; exit 0;; esac
printf '%s\n' "$@" > '{}'
while [ "$#" -gt 0 ]; do
    case "$1" in --output-last-message) shift; output="$1";; esac
    shift
done
cat > '{}'
{body}
"#,
                dir.join("args.txt").display(),
                dir.join("input.json").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn messages() -> Vec<crate::sidekick::Message> {
        vec![crate::sidekick::Message {
            id: "test".into(),
            role: "user".into(),
            text: "Please revise the premise".into(),
            context: "Synopsis".into(),
            images: vec![],
        }]
    }
    #[test]
    #[ignore = "Read-only installed CLI check; no model request"]
    fn installed_codex_preflight_and_login() {
        let dir = tempfile::tempdir().unwrap();
        let exe = executable().unwrap();
        preflight(&exe, dir.path(), &AtomicBool::new(false)).unwrap();
        let output = command(&exe).args(["login", "status"]).output().unwrap();
        assert!(output.status.success());
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains("Logged in using ChatGPT"));
    }
    #[test]
    fn automatic_reply_receives_context_and_validates_structured_edits() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake(
            dir.path(),
            "Logged in using ChatGPT",
            r#"printf '%s' '{"message":"Revised draft.","synopsis":{"title":"BlockSide","premise":"A neighborhood stands together."}}' > "$output""#,
        );
        let reply = reply_with(
            &exe,
            &messages(),
            r#"{"workspace":"All workspaces","series":"BlockSide","database":"/tmp/Drama Factory/db.sqlite"}"#,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(reply.synopsis.unwrap().title, "BlockSide");
        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        assert!(args.contains("mcp_servers.dramafactory.command="));
        assert!(args.contains("/tmp/Drama Factory/db.sqlite"));
        assert!(args.contains("dramafactory_control_app"));
        assert!(!args.contains("dramafactory_generate_video"));
        assert!(!args.contains("dramafactory_publish_episode"));
        let input: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("input.json")).unwrap())
                .unwrap();
        assert_eq!(input["workspace"]["series"], "BlockSide");
        assert_eq!(
            input["conversation"][0]["text"],
            "Please revise the premise"
        );
        assert!(!input["instructions"].as_array().unwrap().is_empty());
    }
    #[test]
    fn api_key_login_never_launches_a_generation() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake(dir.path(), "Logged in using an API key", "exit 0");
        let error = reply_with(&exe, &messages(), "{}", &AtomicBool::new(false))
            .err()
            .unwrap();
        assert!(error.to_string().contains("ChatGPT"));
        assert!(!dir.path().join("input.json").exists());
    }
    #[test]
    fn cancellation_terminates_running_responder_and_rejects_late_output() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake(dir.path(), "Logged in using ChatGPT", "sleep 30");
        let canceled = std::sync::Arc::new(AtomicBool::new(false));
        let flag = canceled.clone();
        let task = std::thread::spawn(move || {
            reply_with(&exe, &messages(), "{}", &flag)
                .err()
                .unwrap()
                .to_string()
        });
        let started = Instant::now();
        while !dir.path().join("input.json").exists() {
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(20));
        }
        canceled.store(true, Ordering::Relaxed);
        assert!(task.join().unwrap().contains("stopped"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
    #[test]
    fn invalid_response_and_provider_failure_are_errors_not_edits() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake(
            dir.path(),
            "Logged in using ChatGPT",
            "echo invalid > \"$output\"",
        );
        assert!(reply_with(&exe, &messages(), "{}", &AtomicBool::new(false)).is_err());
        let exe = fake(dir.path(), "Logged in using ChatGPT", "exit 1");
        let error = reply_with(&exe, &messages(), "{}", &AtomicBool::new(false))
            .err()
            .unwrap();
        assert!(error.to_string().contains("no automatic retry"));
        let reply = crate::sidekick_edit::parse_reply(
            r#"{"message":"Queue","cast_queue":[{"name":"Jen","brief":"Mother"}]}"#,
        )
        .unwrap();
        assert!(validate_reply(&reply).is_err());
    }
}
