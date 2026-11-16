//! Paged local review search; filters do not modify assets or their decisions.
use crate::{production::Asset, store::Store};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssetQuery {
    pub text: String,
    pub media: String,
    pub review: String,
    pub limit: u32,
    pub offset: u32,
}
impl Default for AssetQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            media: "all".into(),
            review: "all".into(),
            limit: 24,
            offset: 0,
        }
    }
}
#[derive(Clone, Serialize)]
pub struct AssetPage {
    pub assets: Vec<Asset>,
    pub total: u64,
    pub offset: u32,
    pub limit: u32,
}
const CATALOG:&str="WITH catalog AS (SELECT rowid AS sequence,*,CASE WHEN json_valid(metadata) THEN metadata ELSE '{}' END AS parsed FROM assets)
FROM catalog a
LEFT JOIN asset_review_notes n ON n.asset_id=a.id
LEFT JOIN dialogue d ON d.id=json_extract(a.parsed,'$.request.line_id')
LEFT JOIN shots s ON s.id=COALESCE(json_extract(a.parsed,'$.request.shot_id'),d.shot_id)
LEFT JOIN library l ON l.id=json_extract(a.parsed,'$.request.library_id')
LEFT JOIN episodes e ON e.id=COALESCE(json_extract(a.parsed,'$.timeline.episode_id'),json_extract(a.parsed,'$.request.episode_id'),s.episode_id)
LEFT JOIN series se ON se.id=COALESCE(e.series_id,l.series_id)
WHERE (?1='' OR instr(lower(a.id||' '||COALESCE(n.note,'')||' '||COALESCE(s.title,'')||' '||COALESCE(l.name,'')||' '||COALESCE(e.title,'')||' '||COALESCE(se.name,'')||' '||COALESCE(json_extract(a.parsed,'$.request.prompt'),'')||' '||COALESCE(json_extract(a.parsed,'$.request.text'),'')||' '||COALESCE(json_extract(a.parsed,'$.name'),'')),lower(?1))>0)
AND (?2='all' OR a.review=?2)
AND (?3='all' OR (?3='video' AND a.width>0 AND a.duration>0) OR (?3='image' AND a.width>0 AND a.duration=0) OR (?3='audio' AND a.width=0 AND a.duration>0))";
impl Store {
    pub fn find_assets(&self, query: &AssetQuery) -> Result<AssetPage> {
        if query.text.len() > 500
            || !["all", "approved", "unreviewed", "rejected"].contains(&query.review.as_str())
            || !["all", "video", "image", "audio"].contains(&query.media.as_str())
            || !(1..=100).contains(&query.limit)
        {
            bail!(
                "Choose a valid media/review filter, search up to 500 bytes and a page size from 1 to 100"
            );
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Deferred,
        )?;
        let (prefix, body) = CATALOG.split_once("\nFROM").expect("static catalog query");
        let count = format!("{prefix} SELECT count(*) FROM{body}");
        let params = rusqlite::params![query.text.trim(), query.review, query.media];
        let total = self.conn.query_row(&count, params, |r| r.get(0))?;
        let sql = format!(
            "{prefix} SELECT a.id,a.job_id,a.path,a.hash,a.width,a.height,a.duration,a.metadata,a.review FROM{body} ORDER BY a.sequence DESC LIMIT ?4 OFFSET ?5"
        );
        let assets = self
            .conn
            .prepare(&sql)?
            .query_map(
                rusqlite::params![
                    query.text.trim(),
                    query.review,
                    query.media,
                    query.limit,
                    query.offset
                ],
                crate::production::asset_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        tx.commit()?;
        Ok(AssetPage {
            assets,
            total,
            offset: query.offset,
            limit: query.limit,
        })
    }
}
