//! 移植元: `Projection/SegmentResolver.swift`。

use serde::{Deserialize, Serialize};

use crate::clock::{FactAnchor, MatchClock, VideoClock};
use crate::configuration::PhaseKind;
use crate::facts::{ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, StoppagePayload};
use crate::ids::FactId;

use super::time_segment::{TimeSegment, TimeSegmentKind};

/// fact log から time segment を構築し、video↔match の時刻変換 / phase 逆引きを提供する。
///
/// 新原則:
/// - 累積秒ベース（`MatchClock` から `phase` 削除）
/// - PhaseStart fact 自身が `[start, end]` を持つ（range projection で phase 逆引き）
/// - Stoppage fact（video mode は range、timer mode は marker）で running segment を carve
/// - `Both` anchor は強制 re-anchor（baseline rolling forward）
/// - shootout は degenerate（matchClock 累積秒は phase 開始値で固定、videoClock のみ進行）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
#[serde(rename_all = "camelCase")]
pub struct SegmentResolver {
    pub segments: Vec<TimeSegment>,
    /// PhaseStart fact から作成された phase 情報（出現順）。
    pub phases: Vec<Phase>,
}

/// Swift の nested type `SegmentResolver.Phase`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    pub fact_id: FactId,
    pub kind: PhaseKind,
    pub match_elapsed_start: Option<f64>,
    pub match_elapsed_end: Option<f64>,
    pub video_elapsed_start: Option<f64>,
    pub video_elapsed_end: Option<f64>,
}

impl SegmentResolver {
    // ── 構築 ──

    pub fn build(facts: &[MatchFact]) -> SegmentResolver {
        let phase_starts = extract_phase_starts(facts);
        let stoppages = extract_stoppages(facts);
        let mut sorted_phases = phase_starts;
        sorted_phases.sort_by(|lhs, rhs| {
            primary_seconds(lhs.payload.start_anchor)
                .total_cmp(&primary_seconds(rhs.payload.start_anchor))
        });
        build_rolling(&sorted_phases, &stoppages)
    }

    // ── 時刻変換 ──

    /// videoClock → matchClock 変換。segment ベースで lookup する。
    pub fn resolve_match_clock(&self, video: VideoClock) -> Option<MatchClock> {
        let segment = self.segment_for_video_elapsed(video.elapsed_seconds)?;
        Some(MatchClock {
            elapsed_seconds: segment.match_elapsed_for_video_elapsed(video.elapsed_seconds),
        })
    }

    /// matchClock → videoClock 変換。segment ベースで lookup する。
    pub fn resolve_video_clock(&self, match_clock: MatchClock) -> Option<VideoClock> {
        // running segment 優先（stopped は同一 matchClock を複数 video 位置に持つので曖昧）。
        if let Some(segment) = self.segments.iter().find(|s| {
            s.kind == TimeSegmentKind::Running
                && s.contains_match_elapsed(match_clock.elapsed_seconds)
        }) && let Some(v) = segment.video_elapsed_for_match_elapsed(match_clock.elapsed_seconds)
        {
            return Some(VideoClock { elapsed_seconds: v });
        }
        if let Some(segment) = self.segments.iter().find(|s| {
            s.kind == TimeSegmentKind::Stopped
                && s.contains_match_elapsed(match_clock.elapsed_seconds)
        }) && let Some(v) = segment.video_elapsed_for_match_elapsed(match_clock.elapsed_seconds)
        {
            return Some(VideoClock { elapsed_seconds: v });
        }
        None
    }

    // ── Phase 逆引き ──

    /// matchClock 累積秒からその時点の PhaseKind を返す。
    /// phase 境界（前 phase end = 次 phase start）では出現順で最初にヒットした phase を返す。
    pub fn phase_kind(&self, match_elapsed_seconds: f64) -> Option<PhaseKind> {
        self.phase_for_match_elapsed(match_elapsed_seconds)
            .map(|phase| phase.kind)
    }

