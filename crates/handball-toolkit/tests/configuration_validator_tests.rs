//! 移植元: `Tests/RecorderDomainTests/ConfigurationValidatorTests.swift`。
//!
//! computed properties テストの `contentKind` assert は移植しない
//! （ContentKind はドメイン未使用の UI helper のため移植対象外 — ADR 0001）。

use handball_toolkit::configuration::{
    CaptureMethod, MatchConfiguration, MatchConfigurationKind, VideoProvider, VideoSource,
};
use handball_toolkit::validation::{ConfigurationValidationError, DomainValidationIssue};
use handball_toolkit::validators::validate_configuration;

fn youtube_source(external_id: &str) -> VideoSource {
    VideoSource {
        provider: VideoProvider::Youtube,
        external_id: external_id.to_owned(),
    }
}

// ── Timer ──

#[test]
fn timer_with_positive_phase_duration_is_valid() {
    let config = MatchConfiguration::Timer {
        phase_duration_seconds: 1800.0,
    };
    assert!(validate_configuration(&config).is_empty());
}

#[test]
fn timer_with_zero_phase_duration_is_blocking() {
    let config = MatchConfiguration::Timer {
        phase_duration_seconds: 0.0,
    };
    let issues = validate_configuration(&config);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Configuration(
            ConfigurationValidationError::NonPositivePhaseDuration { seconds: 0.0 }
        )]
    );
}

#[test]
fn timer_with_negative_phase_duration_is_blocking() {
    let config = MatchConfiguration::Timer {
        phase_duration_seconds: -1.0,
    };
    let issues = validate_configuration(&config);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Configuration(
            ConfigurationValidationError::NonPositivePhaseDuration { seconds: -1.0 }
        )]
    );
}

// ── Video ──

#[test]
fn video_with_external_id_is_valid() {
    let config = MatchConfiguration::Video(youtube_source("abc"));
    assert!(validate_configuration(&config).is_empty());
}

#[test]
fn video_with_empty_external_id_is_blocking() {
    let config = MatchConfiguration::Video(youtube_source("  "));
    let issues = validate_configuration(&config);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Configuration(
            ConfigurationValidationError::EmptyVideoExternalId
        )]
    );
}

// ── VideoHighlight ──

#[test]
fn video_highlight_with_external_id_is_valid() {
    let config = MatchConfiguration::VideoHighlight(youtube_source("abc"));
    assert!(validate_configuration(&config).is_empty());
}

#[test]
fn video_highlight_with_empty_external_id_is_blocking() {
    let config = MatchConfiguration::VideoHighlight(youtube_source(""));
    let issues = validate_configuration(&config);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Configuration(
            ConfigurationValidationError::EmptyVideoExternalId
        )]
    );
}

// ── computed properties ──

#[test]
fn timer_computed_properties_are_correct() {
    let config = MatchConfiguration::Timer {
        phase_duration_seconds: 1800.0,
    };
    assert_eq!(config.capture_method(), CaptureMethod::ManualClock);
    assert_eq!(config.video_source(), None);
    assert_eq!(config.phase_duration_seconds(), Some(1800.0));
    assert_eq!(config.kind(), MatchConfigurationKind::Timer);
}

#[test]
fn video_computed_properties_are_correct() {
    let source = youtube_source("abc");
    let config = MatchConfiguration::Video(source.clone());
    assert_eq!(config.capture_method(), CaptureMethod::Video);
    assert_eq!(config.video_source(), Some(&source));
    assert_eq!(config.phase_duration_seconds(), None);
    assert_eq!(config.kind(), MatchConfigurationKind::Video);
}

#[test]
fn video_highlight_computed_properties_are_correct() {
    let source = youtube_source("abc");
    let config = MatchConfiguration::VideoHighlight(source.clone());
    assert_eq!(config.capture_method(), CaptureMethod::Video);
    assert_eq!(config.video_source(), Some(&source));
    assert_eq!(config.phase_duration_seconds(), None);
    assert_eq!(config.kind(), MatchConfigurationKind::VideoHighlight);
}
