//! サンプル試合 import の調停・組立の計画層（純粋関数・feature 非依存 — ADR 0005 決定 1 の
//! import 版）。移植元: アプリ層 `SampleMatches/V2/MatchMergerV2.swift` +
//! `MatchImporterV2.swift` の `resolveTeam` / `resolvePlayers` / 組立部（handball-project#67）。
//!
//! 責務は 2 つ:
//! 1. **merge 候補算出** — 「parsed DTO + 既存 snapshot in → 候補 out」。背番号 + 正規化名の
//!    exact / partial 照合、候補ソート、default decisions、名前カノニカライズ。
//! 2. **commit 計画** — 「DTO + decisions + 事前生成 ID in → 保存すべき entity と発火順 out」。
//!
//! 永続化の発火（repository を await する orchestration）は feature `uniffi` 配下の
//! `ffi_write::commit_sample_match_import` が担う。シェルに残るのは取得（HTTP / Bundle）と
//! decisions の UI 選択、そして表示名の解決だけになる。
//!
//! 依存方向: 本モジュールは `sample_dto` と domain の**両方を消費する**上位モジュールで、
//! `sample_dto → domain` の一方通行（ADR 0003）は壊さない。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use uuid::Uuid;

use crate::configuration::MatchConfiguration;
use crate::entities::{Match, Player, RosterSelection, Team};
use crate::facts::MatchFact;
use crate::ids::{MatchId, PlayerId, TeamId};
use crate::sample_dto::{
    SampleMatchDecodeErrorV2, SampleMatchDtoV2, SampleTeamDtoV2, decode_configuration, decode_fact,
};

// ── Targets / Decisions（移植元: MatchMergerV2 の同名型）──

/// チームをどう扱うか（既存に統合 / 新規作成）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum TeamTarget {
    Existing { team_id: TeamId },
    CreateNew,
}

/// 個々の parsed 選手をどう解決するか（既存にマージ / 新規作成）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum PlayerTarget {
    Existing { player_id: PlayerId },
    CreateNew,
}

/// 3 ステップ UI で蓄積したユーザー決定。`players` のキーは `SamplePlayerDtoV2.key`
/// （両チームの選手を 1 マップに統合 — 移植元と同じ）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ImportDecisions {
    pub home_team: TeamTarget,
    pub away_team: TeamTarget,
    pub players: HashMap<String, PlayerTarget>,
}

// ── Snapshot / Options ──

/// merge 計算に必要な既存データの snapshot。
///
/// 移植元は `playersByTeamID: [TeamID: [Player]]` を持っていたが、`Player.team_id` が
/// 所属の一次情報なので平坦な `players` に畳んだ（呼び出し側は team ごとに
/// `loadPlayers(teamId:)` した結果を詰めており、両表現は等価）。
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ExistingSnapshot {
    pub teams: Vec<Team>,
    pub players: Vec<Player>,
}

impl ExistingSnapshot {
    /// 指定チーム所属の既存選手（`teams` / `players` の並び順を保つ）。
    fn players_in(&self, team_id: TeamId) -> Vec<&Player> {
        self.players
            .iter()
            .filter(|player| player.team_id == team_id)
            .collect()
    }
}

/// parsed 選手 1 件が既存選手 1 件に完全一致した組。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ExactMatch {
    pub parsed_key: String,
    pub existing: Player,
}

/// parsed 選手 1 件に対する部分一致候補（背番号 か 正規化名 のいずれか一致）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PartialMatch {
    pub parsed_key: String,
    pub candidates: Vec<Player>,
}

/// チーム選択画面に並ぶ 1 候補。`existing` が `None` なら「新規作成」を表す。
///
/// 移植元の `id: UUID`（SwiftUI `Identifiable` 用）は持たない — コアは UUID を生成しない
/// （設計不変条件 2）。Swift 側はシムの計算プロパティで `Identifiable` を再提供する。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TeamOption {
    pub existing: Option<Team>,
    pub exact_matches: Vec<ExactMatch>,
    pub partial_matches: Vec<PartialMatch>,
    pub parsed_only: Vec<String>,
    pub existing_only: Vec<Player>,
}