    /// matchClock 累積秒からその時点の phase index（出現順、regular のみカウント）を返す。
    /// shootout の場合は None。
    pub fn phase_index(&self, match_elapsed_seconds: f64) -> Option<usize> {
        let mut regular_index = 0;
        for phase in &self.phases {
            let Some(m_start) = phase.match_elapsed_start else {
                continue;
            };
            let Some(m_end) = phase.match_elapsed_end else {
                continue;
            };
            if contains_match_clock(match_elapsed_seconds, m_start, m_end) {
                return if phase.kind == PhaseKind::Regular {
                    Some(regular_index)
                } else {
                    None
                };
            }
            if phase.kind == PhaseKind::Regular {
                regular_index += 1;
            }
        }
        None
    }

    /// matchClock 累積秒からその時点の Phase を返す。
    pub fn phase_for_match_elapsed(&self, seconds: f64) -> Option<&Phase> {
        self.phases.iter().find(|phase| {
            let (Some(m_start), Some(m_end)) = (phase.match_elapsed_start, phase.match_elapsed_end)
            else {
                return false;
            };
            contains_match_clock(seconds, m_start, m_end)
        })
    }

    // ── Segment lookup ──

    /// videoClock 累積秒からそれを含む segment を返す。
    pub fn segment_for_video_elapsed(&self, seconds: f64) -> Option<&TimeSegment> {
        self.segments
            .iter()
            .find(|s| s.contains_video_elapsed(seconds))
    }

    /// matchClock 累積秒からそれを含む segment を返す（running 優先）。
    pub fn segment_for_match_elapsed(&self, seconds: f64) -> Option<&TimeSegment> {
        if let Some(running) = self
            .segments
            .iter()
            .find(|s| s.kind == TimeSegmentKind::Running && s.contains_match_elapsed(seconds))
        {
            return Some(running);
        }
        self.segments
            .iter()
            .find(|s| s.kind == TimeSegmentKind::Stopped && s.contains_match_elapsed(seconds))
    }
}

/// 半開区間 `[start, end)` の判定。ただし degenerate（start == end）の場合は単一点として一致を許す。
fn contains_match_clock(value: f64, start: f64, end: f64) -> bool {
    if start == end {
        return value == start;
    }
    value >= start && value < end
}

// ── Build internals ──

struct PhaseEntry {
    fact_id: FactId,
    payload: PhaseStartPayload,
}

struct StoppageEntry {
    fact_id: FactId,
    payload: StoppagePayload,
}

fn extract_phase_starts(facts: &[MatchFact]) -> Vec<PhaseEntry> {
    facts
        .iter()
        .filter_map(|fact| match &fact.payload {
            MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => Some(PhaseEntry {
                fact_id: fact.id,
                payload: *payload,
            }),
            _ => None,
        })
        .collect()
}

