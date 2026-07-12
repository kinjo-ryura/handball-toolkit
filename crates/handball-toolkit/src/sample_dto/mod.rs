//! Sample 配信 JSON（SAMPLE_DTO_V2 schema）の型付き実装。移植元: アプリ層 `SampleMatches/V2/`。
//!
//! handball-sample-matches の `/v2/` パスで配信される試合 JSON の serde 型と
//! domain 型への converter を提供する（ADR 0003 §2 — パリティ検証のコーパス取込経路であり、
//! 将来の JSON 検証 CLI・wasm デモの土台）。
//!
//! **依存は `sample_dto` → domain 型の一方通行を厳守**（domain 側から本モジュールを参照
//! しない）。将来「パーサ抜きでコアだけ使いたい」需要が実在したときの crate 分離を
//! 機械作業に保つため（ADR 0003）。

mod sample_match_dtos;

pub use sample_match_dtos::{
    SCHEMA_VERSION_CURRENT, SampleControlFactDtoV2, SampleFactAnchorDtoV2, SampleFactDtoV2,
    SampleFactPayloadDtoV2, SampleHighlightIndexDtoV2, SampleHighlightSummaryV2, SampleIndexDtoV2,
    SampleMatchClockDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDecodeErrorV2,
    SampleMatchDtoV2, SampleMatchHeaderV2, SampleMatchSummaryV2, SamplePhaseStartPayloadDtoV2,
    SamplePlayFactDtoV2, SamplePlayerDtoV2, SampleStoppagePayloadDtoV2, SampleTeamDtoV2,
    SampleTeamsDtoV2, SampleTimerConfigurationDtoV2, SampleVideoClockDtoV2,
    SampleVideoConfigurationDtoV2, SampleVideoSourceDtoV2,
};
