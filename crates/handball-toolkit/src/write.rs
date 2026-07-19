//! 書き込み経路の計画層（純粋関数・feature 非依存 — ADR 0005 決定 1）。
//!
//! 「検証入力をどう組むか・何をどの順に保存すべきか」の判断を、fact 列 in → 導出結果 out の
//! 純粋関数として置く。永続化の発火（repository を await する orchestration）は
//! feature `uniffi` 配下の `ffi_write` が担う。

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::clock::{FactAnchor, MatchClock, VideoClock};
use crate::configuration::{MatchConfiguration, PhaseKind};
use crate::entities::Match;
use crate::facts::{ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload};
use crate::ids::{FactId, PlayerId, TeamId};
use crate::projection::SegmentResolver;
use crate::validators::RosterContext;

/// home / away 所属選手 1 件の (player, team) 参照。
/// `MatchWriteRepository::load_roster_players` が返す roster 構築材料。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PlayerTeamRef {
    pub player_id: PlayerId,
    pub team_id: TeamId,
}

/// 所属選手一覧から validation 用の `RosterContext` を組む。
///
/// 0 件なら `None` = 参照整合チェックを skip する後方互換ルール（移植元:
/// `SwiftDataMatchRepository.loadRosterContext` の `guard !players.isEmpty else { return nil }`。
/// この判断はシェルからコアへ移した — ADR 0005 決定 1）。
/// 同一 player の重複は先勝ち（移植元 `uniquingKeysWith: { first, _ in first }` と同じ）。
pub fn roster_context_from_players(
    home_team_id: TeamId,
    away_team_id: TeamId,
    players: &[PlayerTeamRef],
) -> Option<RosterContext> {
    if players.is_empty() {
        return None;
    }
    let mut player_team_lookup = BTreeMap::new();
    let mut known_player_ids = BTreeSet::new();
    for player in players {
        player_team_lookup
            .entry(player.player_id)
            .or_insert(player.team_id);
        known_player_ids.insert(player.player_id);
    }
    Some(RosterContext {
        home_team_id,
        away_team_id,
        player_team_lookup,
        known_player_ids: Some(known_player_ids),
    })
}

// ── タイマーモードの phase 自動補完（移植元: RecordingScreenStore.ensureTimerPhasesCovering）──

/// コアが新規 fact を組むための (id, recorded_at) ペア。シェルが必要数だけ事前生成して渡す
/// （ADR 0005 決定 4 — コアは now() / UUID 生成を持たない。sample_dto の
/// `required_id_count` + `new_ids` と同型の供給契約）。
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NewFactStamp {
    pub id: FactId,
    pub recorded_at: DateTime<Utc>,
}

/// 補完すべき D-snap 区間 `[(k-1)·D, k·D]`（1-based k、昇順）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseCompletionSlot {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// `fact` をタイマーモードで永続化する直前に auto-create すべき regular phase 区間を返す。
///
/// - D = `phase_duration_seconds`、phase N = `[(N-1)·D, N·D]`。記録時刻を含む D-snap phase と
///   その手前の欠け phase を昇順で列挙する（出現順導出とクロック位置導出を一致させる連鎖作成）
/// - `.video` / `.videoHighlight`・D <= 0 は空（動画は videoClock 基準で明示 phase 開始）
/// - PhaseStart fact 自身は補完しない（明示 phase 管理はユーザーダイアログ経由 — `startPhase`）
/// - 記録時刻は `fact` の matchClock anchor から取る（無ければ 0 = phase 1 のみ確保）
pub fn phase_completion_plan(
    match_: &Match,
    existing_facts: &[MatchFact],
    fact: &MatchFact,
) -> Vec<PhaseCompletionSlot> {
    if matches!(
        fact.payload,
        MatchFactPayload::Control(ControlFact::PhaseStart(_))
    ) {
        return Vec::new();
    }
    let MatchConfiguration::Timer {
        phase_duration_seconds: duration,
    } = match_.configuration
    else {
        return Vec::new();
    };
    if duration <= 0.0 {
        return Vec::new();
    }

    let seconds = fact
        .anchor()
        .match_clock()
        .map(|clock| clock.elapsed_seconds)
        .unwrap_or(0.0);
    let target_index = (seconds.max(0.0) / duration).floor() as i64 + 1;
    if target_index < 1 {
        return Vec::new();
    }

    // 既存 regular phase が満たす D-snap interval index (1-based) を集める。
    let resolver = SegmentResolver::build(existing_facts);
    let mut covered = BTreeSet::new();
    for phase in &resolver.phases {
        if phase.kind != PhaseKind::Regular {
            continue;
        }
        let Some(start) = phase.match_elapsed_start else {
            continue;
        };
        let index = (start / duration).round() as i64 + 1;
        if index >= 1 {
            covered.insert(index);
        }
    }

    (1..=target_index)
        .filter(|k| !covered.contains(k))
        .map(|k| PhaseCompletionSlot {
            start_seconds: (k - 1) as f64 * duration,
            end_seconds: k as f64 * duration,
        })
        .collect()
}

/// 補完 slot + スタンプから regular PhaseStart fact を組む（発火層が消費順に使う）。
pub fn phase_completion_fact(slot: PhaseCompletionSlot, stamp: NewFactStamp) -> MatchFact {
    MatchFact {
        id: stamp.id,
        recorded_at: stamp.recorded_at,
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: slot.start_seconds,
            }),
            end_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: slot.end_seconds,
            }),
        })),
    }
}

