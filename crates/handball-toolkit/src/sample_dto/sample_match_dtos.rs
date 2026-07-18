//! 移植元: アプリ層 `SampleMatches/V2/SampleMatchDTOsV2.swift`。
//!
//! V2 sample 配信 DTO（`/v2/` path のための schema）。`SAMPLE_DTO_V2.md` 準拠。
//!
//! V1 schema との主な差分:
//! - `MatchConfiguration` は tagged union（`kind` discriminator + sub-payload）
//! - 旧 `phaseRules` / `MatchPhase.timeline` は廃止（phase 情報は PhaseStart fact から再現）
//! - `ControlFact` は 2 case sum type（`phaseStart` / `stoppage`）
//! - `MatchClock` は試合通算累積秒（phase 内秒数ではない）
//!
//! tagged union は Swift 実装と同じ「`kind` 文字列 + optional 兄弟フィールド」のプレーン
//! struct で表現する（serde の enum タグ化はしない）。kind 判定と payload の突合は
//! converter 側で行い、未知値はデコード段階で落とさず `SampleMatchDecodeErrorV2` として
//! 構造化報告する Swift の責務分割を保存するため。
//!
//! serialize は Swift `SampleMatchEncoderV2`（JSONEncoder）の出力とバイト一致させる:
//! nil キー省略（`skip_serializing_if`）・UUID 大文字・秒未満切り捨て（`swift_wire`）を
//! 本ファイルで、整形（pretty / sortedKeys / `/` 非エスケープ）は `sample_match_encoder` が
//! 担う。バイト正は `tests/golden/export/`（ADR 0004 決定 2）。deserialize は従来どおり
//! 寛容（明示 null / UUID 小文字 / RFC 3339 を受理 — 実配信コーパスは python 生成で形式が違う）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `schemaVersion` の現行値。移植元: `SampleMatchSchemaVersionV2.current`。
pub const SCHEMA_VERSION_CURRENT: i64 = 2;

// ── Index ──

/// `/v2/index.json` トップレベル構造。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleIndexDtoV2 {
    pub schema_version: i64,
    pub matches: Vec<SampleMatchSummaryV2>,
}

/// `/v2/index.json` の `matches[]` 要素。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchSummaryV2 {
    pub slug: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub date: DateTime<Utc>,
    pub home_score: i64,
    pub away_score: i64,
    pub has_video: bool,
}

/// `/v2/highlights/index.json` トップレベル構造。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleHighlightIndexDtoV2 {
    pub schema_version: i64,
    pub highlights: Vec<SampleHighlightSummaryV2>,
}

/// `/v2/highlights/index.json` の `highlights[]` 要素。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleHighlightSummaryV2 {
    pub slug: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub date: DateTime<Utc>,
    pub home_team_name: String,
    pub away_team_name: String,
    pub fact_count: i64,
    pub has_video: bool,
}

// ── Match body ──

/// `/v2/matches/{slug}.json` および `/v2/highlights/{slug}.json` の本体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchDtoV2 {
    pub schema_version: i64,
    pub r#match: SampleMatchHeaderV2,
    pub teams: SampleTeamsDtoV2,
    pub facts: Vec<SampleFactDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchHeaderV2 {
    /// ユーザーが付けた試合タイトル。None 許容（`Match.title` が optional）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub date: DateTime<Utc>,
    pub configuration: SampleMatchConfigurationDtoV2,
}

