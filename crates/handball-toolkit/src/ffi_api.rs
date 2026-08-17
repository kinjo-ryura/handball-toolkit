//! FFI 境界の関数公開（feature `uniffi` 時のみ — ADR 0004）。
//!
//! uniffi の生成物を 1 つの Swift モジュールにまとめるため、export 関数もこの crate の
//! namespace に置く（module_name を複数 namespace で共有すると生成ファイルが上書き衝突する）。
//! staticlib 化と bindgen CLI は handball-toolkit-ffi crate が担う。
//!
//! 方針（ADR 0001 関数目録 / ADR 0004 決定 1・4・5）:
//! - 入力は所有値で受けてコアの借用 API へ委譲する薄いラッパのみ。ロジックを書かない
//! - `SegmentResolver` だけ object ハンドル（構築 1 回・参照は self + スカラー）
//! - 自明なアクセサ（`FactAnchor.matchClock` 等）は公開しない — Swift シムが再実装する

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};
use crate::configuration::{MatchConfiguration, PhaseKind, VideoSource};
use crate::entities::{Match, Player, Team};
use crate::facts::{ControlFact, MatchFact, PlayEventKind, PlayFact, StoppageKind};
use crate::ids::{FactId, PlayerId, TeamId};
use crate::projection::{
    LiveMatchProjection, Phase, ScoreProgressionProjection, SegmentResolver, SummaryProjection,
    TimeSegment, TimelineProjection,
};
use crate::sample_dto::{
    self, SampleFactDtoV2, SampleHighlightIndexDtoV2, SampleIndexDtoV2,
    SampleMatchConfigurationDtoV2, SampleMatchConversionResult, SampleMatchDecodeErrorV2,
    SampleMatchDtoV2, SampleTeamDtoV2,
};
use crate::sample_import::{self, ExistingSnapshot, ImportDecisions, TeamOption};
use crate::validation::DomainValidationIssue;
use crate::validators;
use crate::validators::RosterContext;
use crate::write::{
    self, CaptureClockKind, NewFactStamp, PlayFactEdit, VideoMigrationDraftIssue,
    VideoSyncDraftInput,
};

/// コアのバージョン文字列。FFI 疎通確認の最小関数。
#[uniffi::export]
pub fn toolkit_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── projection builders（fact 列 in → projection out — 設計不変条件 4）──

/// `TimelineProjection::build`。
#[uniffi::export]
pub fn build_timeline(match_: Match, facts: Vec<MatchFact>) -> TimelineProjection {
    TimelineProjection::build(&match_, &facts)
}

/// `SummaryProjection::build`（resolver 非依存。`phase_summaries` は空）。
#[uniffi::export]
pub fn build_summary(match_: Match, facts: Vec<MatchFact>) -> SummaryProjection {
    SummaryProjection::build(&match_, &facts)
}

/// `SummaryProjection::build_with_timeline`（timeline の resolver を再利用し phase 別 stats も算出）。
#[uniffi::export]
pub fn build_summary_with_timeline(
    match_: Match,
    timeline: TimelineProjection,
) -> SummaryProjection {
    SummaryProjection::build_with_timeline(&match_, &timeline)
}

/// `ScoreProgressionProjection::build`（facts から timeline を内部構築する convenience）。
#[uniffi::export]
pub fn build_score_progression(
    match_: Match,
    facts: Vec<MatchFact>,
) -> Option<ScoreProgressionProjection> {
    ScoreProgressionProjection::build(&match_, &facts)
}

/// `ScoreProgressionProjection::build_with_timeline`（resolver を二度作らない経路）。
#[uniffi::export]
pub fn build_score_progression_with_timeline(
    match_: Match,
    timeline: TimelineProjection,
) -> Option<ScoreProgressionProjection> {
    ScoreProgressionProjection::build_with_timeline(&match_, &timeline)
}

