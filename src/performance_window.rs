//! Map an episode speech interval into a bounded picture take, without editing the voice.
use crate::{
    production::{Dialogue, Shot},
    store::Store,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceAudioWindow {
    pub source_start: f64,
}
impl Store {
    pub(crate) fn performance_line(&self, id: &str, episode: &str) -> Result<Dialogue> {
        let source: String = self
            .conn
            .query_row("SELECT shot_id FROM dialogue WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .context("Dialogue source line not found")?;
        if self.shot(&source)?.episode_id != episode {
            bail!("Performance speech must belong to the same episode");
        }
        self.require_visible_speaker(id)?;
        self.dialogue(&source)?
            .into_iter()
            .find(|l| l.id == id)
            .context("Dialogue source line not found")
    }
    pub(crate) fn performance_window(
        &self,
        shot: &Shot,
        line: &Dialogue,
        speech_duration: f64,
    ) -> Result<(f64, PerformanceAudioWindow)> {
        for other in self.dialogue(&shot.id)? {
            if other.id != line.id && self.dialogue_direction(&other.id)?.speaker_visible {
                bail!(
                    "This coverage has another visible dialogue line; split speaking coverage before conditioning it on a different source"
                );
            }
        }
        let shots = self.shots(&shot.episode_id)?;
        let target_start: f64 = shots
            .iter()
            .take_while(|s| s.id != shot.id)
            .map(|s| s.duration)
            .sum();
        let line_start = if let Some(start) = line.timeline_start {
            start
        } else {
            let source_start: f64 = shots
                .iter()
                .take_while(|s| s.id != line.shot_id)
                .map(|s| s.duration)
                .sum();
            let assets = self.assets()?;
            let mut offset = 0.;
            for prior in self.dialogue(&line.shot_id)? {
                if prior.id == line.id {
                    break;
                }
                if prior.timeline_start.is_none() {
                    let duration = assets
                        .iter()
                        .find(|a| Some(&a.id) == prior.asset_id.as_ref())
                        .filter(|a| {
                            a.width == 0
                                && a.duration.is_finite()
                                && a.duration > 0.
                                && a.review != "rejected"
                        })
                        .map(|a| a.duration)
                        .context(
                            "Earlier speech timing is unknown; set an explicit episode start",
                        )?;
                    offset += duration;
                }
            }
            source_start + offset
        };
        let relative = shot.source_start + line_start - target_start;
        let duration = shot.source_start + shot.duration;
        if !relative.is_finite()
            || !speech_duration.is_finite()
            || relative >= duration
            || relative + speech_duration <= 0.
        {
            bail!(
                "The selected speech does not overlap this picture take; choose matching speaking coverage"
            );
        }
        let source_primary = self
            .shot_direction(&line.shot_id)?
            .character_ids
            .first()
            .cloned();
        let target_primary = self
            .shot_direction(&shot.id)?
            .character_ids
            .first()
            .cloned();
        if source_primary.is_some() && source_primary != target_primary {
            bail!(
                "This coverage has a different primary character; do not animate a listener with the speaker's voice"
            );
        }
        Ok((
            relative.max(0.),
            PerformanceAudioWindow {
                source_start: (-relative).max(0.),
            },
        ))
    }
}
