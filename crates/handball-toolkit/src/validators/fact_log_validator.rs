//! 移植元: `Validators/FactLogValidator.swift`。
//!
//! fact log 全体に対する timeline validation。
//!
//! 役割:
//! - R3 / R5 / R6: configuration × PhaseStart 整合
//! - R7 / R8: play fact anchor の範囲（PhaseStart range の内 / Stoppage range の外）
//! - R9: configuration × Stoppage 整合
//! - R11: configuration × title 整合
//! - phase 順序 / 連続性（shootout 重複 / shootout 最後 / `Timer` regular 連続性）
//! - Stoppage 重複 / phase 外
//!
//! 個別 fact の value validation は `fact_validator` が担当する。

use chrono::{DateTime, Utc};

use crate::configuration::{MatchConfiguration, PhaseKind};
use crate::entities::Match;
use crate::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayFact, StoppagePayload,
};
use crate::ids::FactId;
use crate::validation::{DomainValidationIssue, TimelineValidationError};

/// `facts` は永続化順（cumulative seconds + recordedAt + id）で並んでいる前提。
/// `match_` は title チェック用。
pub fn validate_fact_log(facts: &[MatchFact], match_: &Match) -> Vec<DomainValidationIssue> {
    let configuration = &match_.configuration;
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    let phase_start_facts = extract_phase_start_facts(facts);
    let stoppage_facts = extract_stoppage_facts(facts);
    let play_facts = extract_play_facts(facts);

    // R3 / R5: timer/video + fact 1 件以上 + PhaseStart fact なし
    if phase_start_facts.is_empty() && !facts.is_empty() {
        match configuration {
            MatchConfiguration::Timer { .. } => issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::TimerWithFactsMissingPhaseStart,
            )),
            MatchConfiguration::Video(_) => issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::VideoWithFactsMissingPhaseStart,
            )),
            MatchConfiguration::VideoHighlight(_) => {}
        }
    }

    // R6: videoHighlight + PhaseStart fact あり
    if matches!(configuration, MatchConfiguration::VideoHighlight(_))
        && !phase_start_facts.is_empty()
    {
        issues.push(DomainValidationIssue::Timeline(
            TimelineValidationError::VideoHighlightContainsPhaseStart,
        ));
    }

    // R9: videoHighlight + Stoppage fact あり
    if matches!(configuration, MatchConfiguration::VideoHighlight(_)) && !stoppage_facts.is_empty()
    {
        issues.push(DomainValidationIssue::Timeline(
            TimelineValidationError::VideoHighlightContainsStoppage,
        ));
    }

    // R11: videoHighlight + title None/空文字
    if matches!(configuration, MatchConfiguration::VideoHighlight(_)) {
        let title_empty = match_
            .title
            .as_ref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true);
        if title_empty {
            issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::VideoHighlightMissingTitle,
            ));
        }
    }

    // shootout 重複 / 最後
    issues.extend(validate_shootout_position(&phase_start_facts));

    // Timer regular 連続性
    if matches!(configuration, MatchConfiguration::Timer { .. }) {
        issues.extend(validate_timer_phase_continuity(&phase_start_facts));
    }

    // Stoppage 重複
    issues.extend(validate_stoppages_overlap(&stoppage_facts, configuration));

    // Stoppage が phase 外
    issues.extend(validate_stoppage_inside_phase(
        &stoppage_facts,
        &phase_start_facts,
        configuration,
    ));

    // R7: play fact が PhaseStart range の外（Video のみ）
    if matches!(configuration, MatchConfiguration::Video(_)) {
        issues.extend(validate_play_inside_phase_range(
            &play_facts,
            &phase_start_facts,
        ));
    }

    // R8: play fact が Stoppage range の中（Video のみ、Timer は edge-case で未実装 — Swift 準拠）
    if matches!(configuration, MatchConfiguration::Video(_)) {
        issues.extend(validate_play_outside_stoppage(&play_facts, &stoppage_facts));
    }

    issues
}

// ── 内部 helper struct（fact を payload 別に取り出す）──

struct PhaseStartFact {
    #[allow(dead_code)]
    id: FactId,
    recorded_at: DateTime<Utc>,
    payload: PhaseStartPayload,
}

struct StoppageFact {
    #[allow(dead_code)]
    id: FactId,
    recorded_at: DateTime<Utc>,
    payload: StoppagePayload,
}

struct PlayMatchFact {
    #[allow(dead_code)]
    id: FactId,
    #[allow(dead_code)]
    recorded_at: DateTime<Utc>,
    payload: PlayFact,
}

// ── Fact 抽出 ──

