//! Read-only planning diagnostics. Export and worker checks remain authoritative.
use crate::store::Store;
use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct ReadinessIssue {
    pub scope: String,
    pub entity_id: String,
    pub title: String,
    pub action: String,
    pub blocks_export: bool,
}
#[derive(Clone, Serialize)]
pub struct EpisodeReadiness {
    pub episode_id: String,
    pub checked_at: String,
    pub shot_count: usize,
    pub dialogue_count: usize,
    pub picture_seconds: f64,
    pub export_request_valid: bool,
    pub issues: Vec<ReadinessIssue>,
    pub limits: String,
}
impl Store {
    pub fn episode_readiness(&self, episode: &str) -> Result<EpisodeReadiness> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Deferred,
        )?;
        let data = self.get_episode(episode)?;
        let shots = self.shots(episode)?;
        let assets = self.assets()?;
        let mut library = self.library(&data.series_id, "character")?;
        library.extend(self.library(&data.series_id, "location")?);
        let mut report = EpisodeReadiness {
            episode_id: episode.into(),
            checked_at: self.conn.query_row("SELECT datetime('now')", [], |row| row.get(0))?,
            shot_count: shots.len(), dialogue_count: 0,
            picture_seconds: shots.iter().map(|s| s.duration).sum(),
            export_request_valid: false, issues: Vec::new(),
            limits: "Saved-state snapshot. Refresh after edits. File presence and export request checks only; worker verifies media hashes and decodes. This does not approve acting, synchronization or story continuity.".into(),
        };
        let mut issue =
            |scope: &str, id: &str, title: &str, action: String, blocks_export: bool| {
                report.issues.push(ReadinessIssue {
                    scope: scope.into(),
                    entity_id: id.into(),
                    title: title.into(),
                    action,
                    blocks_export,
                });
            };
        if data.screenplay.trim().is_empty() {
            issue(
                "screenplay",
                episode,
                "Screenplay",
                "Write and save the screenplay before directing coverage.".into(),
                false,
            );
        }
        for shot in &shots {
            let asset = assets
                .iter()
                .find(|a| Some(&a.id) == shot.asset_id.as_ref());
            match asset {
                None => issue(
                    "picture",
                    &shot.id,
                    &shot.title,
                    "Generate or select a picture take in the shot planner.".into(),
                    true,
                ),
                Some(a) => {
                    if !a.path.is_file() {
                        issue(
                            "picture",
                            &shot.id,
                            &shot.title,
                            "Selected media is missing; restore it or select another take.".into(),
                            true,
                        );
                    }
                    if a.review != "approved" {
                        let note = self
                            .asset_review(&a.id)?
                            .note
                            .chars()
                            .take(240)
                            .collect::<String>();
                        issue(
                            "picture",
                            &shot.id,
                            &shot.title,
                            format!(
                                "Selected take is {}; inspect it in the screening room before approving or replacing it. {}",
                                a.review, note
                            ),
                            true,
                        );
                    }
                    if a.duration + 0.02 < shot.source_start + shot.duration {
                        issue(
                            "picture",
                            &shot.id,
                            &shot.title,
                            "Trim the shot to fit its selected take.".into(),
                            true,
                        );
                    }
                    if let Err(e) = self.validate_performance_take(shot, a) {
                        issue("performance", &shot.id, &shot.title, e.to_string(), true);
                    }
                }
            }
            let direction = self.shot_direction(&shot.id)?;
            if direction.character_ids.is_empty() {
                issue(
                    "direction",
                    &shot.id,
                    &shot.title,
                    "No cast linked. Link recurring characters where the scene calls for them."
                        .into(),
                    false,
                );
            }
            for id in direction
                .character_ids
                .iter()
                .chain(direction.location_id.iter())
            {
                if let Some(entry) = library.iter().find(|e| &e.id == id)
                    && !entry.reference.as_ref().is_some_and(|p| p.is_file())
                {
                    issue("reference", id, &entry.name, "Add a usable reference in the cast/location library before reference-guided coverage.".into(), false);
                }
            }
            for line in self.dialogue(&shot.id)? {
                report.dialogue_count += 1;
                match assets
                    .iter()
                    .find(|a| Some(&a.id) == line.asset_id.as_ref())
                {
                    None => issue(
                        "dialogue",
                        &line.id,
                        &shot.title,
                        "Generate missing episode speech in Voice Direction.".into(),
                        true,
                    ),
                    Some(a) if a.review == "rejected" || !a.path.is_file() => issue(
                        "dialogue",
                        &line.id,
                        &shot.title,
                        "Selected speech is rejected or missing; review or regenerate it.".into(),
                        true,
                    ),
                    _ => {}
                }
            }
        }
        for cue in self.audio_cues(episode)? {
            match assets.iter().find(|a| a.id == cue.asset_id) {
                None => issue("sound", &cue.id, &cue.name, "Cue audio is missing; import or select another track.".into(), true),
                Some(a) if a.review != "approved" || !a.path.is_file() => issue("sound", &cue.id, &cue.name, "Cue audio needs approval or its file is missing; review it in the sound library.".into(), true),
                _ => {}
            }
        }
        // Use the actual export service for timing, sound cues and other gates;
        // this supplements the full missing-take list rather than replacing it.
        let request = self.export_request(episode);
        report.export_request_valid =
            request.is_ok() && !report.issues.iter().any(|i| i.blocks_export);
        if let Err(e) = request {
            let action = e.to_string();
            report.issues.push(ReadinessIssue {
                scope: "export".into(),
                entity_id: episode.into(),
                title: "Export check".into(),
                action,
                blocks_export: true,
            });
        }
        report
            .issues
            .sort_by_key(|i| std::cmp::Reverse(i.blocks_export));
        let mut seen = std::collections::HashSet::new();
        report
            .issues
            .retain(|i| seen.insert((i.scope.clone(), i.entity_id.clone(), i.action.clone())));
        tx.commit()?;
        Ok(report)
    }
}
impl crate::Studio {
    pub(crate) fn readiness_panel(&mut self, ui: &mut eframe::egui::Ui, episode: &str) {
        ui.collapsing("Production checklist", |ui| {
            ui.label("Check saved coverage, speech and export requirements before queueing work.");
            if ui.button("Refresh checklist").clicked() {
                match self.store.episode_readiness(episode) {
                    Ok(report) => self.readiness = Some(report),
                    Err(e) => self.message = e.to_string(),
                }
            }
            if let Some(report) = self.readiness.as_ref().filter(|r| r.episode_id == episode) {
                ui.label(format!(
                    "{} UTC · {} shots · {} lines · {:.1}s",
                    report.checked_at,
                    report.shot_count,
                    report.dialogue_count,
                    report.picture_seconds
                ));
                if report.export_request_valid {
                    ui.label(
                        "Export request checks passed. Perform editorial review before delivery.",
                    );
                }
                for issue in &report.issues {
                    ui.label(format!(
                        "{} · {}: {}",
                        if issue.blocks_export {
                            "Required"
                        } else {
                            "Planning"
                        },
                        issue.title,
                        issue.action
                    ));
                }
                ui.small(&report.limits);
            }
        });
    }
}
