//! 移植元: `Projection/LiveMatchProjection.swift`。

use serde::{Deserialize, Serialize};

use crate::clock::{MatchClock, VideoClock};
use crate::configuration::PhaseKind;
use crate::entities::Match;

use super::segment_resolver::SegmentResolver;
use super::time_segment::{TimeSegment, TimeSegmentKind};
use super::timeline::TimelineProjection;

/// 試合の「現在の生きた状態」を表す projection。
///
/// 旧設計の `currentPhase: MatchPhase` は新設計では `current_phase_kind: Option<PhaseKind>` +
/// `current_phase_index: Option<usize>`（出現順、regular のみ）に分解。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct LiveMatchProjection {
    pub current_phase_kind: Option<PhaseKind>,
    pub current_phase_index: Option<usize>,
    pub timer_state: MatchTimerState,
    pub current_match_clock: Option<MatchClock>,
    pub available_actions: AvailableActions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum MatchTimerState {
    BeforeMatch,
    Playing,
    Timeout,
    Paused,
    BetweenPhases,
    Ended,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct AvailableActions {
    // uniffi(default) は移植元 Swift init のデフォルト引数（全 false）の保存。
    #[cfg_attr(feature = "uniffi", uniffi(default = false))]
    pub can_record_goal: bool,
    #[cfg_attr(feature = "uniffi", uniffi(default = false))]
    pub can_record_shot_missed: bool,
    #[cfg_attr(feature = "uniffi", uniffi(default = false))]
    pub can_record_free_note: bool,
    #[cfg_attr(feature = "uniffi", uniffi(default = false))]
    pub can_start_timeout: bool,
    #[cfg_attr(feature = "uniffi", uniffi(default = false))]
    pub can_resume: bool,
    #[cfg_attr(feature = "uniffi", uniffi(default = false))]
    pub can_start_next_phase: bool,
}

impl LiveMatchProjection {
    /// video mode の build。現在 videoClock を segment 上で lookup し、timerState / phase を決定する。
    /// Swift 版同様 `match` は未使用だが API 対称性のため引数に保持する（ADR 0001 関数目録）。
    ///
    /// 実体は `build_video_mode_with_resolver`。**この関数は元から `timeline.resolver` しか
    /// 読んでいない**ので、委譲しても導出結果は変わらない。
    pub fn build_video_mode(
        _match: &Match,
        timeline: &TimelineProjection,
        current_video_clock: Option<VideoClock>,
    ) -> LiveMatchProjection {
        Self::build_video_mode_with_resolver(&timeline.resolver, current_video_clock)
    }

    /// resolver だけを入力にした build（FFI の 2Hz tick 経路が使う — handball-project#167）。
    ///
    /// 上と結果は同一で、**分けてあるのは境界の転送量のため**。ドメイン上の違いは無い。
    /// `TimelineProjection` を record ごと FFI へ渡すと、resolver が object ハンドルでも
    /// 同居する `resolved_facts` が converter の write 順で全量マーシャリングされ、
    /// fact 列が毎 tick 境界を渡っていた。ハンドル 1 本だけを渡せばそれが消える。
    ///
    /// 一致は `live_match_projection_tests` の `resolver_entry_point_matches_timeline_entry_point`
    /// が見張る（2 経路が育って食い違うのを防ぐため）。
    pub fn build_video_mode_with_resolver(
        resolver: &SegmentResolver,
        current_video_clock: Option<VideoClock>,
    ) -> LiveMatchProjection {
        let Some(current_video_clock) = current_video_clock else {
            return LiveMatchProjection {
                current_phase_kind: None,
                current_phase_index: None,
                timer_state: MatchTimerState::BeforeMatch,
                current_match_clock: None,
                available_actions: available_actions_for(MatchTimerState::BeforeMatch),
            };
        };

        if let Some(segment) =
            resolver.segment_for_video_elapsed(current_video_clock.elapsed_seconds)
        {
            let match_clock = MatchClock {
                elapsed_seconds: segment
                    .match_elapsed_for_video_elapsed(current_video_clock.elapsed_seconds),
            };
            let timer_state = timer_state_for(segment);
            let phase_kind = segment
                .phase_kind
                .or_else(|| resolver.phase_kind(match_clock.elapsed_seconds));
            let phase_index = resolver.phase_index(match_clock.elapsed_seconds);

            return LiveMatchProjection {
                current_phase_kind: phase_kind,
                current_phase_index: phase_index,
                timer_state,
                current_match_clock: Some(match_clock),
                available_actions: available_actions_for(timer_state),
            };
        }

        // segment 外: phase 前 / phase 間 / 試合終了後 の 3 状態を判定する。
        let outside_state = position_outside_phases(resolver, current_video_clock);

        LiveMatchProjection {
            current_phase_kind: None,
            current_phase_index: None,
            timer_state: outside_state,
            current_match_clock: None,
            available_actions: available_actions_for(outside_state),
        }
    }
}

// ── 内部 helper ──

/// segment.kind / stoppage_kind から MatchTimerState を導出。
fn timer_state_for(segment: &TimeSegment) -> MatchTimerState {
    use crate::facts::StoppageKind;
    match segment.kind {
        TimeSegmentKind::Running => MatchTimerState::Playing,
        TimeSegmentKind::Stopped => match segment.stoppage_kind {
            Some(StoppageKind::Timeout) => MatchTimerState::Timeout,
            Some(StoppageKind::Pause) => MatchTimerState::Paused,
            None => MatchTimerState::Paused,
        },
    }
}

/// segment に含まれない videoClock の状態を判定。
/// - 全 phase の videoEnd より前（= 最初の phase より前 or phase 間）→ BeforeMatch / BetweenPhases
/// - 最後の phase の videoEnd 以降 → Ended
fn position_outside_phases(
    resolver: &SegmentResolver,
    current_video_clock: VideoClock,
) -> MatchTimerState {
    let phases_with_video: Vec<(f64, f64)> = resolver
        .phases
        .iter()
        .filter_map(|phase| Some((phase.video_elapsed_start?, phase.video_elapsed_end?)))
        .collect();
    if phases_with_video.is_empty() {
        return MatchTimerState::BeforeMatch;
    }

    let current_secs = current_video_clock.elapsed_seconds;
    let last_end = phases_with_video
        .iter()
        .map(|p| p.1)
        .max_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.0);
    let first_start = phases_with_video
        .iter()
        .map(|p| p.0)
        .min_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.0);

    if current_secs < first_start {
        return MatchTimerState::BeforeMatch;
    }
    if current_secs >= last_end {
        return MatchTimerState::Ended;
    }
    MatchTimerState::BetweenPhases
}