/// `LiveMatchProjection::build_video_mode_with_resolver`。動画モードの 2Hz tick 経路。
///
/// **resolver ハンドルだけを受け取る**（handball-project#167）。以前は `TimelineProjection` を
/// record ごと受けていたため、resolver が object ハンドルでも同居する `resolved_facts` が
/// 生成コードの converter の write 順で**その手前に全量書かれ**、fact 列が毎 tick 境界を
/// 渡っていた。中身は元から `timeline.resolver` しか読んでおらず（`match` も未使用だった）、
/// 転送された fact 列は使われずに捨てられていたので、引数を落としても導出結果は変わらない。
///
/// これで ADR 0004 決定 5 の「facts の再マーシャリングは発生しない」が、この関数についても
/// 実際に成り立つ。**`TimelineProjection` を引数に戻さないこと** — record ごと渡した瞬間に
/// 同じ問題が戻る。fact 列が要る projection は `build_summary_with_timeline` のように
/// tick 経路ではない関数に置く。
#[uniffi::export]
pub fn build_live_match_video_mode(
    resolver: std::sync::Arc<SegmentResolver>,
    current_video_clock: Option<VideoClock>,
) -> LiveMatchProjection {
    LiveMatchProjection::build_video_mode_with_resolver(&resolver, current_video_clock)
}

// ── validators（ADR 0002: 非空 = blocking。文言はシェル所有）──

/// `validators::validate_match`。
#[uniffi::export]
pub fn validate_match(match_: Match) -> Vec<DomainValidationIssue> {
    validators::validate_match(&match_)
}

/// `validators::validate_configuration`。
#[uniffi::export]
pub fn validate_configuration(configuration: MatchConfiguration) -> Vec<DomainValidationIssue> {
    validators::validate_configuration(&configuration)
}

/// `validators::validate_fact_log`（R3-R9 / 連続性 / 重複の whole-log 検証）。
#[uniffi::export]
pub fn validate_fact_log(facts: Vec<MatchFact>, match_: Match) -> Vec<DomainValidationIssue> {
    validators::validate_fact_log(&facts, &match_)
}

/// `validators::validate_match_fact`（1 件の value/anchor/payload + 参照整合）。
#[uniffi::export]
pub fn validate_match_fact(
    fact: MatchFact,
    configuration: MatchConfiguration,
    roster: RosterContext,
) -> Vec<DomainValidationIssue> {
    validators::validate_match_fact(&fact, &configuration, &roster)
}

/// `validators::validate_play_fact`。
#[uniffi::export]
pub fn validate_play_fact(
    play: PlayFact,
    configuration: MatchConfiguration,
    roster: RosterContext,
) -> Vec<DomainValidationIssue> {
    validators::validate_play_fact(&play, &configuration, &roster)
}

/// `validators::validate_control_fact`。
#[uniffi::export]
pub fn validate_control_fact(
    control: ControlFact,
    configuration: MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    validators::validate_control_fact(&control, &configuration)
}

/// `validators::validate_append`（append 直前の集約 validation — 保存可否の単一窓口）。
#[uniffi::export]
pub fn validate_append(
    fact: MatchFact,
    existing_facts: Vec<MatchFact>,
    match_: Match,
    roster: Option<RosterContext>,
) -> Vec<DomainValidationIssue> {
    validators::validate_append(&fact, &existing_facts, &match_, roster.as_ref())
}

/// `validators::validate_update`。
#[uniffi::export]
pub fn validate_update(
    fact: MatchFact,
    existing_facts: Vec<MatchFact>,
    match_: Match,
    roster: Option<RosterContext>,
) -> Vec<DomainValidationIssue> {
    validators::validate_update(&fact, &existing_facts, &match_, roster.as_ref())
}

/// `validators::validate_delete`。
#[uniffi::export]
pub fn validate_delete(
    removed_fact_id: FactId,
    existing_facts: Vec<MatchFact>,
    match_: Match,
) -> Vec<DomainValidationIssue> {
    validators::validate_delete(removed_fact_id, &existing_facts, &match_)
}

/// `write::validate_video_migration_draft`（移行ウィザードの draft 事前検証）。
/// 文言と wizard step への写像はシェル所有。
#[uniffi::export]
pub fn validate_video_migration_draft(
    source_configuration: MatchConfiguration,
    video_source: Option<VideoSource>,
    phase_syncs: Vec<VideoSyncDraftInput>,
    stoppage_syncs: Vec<VideoSyncDraftInput>,
) -> Vec<VideoMigrationDraftIssue> {
    write::validate_video_migration_draft(
        &source_configuration,
        video_source.as_ref(),
        &phase_syncs,
        &stoppage_syncs,
    )
}

// ── 記録入口（記録操作 in → fact / anchor out — handball-project#69）──