fn extract_phase_start_facts(facts: &[MatchFact]) -> Vec<PhaseStartFact> {
    facts
        .iter()
        .filter_map(|fact| match &fact.payload {
            MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => Some(PhaseStartFact {
                id: fact.id,
                recorded_at: fact.recorded_at,
                payload: *payload,
            }),
            _ => None,
        })
        .collect()
}

fn extract_stoppage_facts(facts: &[MatchFact]) -> Vec<StoppageFact> {
    facts
        .iter()
        .filter_map(|fact| match &fact.payload {
            MatchFactPayload::Control(ControlFact::Stoppage(payload)) => Some(StoppageFact {
                id: fact.id,
                recorded_at: fact.recorded_at,
                payload: payload.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn extract_play_facts(facts: &[MatchFact]) -> Vec<PlayMatchFact> {
    facts
        .iter()
        .filter_map(|fact| match &fact.payload {
            MatchFactPayload::Play(play) => Some(PlayMatchFact {
                id: fact.id,
                recorded_at: fact.recorded_at,
                payload: play.clone(),
            }),
            _ => None,
        })
        .collect()
}

// ── shootout ──

fn validate_shootout_position(phase_start_facts: &[PhaseStartFact]) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    let sorted = sort_phase_start_facts_by_primary(phase_start_facts);
    let shootout_indices: Vec<usize> = sorted
        .iter()
        .enumerate()
        .filter_map(|(idx, fact)| (fact.payload.kind == PhaseKind::Shootout).then_some(idx))
        .collect();

    if shootout_indices.len() > 1 {
        issues.push(DomainValidationIssue::Timeline(
            TimelineValidationError::DuplicateShootout,
        ));
    }

    if let Some(&first_shootout_idx) = shootout_indices.first() {
        let has_regular_after = sorted[(first_shootout_idx + 1)..]
            .iter()
            .any(|fact| fact.payload.kind == PhaseKind::Regular);
        if has_regular_after {
            issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::ShootoutNotLast,
            ));
        }
    }

    issues
}

// ── Timer regular 連続性 ──

fn validate_timer_phase_continuity(
    phase_start_facts: &[PhaseStartFact],
) -> Vec<DomainValidationIssue> {
    let sorted = sort_phase_start_facts_by_primary(phase_start_facts);
    let mut issues: Vec<DomainValidationIssue> = Vec::new();
    let mut previous_regular: Option<&PhaseStartFact> = None;

    for fact in sorted
        .iter()
        .filter(|f| f.payload.kind == PhaseKind::Regular)
    {
        if let Some(prev) = previous_regular {
            let prev_end = prev.payload.end_anchor.match_elapsed_seconds();
            let cur_start = fact.payload.start_anchor.match_elapsed_seconds();
            if let (Some(prev_end), Some(cur_start)) = (prev_end, cur_start)
                && prev_end != cur_start
            {
                issues.push(DomainValidationIssue::Timeline(
                    TimelineValidationError::PhaseStartNotContinuousFromPrevious,
                ));
            }
        }
        previous_regular = Some(fact);
    }

    issues
}

// ── Stoppage 重複 ──

fn validate_stoppages_overlap(
    stoppage_facts: &[StoppageFact],
    configuration: &MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    // Stoppage 重複は range が必要 = endAnchor あり前提（Video）。
    // Timer は Stoppage end が None なので「重複判定」自体が成り立たない。
    if !matches!(configuration, MatchConfiguration::Video(_)) {
        return Vec::new();
    }

    let mut sorted: Vec<&StoppageFact> = stoppage_facts.iter().collect();
    sorted.sort_by(|a, b| {
        let a_start = a
            .payload
            .start_anchor
            .video_elapsed_seconds()
            .unwrap_or(0.0);
        let b_start = b
            .payload
            .start_anchor
            .video_elapsed_seconds()
            .unwrap_or(0.0);
        a_start
            .total_cmp(&b_start)
            .then_with(|| a.recorded_at.cmp(&b.recorded_at))
    });

    for i in 0..sorted.len() {
        for j in (i + 1)..sorted.len() {
            if has_video_overlap(&sorted[i].payload, &sorted[j].payload) {
                return vec![DomainValidationIssue::Timeline(
                    TimelineValidationError::StoppagesOverlap,
                )];
            }
        }
    }

    Vec::new()
}

fn has_video_overlap(a: &StoppagePayload, b: &StoppagePayload) -> bool {
    let (Some(a_start), Some(a_end), Some(b_start), Some(b_end)) = (
        a.start_anchor.video_elapsed_seconds(),
        a.end_anchor
            .and_then(|anchor| anchor.video_elapsed_seconds()),
        b.start_anchor.video_elapsed_seconds(),
        b.end_anchor
            .and_then(|anchor| anchor.video_elapsed_seconds()),
    ) else {
        return false;
    };
    a_start < b_end && b_start < a_end
}

// ── Stoppage が phase 内 ──

fn validate_stoppage_inside_phase(
    stoppage_facts: &[StoppageFact],
    phase_start_facts: &[PhaseStartFact],
    configuration: &MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    if !matches!(configuration, MatchConfiguration::Video(_)) {
        return Vec::new();
    }

    let mut issues: Vec<DomainValidationIssue> = Vec::new();
    for stoppage in stoppage_facts {
        let (Some(stoppage_start), Some(stoppage_end)) = (
            stoppage.payload.start_anchor.video_elapsed_seconds(),
            stoppage
                .payload
                .end_anchor
                .and_then(|anchor| anchor.video_elapsed_seconds()),
        ) else {
            continue;
        };
        let inside_phase = phase_start_facts.iter().any(|phase| {
            let (Some(phase_start), Some(phase_end)) = (
                phase.payload.start_anchor.video_elapsed_seconds(),
                phase.payload.end_anchor.video_elapsed_seconds(),
            ) else {
                return false;
            };
            stoppage_start >= phase_start && stoppage_end <= phase_end
        });
        if !inside_phase {
            issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::StoppageOutsidePhaseRange,
            ));
            return issues; // 1 件出れば返す（連続発火を抑制）
        }
    }
    issues
}