impl TeamOption {
    /// 「新規作成」候補（移植元: `TeamOption.newTeam(parsedKeys:)`）。
    pub fn new_team(parsed_keys: Vec<String>) -> Self {
        TeamOption {
            existing: None,
            exact_matches: Vec::new(),
            partial_matches: Vec::new(),
            parsed_only: parsed_keys,
            existing_only: Vec::new(),
        }
    }
}

// ── merge 候補算出（移植元: MatchMergerV2.findTeamOptions / defaultDecisions）──

/// parsed チームについて、既存チーム候補をソートして並べ、末尾に「新規作成」を加えて返す。
///
/// ソート順（移植元の比較子をそのまま保存）:
/// 1. 完全一致数の降順
/// 2. 部分一致数の降順
/// 3. チーム名の昇順
pub fn find_team_options(
    parsed_team: &SampleTeamDtoV2,
    snapshot: &ExistingSnapshot,
) -> Vec<TeamOption> {
    let mut options: Vec<TeamOption> = snapshot
        .teams
        .iter()
        .map(|team| build_option(parsed_team, team, snapshot))
        .collect();

    options.sort_by(|lhs, rhs| {
        rhs.exact_matches
            .len()
            .cmp(&lhs.exact_matches.len())
            .then_with(|| rhs.partial_matches.len().cmp(&lhs.partial_matches.len()))
            .then_with(|| {
                let lhs_name = lhs.existing.as_ref().map(|t| t.name.as_str()).unwrap_or("");
                let rhs_name = rhs.existing.as_ref().map(|t| t.name.as_str()).unwrap_or("");
                lhs_name.cmp(rhs_name)
            })
    });

    options.push(TeamOption::new_team(
        parsed_team
            .players
            .iter()
            .map(|player| player.key.clone())
            .collect(),
    ));
    options
}

/// 選択された home / away 候補から「自動で決められる範囲」の decisions を作る。
///
/// - `exact_matches` → 既存にマージ
/// - `partial_matches` → 候補先頭にマージ（候補が空なら新規作成）
/// - `parsed_only` → 新規作成
pub fn default_decisions(home_option: &TeamOption, away_option: &TeamOption) -> ImportDecisions {
    let mut players: HashMap<String, PlayerTarget> = HashMap::new();
    for option in [home_option, away_option] {
        for exact in &option.exact_matches {
            players.insert(
                exact.parsed_key.clone(),
                PlayerTarget::Existing {
                    player_id: exact.existing.id,
                },
            );
        }
        for partial in &option.partial_matches {
            let target = match partial.candidates.first() {
                Some(first) => PlayerTarget::Existing {
                    player_id: first.id,
                },
                None => PlayerTarget::CreateNew,
            };
            players.insert(partial.parsed_key.clone(), target);
        }
        for key in &option.parsed_only {
            players.insert(key.clone(), PlayerTarget::CreateNew);
        }
    }
    ImportDecisions {
        home_team: team_target(home_option),
        away_team: team_target(away_option),
        players,
    }
}

fn team_target(option: &TeamOption) -> TeamTarget {
    match &option.existing {
        Some(team) => TeamTarget::Existing { team_id: team.id },
        None => TeamTarget::CreateNew,
    }
}

