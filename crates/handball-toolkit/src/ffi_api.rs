//! FFI 境界の関数公開（feature `uniffi` 時のみ — ADR 0004）。
//!
//! uniffi の生成物を 1 つの Swift モジュールにまとめるため、export 関数もこの crate の
//! namespace に置く（module_name を複数 namespace で共有すると生成ファイルが上書き衝突する）。
//! staticlib 化と bindgen CLI は handball-toolkit-ffi crate が担う。
//!
//! 方針（ADR 0001 関数目録 / ADR 0004 決定 1・4・5）:
//! - 入力は所有値で受けてコアの借用 API へ委譲する薄いラッパのみ。ロジックを書かない
//! - `SegmentResolver` だけ object ハンドル（構築 1 回・参照は self + スカラー）
//! - 自明なアクセサ（`FactAnchor.matchClock` 等）は公開しない — Swift シムが再実装する

use crate::clock::{MatchClock, VideoClock};
use crate::configuration::{MatchConfiguration, PhaseKind};
use crate::entities::Match;
use crate::facts::{ControlFact, MatchFact, PlayFact};
use crate::ids::FactId;
use crate::projection::{
    LiveMatchProjection, Phase, ScoreProgressionProjection, SegmentResolver, SummaryProjection,
    TimeSegment, TimelineProjection,
};
use crate::validation::DomainValidationIssue;
use crate::validators;
use crate::validators::RosterContext;

/// コアのバージョン文字列。FFI 疎通確認の最小関数。
#[uniffi::export]
pub fn toolkit_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── projection builders（fact 列 in → projection out — 設計不変条件 4）──

/// `TimelineProjection::build`。
#[uniffi::export]
pub fn build_timeline(match_: Match, facts: Vec<MatchFact>) -> TimelineProjection {
    TimelineProjection::build(&match_, &facts)
}

/// `SummaryProjection::build`（resolver 非依存。`phase_summaries` は空）。
#[uniffi::export]
pub fn build_summary(match_: Match, facts: Vec<MatchFact>) -> SummaryProjection {
    SummaryProjection::build(&match_, &facts)
}

/// `SummaryProjection::build_with_timeline`（timeline の resolver を再利用し phase 別 stats も算出）。
#[uniffi::export]
pub fn build_summary_with_timeline(
    match_: Match,
    timeline: TimelineProjection,
) -> SummaryProjection {
    SummaryProjection::build_with_timeline(&match_, &timeline)
}

/// `ScoreProgressionProjection::build`（facts から timeline を内部構築する convenience）。
#[uniffi::export]
pub fn build_score_progression(
    match_: Match,
    facts: Vec<MatchFact>,
) -> Option<ScoreProgressionProjection> {
    ScoreProgressionProjection::build(&match_, &facts)
}

/// `ScoreProgressionProjection::build_with_timeline`（resolver を二度作らない経路）。
#[uniffi::export]
pub fn build_score_progression_with_timeline(
    match_: Match,
    timeline: TimelineProjection,
) -> Option<ScoreProgressionProjection> {
    ScoreProgressionProjection::build_with_timeline(&match_, &timeline)
}

/// `LiveMatchProjection::build_video_mode`。2Hz tick 経路 — timeline の resolver は
/// object ハンドルのため facts の再マーシャリングは発生しない（ADR 0004 決定 5）。
#[uniffi::export]
pub fn build_live_match_video_mode(
    match_: Match,
    timeline: TimelineProjection,
    current_video_clock: Option<VideoClock>,
) -> LiveMatchProjection {
    LiveMatchProjection::build_video_mode(&match_, &timeline, current_video_clock)
}

// ── validators（ADR 0002: 非空 = blocking。文言はシェル所有）──

/// `validators::validate_match`。
#[uniffi::export]
pub fn validate_match(match_: Match) -> Vec<DomainValidationIssue> {
    validators::validate_match(&match_)
}

/// `validators::validate_configuration`。
#[uniffi::export]
pub fn validate_configuration(configuration: MatchConfiguration) -> Vec<DomainValidationIssue> {
    validators::validate_configuration(&configuration)
}

/// `validators::validate_fact_log`（R3-R9 / 連続性 / 重複の whole-log 検証）。
#[uniffi::export]
pub fn validate_fact_log(facts: Vec<MatchFact>, match_: Match) -> Vec<DomainValidationIssue> {
    validators::validate_fact_log(&facts, &match_)
}

/// `validators::validate_match_fact`（1 件の value/anchor/payload + 参照整合）。
#[uniffi::export]
pub fn validate_match_fact(
    fact: MatchFact,
    configuration: MatchConfiguration,
    roster: RosterContext,
) -> Vec<DomainValidationIssue> {
    validators::validate_match_fact(&fact, &configuration, &roster)
}