// ── Teams / Players ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTeamsDtoV2 {
    pub home: SampleTeamDtoV2,
    pub away: SampleTeamDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTeamDtoV2 {
    pub key: String,
    pub name: String,
    pub players: Vec<SamplePlayerDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePlayerDtoV2 {
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jersey_number: Option<i64>,
}

// ── Configuration (tagged union) ──

/// `MatchConfiguration` の JSON 表現（tagged union pattern）。
///
/// `kind` discriminator で case を切り替え、対応する sub-payload struct を読む。
/// - `timer` → `timer: SampleTimerConfigurationDtoV2`
/// - `video` → `video: SampleVideoConfigurationDtoV2`
/// - `videoHighlight` → `videoHighlight: SampleVideoConfigurationDtoV2`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchConfigurationDtoV2 {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timer: Option<SampleTimerConfigurationDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<SampleVideoConfigurationDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_highlight: Option<SampleVideoConfigurationDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTimerConfigurationDtoV2 {
    pub phase_duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleVideoConfigurationDtoV2 {
    pub source: SampleVideoSourceDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleVideoSourceDtoV2 {
    /// `"youtube"` のみ。将来別 provider を追加するための文字列。
    pub provider: String,
    /// Swift 表記 `externalID` を保存（camelCase 自動変換は `externalId` になるため明示 rename）。
    #[serde(rename = "externalID")]
    pub external_id: String,
}

// ── Facts (tagged union) ──

/// 1 件の事実。payload は play / control の tagged union。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleFactDtoV2 {
    /// 永続化されている UUID（なければ converter 側で採番。ID 供給はシェル注入 — converter 参照）。
    #[serde(
        rename = "factID",
        default,
        with = "swift_wire::uuid_uppercase_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub fact_id: Option<Uuid>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub recorded_at: DateTime<Utc>,
    pub payload: SampleFactPayloadDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleFactPayloadDtoV2 {
    /// `"play"` | `"control"`
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play: Option<SamplePlayFactDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<SampleControlFactDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePlayFactDtoV2 {
    /// `PlayEventKind` raw value（`goal` / `shotMissed` / `freeNote` / `yellowCard` / `twoMinuteSuspension` / `redCard`）。
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_player_key: Option<String>,
    pub anchor: SampleFactAnchorDtoV2,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleControlFactDtoV2 {
    /// `"phaseStart"` | `"stoppage"`
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_start: Option<SamplePhaseStartPayloadDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stoppage: Option<SampleStoppagePayloadDtoV2>,
    /// 開始 anchor。end 情報は `anchor.end_match_elapsed_seconds` / `anchor.end_video_elapsed_seconds`。
    pub anchor: SampleFactAnchorDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePhaseStartPayloadDtoV2 {
    /// `PhaseKind` raw value（`regular` | `shootout`）。
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleStoppagePayloadDtoV2 {
    /// `StoppageKind` raw value（`timeout` | `pause`）。
    pub stoppage_kind: String,
    /// pause の自由記述（timeout では None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ── Anchor ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleFactAnchorDtoV2 {
    /// `FactAnchorKind` raw value（`matchClock` | `videoClock` | `both`）。
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_clock: Option<SampleMatchClockDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_clock: Option<SampleVideoClockDtoV2>,
    /// PhaseStart / Stoppage の end（range 末尾）。PlayFact では両方 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_match_elapsed_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_video_elapsed_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchClockDtoV2 {
    /// 試合通算 matchClock 累積秒数。
    pub elapsed_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleVideoClockDtoV2 {
    pub elapsed_seconds: f64,
}

// ── Swift JSONEncoder 互換の wire 表現 ──

/// Swift `JSONEncoder`（`.iso8601` / Codable の UUID encode）と一致する serialize と、
/// 従来どおり寛容な deserialize の組。バイト正は `tests/golden/export/`。
pub(crate) mod swift_wire {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    /// Swift `.iso8601` 相当: 秒未満は**切り捨て**（`.999` も丸め上げない）・`Z` 表記。
    pub(crate) mod iso8601_seconds {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &DateTime<Utc>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(&value.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<DateTime<Utc>, D::Error> {
            DateTime::<Utc>::deserialize(deserializer)
        }
    }

    /// Swift Codable の UUID encode 相当: 大文字ハイフン表記（デコードは大小不問）。
    pub(crate) mod uuid_uppercase_opt {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &Option<Uuid>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                Some(id) => serializer.serialize_str(&id.to_string().to_uppercase()),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Uuid>, D::Error> {
            Option::<Uuid>::deserialize(deserializer)
        }
    }
}

// ── Errors ──

/// DTO → domain 変換の失敗。移植元: `SampleMatchDecodeErrorV2`。
///
/// エラーコード + パラメータのみの構造化エラー（設計不変条件 3。文言はシェル所有）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SampleMatchDecodeErrorV2 {
    SchemaVersionMismatch { found: i64, expected: i64 },
    UnknownConfigurationKind(String),
    MissingConfigurationPayload(String),
    UnknownPayloadKind(String),
    MissingPayloadBody(String),
    UnknownPlayKind(String),
    UnknownControlKind(String),
    UnknownStoppageKind(String),
    UnknownPhaseKind(String),
    UnknownAnchorKind(String),
    UnknownVideoProvider(String),
    MissingAnchorBody(String),
    UnknownTeamKey(String),
    UnknownPlayerKey(String),
    MissingPhaseStartEnd,
}
