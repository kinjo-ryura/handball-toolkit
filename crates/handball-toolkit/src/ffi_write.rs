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

use uuid::Uuid;

use crate::configuration::{MatchConfiguration, VideoSource};
use crate::entities::{Match, Player, Team};
use crate::facts::MatchFact;
use crate::ids::{FactId, MatchId, PlayerId, TeamId};
use crate::sample_dto::SampleMatchDtoV2;
use crate::sample_import::{self, ImportCommitOutcome, ImportDecisions, ImportWriteBatch};
use crate::validation::DomainValidationIssue;
use crate::validators::{self, RosterContext};
use crate::write::{self, NewFactStamp, PlayerTeamRef, VideoMigrationPlanError, VideoSyncInput};

/// write 入口の失敗（ADR 0005 決定 5。ADR 0002: 構造化 — コード + パラメータのみ）。
///
/// エラー体系は ADR の全形で最初から固定する: `TeamInUse` / `PlayerInUse` は第 4 段
/// （entity CRUD の使用中判定）、`InsufficientNewIds` は第 2 段（phase 自動補完の
/// ID 供給契約）の入口が返す。
///
/// **診断文字列のフィールド名を `message` にしないこと**（handball-project#133）:
/// uniffi の Kotlin backend は error 型を `sealed class ... : kotlin.Exception()` として
/// 生成し、`override val message` を必ず持たせる。`message` という名前のフィールドが
/// あると `Throwable.message` と衝突して**生成コードがコンパイルできない**
/// （Swift は error を enum に落とすため露見しない）。詳細は ADR 0006 実装追記。
#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum CoreWriteError {
    /// validation 違反。発火せず拒否（非空 issues を搬送 — blocking）。
    ValidationFailed { issues: Vec<DomainValidationIssue> },
    /// 使用中チーム削除の拒否（参照整合）。
    TeamInUse { match_count: u32 },
    /// 使用中選手削除の拒否（参照整合）。
    PlayerInUse { fact_count: u32 },
    /// シェル repository 実装の失敗（診断文字列のみ — ユーザー向け文言はシェル所有）。
    Repository { detail: String },
    /// 事前生成 ID / 時刻スタンプの不足（ID 供給契約違反。シェルは再試行できる）。
    InsufficientNewIds { required: usize, provided: usize },
    /// video 移行 commit の計画不成立（sync 欠落・videoClock 導出不能）。wizard の
    /// 事前 validation が通っていれば到達しない安全網（実装順序 4 で追加した variant）。
    MigrationPlanInfeasible { detail: String },
    /// import commit の DTO → domain decode 失敗（未知の teamKey / playerKey・不正な
    /// configuration 等）。移植元 `MatchImporterV2.ImportError.conversionFailed` 相当。
    ImportDecodeFailed { detail: String },
}

impl From<VideoMigrationPlanError> for CoreWriteError {
    fn from(error: VideoMigrationPlanError) -> Self {
        CoreWriteError::MigrationPlanInfeasible {
            detail: format!("{error:?}"),
        }
    }
}

// uniffi::Error が要求する Display。**開発者向け診断のみで、ユーザーに見せない**
// （方針と根拠は ADR 0002 決定 5）。`Repository` 等の `message` も同じ扱い。
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
            detail: error.reason,
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

/// phase 自動補完込み記録に必要なスタンプ数（ADR 0005 決定 4 — 必要数はコアが数える）。
///
/// `record_fact_with_phase_completion` と同じ読み取り（repo 経由）で補完 fact 数を返す。
/// シェルはこの数だけ `NewFactStamp` を生成して記録入口を呼ぶ。数え時と発火時の間に
/// fact 列が変わって不足したら、入口が `InsufficientNewIds` で拒否する（再試行可能）。
#[uniffi::export]
pub async fn count_phase_completion_facts(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
    fact: MatchFact,
) -> Result<usize, CoreWriteError> {
    let match_ = repo.load_match(match_id).await?;
    let existing = repo.load_fact_log(match_id).await?;
    Ok(write::phase_completion_plan(&match_, &existing, &fact).len())
}

