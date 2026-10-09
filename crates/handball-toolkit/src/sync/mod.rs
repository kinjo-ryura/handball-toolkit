//! 端末どうしの同期の計画（handball-project#506。判断の記録は `docs/adr/0007-device-sync.md`）。
//!
//! 近くにある 2 台の端末が全記録（試合・チーム・選手・fact）を交換し、1 回の操作で両方を同じ中身に
//! する。形は 2 つ:
//!
//! - **上書き** — どちらかの端末の中身で、もう片方を丸ごと置き換える（合わせる側のスナップショットを
//!   そのまま [`materialize`] に渡す）
//! - **そろえる** — 記録ごとに ID で比べ、`updated_at` の新しい方を採る（[`reconcile`]）
//!
//! このモジュールは**純粋関数だけ**を持つ（ADR 0005 決定 1 の計画層）。時刻はシェルが `now` で渡し、
//! ID は作らない。つなぐ・交換する・保存するのはシェルで、保存の発火は `ffi_write::apply_sync`。
//!
//! 流れ（始めた側の端末で）:
//! 1. 自分と相手のスナップショットを [`reconcile`] に渡す。利用者に聞くことがあれば
//!    [`SyncReconcileResult::Questions`] が返るので、答えを足して呼び直す（返らなくなるまで）
//! 2. できた中身（[`SyncReconcileResult::Merged`]）を相手へ送る。相手と自分は、それぞれ自分の
//!    スナップショットと合わせて [`materialize`] し、その結果で店を丸ごと置き換える
//!
//! **両方の端末が同じ比較を別々にしない**。比べるのは始めた側だけで、相手はできた中身で上書きする
//! ので、結果が食い違わない。

mod compare;
mod materialize;
mod normalize;
mod payload;
mod reconcile;

pub use compare::{fact_content_equal, match_content_equal, player_content_equal};
pub use materialize::{SyncApplyPlan, VideoRelink, materialize};
pub use payload::{
    SYNC_FORMAT_VERSION, SyncPayload, SyncPayloadError, decode_sync_payload, encode_sync_payload,
};
pub use reconcile::{SyncReconcileResult, reconcile};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::{Match, Player, Team};
use crate::facts::MatchFact;
use crate::ids::{FactId, MatchId, PlayerId, TeamId};

/// 記録 1 件の同期用の時刻。
///
/// - `updated_at` — 同期で比べる中身が最後に変わった時刻。消したときは消した時刻
/// - `deleted_at` — 論理削除した時刻。`None` は消されていない
///
/// 時刻は秒に丸めない（後勝ちの比較と、`recorded_at` を決め手に含む並び順を変えないため）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncStamp {
    pub updated_at: DateTime<Utc>,
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub deleted_at: Option<DateTime<Utc>>,
}

impl SyncStamp {
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// `now` に書き直した、消されていない版。
    pub(crate) fn alive_now(now: DateTime<Utc>) -> SyncStamp {
        SyncStamp {
            updated_at: now,
            deleted_at: None,
        }
    }

    /// `now` に消した版。
    pub(crate) fn deleted_now(now: DateTime<Utc>) -> SyncStamp {
        SyncStamp {
            updated_at: now,
            deleted_at: Some(now),
        }
    }
}

/// 端末内動画（`VideoProvider::Local`）を端末をまたいで見分ける手がかり。
///
/// `VideoSource.external_id`（PHAsset の localIdentifier 等）は端末ごとに違う値なので比べない。
/// 同じ動画かは `cloud_identifier` で見る。`duration_seconds` は、受け取った端末が引き直せず
/// 「動画を選び直す」ときに別の切り出しを止めるために比べる尺。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct LocalVideoIdentity {
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub cloud_identifier: Option<String>,
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncMatch {
    #[serde(rename = "match")]
    pub match_: Match,
    pub stamp: SyncStamp,
    /// 端末内動画の試合だけが持つ。
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub local_video: Option<LocalVideoIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncTeam {
    pub team: Team,
    pub stamp: SyncStamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncPlayer {
    pub player: Player,
    pub stamp: SyncStamp,
}

/// fact は試合の ID を持たないので、組で持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncFact {
    pub match_id: MatchId,
    pub fact: MatchFact,
    pub stamp: SyncStamp,
}

/// 1 台の端末の全記録。**消した記録も含む**（相手で消したことを伝えるため）。
///
/// 端末ごとの値（`Match.is_home_on_left`・`Player.photo`・端末内動画の `external_id`）も
/// その端末の値のまま入る。比べるときは無視し、[`materialize`] が端末ごとに戻す。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncSnapshot {
    pub matches: Vec<SyncMatch>,
    pub teams: Vec<SyncTeam>,
    pub players: Vec<SyncPlayer>,
    pub facts: Vec<SyncFact>,
}

/// 始めた側から見た端末。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum SyncSide {
    /// この端末（始めた側）
    Local,
    /// 相手の端末
    Remote,
}

/// 問いの対象の記録。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SyncRecordRef {
    Match { id: MatchId },
    Team { id: TeamId },
    Player { id: PlayerId },
    Fact { id: FactId },
}

/// 規則で決められず、利用者に聞くことの種類（ADR 0007 決定 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum SyncQuestionKind {
    /// 両方にあり、`updated_at` がまったく同じで中身が違う
    SameTime,
    /// 片方で消した後に、もう片方で変わっていた（`deleted_on` は消した側）
    DeletedThenChanged,
    /// 両方の直しを合わせると、その試合の記録が規則（R3〜R9 等）に合わなくなる。
    /// 答えた側の試合を丸ごと採る
    MergedMatchInvalid,
}

/// 利用者に聞くこと。答えは [`SyncAnswer`] で同じ `record` と `kind` に対して渡す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncQuestion {
    pub kind: SyncQuestionKind,
    pub record: SyncRecordRef,
    /// 問いを試合ごとにまとめて見せるための、記録が属する試合（試合そのものなら同じ ID）。
    /// チーム・選手は試合に属さないので `None`。
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub match_id: Option<MatchId>,
    /// `DeletedThenChanged` のときだけ、消した側。
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub deleted_on: Option<SyncSide>,
}

/// 問いへの答え。`keep` の側の版を採る（`DeletedThenChanged` で消した側を選べば消す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncAnswer {
    pub kind: SyncQuestionKind,
    pub record: SyncRecordRef,
    pub keep: SyncSide,
}