/// `write::capture_play_anchor`（記録 offset の減算 + 0 クランプ + phase / stoppage 境界での
/// クランプ + anchor 組み立て）。境界クランプに fact 列が要る（handball-project#101）。
#[uniffi::export]
pub fn capture_play_anchor(
    base_seconds: f64,
    recording_offset_seconds: f64,
    clock_kind: CaptureClockKind,
    facts: Vec<MatchFact>,
) -> FactAnchor {
    write::capture_play_anchor(base_seconds, recording_offset_seconds, clock_kind, &facts)
}

/// `write::initial_timer_seconds`（記録画面を開いたときのタイマー初期累積秒）。
#[uniffi::export]
pub fn initial_timer_seconds(facts: Vec<MatchFact>) -> f64 {
    write::initial_timer_seconds(&facts)
}

/// `write::build_play_fact`。
#[uniffi::export]
pub fn build_play_fact(
    stamp: NewFactStamp,
    kind: PlayEventKind,
    team_id: Option<TeamId>,
    player_id: Option<PlayerId>,
    anchor: FactAnchor,
    title: Option<String>,
    note: Option<String>,
) -> MatchFact {
    write::build_play_fact(stamp, kind, team_id, player_id, anchor, title, note)
}

/// `write::build_stoppage_fact`。
#[uniffi::export]
pub fn build_stoppage_fact(
    stamp: NewFactStamp,
    kind: StoppageKind,
    start_anchor: FactAnchor,
    end_anchor: Option<FactAnchor>,
    note: Option<String>,
) -> MatchFact {
    write::build_stoppage_fact(stamp, kind, start_anchor, end_anchor, note)
}

/// `write::apply_play_fact_edit`（1 操作分の編集適用。trim / クランプ / anchor 場合分け込み）。
#[uniffi::export]
pub fn apply_play_fact_edit(play: PlayFact, edit: PlayFactEdit) -> PlayFact {
    write::apply_play_fact_edit(play, edit)
}

/// `FactAnchor::with_elapsed_seconds`（時刻編集で入力された累積秒の書き戻し）。
///
/// 自明なアクセサではなく**遷移規則**（`Both` の片側保持 = 強制同期点の意味そのもの）なので、
/// シム再実装の許可基準（ADR 0004 決定 4）に当てはまらず境界へ出す。iOS / Mac の時刻編集 UI が
/// 同じ 9 分岐を各自持っていた状態の解消（handball-project#168）。
#[uniffi::export]
pub fn anchor_with_elapsed(anchor: FactAnchor, kind: FactAnchorKind, seconds: f64) -> FactAnchor {
    anchor.with_elapsed_seconds(kind, seconds)
}

// ── sample_dto（SAMPLE_DTO_V2 の parse / 変換 / export — ADR 0004 決定 2）──

/// sample_dto FFI の失敗（ADR 0002: 構造化 — コード + パラメータのみ。文言はシェル所有）。
///
/// Swift では throws で受ける。`Decode` は移植エラー型 `SampleMatchDecodeErrorV2` を
/// そのまま搬送する。
///
/// **診断文字列のフィールド名を `message` にしないこと**（handball-project#133）:
/// uniffi の Kotlin backend は error 型を `sealed class ... : kotlin.Exception()` として
/// 生成し、`override val message` を必ず持たせる。`message` という名前のフィールドが
/// あると `Throwable.message` と衝突して**生成コードがコンパイルできない**
/// （Swift は error を enum に落とすため露見しない）。詳細は ADR 0006 実装追記。
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum SampleDtoError {
    /// JSON が SAMPLE_DTO_V2 schema として parse できない（serde の診断文字列を添付）。
    InvalidJson { detail: String },
    /// DTO → domain の decode 失敗。
    Decode { error: SampleMatchDecodeErrorV2 },
    /// 事前生成 ID の不足。`sample_match_required_id_count` の値だけ生成して渡す。
    InsufficientNewIds { required: usize, provided: usize },
}

// uniffi::Error が要求する Display。**開発者向け診断のみで、ユーザーに見せない**
// （方針と根拠は ADR 0002 決定 5）。
impl std::fmt::Display for SampleDtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

fn invalid_json(error: serde_json::Error) -> SampleDtoError {
    SampleDtoError::InvalidJson {
        detail: error.to_string(),
    }
}

/// 配信 JSON → DTO（`/v2/matches/{slug}.json` / `/v2/highlights/{slug}.json` の本体）。
#[uniffi::export]
pub fn parse_sample_match(json: String) -> Result<SampleMatchDtoV2, SampleDtoError> {
    serde_json::from_str(&json).map_err(invalid_json)
}

