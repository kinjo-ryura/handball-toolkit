//! validation エラー体系。移植元: `Validation/` ディレクトリ（ADR 0001 ミラー表）。
//!
//! 移植で唯一の意図的再設計ポイント（ADR 0002）: 文言レイヤ
//! （`DomainValidationMessage.swift` / `userMessage`）はコアへ移植せず、各シェルが
//! `(scope, code)` → ローカライズ文言の写像を所有する。ここにあるのはエラーコード +
//! パラメータのみで、serde 形式が FFI / JSON 境界のワイヤ形式
//! `{ "scope": ..., "code": ..., "params": {...} }` を与える。
//! code は Swift の case 名そのままの安定契約（改名は breaking change）。

mod configuration_validation_error;
mod domain_validation_issue;
mod fact_validation_error;
mod match_validation_error;
mod timeline_validation_error;

pub use configuration_validation_error::ConfigurationValidationError;
pub use domain_validation_issue::DomainValidationIssue;
pub use fact_validation_error::FactValidationError;
pub use match_validation_error::MatchValidationError;
pub use timeline_validation_error::TimelineValidationError;