/// phase 自動補完込みの fact append 入口（ADR 0005 実装順序 3 —
/// 移植元: `RecordingScreenStore.ensureTimerPhasesCovering` + append の連鎖）。
///
/// 読む → 補完計画 → (補完 phase を 1 件ずつ 検証 → 発火) → 本 fact を検証 → 発火。
/// 挙動パリティ（決定 7）: 連鎖は逐次・非 atomic — 途中の validation 違反は以降を発火せず
/// 拒否するが、発火済みの補完 phase はロールバックしない（現行の連鎖 append と同じ）。
#[uniffi::export]
pub async fn record_fact_with_phase_completion(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
    fact: MatchFact,
    new_stamps: Vec<NewFactStamp>,
) -> Result<(), CoreWriteError> {
    let (match_, existing, roster) = load_validation_inputs(repo.as_ref(), match_id).await?;
    let plan = write::phase_completion_plan(&match_, &existing, &fact);
    if new_stamps.len() < plan.len() {
        return Err(CoreWriteError::InsufficientNewIds {
            required: plan.len(),
            provided: new_stamps.len(),
        });
    }

    let mut working = existing;
    for (slot, stamp) in plan.into_iter().zip(new_stamps) {
        let phase_fact = write::phase_completion_fact(slot, stamp);
        let issues = validators::validate_append(&phase_fact, &working, &match_, roster.as_ref());
        if !issues.is_empty() {
            return Err(CoreWriteError::ValidationFailed { issues });
        }
        repo.append_fact(match_id, phase_fact.clone()).await?;
        working.push(phase_fact);
    }

    let issues = validators::validate_append(&fact, &working, &match_, roster.as_ref());
    if !issues.is_empty() {
        return Err(CoreWriteError::ValidationFailed { issues });
    }
    repo.append_fact(match_id, fact).await
}

/// タイマー → 動画移行 commit の入口（ADR 0005 実装順序 4 —
/// 移植元: `MigrateToVideoStore.commit` の保存順序設計）。
///
/// 順序設計をコアが所有する:
/// 1. 更新後 facts を計画（純粋関数 `video_migration_plan` — control → play の順）
/// 2. Match.configuration を先に `.video` へ save（素朴 CRUD・検証なし。既存 facts が
///    matchClock anchor の途中状態でも通り、後続 update が `.video` config 下で検証される）
/// 3. facts を計画順に逐次 validate → update（挙動パリティ: 非 atomic、途中失敗は再実行で復旧）
#[uniffi::export]
pub async fn commit_video_migration(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
    video_source: VideoSource,
    phase_syncs: Vec<VideoSyncInput>,
    stoppage_syncs: Vec<VideoSyncInput>,
) -> Result<(), CoreWriteError> {
    let mut match_ = repo.load_match(match_id).await?;
    let facts = repo.load_fact_log(match_id).await?;
    let players = repo
        .load_roster_players(match_.home_team_id, match_.away_team_id)
        .await?;
    let roster =
        write::roster_context_from_players(match_.home_team_id, match_.away_team_id, &players);

    let updated = write::video_migration_plan(&facts, &phase_syncs, &stoppage_syncs)?;

    match_.configuration = MatchConfiguration::Video(video_source);
    repo.save_match(match_.clone()).await?;

    let mut working = facts;
    for fact in updated {
        let issues = validators::validate_update(&fact, &working, &match_, roster.as_ref());
        if !issues.is_empty() {
            return Err(CoreWriteError::ValidationFailed { issues });
        }
        repo.update_fact(match_id, fact.clone()).await?;
        if let Some(slot) = working.iter_mut().find(|f| f.id == fact.id) {
            *slot = fact;
        }
    }
    Ok(())
}