// ── R7: play fact が PhaseStart range の内 ──

fn validate_play_inside_phase_range(
    play_facts: &[PlayMatchFact],
    phase_start_facts: &[PhaseStartFact],
) -> Vec<DomainValidationIssue> {
    if phase_start_facts.is_empty() {
        return Vec::new();
    }

    let mut issues: Vec<DomainValidationIssue> = Vec::new();
    let mut reported = false;
    for play in play_facts {
        let Some(play_seconds) = play.payload.anchor.video_elapsed_seconds() else {
            continue;
        };
        let inside_some = phase_start_facts.iter().any(|phase| {
            let (Some(phase_start), Some(phase_end)) = (
                phase.payload.start_anchor.video_elapsed_seconds(),
                phase.payload.end_anchor.video_elapsed_seconds(),
            ) else {
                return false;
            };
            play_seconds >= phase_start && play_seconds <= phase_end
        });
        if !inside_some && !reported {
            issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::PlayRecordedOutsidePhaseRange { kind: None },
            ));
            reported = true;
        }
    }
    issues
}

// ── R8: play fact が Stoppage range の外 ──

fn validate_play_outside_stoppage(
    play_facts: &[PlayMatchFact],
    stoppage_facts: &[StoppageFact],
) -> Vec<DomainValidationIssue> {
    if stoppage_facts.is_empty() {
        return Vec::new();
    }

    let mut issues: Vec<DomainValidationIssue> = Vec::new();
    let mut reported = false;
    for play in play_facts {
        let Some(play_seconds) = play.payload.anchor.video_elapsed_seconds() else {
            continue;
        };
        let in_some = stoppage_facts.iter().any(|stoppage| {
            let (Some(stoppage_start), Some(stoppage_end)) = (
                stoppage.payload.start_anchor.video_elapsed_seconds(),
                stoppage
                    .payload
                    .end_anchor
                    .and_then(|anchor| anchor.video_elapsed_seconds()),
            ) else {
                return false;
            };
            play_seconds > stoppage_start && play_seconds < stoppage_end
        });
        if in_some && !reported {
            issues.push(DomainValidationIssue::Timeline(
                TimelineValidationError::PlayRecordedInsideStoppage,
            ));
            reported = true;
        }
    }
    issues
}

// ── PhaseStart sort ──

fn sort_phase_start_facts_by_primary(facts: &[PhaseStartFact]) -> Vec<&PhaseStartFact> {
    let mut sorted: Vec<&PhaseStartFact> = facts.iter().collect();
    sorted.sort_by(|a, b| {
        let a_sec = a
            .payload
            .start_anchor
            .match_elapsed_seconds()
            .or(a.payload.start_anchor.video_elapsed_seconds())
            .unwrap_or(0.0);
        let b_sec = b
            .payload
            .start_anchor
            .match_elapsed_seconds()
            .or(b.payload.start_anchor.video_elapsed_seconds())
            .unwrap_or(0.0);
        a_sec
            .total_cmp(&b_sec)
            .then_with(|| a.recorded_at.cmp(&b.recorded_at))
    });
    sorted
}
