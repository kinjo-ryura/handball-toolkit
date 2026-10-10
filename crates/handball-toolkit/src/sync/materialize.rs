//! そろえた中身を、この端末に保存する形へ戻す（ADR 0007 決定 2）。
//!
//! そろえた中身には、相手の端末の値がそのまま入っている。
//! 端末ごとの値は、この端末に同じ記録があればこの端末の値を残す:
//!
//! - 試合の左右配置（`is_home_on_left`）— 見方の設定
//! - 選手の写真（`photo`）— 写真は運ばない。この端末に無い選手は写真なし
//! - 端末内動画の参照（`external_id`）— 同じ動画（`cloud_identifier` が同じ、またはどちらかが
//!   取れない）ならこの端末の参照を残す。違う動画・この端末に無い試合は届いた値を入れ、
//!   [`VideoRelink`] で引き直しを頼む

use serde::{Deserialize, Serialize};

use crate::configuration::{MatchConfiguration, VideoProvider};
use crate::ids::MatchId;

use super::compare::same_local_video;
use super::normalize::normalized;
use super::{LocalVideoIdentity, SyncMatch, SyncSnapshot};

/// この端末に保存する中身。**店の全記録をこれと同じにする** — ここに無い記録は消す。
/// そろえた中身は、採らなかった記録も削除の記録として持つ（`reconcile`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncApplyPlan {
    pub snapshot: SyncSnapshot,
    /// 端末内動画の参照を引き直す試合。シェルは手がかりを試合ごとに置き、初回再生で引き直す
    /// （試合ファイルで受け取った試合と同じ経路）。
    pub video_relinks: Vec<VideoRelink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct VideoRelink {
    pub match_id: MatchId,
    pub identity: LocalVideoIdentity,
}

/// `incoming`（そろえた中身）を、`local`（この端末の今の全記録）の
/// 端末ごとの値で戻す。
pub fn materialize(incoming: &SyncSnapshot, local: &SyncSnapshot) -> SyncApplyPlan {
    let mut snapshot = normalized(incoming);
    let mut video_relinks: Vec<VideoRelink> = Vec::new();

    for m in &mut snapshot.matches {
        let own = local
            .matches
            .iter()
            .find(|own| own.match_.id == m.match_.id);
        if let Some(own) = own {
            m.match_.is_home_on_left = own.match_.is_home_on_left;
        }
        if !is_local_video(&m.match_.configuration) {
            continue;
        }
        match own {
            Some(own) if keeps_own_reference(own, m) => {
                m.match_.configuration = own.match_.configuration.clone();
                m.local_video = own.local_video.clone();
            }
            _ => video_relinks.push(VideoRelink {
                match_id: m.match_.id,
                identity: m.local_video.clone().unwrap_or_default(),
            }),
        }
    }

    for p in &mut snapshot.players {
        p.player.photo = local
            .players
            .iter()
            .find(|own| own.player.id == p.player.id)
            .and_then(|own| own.player.photo.clone());
    }

    SyncApplyPlan {
        snapshot,
        video_relinks,
    }
}

fn is_local_video(configuration: &MatchConfiguration) -> bool {
    match configuration {
        MatchConfiguration::Video(source) | MatchConfiguration::VideoHighlight(source) => {
            source.provider == VideoProvider::Local
        }
        MatchConfiguration::Timer { .. } => false,
    }
}

/// この端末の参照を残すか。同じ記録方法（動画 / ハイライト）の端末内動画で、同じ動画のとき。
fn keeps_own_reference(own: &SyncMatch, incoming: &SyncMatch) -> bool {
    own.match_.configuration.kind() == incoming.match_.configuration.kind()
        && is_local_video(&own.match_.configuration)
        && same_local_video(own.local_video.as_ref(), incoming.local_video.as_ref())
}