/// import commit の atomic 発火 repository（シェルが実装して注入する foreign trait —
/// ADR 0005 決定 1 の 2026-07-22 追記・handball-project#83）。
///
/// `commit_import` を **1 `context.save()`** で実装し、全成功 or 1 件も保存しない（atomic）。
/// 検証はコアが呼ぶ前に `import_commit_batch` で済ませる — この実装は**検証なしの素朴バッチ**。
/// トランザクション境界は DB ハンドルを握るシェル側にしか張れないため、コアは組み上げた
/// バッチを値で 1 回渡すだけ（トランザクションオブジェクトを FFI 越しに持ち回らない = 決定 6）。
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait ImportWriteRepository: Send + Sync + std::fmt::Debug {
    /// batch の全 entity / fact を 1 トランザクションで保存する（atomic）。
    async fn commit_import(&self, batch: ImportWriteBatch) -> Result<(), CoreWriteError>;
}

/// サンプル試合 import commit の入口（handball-project#67 / #83 — ADR 0005 の import 版）。
///
/// 移植元: `MatchImporterV2.commit(parsed:decisions:...)` の ID 解決 + 組立 + 保存順序。
/// シェルに残るのは取得（HTTP / Bundle）と decisions の UI 選択、表示名の解決だけになる。
///
/// 順序設計をコアが所有する:
/// 1. 計画（純粋関数 `import_commit_plan` — 既存に統合した entity は save 対象に積まない）
/// 2. 既存 roster（新規保存前の 2 チーム所属選手）を読む — 検証入力の最小 read（決定 1）
/// 3. `import_commit_batch` で検証 + 組立（プレフィックス検証は現行の逐次 append と同一）
/// 4. `ImportWriteRepository::commit_import` へ 1 バッチを渡し、**1 `context.save()` で atomic 発火**
///
/// **facts は `import_commit_plan` が永続化順へ整列する**（移植元から意図的に乖離 —
/// handball-project#72）。整列によりプレフィックス検証が通る（`.video` の途中状態が R3 / R5 に
/// 抵触しない）。
///
/// atomic 化（決定 7 の 2026-07-22 追記・#83）: 従来の「entity 逐次 save → facts 逐次 append」
/// を撤去し、検証は in-memory・発火は 1 バッチにした。**途中失敗で孤児レコード（facts 0 件の
/// 試合 + 孤児チーム）が残らない**。dev 専用経路（`DevDataViewV2` / `#if DEBUG`）限定で、
/// record / phase 補完 / migrate の逐次・非 atomic は変えていない。
#[uniffi::export]
pub async fn commit_sample_match_import(
    match_repo: Arc<dyn MatchWriteRepository>,
    import_repo: Arc<dyn ImportWriteRepository>,
    dto: SampleMatchDtoV2,
    decisions: ImportDecisions,
    new_ids: Vec<Uuid>,
) -> Result<ImportCommitOutcome, CoreWriteError> {
    let required = sample_import::required_import_id_count(&dto, &decisions);
    if new_ids.len() < required {
        return Err(CoreWriteError::InsufficientNewIds {
            required,
            provided: new_ids.len(),
        });
    }
    let mut ids = new_ids.into_iter();
    let plan = sample_import::import_commit_plan(&dto, &decisions, || {
        ids.next().expect("必要数は事前検査済み")
    })
    .map_err(|error| CoreWriteError::ImportDecodeFailed {
        detail: format!("{error:?}"),
    })?;

    let outcome = plan.outcome;
    // 既存 roster（新規保存前の DB 真実）。新規 team なら空、reused team なら既存選手を返す。
    let existing_roster = match_repo
        .load_roster_players(plan.r#match.home_team_id, plan.r#match.away_team_id)
        .await?;

    let batch = sample_import::import_commit_batch(plan, &existing_roster)
        .map_err(|issues| CoreWriteError::ValidationFailed { issues })?;

    import_repo.commit_import(batch).await?;
    Ok(outcome)
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

// ── entity CRUD の write 入口（ADR 0005 実装順序 5 — 第 4 段）──

/// チーム / 選手スコープの write repository（シェルが実装して注入する foreign trait —
/// ADR 0005 決定 1）。read は削除の参照整合判定の材料（カウント）に限る。
/// `delete_team` は所属選手の cascade 削除を実装内に含む（判断ではなくストレージ操作の
/// セマンティクス — 1 save の原子性を保つ。決定 2）。
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait TeamWriteRepository: Send + Sync + std::fmt::Debug {
    // ── read（削除の参照整合判定の材料）──

    /// このチームを home / away いずれかで参照する試合数。
    async fn count_matches_referencing_team(&self, team_id: TeamId) -> Result<u32, CoreWriteError>;
    /// この選手を playerID / relatedPlayerID で参照する fact 数。
    async fn count_facts_referencing_player(
        &self,
        player_id: PlayerId,
    ) -> Result<u32, CoreWriteError>;

    // ── write（素朴 CRUD。delete_team は cascade 内包）──

    async fn save_team(&self, team: Team) -> Result<(), CoreWriteError>;
    async fn delete_team(&self, team_id: TeamId) -> Result<(), CoreWriteError>;
    async fn save_player(&self, player: Player) -> Result<(), CoreWriteError>;
    async fn delete_player(&self, player_id: PlayerId) -> Result<(), CoreWriteError>;
}

/// match ヘッダ save の write 入口（passthrough — 現行 saveMatch は検証なし。パリティ維持）。
/// 意義は目録の単一化と可視性遮断: 全書き込みがコア入口を通ることを型で保証する（決定 2・3）。
#[uniffi::export]
pub async fn record_save_match(
    repo: Arc<dyn MatchWriteRepository>,
    match_: Match,
) -> Result<(), CoreWriteError> {
    repo.save_match(match_).await
}

/// match 削除の write 入口（passthrough — facts 込み削除の実体は repository 実装）。
#[uniffi::export]
pub async fn record_delete_match(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: MatchId,
) -> Result<(), CoreWriteError> {
    repo.delete_match(match_id).await
}

/// team save の write 入口（passthrough）。
#[uniffi::export]
pub async fn record_save_team(
    repo: Arc<dyn TeamWriteRepository>,
    team: Team,
) -> Result<(), CoreWriteError> {
    repo.save_team(team).await
}

/// team 削除の write 入口: 使用中判定 → 合格時のみ発火（判定はコアが持つ — 決定 2）。
///
/// チェックと削除は 2 FFI 呼び出しに分かれ理論上の時間窓は現行より広がるが、
/// 現行も context 間の直列化保証は無く、保証クラスは best-effort のまま変わらない（決定 2）。
#[uniffi::export]
pub async fn record_delete_team(
    repo: Arc<dyn TeamWriteRepository>,
    team_id: TeamId,
) -> Result<(), CoreWriteError> {
    let match_count = repo.count_matches_referencing_team(team_id).await?;
    if match_count > 0 {
        return Err(CoreWriteError::TeamInUse { match_count });
    }
    repo.delete_team(team_id).await
}

/// player save の write 入口（passthrough）。
#[uniffi::export]
pub async fn record_save_player(
    repo: Arc<dyn TeamWriteRepository>,
    player: Player,
) -> Result<(), CoreWriteError> {
    repo.save_player(player).await
}

/// player 削除の write 入口: 使用中判定 → 合格時のみ発火（dangling 参照の防止）。
#[uniffi::export]
pub async fn record_delete_player(
    repo: Arc<dyn TeamWriteRepository>,
    player_id: PlayerId,
) -> Result<(), CoreWriteError> {
    let fact_count = repo.count_facts_referencing_player(player_id).await?;
    if fact_count > 0 {
        return Err(CoreWriteError::PlayerInUse { fact_count });
    }
    repo.delete_player(player_id).await
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
