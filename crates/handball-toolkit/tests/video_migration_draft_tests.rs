//! 移行ウィザードの事前検証の挙動固定（handball-project#68 / #351）。
//!
//! 移植元 `VideoModeMigrationValidator`（Swift）とそのテスト
//! `MigrateToVideoStoreTests` の Validator 節を同セマンティクスで固定する。
//! 文言・wizard step への写像はシェル所有のため対象外。
//!
//! 入口は 2 本ある。移行元として妥当かは `video_migration_source_state`（fact 列が要るので
//! 読み込み時に 1 回）、draft の入力内容は `validate_video_migration_draft`（入力のたび）。

use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
};
use handball_toolkit::ids::{FactId, PlayerId};
use handball_toolkit::write::{
    VideoMigrationDraftIssue, VideoSyncDraftInput, validate_video_migration_draft,
    video_migration_source_state,
};
use uuid::Uuid;

fn timer_configuration() -> MatchConfiguration {
    MatchConfiguration::Timer {
        phase_duration_seconds: 1800.0,
    }
}

fn video_source() -> VideoSource {
    VideoSource {
        provider: VideoProvider::Youtube,
        external_id: "abc".to_string(),
    }
}

fn sync(id: u128, start: Option<f64>, end: Option<f64>) -> VideoSyncDraftInput {
    VideoSyncDraftInput {
        fact_id: FactId(Uuid::from_u128(id)),
        video_start_seconds: start,
        video_end_seconds: end,
    }
}

// ── 移行元として妥当か（handball-project#351）──

fn phase_start(id: u128, start_anchor: FactAnchor, end_anchor: FactAnchor) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: chrono::DateTime::from_timestamp(1, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor,
            end_anchor,
        })),
    }
}

fn goal(id: u128, anchor: FactAnchor) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: chrono::DateTime::from_timestamp(10, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: None,
            player_id: Some(PlayerId(Uuid::from_u128(50))),
            related_player_id: None,
            anchor,
            title: None,
            note: None,
        }),
    }
}

fn match_clock(seconds: f64) -> FactAnchor {
    FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: seconds,
    })
}

fn video_clock(seconds: f64) -> FactAnchor {
    FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: seconds,
    })
}

fn both(match_seconds: f64, video_seconds: f64) -> FactAnchor {
    FactAnchor::Both {
        match_clock: MatchClock {
            elapsed_seconds: match_seconds,
        },
        video_clock: VideoClock {
            elapsed_seconds: video_seconds,
        },
    }
}

#[test]
fn timer_試合は常に移行元として使える() {
    let facts = vec![
        phase_start(1, match_clock(0.0), match_clock(1800.0)),
        goal(2, match_clock(600.0)),
    ];
    let state = video_migration_source_state(&timer_configuration(), &facts);
    assert_eq!(state.issue, None);
    assert_eq!(state.unsynced_fact_count, 0);
    assert_eq!(state.video_anchored_fact_count, 0);
}

#[test]
fn 移行が済んだ_video_試合は_source_configuration_not_timer() {
    let facts = vec![
        phase_start(1, both(0.0, 720.0), both(1800.0, 2520.0)),
        goal(2, video_clock(1320.0)),
    ];
    let state = video_migration_source_state(&MatchConfiguration::Video(video_source()), &facts);
    assert_eq!(
        state.issue,
        Some(VideoMigrationDraftIssue::SourceConfigurationNotTimer)
    );
    assert_eq!(state.unsynced_fact_count, 0);
}

#[test]
fn 未同期の記録が残る_video_試合は移行元として使える() {
    // 移行 commit は非 atomic（ADR 0005 決定 7）— control まで書けて play で落ちた形。
    let facts = vec![
        phase_start(1, both(0.0, 720.0), both(1800.0, 2520.0)),
        goal(2, video_clock(1320.0)),
        goal(3, match_clock(900.0)),
    ];
    let state = video_migration_source_state(&MatchConfiguration::Video(video_source()), &facts);
    assert_eq!(state.issue, None);
    assert_eq!(state.unsynced_fact_count, 1);
    // 既に動画位置を持つ記録は同期点を変えても追随しない（シェルが件数を警告に使う）。
    assert_eq!(state.video_anchored_fact_count, 1);
}

#[test]
fn phase_の_anchor_が片方だけ未変換でも未同期に数える() {
    // start だけ書けて end が matchClock のまま、という途中状態も拾う。
    let facts = vec![phase_start(1, both(0.0, 720.0), match_clock(1800.0))];
    let state = video_migration_source_state(&MatchConfiguration::Video(video_source()), &facts);
    assert_eq!(state.issue, None);
    assert_eq!(state.unsynced_fact_count, 1);
}

#[test]
fn video_highlight_は未同期が残っても移行元にしない() {
    // ハイライト集は移行 commit が作らない（`commit_video_migration` は常に `.video` を書く）。
    let facts = vec![goal(1, match_clock(600.0))];
    let state =
        video_migration_source_state(&MatchConfiguration::VideoHighlight(video_source()), &facts);
    assert_eq!(
        state.issue,
        Some(VideoMigrationDraftIssue::SourceConfigurationNotTimer)
    );
}

