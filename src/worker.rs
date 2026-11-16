use crate::store::Store;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::params;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

/// Exactly one worker owns this database's queue. OS lock is released on crash.
/// Acquiring it makes it safe to recover abandoned running rows.
pub fn run(path: PathBuf, once: bool) -> Result<()> {
    let store = Store::open(path)?;
    let lock_path = store.root().join("worker.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    if once {
        lock.try_lock_exclusive()
            .context("Another worker owns this queue")?;
    } else {
        lock.lock_exclusive()?;
    }
    store.conn.execute("UPDATE jobs SET status=CASE WHEN cancel_requested=1 THEN 'canceled' ELSE 'failed' END,error=CASE WHEN cancel_requested=1 THEN 'Canceled worker interrupted; attempt files preserved.' ELSE 'Worker interrupted. Retry to resume this request.' END,updated_at=datetime('now') WHERE status='running'",[])?;
    loop {
        let id:Option<String>=store.conn.query_row("UPDATE jobs SET status='running',attempts=attempts+1,progress=0,error='',updated_at=datetime('now') WHERE id=(SELECT id FROM jobs WHERE status='queued' AND cancel_requested=0 ORDER BY rowid LIMIT 1) RETURNING id",[],|r|r.get(0)).optional()?;
        if let Some(id) = id {
            let kind: String =
                store
                    .conn
                    .query_row("SELECT kind FROM jobs WHERE id=?1", [&id], |r| r.get(0))?;
            store.conn.execute("INSERT INTO job_timing(job_id,started_unix,finished_unix) VALUES(?1,unixepoch(),NULL) ON CONFLICT(job_id) DO UPDATE SET started_unix=excluded.started_unix,finished_unix=NULL",[&id])?;
            store
                .conn
                .execute("UPDATE jobs SET phase='Starting' WHERE id=?1", [&id])?;
            let result = match kind.as_str() {
                "openrouter_video" => crate::hosted_video::execute(&store, &id),
                "openrouter_image" => crate::hosted_image::execute(&store, &id),
                "rehearsal" => execute_rehearsal(&store, &id),
                "export" => execute_export(&store, &id),
                _ => Err(anyhow::anyhow!(
                    "This local-generation job type was removed. Hosted OpenRouter generation is being connected; original attempt files remain available."
                )),
            };
            if let Err(e) = result {
                store.conn.execute("UPDATE jobs SET status=CASE WHEN cancel_requested=1 THEN 'canceled' ELSE 'failed' END,error=?2,updated_at=datetime('now') WHERE id=?1",params![id,format!("{e:#}")])?;
            }
            store.conn.execute(
                "UPDATE job_timing SET finished_unix=unixepoch() WHERE job_id=?1",
                [&id],
            )?;
        } else {
            break;
        }
    }
    Ok(())
}
use rusqlite::OptionalExtension;
pub fn validate_video(ffmpeg: &Path, path: &Path) -> Result<()> {
    if !path.is_file() || path.metadata()?.len() == 0 {
        bail!("Generated video is missing or empty");
    }
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-f", "null", "-"])
        .output()?;
    if !out.status.success() {
        bail!(
            "Video integrity check failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}
pub fn hash_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
pub fn spawn_detached(path: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut command = Command::new(exe);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .arg("--worker")
        .env("DRAMAFACTORY_DATABASE", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            OpenOptions::new().create(true).append(true).open(
                path.parent()
                    .context("Database directory missing")?
                    .join("worker-service.log"),
            )?,
        ))
        .spawn()?;
    Ok(())
}
fn execute_export(store: &Store, id: &str) -> Result<()> {
    let c = store.config()?;
    let text: String = store
        .conn
        .query_row("SELECT request FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
    let r: crate::production::ExportRequest = serde_json::from_str(&text)?;
    if r.clips.is_empty() {
        bail!("Timeline is empty");
    }
    let dir = store
        .root()
        .join("jobs")
        .join(id)
        .join(format!("attempt-{}", store.job(id)?.attempts));
    std::fs::create_dir_all(&dir)?;
    let dir = std::fs::canonicalize(dir)?;
    if !r.captions.is_empty() {
        std::fs::write(dir.join("episode.srt"), crate::captions::srt(&r.captions))?;
        std::fs::write(
            dir.join("episode.ass"),
            crate::captions::ass(&r.captions, r.width, r.height),
        )?;
    }
    std::fs::write(dir.join("timeline.json"), serde_json::to_vec_pretty(&r)?)?;
    let output = dir.join("episode.mp4");
    let mut command = Command::new(std::fs::canonicalize(&c.ffmpeg)?);
    command.current_dir(&dir);
    command.args(["-hide_banner", "-v", "error", "-nostdin"]);
    let mut graph = String::new();
    let mut duration = 0.;
    for (i, clip) in r.clips.iter().enumerate() {
        if hash_file(&clip.path)? != clip.hash {
            bail!("Source asset changed: {}", clip.asset_id);
        }
        command.arg("-i").arg(std::fs::canonicalize(&clip.path)?);
        clip.picture_fades.validate(clip.duration)?;
        graph.push_str(&format!("[{i}:v]trim=start={}:duration={},setpts=PTS-STARTPTS,scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1,fps={}",clip.source_start,clip.duration,r.width,r.height,r.width,r.height,r.fps));
        if clip.picture_fades.fade_in > 0. {
            graph.push_str(&format!(",fade=t=in:st=0:d={}", clip.picture_fades.fade_in));
        }
        if clip.picture_fades.fade_out > 0. {
            graph.push_str(&format!(
                ",fade=t=out:st={}:d={}",
                clip.duration - clip.picture_fades.fade_out,
                clip.picture_fades.fade_out
            ));
        }
        graph.push_str(&format!("[v{i}];"));
        duration += clip.duration;
    }
    let mut audio_input = r.clips.len();
    for (i, clip) in r.clips.iter().enumerate() {
        if clip.audio.is_empty() {
            graph.push_str(&format!(
                "anullsrc=r=48000:cl=stereo,atrim=duration={}[a{i}];",
                clip.duration
            ));
        } else {
            for (j, audio) in clip.audio.iter().enumerate() {
                if hash_file(&audio.path)? != audio.hash {
                    bail!("Dialogue source changed");
                }
                command.arg("-i").arg(std::fs::canonicalize(&audio.path)?);
                graph.push_str(&format!("[{audio_input}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS[line{i}_{j}];"));
                audio_input += 1;
            }
            for j in 0..clip.audio.len() {
                graph.push_str(&format!("[line{i}_{j}]"));
            }
            graph.push_str(&format!(
                "concat=n={}:v=0:a=1,apad,atrim=duration={},asetpts=PTS-STARTPTS[a{i}];",
                clip.audio.len(),
                clip.duration
            ));
        }
    }
    for i in 0..r.clips.len() {
        graph.push_str(&format!("[v{i}][a{i}]"));
    }
    graph.push_str(&format!("concat=n={}:v=1:a=1[outv][outa]", r.clips.len()));
    if !r.cues.is_empty() {
        graph.push(';');
        for (i, entry) in r.cues.iter().enumerate() {
            if hash_file(&entry.path)? != entry.hash {
                bail!("Audio cue source changed: {}", entry.cue.name);
            }
            command.arg("-i").arg(std::fs::canonicalize(&entry.path)?);
            let cue = &entry.cue;
            graph.push_str(&format!("[{audio_input}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,atrim=start={}:duration={},asetpts=PTS-STARTPTS,volume={}dB",cue.source_start,cue.duration,cue.gain_db));
            if cue.fade_in > 0. {
                graph.push_str(&format!(",afade=t=in:st=0:d={}", cue.fade_in));
            }
            if cue.fade_out > 0. {
                graph.push_str(&format!(
                    ",afade=t=out:st={}:d={}",
                    cue.duration - cue.fade_out,
                    cue.fade_out
                ));
            }
            graph.push_str(&format!(
                ",adelay={}S:all=1[cue{i}];",
                (cue.start * 48000.).round() as u64
            ));
            audio_input += 1;
        }
        graph.push_str("[outa]");
        for i in 0..r.cues.len() {
            graph.push_str(&format!("[cue{i}]"));
        }
        graph.push_str(&format!("amix=inputs={}:duration=first:dropout_transition=0:normalize=0,alimiter=limit=0.95:level=0:latency=1[mixed]",r.cues.len()+1));
    }
    let captioned = r.burn_captions && !r.captions.is_empty();
    if captioned {
        graph.push_str(";[outv]ass=filename=episode.ass[captioned]");
    }
    command
        .args([
            "-filter_complex",
            &graph,
            "-map",
            if captioned { "[captioned]" } else { "[outv]" },
            "-map",
            if r.cues.is_empty() {
                "[outa]"
            } else {
                "[mixed]"
            },
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-t",
            &duration.to_string(),
            "-movflags",
            "+faststart",
            "-n",
        ])
        .args(["-progress", "pipe:1", "-nostats"])
        .arg(&output)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(File::create(dir.join("worker.log"))?));
    let mut child = ManagedChild(command.spawn()?);
    let stdout = child
        .stdout
        .take()
        .context("Export progress stream missing")?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if let Some(time) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<f64>().ok())
            {
                let _ = tx.send(time);
            }
        }
    });
    let start = std::time::Instant::now();
    loop {
        while let Ok(time) = rx.try_recv() {
            store.conn.execute(
                "UPDATE jobs SET progress=?2,phase='Assembling episode' WHERE id=?1",
                params![id, (time / (duration * 1e6)).clamp(0., 0.99)],
            )?;
        }
        let canceled: bool =
            store
                .conn
                .query_row("SELECT cancel_requested FROM jobs WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        if canceled || start.elapsed() > Duration::from_secs(3600) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Export canceled or timed out");
        }
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!(
                    "FFmpeg assembly failed. See {}",
                    dir.join("worker.log").display()
                );
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    reader
        .join()
        .map_err(|_| anyhow::anyhow!("Export progress reader failed"))?;
    validate_video(&c.ffmpeg, &output)?;
    let info = crate::media::probe(&c.ffmpeg, &output)?;
    if info.width != r.width
        || info.height != r.height
        || (info.duration - duration).abs() > 0.1
        || !info.audio
    {
        bail!("Export failed duration, resolution or audio validation");
    }
    let asset_id = uuid::Uuid::new_v4().to_string();
    let hash = hash_file(&output)?;
    let subtitle_file = if r.captions.is_empty() {
        None
    } else {
        Some(dir.join("episode.srt"))
    };
    let subtitle_hash = subtitle_file.as_ref().map(|p| hash_file(p)).transpose()?;
    let metadata = json!({"kind":"episode_export","timeline":r,"info":info,"audio":if r.continuity_context.get("hosted_segments").is_some(){"Original generated segment soundtracks, once, in saved order"}else{"Shot dialogue and timed dialogue bridges/music/SFX with peak limiting"},"subtitle_file":subtitle_file,"subtitle_sha256":subtitle_hash,"captions_burned":captioned,"caption_timing":"Measured line boundaries; long-line chunk times are estimated, not word-aligned"});
    let tx = store.conn.unchecked_transaction()?;
    tx.execute("INSERT INTO assets(id,job_id,path,hash,width,height,duration,metadata) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![asset_id,id,output.to_string_lossy(),hash,info.width,info.height,info.duration,metadata.to_string()])?;
    crate::completion::finish(&tx, id, &asset_id, "Episode export ready")?;
    tx.commit()?;
    let _ = Command::new(&c.ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(output)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-y"])
        .arg(dir.join("poster.png"))
        .output();
    Ok(())
}
fn execute_rehearsal(store: &Store, id: &str) -> Result<()> {
    let text: String =
        store
            .conn
            .query_row("SELECT request FROM jobs WHERE id=?1", [id], |row| {
                row.get(0)
            })?;
    let request: crate::rehearsal::RehearsalRequest = serde_json::from_str(&text)?;
    let directory = store
        .root()
        .join("jobs")
        .join(id)
        .join(format!("attempt-{}", store.job(id)?.attempts));
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join("request.json"),
        serde_json::to_vec_pretty(&request)?,
    )?;
    let output = directory.join("rehearsal.wav");
    let mut command = Command::new(store.config()?.ffmpeg);
    command.args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo",
    ]);
    let mut filter = format!("[0:a]atrim=duration={}[silence];", request.duration);
    let mut inputs = String::from("[silence]");
    for (index, line) in request.lines.iter().enumerate() {
        if hash_file(&line.path)? != line.hash {
            bail!("Speech source changed after rehearsal was queued");
        }
        command.arg("-i").arg(&line.path);
        let input = index + 1;
        let delay = (line.start * 48000.).round() as u64;
        filter.push_str(&format!("[{input}:a]aresample=48000,aformat=channel_layouts=stereo,atrim=duration={},adelay={delay}S:all=1[line{index}];",line.duration));
        inputs.push_str(&format!("[line{index}]"));
    }
    filter.push_str(&format!("{inputs}amix=inputs={}:duration=first:normalize=0,alimiter=limit=0.95:level=false:latency=true[out]",request.lines.len()+1));
    let filter_path = directory.join("mix.ffgraph");
    std::fs::write(&filter_path, &filter)?;
    command
        .arg("-filter_complex")
        .arg(filter)
        .args(["-map", "[out]", "-t"])
        .arg(request.duration.to_string())
        .args(["-c:a", "pcm_s16le", "-n"])
        .arg(&output);
    let log = File::create(directory.join("worker.log"))?;
    let mut child = ManagedChild(
        command
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()?,
    );
    store.conn.execute(
        "UPDATE jobs SET phase='Mixing dialogue rehearsal',progress=0.3 WHERE id=?1",
        [id],
    )?;
    let start = std::time::Instant::now();
    loop {
        let canceled: bool = store.conn.query_row(
            "SELECT cancel_requested FROM jobs WHERE id=?1",
            [id],
            |row| row.get(0),
        )?;
        if canceled || start.elapsed() > Duration::from_secs(300) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Dialogue rehearsal canceled or timed out");
        }
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!(
                    "Dialogue mix failed; see {}",
                    directory.join("worker.log").display()
                );
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let reader = hound::WavReader::open(&output)?;
    let duration = reader.duration() as f64 / reader.spec().sample_rate as f64;
    if (duration - request.duration).abs() > 0.02 {
        bail!("Dialogue rehearsal has unexpected duration");
    }
    let asset_id = uuid::Uuid::new_v4().to_string();
    let metadata = json!({"kind":"dialogue_rehearsal","request":request,"note":"Audio-only timing rehearsal; not a final episode or an acting-quality approval"});
    let tx = store.conn.unchecked_transaction()?;
    tx.execute("INSERT INTO assets(id,job_id,path,hash,width,height,duration,metadata) VALUES(?1,?2,?3,?4,0,0,?5,?6)",params![asset_id,id,output.to_string_lossy(),hash_file(&output)?,duration,metadata.to_string()])?;
    crate::completion::finish(&tx, id, &asset_id, "Audio rehearsal ready")?;
    tx.commit()?;
    Ok(())
}

/// Reap export subprocesses on every exit path, including validation errors.
struct ManagedChild(std::process::Child);
impl std::ops::Deref for ManagedChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