/// `/v2/index.json` の parse。
#[uniffi::export]
pub fn parse_sample_index(json: String) -> Result<SampleIndexDtoV2, SampleDtoError> {
    serde_json::from_str(&json).map_err(invalid_json)
}

/// `/v2/highlights/index.json` の parse。
#[uniffi::export]
pub fn parse_sample_highlight_index(
    json: String,
) -> Result<SampleHighlightIndexDtoV2, SampleDtoError> {
    serde_json::from_str(&json).map_err(invalid_json)
}

/// `sample_dto::required_id_count` — `convert_sample_match` へ渡す事前生成 ID の必要数。
#[uniffi::export]
pub fn sample_match_required_id_count(dto: SampleMatchDtoV2) -> usize {
    sample_dto::required_id_count(&dto)
}

/// `sample_dto::convert`。ID 生成の closure 注入は FFI では callback interface を要するため、
/// シェルが必要数を事前生成した `new_ids` で置き換える（ADR 0004 決定 2 — stateless 維持）。
#[uniffi::export]
pub fn convert_sample_match(
    slug: String,
    dto: SampleMatchDtoV2,
    configuration_override: Option<MatchConfiguration>,
    new_ids: Vec<Uuid>,
) -> Result<SampleMatchConversionResult, SampleDtoError> {
    let required = sample_dto::required_id_count(&dto);
    if new_ids.len() < required {
        return Err(SampleDtoError::InsufficientNewIds {
            required,
            provided: new_ids.len(),
        });
    }
    let mut ids = new_ids.into_iter();
    sample_dto::convert(&slug, &dto, configuration_override, || {
        ids.next().expect("必要数は事前検査済み")
    })
    .map_err(|error| SampleDtoError::Decode { error })
}

/// `sample_dto::decode_configuration`（importer の merge 調停が単体で使う）。
#[uniffi::export]
pub fn decode_sample_configuration(
    dto: SampleMatchConfigurationDtoV2,
) -> Result<MatchConfiguration, SampleDtoError> {
    sample_dto::decode_configuration(&dto).map_err(|error| SampleDtoError::Decode { error })
}

/// `sample_dto::decode_fact`（importer の merge 調停が、既存 DB と突合済みの
/// teamKey / playerKey 写像で 1 fact ずつ decode する経路）。
/// `fallback_id` は factID 無し fact 用にシェルが 1 個だけ事前生成して渡す。
#[uniffi::export]
pub fn decode_sample_fact(
    dto: SampleFactDtoV2,
    teams_by_key: BTreeMap<String, TeamId>,
    players_by_key: BTreeMap<String, PlayerId>,
    fallback_id: Uuid,
) -> Result<MatchFact, SampleDtoError> {
    let mut fallback = Some(fallback_id);
    sample_dto::decode_fact(&dto, &teams_by_key, &players_by_key, || {
        fallback.take().expect("factID 無し fact の採番は 1 回だけ")
    })
    .map_err(|error| SampleDtoError::Decode { error })
}

/// `sample_dto::export_match`（domain → DTO。ドメイン → SAMPLE_DTO_V2 の Rust 一本化）。
#[uniffi::export]
pub fn export_sample_match(
    match_: Match,
    home_team: Team,
    away_team: Team,
    home_players: Vec<Player>,
    away_players: Vec<Player>,
    facts: Vec<MatchFact>,
) -> SampleMatchDtoV2 {
    sample_dto::export_match(
        &match_,
        &home_team,
        &away_team,
        &home_players,
        &away_players,
        &facts,
    )
}

/// `sample_dto::encode_sample_match`（Swift JSONEncoder 互換のバイト出力 — share sheet /
/// handball-sample-matches への投入用）。
#[uniffi::export]
pub fn encode_sample_match(dto: SampleMatchDtoV2) -> String {
    sample_dto::encode_sample_match(&dto)
}

/// `sample_dto::default_slug`（エクスポートのファイル名向け ASCII slug）。
#[uniffi::export]
pub fn sample_match_default_slug(match_: Match, home_team: Team, away_team: Team) -> String {
    sample_dto::default_slug(&match_, &home_team, &away_team)
}

// ── import の merge 調停（handball-project#67。発火は ffi_write）──

/// `sample_import::find_team_options`（既存チーム候補のソート + 末尾に「新規作成」）。
#[uniffi::export]
pub fn find_import_team_options(
    parsed_team: SampleTeamDtoV2,
    snapshot: ExistingSnapshot,
) -> Vec<TeamOption> {
    sample_import::find_team_options(&parsed_team, &snapshot)
}