fn build_option(
    parsed_team: &SampleTeamDtoV2,
    existing: &Team,
    snapshot: &ExistingSnapshot,
) -> TeamOption {
    let existing_players = snapshot.players_in(existing.id);

    // 完全一致キー → 既存選手（重複キーは先勝ち — 移植元 `uniquingKeysWith: { lhs, _ in lhs }`）。
    let mut by_exact_key: BTreeMap<String, &Player> = BTreeMap::new();
    for player in &existing_players {
        by_exact_key
            .entry(exact_key(player.jersey_number, &player.name))
            .or_insert(player);
    }
    // 背番号 / 正規化名の逆引き（バケット内は既存選手の並び順を保つ）。
    let mut by_jersey: BTreeMap<i64, Vec<&Player>> = BTreeMap::new();
    for player in &existing_players {
        if let Some(jersey) = player.jersey_number {
            by_jersey.entry(jersey).or_default().push(player);
        }
    }
    let mut by_normalized_name: BTreeMap<String, Vec<&Player>> = BTreeMap::new();
    for player in &existing_players {
        by_normalized_name
            .entry(normalize_name(&player.name))
            .or_default()
            .push(player);
    }

    let mut exact_matches: Vec<ExactMatch> = Vec::new();
    let mut partial_matches: Vec<PartialMatch> = Vec::new();
    let mut parsed_only: Vec<String> = Vec::new();
    let mut consumed: BTreeSet<PlayerId> = BTreeSet::new();

    for parsed in &parsed_team.players {
        let normalized = normalize_name(&parsed.name);
        let key = exact_key(parsed.jersey_number, &parsed.name);

        // 1) 完全一致（背番号 + 正規化名の両方）
        if let Some(existing) = by_exact_key.get(&key)
            && !consumed.contains(&existing.id)
        {
            exact_matches.push(ExactMatch {
                parsed_key: parsed.key.clone(),
                existing: (*existing).clone(),
            });
            consumed.insert(existing.id);
            continue;
        }

        // 2) 部分一致（背番号 か 正規化名 のいずれか）
        let mut candidates: Vec<&Player> = Vec::new();
        if let Some(jersey) = parsed.jersey_number {
            for player in by_jersey.get(&jersey).into_iter().flatten() {
                if !consumed.contains(&player.id) {
                    candidates.push(player);
                }
            }
        }
        for player in by_normalized_name.get(&normalized).into_iter().flatten() {
            if !consumed.contains(&player.id) && !candidates.iter().any(|c| c.id == player.id) {
                candidates.push(player);
            }
        }

        if candidates.is_empty() {
            parsed_only.push(parsed.key.clone());
        } else {
            partial_matches.push(PartialMatch {
                parsed_key: parsed.key.clone(),
                candidates: candidates.into_iter().cloned().collect(),
            });
        }
    }

    // existingOnly = exact / partial のどちらの候補にも現れなかった既存選手。
    let mut referenced = consumed;
    for partial in &partial_matches {
        for player in &partial.candidates {
            referenced.insert(player.id);
        }
    }
    let existing_only = existing_players
        .into_iter()
        .filter(|player| !referenced.contains(&player.id))
        .cloned()
        .collect();

    TeamOption {
        existing: Some(existing.clone()),
        exact_matches,
        partial_matches,
        parsed_only,
        existing_only,
    }
}

fn exact_key(jersey: Option<i64>, name: &str) -> String {
    let jersey = jersey.map(|j| j.to_string()).unwrap_or_else(|| "-".into());
    format!("{jersey}|{}", normalize_name(name))
}

/// 選手名・チーム名のカノニカル形式に揃える。
///
/// 全角スペース（U+3000）→ 半角スペース、連続する空白を 1 個に畳む、前後の空白を除去。
/// 異体字（髙↔高、﨑↔崎 等）は対象外 — マージ判定 UI で人手判断に委ねる方針（移植元と同じ）。
pub fn normalize_name(name: &str) -> String {
    name.replace('\u{3000}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ── commit 計画（移植元: MatchImporterV2.commit の resolveTeam / resolvePlayers / 組立）──

/// import commit で保存すべきものと発火順。
///
/// entity（team / player / match）の順序は移植元をそのまま保存する。
/// facts のみ **永続化順へ整列する**（`sort_by_persistence_order` を参照 —
/// 移植元から意図的に乖離。handball-project#72）。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportCommitPlan {
    /// 新規作成するチームのみ（既存に統合したチームは save しない）。home → away の順。
    pub teams_to_save: Vec<Team>,
    /// 新規作成する選手のみ。home 所属 → away 所属の順（各チーム内は DTO の並び順）。
    pub players_to_save: Vec<Player>,
    pub r#match: Match,
    /// 永続化順（累積秒 → recordedAt → id）へ整列済み。DTO の並び順ではない。
    pub facts: Vec<MatchFact>,
    pub outcome: ImportCommitOutcome,
}

/// commit の結果集計（wizard の完了画面で表示する）。
///
/// 表示名（existing なら DB の現名称、createNew なら DTO 名）はシェルが解決する —
/// 検証入力ではない read をコアの repository 契約へ足さないため（ADR 0005 決定 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ImportCommitOutcome {
    pub match_id: MatchId,
    pub home_team_id: TeamId,
    pub away_team_id: TeamId,
    pub teams_created: u32,
    pub teams_reused: u32,
    pub players_created: u32,
    pub players_reused: u32,
    pub fact_count: u32,
}