// ── タイマー → 動画移行の commit 計画（移植元: MigrateToVideoStore.buildUpdatedFacts）──

/// video 移行のユーザー入力: control fact 1 件に対する video 区間。
/// matchClock 側は fact 自身が持つ値を使う（draft の matchClock は facts の
/// read-only ミラーであり、コアは DB 真実から同じ値を導く）。
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct VideoSyncInput {
    pub fact_id: FactId,
    pub video_start_seconds: f64,
    pub video_end_seconds: f64,
}

/// video 移行 commit の計画が成立しない理由（発火層が `CoreWriteError` へ写像する）。
/// wizard 側の事前 validation が通っていれば実行時には到達しない安全網。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoMigrationPlanError {
    MissingPhaseSync {
        fact_id: FactId,
    },
    MissingStoppageSync {
        fact_id: FactId,
    },
    CannotResolveVideoClock {
        fact_id: FactId,
        match_clock_seconds: f64,
    },
}

/// 更新後の facts（control 全部 → play 全部の順）を構築する。
///
/// - PhaseStart: anchor を `.both(matchClock: 既存, videoClock: ユーザー入力)` に書き換え
/// - Stoppage: 同上 + endAnchor を追加（Stoppage 中に matchClock は進まないため
///   end の matchClock は start と同値）
/// - Play: 更新済み control から組んだ `SegmentResolver` で videoClock を導出し
///   `.videoClock` 単独に書き換え（既に videoClock / both なら触らない — 安全側）
///
/// 返り順（control → play）が commit の発火順。play 変換時点で全 phase が
/// video range 化済み = R7（play が phase range 内)も満たす順序設計。
pub fn video_migration_plan(
    facts: &[MatchFact],
    phase_syncs: &[VideoSyncInput],
    stoppage_syncs: &[VideoSyncInput],
) -> Result<Vec<MatchFact>, VideoMigrationPlanError> {
    let phase_by_id: BTreeMap<FactId, &VideoSyncInput> =
        phase_syncs.iter().map(|s| (s.fact_id, s)).collect();
    let stoppage_by_id: BTreeMap<FactId, &VideoSyncInput> =
        stoppage_syncs.iter().map(|s| (s.fact_id, s)).collect();

    let mut updated_control: Vec<MatchFact> = Vec::new();
    let mut plays_to_convert: Vec<MatchFact> = Vec::new();

    for fact in facts {
        match &fact.payload {
            MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
                let sync = phase_by_id
                    .get(&fact.id)
                    .ok_or(VideoMigrationPlanError::MissingPhaseSync { fact_id: fact.id })?;
                let match_start = payload
                    .start_anchor
                    .match_clock()
                    .map(|c| c.elapsed_seconds)
                    .unwrap_or(0.0);
                let match_end = payload
                    .end_anchor
                    .match_clock()
                    .map(|c| c.elapsed_seconds)
                    .unwrap_or(0.0);
                let mut new_payload = *payload;
                new_payload.start_anchor = FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: match_start,
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: sync.video_start_seconds,
                    },
                };
                new_payload.end_anchor = FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: match_end,
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: sync.video_end_seconds,
                    },
                };
                let mut new_fact = fact.clone();
                new_fact.payload = MatchFactPayload::Control(ControlFact::PhaseStart(new_payload));
                updated_control.push(new_fact);
            }
            MatchFactPayload::Control(ControlFact::Stoppage(payload)) => {
                let sync = stoppage_by_id
                    .get(&fact.id)
                    .ok_or(VideoMigrationPlanError::MissingStoppageSync { fact_id: fact.id })?;
                let match_start = payload
                    .start_anchor
                    .match_clock()
                    .map(|c| c.elapsed_seconds)
                    .unwrap_or(0.0);
                let mut new_payload = payload.clone();
                new_payload.start_anchor = FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: match_start,
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: sync.video_start_seconds,
                    },
                };
                new_payload.end_anchor = Some(FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: match_start,
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: sync.video_end_seconds,
                    },
                });
                let mut new_fact = fact.clone();
                new_fact.payload = MatchFactPayload::Control(ControlFact::Stoppage(new_payload));
                updated_control.push(new_fact);
            }
            MatchFactPayload::Play(_) => plays_to_convert.push(fact.clone()),
        }
    }

    // 更新済み control 全部から SegmentResolver を構築し、play fact の anchor を変換。
    let resolver = SegmentResolver::build(&updated_control);
    let mut updated_plays: Vec<MatchFact> = Vec::new();
    for mut fact in plays_to_convert {
        let MatchFactPayload::Play(play) = &mut fact.payload else {
            unreachable!("plays_to_convert は Play のみ");
        };
        let FactAnchor::MatchClock(match_clock) = play.anchor else {
            // 既に videoClock / both なら触らない（安全側）。
            updated_plays.push(fact);
            continue;
        };
        let video = resolver.resolve_video_clock(match_clock).ok_or(
            VideoMigrationPlanError::CannotResolveVideoClock {
                fact_id: fact.id,
                match_clock_seconds: match_clock.elapsed_seconds,
            },
        )?;
        play.anchor = FactAnchor::VideoClock(video);
        updated_plays.push(fact);
    }

    updated_control.extend(updated_plays);
    Ok(updated_control)
}
