//! Guarded publishing-text proposals. No upload or scheduling operations.
use crate::publish::ReleaseDraft;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, PartialEq)]
pub(crate) struct Snapshot {
    pub episode_id: String,
    pub source: String,
    pub saved: Option<String>,
    pub draft: ReleaseDraft,
}
impl Snapshot {
    pub fn context(&self) -> serde_json::Value {
        serde_json::json!({"episode_id":self.episode_id,"title":self.draft.title,"tiktok":self.draft.captions[0],"youtube":self.draft.captions[1],"instagram":self.draft.captions[2],"selected_platforms":self.draft.destinations,"has_tiktok_cover":!self.draft.tiktok_cover_png.is_empty()})
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub episode_id: String,
    pub title: Option<String>,
    pub tiktok: Option<String>,
    pub youtube: Option<String>,
    pub instagram: Option<String>,
}
impl Edit {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.episode_id.is_empty(),
            "Choose an episode for the publishing draft"
        );
        for value in [&self.title, &self.tiktok, &self.youtube, &self.instagram]
            .into_iter()
            .flatten()
        {
            ensure!(value.len() <= 16000, "Publishing text is too long");
        }
        Ok(())
    }
}
pub fn schema() -> serde_json::Value {
    serde_json::json!({"type":"object","additionalProperties":false,"required":["episode_id","title","tiktok","youtube","instagram"],"properties":{"episode_id":{"type":"string"},"title":{"type":["string","null"]},"tiktok":{"type":["string","null"]},"youtube":{"type":["string","null"]},"instagram":{"type":["string","null"]}}})
}
pub(crate) fn apply(current: &Snapshot, expected: &Snapshot, edit: &Edit) -> Result<ReleaseDraft> {
    edit.validate()?;
    ensure!(
        current == expected && edit.episode_id == current.episode_id,
        "The episode, publishing draft or saved version changed while Sidekick was replying. Newer work was preserved."
    );
    let mut draft = current.draft.clone();
    if let Some(title) = &edit.title {
        draft.title = title.clone();
    }
    for (i, text) in [&edit.tiktok, &edit.youtube, &edit.instagram]
        .into_iter()
        .enumerate()
    {
        if let Some(text) = text {
            draft.captions[i] = text.clone();
        }
    }
    Ok(draft)
}