/// `sample_import::default_decisions`（exact → 既存 / partial → 候補先頭 / parsedOnly → 新規）。
#[uniffi::export]
pub fn default_import_decisions(
    home_option: TeamOption,
    away_option: TeamOption,
) -> ImportDecisions {
    sample_import::default_decisions(&home_option, &away_option)
}

/// `sample_import::TeamOption::new_team`（「新規作成」候補。コアは UUID を生成しないため
/// Swift 側の `Identifiable` はシムが補う）。
#[uniffi::export]
pub fn new_import_team_option(parsed_keys: Vec<String>) -> TeamOption {
    TeamOption::new_team(parsed_keys)
}

/// `sample_import::normalize_name`（全角空白 → 半角・空白畳み込み・前後除去）。
#[uniffi::export]
pub fn normalize_import_name(name: String) -> String {
    sample_import::normalize_name(&name)
}

/// `sample_import::required_import_id_count` — `commit_sample_match_import` へ渡す
/// 事前生成 ID の必要数（消費順の知識をシェルへ漏らさない — ADR 0005 決定 4）。
#[uniffi::export]
pub fn sample_import_required_id_count(dto: SampleMatchDtoV2, decisions: ImportDecisions) -> usize {
    sample_import::required_import_id_count(&dto, &decisions)
}

// ── SegmentResolver（object ハンドル — ADR 0004 決定 5）──
//
// Rust のメソッド名は inherent impl と衝突しないよう suffix を付け、
// `name = ...` で FFI 上の名前（= 移植元 Swift API と同形）へ戻す。

#[uniffi::export]
impl SegmentResolver {
    /// `SegmentResolver::build`。構築は 1 回、以後の参照は self + スカラーのみ。
    #[uniffi::constructor(name = "build")]
    pub fn build_from_facts(facts: Vec<MatchFact>) -> Self {
        SegmentResolver::build(&facts)
    }

    /// フィールド `segments` の取得（object はフィールドを直接公開できない）。
    pub fn all_segments(&self) -> Vec<TimeSegment> {
        self.segments.clone()
    }

    /// フィールド `phases` の取得。
    pub fn all_phases(&self) -> Vec<Phase> {
        self.phases.clone()
    }

    /// `resolve_match_clock`（videoClock → matchClock）。
    #[uniffi::method(name = "resolve_match_clock")]
    pub fn resolve_match_clock_ffi(&self, video: VideoClock) -> Option<MatchClock> {
        self.resolve_match_clock(video)
    }

    /// `resolve_video_clock`（matchClock → videoClock）。
    #[uniffi::method(name = "resolve_video_clock")]
    pub fn resolve_video_clock_ffi(&self, match_clock: MatchClock) -> Option<VideoClock> {
        self.resolve_video_clock(match_clock)
    }

    /// `phase_kind`。
    #[uniffi::method(name = "phase_kind")]
    pub fn phase_kind_ffi(&self, match_elapsed_seconds: f64) -> Option<PhaseKind> {
        self.phase_kind(match_elapsed_seconds)
    }

    /// `phase_index`（regular のみカウント。shootout は None）。
    #[uniffi::method(name = "phase_index")]
    pub fn phase_index_ffi(&self, match_elapsed_seconds: f64) -> Option<usize> {
        self.phase_index(match_elapsed_seconds)
    }

    /// `phase_for_match_elapsed`（借用返しは所有値返しに変更 — ADR 0004 決定 4）。
    #[uniffi::method(name = "phase_for_match_elapsed")]
    pub fn phase_for_match_elapsed_ffi(&self, seconds: f64) -> Option<Phase> {
        self.phase_for_match_elapsed(seconds).cloned()
    }

    /// `segment_for_video_elapsed`（同上）。
    #[uniffi::method(name = "segment_for_video_elapsed")]
    pub fn segment_for_video_elapsed_ffi(&self, seconds: f64) -> Option<TimeSegment> {
        self.segment_for_video_elapsed(seconds).cloned()
    }

    /// `segment_for_match_elapsed`（同上。running 優先）。
    #[uniffi::method(name = "segment_for_match_elapsed")]
    pub fn segment_for_match_elapsed_ffi(&self, seconds: f64) -> Option<TimeSegment> {
        self.segment_for_match_elapsed(seconds).cloned()
    }
}