fn extract_stoppages(facts: &[MatchFact]) -> Vec<StoppageEntry> {
    facts
        .iter()
        .filter_map(|fact| match &fact.payload {
            MatchFactPayload::Control(ControlFact::Stoppage(payload)) => Some(StoppageEntry {
                fact_id: fact.id,
                payload: payload.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn primary_seconds(anchor: FactAnchor) -> f64 {
    anchor
        .video_elapsed_seconds()
        .or(anchor.match_elapsed_seconds())
        .unwrap_or(0.0)
}

/// PhaseStart を順に走査しつつ、各 phase の matchClock baseline を rolling forward で導出する。
fn build_rolling(sorted_phases: &[PhaseEntry], all_stoppages: &[StoppageEntry]) -> SegmentResolver {
    let mut phase_entries: Vec<Phase> = Vec::new();
    let mut segment_list: Vec<TimeSegment> = Vec::new();
    let mut rolling_match: f64 = 0.0;

    for entry in sorted_phases {
        let payload = entry.payload;
        let fact_id = entry.fact_id;

        // matchClock baseline: anchor が明示していれば override、なければ前 phase の end を継承。
        let match_start = match payload.start_anchor.match_elapsed_seconds() {
            Some(explicit) => explicit,
            None => rolling_match,
        };

        let video_start = payload.start_anchor.video_elapsed_seconds();
        let video_end = payload.end_anchor.video_elapsed_seconds();

        // 現 phase 内に入る stoppage を抽出。PhaseStart の primary clock 軸で in-range 判定する。
        let contained_stoppages =
            stoppages_contained(&payload, match_start, video_start, video_end, all_stoppages);

        // matchClock end:
        // 1. endAnchor が明示していれば override
        // 2. shootout は degenerate（matchStart 固定）
        // 3. video mode（start/end どちらも videoClock がある）は running 区間累積で算出
        // 4. 上記いずれでもなければ matchStart にフォールバック
        let match_end = if let Some(explicit) = payload.end_anchor.match_elapsed_seconds() {
            explicit
        } else if payload.kind == PhaseKind::Shootout {
            match_start
        } else if let (Some(v_s), Some(v_e)) = (video_start, video_end) {
            let running_duration = total_running_duration((v_s, v_e), &contained_stoppages);
            match_start + running_duration
        } else {
            match_start
        };

        phase_entries.push(Phase {
            fact_id,
            kind: payload.kind,
            match_elapsed_start: Some(match_start),
            match_elapsed_end: Some(match_end),
            video_elapsed_start: video_start,
            video_elapsed_end: video_end,
        });

        segment_list.extend(build_segments(
            fact_id,
            payload.kind,
            match_start,
            match_end,
            video_start,
            video_end,
            &contained_stoppages,
        ));

        rolling_match = match_end;
    }

    SegmentResolver {
        segments: segment_list,
        phases: phase_entries,
    }
}

/// 1 phase 内の stoppage を抽出 + start 順 sort。
/// phase が videoClock を持つ場合は videoClock で in-range 判定、持たない場合は matchClock。
fn stoppages_contained<'a>(
    payload: &PhaseStartPayload,
    phase_start: f64,
    video_start: Option<f64>,
    video_end: Option<f64>,
    stoppages: &'a [StoppageEntry],
) -> Vec<&'a StoppageEntry> {
    if let (Some(v_s), Some(v_e)) = (video_start, video_end) {
        let mut in_range: Vec<(&StoppageEntry, f64)> = stoppages
            .iter()
            .filter_map(|entry| {
                let s = entry.payload.start_anchor.video_elapsed_seconds()?;
                if s >= v_s && s < v_e {
                    Some((entry, s))
                } else {
                    None
                }
            })
            .collect();
        in_range.sort_by(|a, b| a.1.total_cmp(&b.1));
        return in_range.into_iter().map(|(entry, _)| entry).collect();
    }
    // timer mode: matchClock 軸の in-range 判定。
    let Some(m_end) = payload.end_anchor.match_elapsed_seconds() else {
        return Vec::new();
    };
    let mut in_range: Vec<(&StoppageEntry, f64)> = stoppages
        .iter()
        .filter_map(|entry| {
            let s = entry.payload.start_anchor.match_elapsed_seconds()?;
            if s >= phase_start && s < m_end {
                Some((entry, s))
            } else {
                None
            }
        })
        .collect();
    in_range.sort_by(|a, b| a.1.total_cmp(&b.1));
    in_range.into_iter().map(|(entry, _)| entry).collect()
}

/// video mode の phase 内、stoppage を除いた running 区間の合計時間を算出。
fn total_running_duration(phase_range: (f64, f64), stoppages: &[&StoppageEntry]) -> f64 {
    let (v_start, v_end) = phase_range;
    let total = v_end - v_start;
    let stopped = stoppages.iter().fold(0.0, |acc, entry| {
        let (Some(s), Some(e)) = (
            entry.payload.start_anchor.video_elapsed_seconds(),
            entry
                .payload
                .end_anchor
                .and_then(|anchor| anchor.video_elapsed_seconds()),
        ) else {
            return acc;
        };
        let clamped_start = s.max(v_start);
        let clamped_end = e.min(v_end);
        acc + (clamped_end - clamped_start).max(0.0)
    });
    (total - stopped).max(0.0)
}

fn build_segments(
    phase_fact_id: FactId,
    kind: PhaseKind,
    match_start: f64,
    match_end: f64,
    video_start: Option<f64>,
    video_end: Option<f64>,
    stoppages: &[&StoppageEntry],
) -> Vec<TimeSegment> {
    // shootout: degenerate（matchClock 固定、videoClock のみ進行）。
    if kind == PhaseKind::Shootout {
        return vec![TimeSegment {
            kind: TimeSegmentKind::Running,
            phase_kind: Some(PhaseKind::Shootout),
            match_elapsed_start: match_start,
            match_elapsed_end: Some(match_start),
            video_elapsed_start: video_start,
            video_elapsed_end: video_end,
            start_fact_id: Some(phase_fact_id),
            end_fact_id: Some(phase_fact_id),
            stoppage_kind: None,
        }];
    }

    // video mode: stoppage で running 区間を carve する。
    if let (Some(v_s), Some(v_e)) = (video_start, video_end) {
        return carve_regular_phase_segments(phase_fact_id, kind, match_start, v_s, v_e, stoppages);
    }

    // timer mode: 単一 running segment、video 情報なし。
    // stoppage はタイマーモードでは marker（endAnchor nil）のため segment 化しない。
    vec![TimeSegment {
        kind: TimeSegmentKind::Running,
        phase_kind: Some(kind),
        match_elapsed_start: match_start,
        match_elapsed_end: Some(match_end),
        video_elapsed_start: None,
        video_elapsed_end: None,
        start_fact_id: Some(phase_fact_id),
        end_fact_id: Some(phase_fact_id),
        stoppage_kind: None,
    }]
}

fn carve_regular_phase_segments(
    phase_fact_id: FactId,
    kind: PhaseKind,
    match_start: f64,
    video_start: f64,
    video_end: f64,
    stoppages: &[&StoppageEntry],
) -> Vec<TimeSegment> {
    let mut result: Vec<TimeSegment> = Vec::new();
    let mut cursor_match = match_start;
    let mut cursor_video = video_start;
    let mut prev_fact_id = phase_fact_id;

    for stoppage in stoppages {
        let Some(s_start) = stoppage.payload.start_anchor.video_elapsed_seconds() else {
            continue;
        };
        let s_end = stoppage
            .payload
            .end_anchor
            .and_then(|anchor| anchor.video_elapsed_seconds())
            .unwrap_or(s_start);
        let clamped_start = s_start.max(cursor_video);
        let clamped_end = s_end.min(video_end);

        if clamped_start > cursor_video {
            // running segment from cursor to stoppage start
            let duration = clamped_start - cursor_video;
            result.push(TimeSegment {
                kind: TimeSegmentKind::Running,
                phase_kind: Some(kind),
                match_elapsed_start: cursor_match,
                match_elapsed_end: Some(cursor_match + duration),
                video_elapsed_start: Some(cursor_video),
                video_elapsed_end: Some(clamped_start),
                start_fact_id: Some(prev_fact_id),
                end_fact_id: Some(stoppage.fact_id),
                stoppage_kind: None,
            });
            cursor_match += duration;
            cursor_video = clamped_start;
        }

        if clamped_end > clamped_start {
            // stopped segment（video 軸では duration あり、match 軸では固定）
            result.push(TimeSegment {
                kind: TimeSegmentKind::Stopped,
                phase_kind: Some(kind),
                match_elapsed_start: cursor_match,
                match_elapsed_end: Some(cursor_match),
                video_elapsed_start: Some(clamped_start),
                video_elapsed_end: Some(clamped_end),
                start_fact_id: Some(stoppage.fact_id),
                end_fact_id: Some(stoppage.fact_id),
                stoppage_kind: Some(stoppage.payload.kind),
            });
            cursor_video = clamped_end;
            prev_fact_id = stoppage.fact_id;
        }
    }

    if video_end > cursor_video {
        let duration = video_end - cursor_video;
        result.push(TimeSegment {
            kind: TimeSegmentKind::Running,
            phase_kind: Some(kind),
            match_elapsed_start: cursor_match,
            match_elapsed_end: Some(cursor_match + duration),
            video_elapsed_start: Some(cursor_video),
            video_elapsed_end: Some(video_end),
            start_fact_id: Some(prev_fact_id),
            end_fact_id: Some(phase_fact_id),
            stoppage_kind: None,
        });
    }

    result
}
