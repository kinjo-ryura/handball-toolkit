//! 移植元: `Validators/ConfigurationValidator.swift`。

use crate::configuration::MatchConfiguration;
use crate::validation::{ConfigurationValidationError, DomainValidationIssue};

/// MatchConfiguration の value validation。
///
/// 3 variant sum type 化により、旧 PhaseRules 整合チェックは型レベルで不要に。
/// 残るのは payload の値範囲チェックのみ。
pub fn validate_configuration(config: &MatchConfiguration) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    match config {
        MatchConfiguration::Timer {
            phase_duration_seconds,
        } => {
            if *phase_duration_seconds <= 0.0 {
                issues.push(DomainValidationIssue::Configuration(
                    ConfigurationValidationError::NonPositivePhaseDuration {
                        seconds: *phase_duration_seconds,
                    },
                ));
            }
        }
        MatchConfiguration::Video(source) | MatchConfiguration::VideoHighlight(source) => {
            if source.external_id.trim().is_empty() {
                issues.push(DomainValidationIssue::Configuration(
                    ConfigurationValidationError::EmptyVideoExternalId,
                ));
            }
        }
    }

    issues
}
