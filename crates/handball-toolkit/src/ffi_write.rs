//! 発火層: 保存・更新の write orchestration（feature `uniffi` 時のみ — ADR 0005）。
//!
//! シェルが repository 実装（foreign trait）を注入し、コアが「読む → 検証 → 発火」を
//! 一続きで実行する。検証入力は保存瞬間の DB 真実 — シェルの読み込み済みコピーではなく、
//! 毎回 repository から読み直す（ADR 0005 決定 1）。validation 違反は発火せず
//! `CoreWriteError::ValidationFailed` で拒否する（blocking 契約 — ADR 0002）。
//!
//! 設計不変条件との関係（ADR 0005 で改定した条件 1）:
//! - コアは repository を**保持しない** — 毎呼び出しで `Arc<dyn>` を受け、返す前に手放す
//!   （long-lived object を作らないことで循環参照を構造的に排除 — ADR 0005 決定 6）
//! - ID / 時刻は生成しない（条件 2）— コアが新規 fact を組む経路（第 2 段 phase 自動補完）は
//!   シェルが事前生成したスタンプバッチを渡す

use std::sync::Arc;

use crate::entities::Match;
use crate::facts::MatchFact;
use crate::ids::{FactId, MatchId, TeamId};
use crate::validation::DomainValidationIssue;
use crate::validators::{self, RosterContext};
use crate::write::{self, PlayerTeamRef};

/// write 入口の失敗（ADR 0005 決定 5。ADR 0002: 構造化 — コード + パラメータのみ）。
///
/// エラー体系は ADR の全形で最初から固定する: `TeamInUse` / `PlayerInUse` は第 4 段
/// （entity CRUD の使用中判定）、`InsufficientNewIds` は第 2 段（phase 自動補完の
/// ID 供給契約）の入口が返す。
#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum CoreWriteError {
    /// validation 違反。発火せず拒否（非空 issues を搬送 — blocking）。
    ValidationFailed { issues: Vec<DomainValidationIssue> },
    /// 使用中チーム削除の拒否（参照整合）。
    TeamInUse { match_count: u32 },
    /// 使用中選手削除の拒否（参照整合）。
    PlayerInUse { fact_count: u32 },
    /// シェル repository 実装の失敗（診断文字列のみ — ユーザー向け文言はシェル所有）。
    Repository { message: String },
    /// 事前生成 ID / 時刻スタンプの不足（ID 供給契約違反。シェルは再試行できる）。
    InsufficientNewIds { required: u32, provided: u32 },
}

// uniffi::Error は Display を要求する。開発者向け診断のみ（ユーザー向け文言は
// シェル所有 — 設計不変条件 3）なので Debug 表現をそのまま使う。
impl std::fmt::Display for CoreWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

// Swift 実装が投げた「CoreWriteError 以外」（panic 相当・未知エラー）を構造化エラーへ畳む。
// この impl が無いと uniffi は Rust 側で panic し、panic=abort の release 構成では
// アプリクラッシュになる（ADR 0005 スパイク欄 — 実装必須）。
impl From<uniffi::UnexpectedUniFFICallbackError> for CoreWriteError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        CoreWriteError::Repository {
            message: error.reason,
        }
    }
}

/// 試合スコープの write repository（シェルが実装して注入する foreign trait — ADR 0005 決定 1）。
///
/// read は write 経路の検証入力に必要な**最小セット**に限る（それを超える汎用 read 面の
/// 注入はしない）。write は検証なしの素朴 CRUD — 「保存してよいか」の判断はコアの
/// write 入口が持ち、シェル実装は判断を持たない。
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait MatchWriteRepository: Send + Sync + std::fmt::Debug {
    // ── read（検証入力の最小セット）──

    /// 対象試合のヘッダ（configuration 込み）。
    async fn load_match(&self, match_id: MatchId) -> Result<Match, CoreWriteError>;
    /// 対象試合の全 fact 列（既存 repository と同じ整列で返す）。
    async fn load_fact_log(&self, match_id: MatchId) -> Result<Vec<MatchFact>, CoreWriteError>;
    /// home / away 両チーム所属の (player, team) 一覧（roster 構築材料。0 件は参照整合 skip）。
    async fn load_roster_players(
        &self,
        home_team_id: TeamId,
        away_team_id: TeamId,
    ) -> Result<Vec<PlayerTeamRef>, CoreWriteError>;

    // ── write（素朴 CRUD・検証なし）──

    async fn save_match(&self, match_: Match) -> Result<(), CoreWriteError>;
    async fn delete_match(&self, match_id: MatchId) -> Result<(), CoreWriteError>;
    async fn append_fact(&self, match_id: MatchId, fact: MatchFact) -> Result<(), CoreWriteError>;
    async fn update_fact(&self, match_id: MatchId, fact: MatchFact) -> Result<(), CoreWriteError>;
    async fn delete_fact(&self, match_id: MatchId, fact_id: FactId) -> Result<(), CoreWriteError>;
}

/// fact append の write 入口: 読む → `validate_append` → 合格時のみ発火。
#[uniffi::export]
pub async fn record_append_fact(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
    fact: MatchFact,
) -> Result<(), CoreWriteError> {
    let (match_, existing, roster) = load_validation_inputs(repo.as_ref(), match_id).await?;
    let issues = validators::validate_append(&fact, &existing, &match_, roster.as_ref());
    if !issues.is_empty() {
        return Err(CoreWriteError::ValidationFailed { issues });
    }
    repo.append_fact(match_id, fact).await
}

/// fact update の write 入口: 読む → `validate_update` → 合格時のみ発火。
#[uniffi::export]
pub async fn record_update_fact(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
    fact: MatchFact,
) -> Result<(), CoreWriteError> {
    let (match_, existing, roster) = load_validation_inputs(repo.as_ref(), match_id).await?;
    let issues = validators::validate_update(&fact, &existing, &match_, roster.as_ref());
    if !issues.is_empty() {
        return Err(CoreWriteError::ValidationFailed { issues });
    }
    repo.update_fact(match_id, fact).await
}

/// fact delete の write 入口: 読む → `validate_delete` → 合格時のみ発火。
/// 削除の検証は whole-log のみで roster を見ない（`validate_delete` と同じ契約）。
#[uniffi::export]
pub async fn record_delete_fact(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
    fact_id: FactId,
) -> Result<(), CoreWriteError> {
    let match_ = repo.load_match(match_id).await?;
    let existing = repo.load_fact_log(match_id).await?;
    let issues = validators::validate_delete(fact_id, &existing, &match_);
    if !issues.is_empty() {
        return Err(CoreWriteError::ValidationFailed { issues });
    }
    repo.delete_fact(match_id, fact_id).await
}

/// append / update 共通の検証入力読み取り（match → fact 列 → roster の 3 read）。
async fn load_validation_inputs(
    repo: &dyn MatchWriteRepository,
    match_id: MatchId,
) -> Result<(Match, Vec<MatchFact>, Option<RosterContext>), CoreWriteError> {
    let match_ = repo.load_match(match_id).await?;
    let existing = repo.load_fact_log(match_id).await?;
    let players = repo
        .load_roster_players(match_.home_team_id, match_.away_team_id)
        .await?;
    let roster =
        write::roster_context_from_players(match_.home_team_id, match_.away_team_id, &players);
    Ok((match_, existing, roster))
}
