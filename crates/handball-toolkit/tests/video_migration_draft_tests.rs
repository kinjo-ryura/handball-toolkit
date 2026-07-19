//! 移行ウィザードの draft 事前検証の挙動固定（handball-project#68）。
//!
//! 移植元 `VideoModeMigrationValidator`（Swift）とそのテスト
//! `MigrateToVideoStoreTests` の Validator 節を同セマンティクスで固定する。
//! 文言・wizard step への写像はシェル所有のため対象外。

use handball_toolkit::configuration::{MatchConfiguration, VideoProvider, VideoSource};
use handball_toolkit::ids::FactId;
use handball_toolkit::write::{
    VideoMigrationDraftIssue, VideoSyncDraftInput, validate_video_migration_draft,
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

#[test]
fn video_source_未確定なら_missing_video_source() {
    let issues = validate_video_migration_draft(&timer_configuration(), None, &[], &[]);
    assert_eq!(issues, vec![VideoMigrationDraftIssue::MissingVideoSource]);
}

#[test]
fn timer_以外の試合は_source_configuration_not_timer() {
    let issues = validate_video_migration_draft(
        &MatchConfiguration::Video(video_source()),
        Some(&video_source()),
        &[],
        &[],
    );
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::SourceConfigurationNotTimer]
    );
}

#[test]
fn 違反は放出順を保存する_configuration_が_source_より先() {
    let issues = validate_video_migration_draft(
        &MatchConfiguration::VideoHighlight(video_source()),
        None,
        &[],
        &[],
    );
    assert_eq!(
        issues,
        vec![
            VideoMigrationDraftIssue::SourceConfigurationNotTimer,
            VideoMigrationDraftIssue::MissingVideoSource,
        ]
    );
}

#[test]
fn phase_の_start_未入力を検出する() {
    let issues = validate_video_migration_draft(
        &timer_configuration(),
        Some(&video_source()),
        &[sync(1, None, Some(1800.0))],
        &[],
    );
    assert_eq!(
        issues,
        vec![VideoMigrationDraftIssue::MissingPhaseVideoStart {
            fact_id: FactId(Uuid::from_u128(1)),
        }]
    );
}

#[test]
fn phase_の_end_未入力を検出する() {
    let issues = validate_video_migration_draft(
        &timer_configuration(),
        Some(&video_source()),
        &[sync(1, Some(0.0), None)],
        &[],
    );
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
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
        &timer_configuration(),
        Some(&video_source()),
        &[
            sync(1, Some(720.0), Some(2520.0)),
            sync(2, Some(2600.0), Some(4400.0)),
        ],
        &[sync(10, Some(1000.0), Some(1060.0))],
    );
    assert_eq!(issues, vec![]);
}
