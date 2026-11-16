//! Timeline diagnostics use registered speech durations, never estimated WPM.
use crate::store::Store;
use anyhow::Result;
use serde::Serialize;
#[derive(Serialize)]
pub struct TimedDialogue {
    pub line_id: String,
    pub shot_id: String,
    pub shot_title: String,
    pub text: String,
    pub start: Option<f64>,
    pub duration: Option<f64>,
    pub end: Option<f64>,
    pub speaker_visible: bool,
    pub crosses_shot_boundary: bool,
    pub beyond_episode: bool,
}
#[derive(Serialize)]
pub struct DialogueOverlap {
    pub first_line_id: String,
    pub second_line_id: String,
    pub start: f64,
    pub duration: f64,
}
#[derive(Serialize)]
pub struct DialogueGap {
    pub start: f64,
    pub duration: f64,
}
#[derive(Serialize)]
pub struct DialogueTiming {
    pub episode_id: String,
    pub checked_at: String,
    pub picture_seconds: f64,
    pub unmeasured_lines: usize,
    pub lines: Vec<TimedDialogue>,
    pub overlaps: Vec<DialogueOverlap>,
    pub gaps_without_dialogue: Option<Vec<DialogueGap>>,
    pub limits: String,
}
impl Store {
    pub fn dialogue_timing(&self, episode: &str) -> Result<DialogueTiming> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Deferred,
        )?;
        self.get_episode(episode)?;
        let shots = self.shots(episode)?;
        let assets = self.assets()?;
        let total = shots.iter().map(|s| s.duration).sum();
        let mut lines = Vec::new();
        let mut shot_start = 0.;
        for shot in shots {
            let mut local_start = Some(0.);
            for line in self.dialogue(&shot.id)? {
                let duration = assets
                    .iter()
                    .find(|a| Some(&a.id) == line.asset_id.as_ref())
                    .filter(|a| {
                        a.width == 0
                            && a.duration.is_finite()
                            && a.duration > 0.
                            && a.review != "rejected"
                            && a.path.is_file()
                    })
                    .map(|a| a.duration);
                let start = line
                    .timeline_start
                    .or_else(|| local_start.map(|offset| shot_start + offset));
                let end = start.zip(duration).map(|(s, d)| s + d);
                if line.timeline_start.is_none() {
                    local_start = local_start.zip(duration).map(|(s, d)| s + d);
                }
                let speaker_visible = self.dialogue_direction(&line.id)?.speaker_visible;
                lines.push(TimedDialogue {
                    line_id: line.id,
                    shot_id: shot.id.clone(),
                    shot_title: shot.title.clone(),
                    text: line.text,
                    start,
                    duration,
                    end,
                    speaker_visible,
                    crosses_shot_boundary: start.is_some_and(|s| s < shot_start - 0.001)
                        || end.is_some_and(|e| e > shot_start + shot.duration + 0.001),
                    beyond_episode: end.is_some_and(|e| e > total + 0.001),
                });
            }
            shot_start += shot.duration;
        }
        let mut overlaps = Vec::new();
        for (i, a) in lines.iter().enumerate() {
            for b in lines.iter().skip(i + 1) {
                if let (Some(sa), Some(ea), Some(sb), Some(eb)) = (a.start, a.end, b.start, b.end) {
                    let start = sa.max(sb);
                    let duration = ea.min(eb) - start;
                    if duration > 0.001 {
                        overlaps.push(DialogueOverlap {
                            first_line_id: a.line_id.clone(),
                            second_line_id: b.line_id.clone(),
                            start,
                            duration,
                        });
                    }
                }
            }
        }
        let gaps_without_dialogue = if lines.iter().all(|l| l.start.is_some() && l.end.is_some()) {
            let mut ranges = lines
                .iter()
                .filter_map(|l| l.start.zip(l.end))
                .collect::<Vec<_>>();
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut gaps = Vec::new();
            let mut cursor = 0f64;
            for (start, end) in ranges {
                if start.min(total) > cursor + 0.001 {
                    gaps.push(DialogueGap {
                        start: cursor,
                        duration: start.min(total) - cursor,
                    });
                }
                cursor = cursor.max(end).min(total);
            }
            if total > cursor + 0.001 {
                gaps.push(DialogueGap {
                    start: cursor,
                    duration: total - cursor,
                });
            }
            Some(gaps)
        } else {
            None
        };
        let result = DialogueTiming {
            episode_id: episode.into(),
            checked_at: self.conn.query_row("SELECT datetime('now')", [], |row| row.get(0))?,
            picture_seconds: total,
            unmeasured_lines: lines.iter().filter(|l| l.duration.is_none()).count(),
            lines, overlaps, gaps_without_dialogue,
            limits: "Snapshot of saved placement and durations measured when speech was registered. Missing/rejected files are unmeasured; later implicit starts stay unknown after unmeasured speech. Overlaps may be intentional interruptions. Dialogue-free gaps may contain acting, music or effects; they are not a silence measurement. File hashes, listening quality and word alignment are not verified by this report.".into(),
        };
        tx.commit()?;
        Ok(result)
    }
}
impl crate::Studio {
    pub(crate) fn dialogue_timing_panel(&mut self, ui: &mut eframe::egui::Ui, episode: &str) {
        ui.collapsing("Episode dialogue timing",|ui| {
            if ui.button("Refresh measured timing").clicked(){match self.store.dialogue_timing(episode){Ok(report)=>self.dialogue_timing_report=Some(report),Err(e)=>self.message=e.to_string()}}
            if let Some(report)=self.dialogue_timing_report.as_ref().filter(|r|r.episode_id==episode) {
                ui.label(format!("{} UTC · {} lines · {} unmeasured · {} overlaps",report.checked_at,report.lines.len(),report.unmeasured_lines,report.overlaps.len()));
                if !report.overlaps.is_empty(){ui.label(eframe::egui::RichText::new("Listen to overlapping lines; decide whether they are intentional before picture production.").color(crate::GOLD));}
                eframe::egui::ScrollArea::vertical().id_salt("dialogue-timing-report").max_height(260.).show(ui,|ui|{
                    for line in &report.lines {
                        let range=line.start.zip(line.end).map(|(a,b)|format!("{a:.2}–{b:.2}s")).unwrap_or_else(||"Timing incomplete".into());
                        ui.label(format!("{range} · {} · {}",line.shot_title,if line.speaker_visible{"on screen"}else{"off screen"}));ui.small(&line.text);
                        if line.beyond_episode {ui.label(eframe::egui::RichText::new("Extends beyond the episode").color(crate::GOLD));}
                        else if line.crosses_shot_boundary {ui.small("Spans a source-shot boundary; review the coverage across this cut.");}
                        ui.separator();
                    }
                    for overlap in &report.overlaps {
                        let excerpt=|id:&str|report.lines.iter().find(|l|l.line_id==id).map(|l|l.text.chars().take(70).collect::<String>()).unwrap_or_default();
                        ui.small(format!("Overlap at {:.2}s for {:.2}s: “{}” / “{}”",overlap.start,overlap.duration,excerpt(&overlap.first_line_id),excerpt(&overlap.second_line_id)));
                    }
                    if let Some(gaps)=&report.gaps_without_dialogue {
                        for gap in gaps.iter().filter(|g|g.duration>=1.) {ui.small(format!("No dialogue {:.2}–{:.2}s · allow for acting, music or effects",gap.start,gap.start+gap.duration));}
                    }
                });
                ui.small(&report.limits);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_implicit_starts_are_not_invented_and_explicit_overlaps_are_measured() {
        let root =
            std::env::temp_dir().join(format!("df-dialogue-timing-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.join("studio.sqlite")).unwrap();
        let series = store.create_series("Timing fixture", "").unwrap();
        let episode = store.create_episode(&series.id, "Fixture").unwrap();
        let shot = store
            .add_shot(&episode.id, "Fixture", "Not generated media", "", 3.)
            .unwrap();
        let path = root.join("registered-duration-fixture.dat");
        std::fs::write(&path, b"explicit metadata fixture").unwrap();
        store.conn.execute("INSERT INTO assets VALUES('speech','fixture',?1,'fixture',0,0,1,'{}','unreviewed')",[path.to_string_lossy().to_string()]).unwrap();
        let mut ids = Vec::new();
        for start in [None, None, Some(0.5), Some(1.)] {
            let line = crate::production::Dialogue {
                id: uuid::Uuid::new_v4().to_string(),
                shot_id: shot.id.clone(),
                text: "Fixture line".into(),
                voice: "af_heart".into(),
                speed: 1.,
                asset_id: None,
                timeline_start: start,
                revision: 0,
            };
            store.save_dialogue_draft(&line).unwrap();
            ids.push(line.id);
        }
        for id in &ids[1..] {
            store
                .conn
                .execute("UPDATE dialogue SET asset_id='speech' WHERE id=?1", [id])
                .unwrap();
        }
        store.set_dialogue_visibility(&ids[3], false, true).unwrap();
        let report = store.dialogue_timing(&episode.id).unwrap();
        assert!(report.gaps_without_dialogue.is_none());
        assert_eq!(report.unmeasured_lines, 1);
        assert_eq!(report.lines[0].start, Some(0.));
        assert_eq!(report.lines[1].start, None);
        assert_eq!(report.lines[1].duration, Some(1.));
        assert_eq!(report.lines[2].start, Some(0.5));
        assert_eq!(report.overlaps.len(), 1);
        assert_eq!(report.overlaps[0].start, 1.);
        assert_eq!(report.overlaps[0].duration, 0.5);
        assert!(!report.lines[3].speaker_visible);
        let reaction = store
            .add_shot(&episode.id, "Later reaction", "Fixture", "", 2.)
            .unwrap();
        let bridge = crate::production::Dialogue {
            id: uuid::Uuid::new_v4().to_string(),
            shot_id: reaction.id,
            text: "J-cut fixture".into(),
            voice: "af_heart".into(),
            speed: 1.,
            asset_id: None,
            timeline_start: Some(2.5),
            revision: 0,
        };
        store.save_dialogue_draft(&bridge).unwrap();
        store
            .conn
            .execute(
                "UPDATE dialogue SET asset_id='speech' WHERE id=?1",
                [&bridge.id],
            )
            .unwrap();
        let report = store.dialogue_timing(&episode.id).unwrap();
        assert!(
            report
                .lines
                .iter()
                .find(|l| l.line_id == bridge.id)
                .unwrap()
                .crosses_shot_boundary
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
