//! Local, non-destructive freeze/zoom endings; never calls a hosted model.
use crate::{
    editing_room::{Clip, Cut},
    store::Store,
};
use anyhow::{Context, Result, ensure};
use eframe::egui;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Clone, serde::Serialize)]
pub struct Recipe {
    pub at: f64,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
    pub duration: f64,
    pub speed: f64,
    pub sound: u8,
    pub sound_path: Option<PathBuf>,
    pub gain: f64,
    pub sound_at_end: bool,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            at: 0.,
            x: 0.5,
            y: 0.5,
            zoom: 1.8,
            duration: 2.,
            speed: 0.4,
            sound: 0,
            sound_path: None,
            gain: -12.,
            sound_at_end: false,
        }
    }
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub recipe: Recipe,
    pub expected: Option<Cut>,
    pending: Option<mpsc::Receiver<Result<(Cut, Cut)>>>,
    pub status: String,
}
impl Panel {
    pub fn begin(&mut self, cut: &Cut, at: f64) {
        if self.pending.is_none() {
            self.recipe = Recipe {
                at,
                ..Default::default()
            };
            self.expected = Some(cut.clone());
            self.open = true;
            self.status.clear();
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn poll(&mut self) -> Option<Result<(Cut, Cut)>> {
        let result = self.pending.as_ref()?.try_recv();
        match result {
            Ok(v) => {
                self.pending = None;
                Some(v)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => {
                self.pending = None;
                Some(Err(anyhow::anyhow!("Cliffhanger render stopped")))
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, store: &Store, episode: &str) {
        ui.horizontal(|ui| {
            ui.strong("Cliffhanger ending");
            if ui.small_button("Close").clicked() {
                self.open = false;
            }
        });
        ui.separator();
        ui.label(format!("Freeze at {:.2}s", self.recipe.at));
        ui.label("Click to set the zoom target, or drag in the viewer to reposition the frame.");
        let busy = self.busy();
        ui.add_enabled_ui(!busy,|ui| {
 ui.add(egui::Slider::new(&mut self.recipe.duration,0.5..=6.).text("Ending seconds"));
 ui.add(egui::Slider::new(&mut self.recipe.zoom,1.0..=3.).text("Zoom"));
 self.recipe.speed=self.recipe.speed.min(self.recipe.duration);
 ui.add(egui::Slider::new(&mut self.recipe.speed,0.1..=self.recipe.duration).text("Zoom seconds"));
 ui.horizontal(|ui| {if ui.button("Punch in").clicked(){self.recipe.speed=0.2;}
 if ui.button("Slow push").clicked(){self.recipe.speed=self.recipe.duration;}});
 egui::ComboBox::from_label("Sound sting").selected_text(match self.recipe.sound{1=>"Low hit",2=>"Imported sound",_=>"None"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.recipe.sound,0,"None");ui.selectable_value(&mut self.recipe.sound,1,"Low hit");ui.selectable_value(&mut self.recipe.sound,2,"Imported sound");});
 if self.recipe.sound==2 && ui.button("Choose audio…").clicked(){self.recipe.sound_path=rfd::FileDialog::new().add_filter("Audio",&["wav","mp3","m4a","aiff"]).pick_file();}
 if let Some(path)=&self.recipe.sound_path {ui.label(path.file_name().unwrap_or_default().to_string_lossy());}
 if self.recipe.sound!=0 {ui.add(egui::Slider::new(&mut self.recipe.gain,-40. ..=0.).text("Sting dB"));ui.checkbox(&mut self.recipe.sound_at_end,"Play when zoom lands");}
 ui.label("Replaces the rest of the episode in this edit. Originals stay intact; Undo restores the previous ending. Background music continues.");
 if ui.button("Create ending").clicked() && let Some(expected)=self.expected.clone(){
   let db=store.path.clone();let episode=episode.to_owned();let recipe=self.recipe.clone();let(tx,rx)=mpsc::channel();self.pending=Some(rx);self.status="Creating a short local effect…".into();
   std::thread::spawn(move||{let result=render(db,&episode,&expected,&recipe).map(|next|(expected,next));let _=tx.send(result);});
 }
 });
        if busy {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(80));
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }
}
pub fn ending_cut(cut: &Cut, at: f64, mut freeze: Clip) -> Result<Cut> {
    ensure!(at.is_finite() && at >= 0., "Choose a frame inside a clip");
    let index = cut
        .clips
        .iter()
        .enumerate()
        .find(|(i, c)| at >= cut.clip_time(*i) && at < cut.clip_time(*i) + c.duration)
        .map(|(i, _)| i)
        .context("Choose a frame inside a video clip, not a gap")?;
    let offset = at - cut.clip_time(index);
    let mut next = Cut {
        clips: cut.clips[..index].to_vec(),
    };
    if offset >= 0.04 {
        let mut moving = cut.clips[index].clone();
        moving.duration = offset;
        next.clips.push(moving);
    } else {
        freeze.gap_before = cut.clips[index].gap_before;
    }
    next.clips.push(freeze);
    Ok(next)
}
fn run(command: &mut Command) -> Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "Could not render cliffhanger media with FFmpeg"
            );
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Cliffhanger render timed out");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn render(db: PathBuf, episode: &str, cut: &Cut, r: &Recipe) -> Result<Cut> {
    ensure!(
        r.at.is_finite()
            && r.x.is_finite()
            && r.y.is_finite()
            && (0. ..=1.).contains(&r.x)
            && (0. ..=1.).contains(&r.y)
            && (1. ..=3.).contains(&r.zoom)
            && (0.5..=6.).contains(&r.duration)
            && (0.1..=r.duration).contains(&r.speed),
        "Invalid ending settings"
    );
    let store = Store::open(db)?;
    ensure!(
        store.episode_cut(episode)? == *cut,
        "Timeline changed; choose the freeze frame again"
    );
    let (asset_id, source) = cut
        .source_at(r.at)
        .context("Choose a frame inside a clip")?;
    let asset = store
        .assets()?
        .into_iter()
        .find(|a| a.id == asset_id)
        .context("Source clip missing")?;
    ensure!(
        crate::worker::hash_file(&asset.path)? == asset.hash,
        "Source video changed"
    );
    let ffmpeg = store.config()?.ffmpeg;
    let id = uuid::Uuid::new_v4().to_string();
    let dir = store.root().join("cliffhangers").join(&id);
    std::fs::create_dir_all(&dir)?;
    let still = dir.join("freeze.png");
    let output = dir.join("ending.mp4");
    let (w, h) = store.video_dimensions(720, 1280);
    run(Command::new(&ffmpeg).args(["-v","error","-nostdin","-ss",&source.min((asset.duration-0.04).max(0.)).to_string(),"-i"]).arg(&asset.path).args(["-frames:v","1","-vf",&format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1")]).arg(&still))?;
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-v", "error", "-nostdin", "-loop", "1", "-i"])
        .arg(&still);
    match r.sound {
        1 => {
            cmd.args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=65:duration=0.6:sample_rate=48000",
            ]);
        }
        2 => {
            cmd.arg("-i").arg(
                r.sound_path
                    .as_ref()
                    .context("Choose the sound file first")?,
            );
        }
        _ => {
            cmd.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
        }
    }
    let frames = (r.duration * 30.).round() as u32;
    let delay = if r.sound_at_end {
        (r.speed.min((r.duration - 0.15).max(0.)) * 1000.).round() as u32
    } else {
        0
    };
    let zoom = format!("1+{}*min(on/{},1)", r.zoom - 1., r.speed * 30.);
    let sound_fade = if r.sound == 1 {
        "afade=t=out:st=0.4:d=0.2,".to_owned()
    } else {
        String::new()
    };
    let graph = format!(
        "[0:v]zoompan=z='{zoom}':x='max(0,min(iw-iw/zoom,iw*{}-iw/zoom/2))':y='max(0,min(ih-ih/zoom,ih*{}-ih/zoom/2))':d=1:s={w}x{h}:fps=30[v];[1:a]volume={}dB,{sound_fade}adelay={delay}|{delay},apad,atrim=duration={}[a]",
        r.x, r.y, r.gain, r.duration
    );
    run(cmd
        .args([
            "-filter_complex",
            &graph,
            "-map",
            "[v]",
            "-map",
            "[a]",
            "-frames:v",
            &frames.to_string(),
            "-t",
            &r.duration.to_string(),
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
        ])
        .arg(&output))?;
    let info = crate::media::probe(&ffmpeg, &output)?;
    ensure!(
        info.duration > 0. && info.audio,
        "Invalid cliffhanger output"
    );
    let hash = crate::worker::hash_file(&output)?;
    let metadata = serde_json::json!({"kind":"cliffhanger","episode_id":episode,"source_asset_id":asset.id,"recipe":r});
    store.conn.execute("INSERT INTO assets(id,job_id,path,hash,width,height,duration,metadata,review) VALUES(?1,'local-effect',?2,?3,?4,?5,?6,?7,'unreviewed')",rusqlite::params![id,output.to_string_lossy(),hash,info.width,info.height,info.duration,metadata.to_string()])?;
    ending_cut(
        cut,
        r.at,
        Clip {
            id: uuid::Uuid::new_v4().to_string(),
            asset_id: id,
            title: "Freeze ending".into(),
            gap_before: 0.,
            audio_fade_out: 0.,
            audio_fade_in: 0.,
            audio_gain_db: 0.,
            audio_muted: false,
            start: 0.,
            duration: frames as f64 / 30.,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_freeze_render_and_undo_preserve_original_cut() {
        let Some(ffmpeg) = std::env::var_os("DRAMAFACTORY_TEST_FFMPEG") else {
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path().join("test.sqlite")).unwrap();
        let mut config = store.config().unwrap();
        config.ffmpeg = ffmpeg.into();
        store.save_config(&config).unwrap();
        let series = store.create_series("Test", "").unwrap();
        let episode = store.create_episode(&series.id, "One").unwrap();
        let path = root.path().join("source.mp4");
        run(Command::new(&config.ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=270x480:r=30:d=3",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path))
        .unwrap();
        let hash = crate::worker::hash_file(&path).unwrap();
        let asset_id = uuid::Uuid::new_v4().to_string();
        let metadata = serde_json::json!({"kind":"cliffhanger","episode_id":episode.id});
        store.conn.execute("INSERT INTO assets(id,job_id,path,hash,width,height,duration,metadata,review) VALUES(?1,'fixture',?2,?3,270,480,3,?4,'unreviewed')",rusqlite::params![asset_id,path.to_string_lossy(),hash,metadata.to_string()]).unwrap();
        let clip = Clip {
            id: uuid::Uuid::new_v4().to_string(),
            asset_id,
            title: "Original".into(),
            gap_before: 0.,
            audio_fade_out: 0.,
            audio_fade_in: 0.,
            audio_gain_db: 0.,
            audio_muted: false,
            start: 0.,
            duration: 3.,
        };
        let mut second = clip.clone();
        second.id = uuid::Uuid::new_v4().to_string();
        let before = Cut {
            clips: vec![clip, second],
        };
        store
            .save_episode_cut(&episode.id, &Cut::default(), &before)
            .unwrap();
        let r = Recipe {
            at: 1.,
            sound: 1,
            ..Default::default()
        };
        let next = render(store.path.clone(), &episode.id, &before, &r).unwrap();
        assert_eq!(next.clips.len(), 2);
        assert_eq!(next.clips[0].duration, 1.);
        assert!((next.duration() - 3.).abs() < 0.01);
        store.save_episode_cut(&episode.id, &before, &next).unwrap();
        store.cut_export_request(&episode.id, &next).unwrap();
        store.save_episode_cut(&episode.id, &next, &before).unwrap();
        assert_eq!(store.episode_cut(&episode.id).unwrap(), before);
        assert_eq!(crate::worker::hash_file(&path).unwrap(), hash);
        assert!(ending_cut(&before, 10., before.clips[0].clone()).is_err());
    }
}