#[test]
fn 記録_0_件の_video_試合は移行済み扱い() {
    let state = video_migration_source_state(&MatchConfiguration::Video(video_source()), &[]);
    assert_eq!(
        state.issue,
        Some(VideoMigrationDraftIssue::SourceConfigurationNotTimer)
    );
    assert_eq!(state.unsynced_fact_count, 0);
}

// ── draft の入力内容 ──

#[test]
fn video_source_未確定なら_missing_video_source() {
    let issues = validate_video_migration_draft(None, &[], &[]);
    assert_eq!(issues, vec![VideoMigrationDraftIssue::MissingVideoSource]);
}

#[test]
fn phase_の_start_未入力を検出する() {
    let issues =
        validate_video_migration_draft(Some(&video_source()), &[sync(1, None, Some(1800.0))], &[]);
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::MissingPhaseVideoStart {
            fact_id: FactId(Uuid::from_u128(1)),
        }]
    );
}

#[test]
fn phase_の_end_未入力を検出する() {
    let issues =
        validate_video_migration_draft(Some(&video_source()), &[sync(1, Some(0.0), None)], &[]);
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::MissingPhaseVideoEnd {
            fact_id: FactId(Uuid::from_u128(1)),
        }]
    );
}

#[test]
fn phase_の_end_が_start_以下なら違反_同値も含む() {
    let before = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(1000.0), Some(500.0))],
        &[],
    );
    assert_eq!(
        before,
        vec![VideoMigrationDraftIssue::PhaseVideoEndBeforeStart {
            fact_id: FactId(Uuid::from_u128(1)),
        }]
    );

    let equal = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(1000.0), Some(1000.0))],
        &[],
    );
    assert_eq!(
        equal,
        vec![VideoMigrationDraftIssue::PhaseVideoEndBeforeStart {
            fact_id: FactId(Uuid::from_u128(1)),
        }]
    );
}

#[test]
fn 二つの_phase_範囲の_overlap_を検出する() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[
            sync(1, Some(0.0), Some(1900.0)),
            sync(2, Some(1800.0), Some(3600.0)),
        ],
        &[],
    );
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::PhaseVideoRangesOverlap {
            first_fact_id: FactId(Uuid::from_u128(1)),
            second_fact_id: FactId(Uuid::from_u128(2)),
        }]
    );
}

#[test]
fn 隣接する_phase_範囲は_overlap_ではない() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[
            sync(1, Some(0.0), Some(1800.0)),
            sync(2, Some(1800.0), Some(3600.0)),
        ],
        &[],
    );
    assert_eq!(issues, vec![]);
}

#[test]
fn stoppage_の_start_end_未入力を検出する() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(0.0), Some(1800.0))],
        &[sync(10, None, None)],
    );
    assert_eq!(
        issues,
        vec![
            VideoMigrationDraftIssue::MissingStoppageVideoStart {
                fact_id: FactId(Uuid::from_u128(10)),
            },
            VideoMigrationDraftIssue::MissingStoppageVideoEnd {
                fact_id: FactId(Uuid::from_u128(10)),
            },
        ]
    );
}

#[test]
fn stoppage_の_end_が_start_以下なら違反() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(0.0), Some(1800.0))],
        &[sync(10, Some(600.0), Some(600.0))],
    );
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::StoppageVideoEndBeforeStart {
            fact_id: FactId(Uuid::from_u128(10)),
        }]
    );
}

#[test]
fn stoppage_が_phase_範囲外なら違反() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(0.0), Some(1800.0))],
        &[sync(10, Some(2000.0), Some(2060.0))],
    );
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::StoppageVideoOutsidePhaseRange {
            fact_id: FactId(Uuid::from_u128(10)),
        }]
    );
}

#[test]
fn 二つの_stoppage_範囲の_overlap_を検出する() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(0.0), Some(1800.0))],
        &[
            sync(10, Some(500.0), Some(600.0)),
            sync(11, Some(550.0), Some(650.0)),
        ],
    );
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::StoppageVideoRangesOverlap {
            first_fact_id: FactId(Uuid::from_u128(10)),
            second_fact_id: FactId(Uuid::from_u128(11)),
        }]
    );
}

#[test]
fn 入力未完了の_sync_は_overlap_と範囲チェックの対象外() {
    // stoppage は end 未入力 → missing のみで、範囲外 / overlap には数えない。
    // phase も start のみ → phase 範囲が空になるが、stoppage 側が未完了なので範囲外違反は出ない。
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[sync(1, Some(0.0), None)],
        &[sync(10, Some(500.0), None)],
    );
    assert_eq!(
        issues,
        vec![
            VideoMigrationDraftIssue::MissingPhaseVideoEnd {
                fact_id: FactId(Uuid::from_u128(1)),
            },
            VideoMigrationDraftIssue::MissingStoppageVideoEnd {
                fact_id: FactId(Uuid::from_u128(10)),
            },
        ]
    );
}

#[test]
fn 完全に入力済みの正しい_draft_は違反なし() {
    let issues = validate_video_migration_draft(
        Some(&video_source()),
        &[
            sync(1, Some(720.0), Some(2520.0)),
            sync(2, Some(2600.0), Some(4400.0)),
        ],
        &[sync(10, Some(1000.0), Some(1060.0))],
    );
    assert_eq!(issues, vec![]);
}