/// `import_commit_plan` が消費する新規 ID の数。
///
/// 内訳は生成順どおり: 新規 home team + 新規 away team + 新規 home 選手 + 新規 away 選手 +
/// match 1 + `factID` 無し fact。消費順の知識をシェルへ漏らさないためコア側に置く
/// （ADR 0005 決定 4 / ADR 0004 決定 2 の `required_id_count` と同型の供給契約）。
pub fn required_import_id_count(dto: &SampleMatchDtoV2, decisions: &ImportDecisions) -> usize {
    let new_team_count = [decisions.home_team, decisions.away_team]
        .iter()
        .filter(|target| matches!(target, TeamTarget::CreateNew))
        .count();
    let new_player_count = [&dto.teams.home, &dto.teams.away]
        .iter()
        .flat_map(|team| team.players.iter())
        .filter(|player| {
            !matches!(
                decisions.players.get(&player.key),
                Some(PlayerTarget::Existing { .. })
            )
        })
        .count();
    let fallback_fact_count = dto.facts.iter().filter(|f| f.fact_id.is_none()).count();
    new_team_count + new_player_count + 1 + fallback_fact_count
}

/// DTO + decisions から「保存すべき entity と発火順」を組む。
///
/// - 既存に統合 → 該当 ID を Match / Fact 内の参照に使う（Team / Player 自体は save しない）
/// - 新規作成 → 供給された ID で Team / Player を組み、save 対象に積む
/// - decisions に無い選手キーは新規作成（移植元 `decisions[key] ?? .createNew`）
pub fn import_commit_plan(
    dto: &SampleMatchDtoV2,
    decisions: &ImportDecisions,
    mut new_id: impl FnMut() -> Uuid,
) -> Result<ImportCommitPlan, SampleMatchDecodeErrorV2> {
    let mut teams_to_save: Vec<Team> = Vec::new();
    let mut teams_created: u32 = 0;
    let mut teams_reused: u32 = 0;

    let home_team_id = resolve_team(
        decisions.home_team,
        &dto.teams.home,
        &mut new_id,
        &mut teams_to_save,
        &mut teams_created,
        &mut teams_reused,
    );
    let away_team_id = resolve_team(
        decisions.away_team,
        &dto.teams.away,
        &mut new_id,
        &mut teams_to_save,
        &mut teams_created,
        &mut teams_reused,
    );

    let mut teams_by_key: BTreeMap<String, TeamId> = BTreeMap::new();
    teams_by_key.insert(dto.teams.home.key.clone(), home_team_id);
    teams_by_key.insert(dto.teams.away.key.clone(), away_team_id);

    let mut players_by_key: BTreeMap<String, PlayerId> = BTreeMap::new();
    let mut players_to_save: Vec<Player> = Vec::new();
    let mut players_created: u32 = 0;
    let mut players_reused: u32 = 0;
    for (parsed_team, team_id) in [
        (&dto.teams.home, home_team_id),
        (&dto.teams.away, away_team_id),
    ] {
        resolve_players(
            parsed_team,
            team_id,
            &decisions.players,
            &mut new_id,
            &mut players_by_key,
            &mut players_to_save,
            &mut players_created,
            &mut players_reused,
        );
    }

    let configuration: MatchConfiguration = decode_configuration(&dto.r#match.configuration)?;
    let match_ = Match {
        id: MatchId(new_id()),
        title: dto.r#match.display_name.clone(),
        date: dto.r#match.date,
        home_team_id,
        away_team_id,
        configuration,
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    };

    let mut facts = dto
        .facts
        .iter()
        .map(|fact_dto| decode_fact(fact_dto, &teams_by_key, &players_by_key, &mut new_id))
        .collect::<Result<Vec<_>, _>>()?;
    sort_by_persistence_order(&mut facts);

    let outcome = ImportCommitOutcome {
        match_id: match_.id,
        home_team_id,
        away_team_id,
        teams_created,
        teams_reused,
        players_created,
        players_reused,
        fact_count: facts.len() as u32,
    };
    Ok(ImportCommitPlan {
        teams_to_save,
        players_to_save,
        r#match: match_,
        facts,
        outcome,
    })
}

/// fact 列を永続化順（`PERSISTENCE_MODEL_V1` の sort 規約: 累積秒 → recordedAt → id）へ整列する。
///
/// DTO の `facts` は **記録順（recordedAt 順）** であって時刻順とは限らない。実際、配信中の
/// `.video` サンプルは phase 開始（videoClock 1086s）が配列の 38 番目にあり、
/// それより後ろの時刻（1130s〜）の play が先に並んでいる。
///
/// 一方 `commit_sample_match_import` は facts を 1 件ずつ `record_append_fact` で発火し、
/// その都度 **whole-log 検証**（`validate_fact_log`）を通す。この検証は
/// 「facts は永続化順で並んでいる前提」で書かれており、かつ R3 / R5 は
/// 「fact が 1 件以上あって PhaseStart が無い」だけで落ちる。
/// そのため DTO 順のまま append すると、`.video` は最初の play を積んだ時点で
/// `VideoWithFactsMissingPhaseStart` により必ず失敗していた（handball-project#72）。
///
/// 整列は shell 側ではなくここで行う。保存順序の所有はコア側にある（ADR 0005 決定 2 追記）ため。
/// 読み出し側（`SwiftDataMatchRepository.factRecordOrder`）と同じ規約に揃えてあるので、
/// 整列しても永続化後の観測結果は変わらない。
///
/// 注: 移植元 Swift（`MatchImporterV2.commit` の for ループ）は整列しない。ここは
/// **意図的にオラクルから乖離**する — 移植元は同じ経路で `.video` を取り込めないため
/// （iOS の公開サンプルは read-only の in-memory repository 経由で、書き込み検証を通らない）。
fn sort_by_persistence_order(facts: &mut [MatchFact]) {
    facts.sort_by(|lhs, rhs| {
        cumulative_seconds(lhs)
            .total_cmp(&cumulative_seconds(rhs))
            .then_with(|| lhs.recorded_at.cmp(&rhs.recorded_at))
            .then_with(|| lhs.id.cmp(&rhs.id))
    });
}

/// 整列キーの代表時刻。累積秒（matchClock）を優先し、無ければ動画秒を使う。
/// どちらも無い fact は末尾へ寄せる（読み出し側の `?? .infinity` と同じ扱い）。
fn cumulative_seconds(fact: &MatchFact) -> f64 {
    let anchor = fact.anchor();
    anchor
        .match_elapsed_seconds()
        .or_else(|| anchor.video_elapsed_seconds())
        .unwrap_or(f64::INFINITY)
}

#[allow(clippy::too_many_arguments)]
fn resolve_team(
    target: TeamTarget,
    parsed_team: &SampleTeamDtoV2,
    new_id: &mut impl FnMut() -> Uuid,
    to_save: &mut Vec<Team>,
    created: &mut u32,
    reused: &mut u32,
) -> TeamId {
    match target {
        TeamTarget::Existing { team_id } => {
            *reused += 1;
            team_id
        }
        TeamTarget::CreateNew => {
            let team = Team {
                id: TeamId(new_id()),
                name: parsed_team.name.clone(),
            };
            let id = team.id;
            to_save.push(team);
            *created += 1;
            id
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_players(
    parsed_team: &SampleTeamDtoV2,
    team_id: TeamId,
    decisions: &HashMap<String, PlayerTarget>,
    new_id: &mut impl FnMut() -> Uuid,
    players_by_key: &mut BTreeMap<String, PlayerId>,
    to_save: &mut Vec<Player>,
    created: &mut u32,
    reused: &mut u32,
) {
    for parsed in &parsed_team.players {
        match decisions.get(&parsed.key) {
            Some(PlayerTarget::Existing { player_id }) => {
                players_by_key.insert(parsed.key.clone(), *player_id);
                *reused += 1;
            }
            // decisions に無いキーは新規作成（移植元 `?? .createNew`）。
            Some(PlayerTarget::CreateNew) | None => {
                let player = Player {
                    id: PlayerId(new_id()),
                    team_id,
                    name: parsed.name.clone(),
                    jersey_number: parsed.jersey_number,
                    photo: None,
                };
                players_by_key.insert(parsed.key.clone(), player.id);
                to_save.push(player);
                *created += 1;
            }
        }
    }
}
