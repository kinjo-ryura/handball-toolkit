//! 移植元: `Validators/MatchWriteValidator.swift`。
//!
//! fact の append / update を永続化する**直前**に走らせる集約 validation。
//!
//! 役割: repository（live 記録経路 / import）が illegal fact を保存するのを構造的に防ぐ
//! enforcement の単一窓口。`fact_validator`（1 件の value/anchor/payload）と
//! `fact_log_validator`（log 全体の R3-R9 / 連続性 / 重複）を合成し、検出 issue を返す。
//!
//! 非空を返したらシェルは書き込みを拒否する（blocking 契約 — ADR 0002）。
//!
//! roster（player↔team 参照整合 / dangling 検出）は `roster` 引数で注入する。
//! 渡されない場合は `RosterContext::empty` を使い参照整合を**見ない**（後方互換）。シェルは
//! home/away ロスターから `known_player_ids` 付き roster を構築して渡し、dangling player 参照を
//! blocking 検出する。

use crate::entities::Match;
use crate::facts::MatchFact;
use crate::ids::FactId;
use crate::validation::DomainValidationIssue;

use super::fact_log_validator::validate_fact_log;
use super::fact_validator::{RosterContext, validate_match_fact};

/// `fact` を `existing_facts` に append した結果の log を検証する。
/// `roster` を渡すと player 参照整合（dangling / team 不一致）も検証する。
pub fn validate_append(
    fact: &MatchFact,
    existing_facts: &[MatchFact],
    match_: &Match,
    roster: Option<&RosterContext>,
) -> Vec<DomainValidationIssue> {
    let mut resulting: Vec<MatchFact> = existing_facts.to_vec();
    resulting.push(fact.clone());
    validate_changed(fact, &resulting, match_, roster)
}

/// `fact` で同 id の既存 fact を置換した結果の log を検証する。
pub fn validate_update(
    fact: &MatchFact,
    existing_facts: &[MatchFact],
    match_: &Match,
    roster: Option<&RosterContext>,
) -> Vec<DomainValidationIssue> {
    let resulting: Vec<MatchFact> = existing_facts
        .iter()
        .map(|f| {
            if f.id == fact.id {
                fact.clone()
            } else {
                f.clone()
            }
        })
        .collect();
    validate_changed(fact, &resulting, match_, roster)
}

/// `removed_fact_id` を log から除去した結果を検証する（削除も append / update と同じ窓口を通す）。
///
/// 削除では「変更された 1 件」が消えるため、per-fact の `fact_validator` は走らせず、
/// whole-log の `fact_log_validator`（R3-R9 / phase 連続性 / stoppage 重複 等）のみを適用する。
/// 例: 中に play fact が残る PhaseStart を削除すると `playRecordedOutsidePhaseRange` 等で blocking。
pub fn validate_delete(
    removed_fact_id: FactId,
    existing_facts: &[MatchFact],
    match_: &Match,
) -> Vec<DomainValidationIssue> {
    let resulting: Vec<MatchFact> = existing_facts
        .iter()
        .filter(|f| f.id != removed_fact_id)
        .cloned()
        .collect();
    validate_fact_log(&resulting, match_)
}

fn validate_changed(
    changed_fact: &MatchFact,
    resulting_facts: &[MatchFact],
    match_: &Match,
    roster: Option<&RosterContext>,
) -> Vec<DomainValidationIssue> {
    let default_roster;
    let effective_roster = match roster {
        Some(r) => r,
        None => {
            default_roster = RosterContext::empty(match_.home_team_id, match_.away_team_id);
            &default_roster
        }
    };
    let mut issues = validate_match_fact(changed_fact, &match_.configuration, effective_roster);
    issues.extend(validate_fact_log(resulting_facts, match_));
    issues
}