/// `validators::validate_play_fact`。
#[uniffi::export]
pub fn validate_play_fact(
    play: PlayFact,
    configuration: MatchConfiguration,
    roster: RosterContext,
) -> Vec<DomainValidationIssue> {
    validators::validate_play_fact(&play, &configuration, &roster)
}

/// `validators::validate_control_fact`。
#[uniffi::export]
pub fn validate_control_fact(
    control: ControlFact,
    configuration: MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    validators::validate_control_fact(&control, &configuration)
}

/// `validators::validate_append`（append 直前の集約 validation — 保存可否の単一窓口）。
#[uniffi::export]
pub fn validate_append(
    fact: MatchFact,
    existing_facts: Vec<MatchFact>,
    match_: Match,
    roster: Option<RosterContext>,
) -> Vec<DomainValidationIssue> {
    validators::validate_append(&fact, &existing_facts, &match_, roster.as_ref())
}

/// `validators::validate_update`。
#[uniffi::export]
pub fn validate_update(
    fact: MatchFact,
    existing_facts: Vec<MatchFact>,
    match_: Match,
    roster: Option<RosterContext>,
) -> Vec<DomainValidationIssue> {
    validators::validate_update(&fact, &existing_facts, &match_, roster.as_ref())
}

/// `validators::validate_delete`。
#[uniffi::export]
pub fn validate_delete(
    removed_fact_id: FactId,
    existing_facts: Vec<MatchFact>,
    match_: Match,
) -> Vec<DomainValidationIssue> {
    validators::validate_delete(removed_fact_id, &existing_facts, &match_)
}

// ── SegmentResolver（object ハンドル — ADR 0004 決定 5）──
//
// Rust のメソッド名は inherent impl と衝突しないよう suffix を付け、
// `name = ...` で FFI 上の名前（= 移植元 Swift API と同形）へ戻す。

#[uniffi::export]
impl SegmentResolver {
    /// `SegmentResolver::build`。構築は 1 回、以後の参照は self + スカラーのみ。
    #[uniffi::constructor(name = "build")]
    pub fn build_from_facts(facts: Vec<MatchFact>) -> Self {
        SegmentResolver::build(&facts)
    }

    /// フィールド `segments` の取得（object はフィールドを直接公開できない）。
    pub fn all_segments(&self) -> Vec<TimeSegment> {
        self.segments.clone()
    }

    /// フィールド `phases` の取得。
    pub fn all_phases(&self) -> Vec<Phase> {
        self.phases.clone()
    }

    /// `resolve_match_clock`（videoClock → matchClock）。
    #[uniffi::method(name = "resolve_match_clock")]
    pub fn resolve_match_clock_ffi(&self, video: VideoClock) -> Option<MatchClock> {
        self.resolve_match_clock(video)
    }

    /// `resolve_video_clock`（matchClock → videoClock）。
    #[uniffi::method(name = "resolve_video_clock")]
    pub fn resolve_video_clock_ffi(&self, match_clock: MatchClock) -> Option<VideoClock> {
        self.resolve_video_clock(match_clock)
    }

    /// `phase_kind`。
    #[uniffi::method(name = "phase_kind")]
    pub fn phase_kind_ffi(&self, match_elapsed_seconds: f64) -> Option<PhaseKind> {
        self.phase_kind(match_elapsed_seconds)
    }

    /// `phase_index`（regular のみカウント。shootout は None）。
    #[uniffi::method(name = "phase_index")]
    pub fn phase_index_ffi(&self, match_elapsed_seconds: f64) -> Option<usize> {
        self.phase_index(match_elapsed_seconds)
    }

    /// `phase_for_match_elapsed`（借用返しは所有値返しに変更 — ADR 0004 決定 4）。
    #[uniffi::method(name = "phase_for_match_elapsed")]
    pub fn phase_for_match_elapsed_ffi(&self, seconds: f64) -> Option<Phase> {
        self.phase_for_match_elapsed(seconds).cloned()
    }

    /// `segment_for_video_elapsed`（同上）。
    #[uniffi::method(name = "segment_for_video_elapsed")]
    pub fn segment_for_video_elapsed_ffi(&self, seconds: f64) -> Option<TimeSegment> {
        self.segment_for_video_elapsed(seconds).cloned()
    }

    /// `segment_for_match_elapsed`（同上。running 優先）。
    #[uniffi::method(name = "segment_for_match_elapsed")]
    pub fn segment_for_match_elapsed_ffi(&self, seconds: f64) -> Option<TimeSegment> {
        self.segment_for_match_elapsed(seconds).cloned()
    }
}
