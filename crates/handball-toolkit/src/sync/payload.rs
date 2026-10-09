//! 同期で端末の間を運ぶ形（ADR 0007 決定 6）。
//!
//! 中身は [`SyncSnapshot`] をそのまま JSON にしたもの（コアの型の serde 表現）。**試合ファイル
//! （SAMPLE_DTO_V2）とは別の形**で、旧版のアプリが読める必要は無い — 同期は両方の端末が同じ
//! 版の形を持つときにしか起きない。そのかわり、最初に `format_version` を比べ、違えば中身を
//! 読まずに [`SyncPayloadError::UnsupportedFormatVersion`] を返す（シェルは「相手の端末の
//! アプリを更新してください」と出せる）。
//!
//! **形を変えたら `SYNC_FORMAT_VERSION` を上げる**。コアの型（`Match` / `MatchFact` 等）の
//! serde 表現もこの形の一部なので、それらの破壊的変更でも上げる。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::SyncSnapshot;

/// 今の形の版。
pub const SYNC_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncPayload {
    pub format_version: u32,
    /// 送った端末の ID（その端末が初回起動時に作って持つ）。相手の表示と、自分自身と
    /// つながっていないかの確認にだけ使う。記録ごとには持たない。
    pub device_id: Uuid,
    pub snapshot: SyncSnapshot,
}

/// 運ぶ形を読み書きできなかった。
///
/// **診断文字列のフィールド名を `message` にしない**（Kotlin で `Throwable.message` と衝突する —
/// `CoreWriteError` の doc）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum SyncPayloadError {
    /// JSON として読めない、または形が合わない（serde の診断文字列を添付）。
    InvalidJson { detail: String },
    /// 版が違う。`found` が `supported` より新しければこの端末、古ければ相手の端末のアプリが古い。
    UnsupportedFormatVersion { found: u32, supported: u32 },
    /// JSON に書けなかった（数値に表せない値など。実際には来ない想定の安全網）。
    EncodeFailed { detail: String },
}

// uniffi::Error が要求する Display。開発者向けの診断のみ（文言はシェルが持つ — ADR 0002 決定 5）。
impl std::fmt::Display for SyncPayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// 運ぶ形の JSON にする。
pub fn encode_sync_payload(payload: &SyncPayload) -> Result<String, SyncPayloadError> {
    serde_json::to_string(payload).map_err(|error| SyncPayloadError::EncodeFailed {
        detail: error.to_string(),
    })
}

/// 運ぶ形の JSON を読む。版を先に見て、違えば中身を読まない。
pub fn decode_sync_payload(json: &str) -> Result<SyncPayload, SyncPayloadError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        format_version: u32,
    }

    let header: Header = serde_json::from_str(json).map_err(invalid_json)?;
    if header.format_version != SYNC_FORMAT_VERSION {
        return Err(SyncPayloadError::UnsupportedFormatVersion {
            found: header.format_version,
            supported: SYNC_FORMAT_VERSION,
        });
    }
    serde_json::from_str(json).map_err(invalid_json)
}

fn invalid_json(error: serde_json::Error) -> SyncPayloadError {
    SyncPayloadError::InvalidJson {
        detail: error.to_string(),
    }
}