// ── actions ──

/// 状態ごとに「今どの操作ができるか」を返す。
///
/// play fact 3 種（goal / shotMissed / freeNote）は **`Playing` でのみ true** で、常に同値。
/// R7（phase range 外の単一 anchor fact 禁止）/ R8（Stoppage 区間内の単一 anchor fact 禁止）は
/// kind を問わず掛かる（`validators/fact_log_validator.rs`）ので、停止区間 / phase 間 /
/// 試合終了後に「記録できる」と返す play kind は存在しない。
///
/// 移植元 Swift は `can_record_free_note` を `Timeout` / `Paused` / `BetweenPhases` / `Ended` でも
/// true にしていた（R7 / R8 導入前の設計の取り残し）。コアが「できる」と言った操作をコア自身の
/// validation が拒否する自己矛盾だったため、完走後に validation 側へ寄せた
/// （handball-project#177。ADR 0001「移植完了後に意図的に変えた挙動」）。
fn available_actions_for(state: MatchTimerState) -> AvailableActions {
    match state {
        MatchTimerState::BeforeMatch => AvailableActions {
            can_start_next_phase: true,
            ..AvailableActions::default()
        },
        MatchTimerState::Playing => AvailableActions {
            can_record_goal: true,
            can_record_shot_missed: true,
            can_record_free_note: true,
            can_start_timeout: true,
            can_resume: false,
            can_start_next_phase: false,
        },
        MatchTimerState::Timeout | MatchTimerState::Paused => AvailableActions {
            can_resume: true,
            ..AvailableActions::default()
        },
        MatchTimerState::BetweenPhases | MatchTimerState::Ended => AvailableActions {
            can_start_next_phase: true,
            ..AvailableActions::default()
        },
    }
}
