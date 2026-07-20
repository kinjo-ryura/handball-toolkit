//! 書き込み経路の計画層（純粋関数・feature 非依存 — ADR 0005 決定 1）。
//!
//! 「検証入力をどう組むか・何をどの順に保存すべきか」の判断を、fact 列 in → 導出結果 out の
//! 純粋関数として置く。永続化の発火（repository を await する orchestration）は
//! feature `uniffi` 配下の `ffi_write` が担う。

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::clock::{FactAnchor, MatchClock, VideoClock};
use crate::configuration::{MatchConfiguration, PhaseKind, VideoSource};
use crate::entities::Match;
use crate::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
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

// ── 移行ウィザードの draft 事前検証（移植元: VideoModeMigrationValidator）──

/// video 移行のユーザー入力（wizard 編集途中）: control fact 1 件に対する video 区間。
/// commit 用の `VideoSyncInput` と違い未入力（None）を許す。
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct VideoSyncDraftInput {
    pub fact_id: FactId,
    pub video_start_seconds: Option<f64>,
    pub video_end_seconds: Option<f64>,
}

/// draft 検証の違反種別。文言と wizard step への写像はシェル所有（ADR 0002 と同じ分担）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum VideoMigrationDraftIssue {
    /// 移行対象が `.timer` 試合でない（`.video` / `.videoHighlight` には適用不可）。
    SourceConfigurationNotTimer,
    /// video source が未確定（URL 未入力 / 解析不可。URL 解析はシェルの責務）。
    MissingVideoSource,
    /// PhaseSync の videoClock start が未入力。
    MissingPhaseVideoStart { fact_id: FactId },
    /// PhaseSync の videoClock end が未入力。
    MissingPhaseVideoEnd { fact_id: FactId },
    /// PhaseSync の videoClock end が start 以下。
    PhaseVideoEndBeforeStart { fact_id: FactId },
    /// 2 phase の videoClock 範囲が overlap している。
    PhaseVideoRangesOverlap {
        first_fact_id: FactId,
        second_fact_id: FactId,
    },
    /// StoppageSync の videoClock start が未入力。
    MissingStoppageVideoStart { fact_id: FactId },
    /// StoppageSync の videoClock end が未入力。
    MissingStoppageVideoEnd { fact_id: FactId },
    /// StoppageSync の videoClock end が start 以下。
    StoppageVideoEndBeforeStart { fact_id: FactId },
    /// Stoppage の videoClock 範囲が phase 範囲外。
    StoppageVideoOutsidePhaseRange { fact_id: FactId },
    /// 2 stoppage の videoClock 範囲が overlap している。
    StoppageVideoRangesOverlap {
        first_fact_id: FactId,
        second_fact_id: FactId,
    },
}

/// 移行ウィザードの draft 全体を検証する（移植元: `VideoModeMigrationValidator.validate`。
/// 放出順まで同セマンティクス）。
///
/// 検証ルール:
/// 1. 移行対象が `.timer` 試合であること
/// 2. video source が確定していること（有無のみ。URL 解析はシェル）
/// 3. PhaseSync: videoStart / videoEnd 入力済み・end > start・2 phase の範囲が overlap しない
/// 4. StoppageSync: 同上 + 範囲がいずれかの phase 範囲内に収まること
///
/// commit 時の安全網は `video_migration_plan`（存在・導出可否）と逐次 validation が担い、
/// 本関数は wizard の「次へ」活性・フィールド hint のための事前検証を一手に持つ。
pub fn validate_video_migration_draft(
    source_configuration: &MatchConfiguration,
    video_source: Option<&VideoSource>,
    phase_syncs: &[VideoSyncDraftInput],
    stoppage_syncs: &[VideoSyncDraftInput],
) -> Vec<VideoMigrationDraftIssue> {
    let mut issues: Vec<VideoMigrationDraftIssue> = Vec::new();

    if !matches!(source_configuration, MatchConfiguration::Timer { .. }) {
        issues.push(VideoMigrationDraftIssue::SourceConfigurationNotTimer);
    }
    if video_source.is_none() {
        issues.push(VideoMigrationDraftIssue::MissingVideoSource);
    }

    validate_phase_sync_drafts(phase_syncs, &mut issues);
    validate_stoppage_sync_drafts(stoppage_syncs, phase_syncs, &mut issues);
    issues
}

/// 入力完了（start / end 両方あり・end > start）の sync だけを (fact_id, start, end) に絞る。
fn completed_draft_ranges(syncs: &[VideoSyncDraftInput]) -> Vec<(FactId, f64, f64)> {
    syncs
        .iter()
        .filter_map(|s| match (s.video_start_seconds, s.video_end_seconds) {
            (Some(start), Some(end)) if end > start => Some((s.fact_id, start, end)),
            _ => None,
        })
        .collect()
}

fn validate_phase_sync_drafts(
    syncs: &[VideoSyncDraftInput],
    issues: &mut Vec<VideoMigrationDraftIssue>,
) {
    for sync in syncs {
        if sync.video_start_seconds.is_none() {
            issues.push(VideoMigrationDraftIssue::MissingPhaseVideoStart {
                fact_id: sync.fact_id,
            });
        }
        if sync.video_end_seconds.is_none() {
            issues.push(VideoMigrationDraftIssue::MissingPhaseVideoEnd {
                fact_id: sync.fact_id,
            });
        }
        if let (Some(start), Some(end)) = (sync.video_start_seconds, sync.video_end_seconds)
            && end <= start
        {
            issues.push(VideoMigrationDraftIssue::PhaseVideoEndBeforeStart {
                fact_id: sync.fact_id,
            });
        }
    }

    let completed = completed_draft_ranges(syncs);
    for i in 0..completed.len() {
        for j in (i + 1)..completed.len() {
            let (id_a, start_a, end_a) = completed[i];
            let (id_b, start_b, end_b) = completed[j];
            if start_a < end_b && start_b < end_a {
                issues.push(VideoMigrationDraftIssue::PhaseVideoRangesOverlap {
                    first_fact_id: id_a,
                    second_fact_id: id_b,
                });
            }
        }
    }
}

fn validate_stoppage_sync_drafts(
    syncs: &[VideoSyncDraftInput],
    phase_syncs: &[VideoSyncDraftInput],
    issues: &mut Vec<VideoMigrationDraftIssue>,
) {
    for sync in syncs {
        if sync.video_start_seconds.is_none() {
            issues.push(VideoMigrationDraftIssue::MissingStoppageVideoStart {
                fact_id: sync.fact_id,
            });
        }
        if sync.video_end_seconds.is_none() {
            issues.push(VideoMigrationDraftIssue::MissingStoppageVideoEnd {
                fact_id: sync.fact_id,
            });
        }
        if let (Some(start), Some(end)) = (sync.video_start_seconds, sync.video_end_seconds)
            && end <= start
        {
            issues.push(VideoMigrationDraftIssue::StoppageVideoEndBeforeStart {
                fact_id: sync.fact_id,
            });
        }
    }

    // Stoppage の videoClock 範囲がいずれかの phase 範囲内かチェック（入力完了分のみ）。
    let phase_ranges = completed_draft_ranges(phase_syncs);
    for (fact_id, start, end) in completed_draft_ranges(syncs) {
        let contained = phase_ranges
            .iter()
            .any(|(_, phase_start, phase_end)| start >= *phase_start && end <= *phase_end);
        if !contained {
            issues.push(VideoMigrationDraftIssue::StoppageVideoOutsidePhaseRange { fact_id });
        }
    }

    let completed = completed_draft_ranges(syncs);
    for i in 0..completed.len() {
        for j in (i + 1)..completed.len() {
            let (id_a, start_a, end_a) = completed[i];
            let (id_b, start_b, end_b) = completed[j];
            if start_a < end_b && start_b < end_a {
                issues.push(VideoMigrationDraftIssue::StoppageVideoRangesOverlap {
                    first_fact_id: id_a,
                    second_fact_id: id_b,
                });
            }
        }
    }
}

// ── 記録入口の純粋ヘルパー（移植元: RecordingScreenStore の残留計算 — handball-project#69）──
//
// 「記録操作 in → fact / anchor out」の粗い粒度で置く（設計不変条件 4）。clamp・正規化・
// anchor の場合分けは各入口の内部に吸収し、シェルには状態保持と橋渡しだけを残す。
// タイマーの delta 加算（now - last）は Date 演算かつ 2Hz 経路であり、ドメイン規則ではなく
// シェルの UI 状態遷移なのでコアには持ち込まない。

/// 記録時に anchor をどの時計で組むか（capture method に対応）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum CaptureClockKind {
    /// タイマーモード: 試合タイマーの累積秒を基準にする。
    MatchClock,
    /// 動画モード / ハイライト: 動画の再生位置を基準にする。
    VideoClock,
}

/// play event を捕捉した瞬間の anchor を組む（移植元: `RecordingScreenStore.capturePlayEvent` /
/// `recordFreeNote` / `capturePlayEventInVideoMode` の `max(0, base - offset)`）。
///
/// `recording_offset_seconds` は「事象が起きてからボタンを押すまでの遅れ」の補正で、
/// 基準秒から引く。結果が負になったら 0 にクランプする（時計は負にならない）。
pub fn capture_play_anchor(
    base_seconds: f64,
    recording_offset_seconds: f64,
    clock_kind: CaptureClockKind,
) -> FactAnchor {
    let elapsed_seconds = (base_seconds - recording_offset_seconds).max(0.0);
    match clock_kind {
        CaptureClockKind::MatchClock => FactAnchor::MatchClock(MatchClock { elapsed_seconds }),
        CaptureClockKind::VideoClock => FactAnchor::VideoClock(VideoClock { elapsed_seconds }),
    }
}

/// 記録画面を開いたときのタイマー初期累積秒（移植元: `RecordingScreenStore.lastPlayMatchClock`
/// + `load()` の `?? 0`）。
///
/// fact 列を末尾から走査し、最初に見つかった play fact の matchClock を返す。
/// play fact が無い / 直近 play が videoClock 単独なら 0（タイマーは頭出し）。
pub fn initial_timer_seconds(facts: &[MatchFact]) -> f64 {
    facts
        .iter()
        .rev()
        .find_map(|fact| match &fact.payload {
            MatchFactPayload::Play(play) => Some(play.anchor.match_clock()),
            MatchFactPayload::Control(_) => None,
        })
        .flatten()
        .map(|clock| clock.elapsed_seconds)
        .unwrap_or(0.0)
}

/// 新規 play fact を組む（移植元: `confirmPlayEvent` / `confirmPendingFreeNote` /
/// `recordFreeNote` の fact 生成）。
///
/// `title` / `note` は正規化せずそのまま載せる（移植元の挙動 — ADR 0005 決定 7 のパリティ維持）。
pub fn build_play_fact(
    stamp: NewFactStamp,
    kind: PlayEventKind,
    team_id: Option<TeamId>,
    player_id: Option<PlayerId>,
    anchor: FactAnchor,
    title: Option<String>,
    note: Option<String>,
) -> MatchFact {
    MatchFact {
        id: stamp.id,
        recorded_at: stamp.recorded_at,
        payload: MatchFactPayload::Play(PlayFact {
            kind,
            team_id,
            player_id,
            related_player_id: None,
            anchor,
            title,
            note,
        }),
    }
}

/// 新規 stoppage fact を組む（移植元: `recordTimeout` / `recordTimerPause` /
/// `recordVideoStoppage` の fact 生成）。
///
/// `end_anchor` はタイマーモードでは None（開始のみの marker）、動画モードでは区間の終端。
/// `note` の扱いは `build_play_fact` と同じ（正規化しない — パリティ維持）。
pub fn build_stoppage_fact(
    stamp: NewFactStamp,
    kind: StoppageKind,
    start_anchor: FactAnchor,
    end_anchor: Option<FactAnchor>,
    note: Option<String>,
) -> MatchFact {
    MatchFact {
        id: stamp.id,
        recorded_at: stamp.recorded_at,
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind,
            start_anchor,
            end_anchor,
            note,
        })),
    }
}

/// 既存 play fact への 1 操作分の編集（移植元: `RecordingScreenStore` の
/// `updateFactNote` / `updateFactTitle` / `updateFactPlayer` / `updateFactKind` /
/// `updateFactMatchClock` / `updateFactVideoClock`）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum PlayFactEdit {
    /// メモを差し替える（前後空白除去 → 空文字なら None）。
    Note { text: Option<String> },
    /// タイトルを差し替える（同上）。
    Title { text: Option<String> },
    /// 選手を差し替える（None で選手なしにする）。
    Player { player_id: Option<PlayerId> },
    /// イベント種別を差し替える。
    Kind { kind: PlayEventKind },
    /// matchClock を差し替える（タイマーモード想定）。anchor は `.matchClock` 単独になる。
    MatchClock { elapsed_seconds: f64 },
    /// videoClock を差し替える（動画モード想定）。`.matchClock` 単独の fact は変更しない。
    VideoClock { elapsed_seconds: f64 },
}

/// play fact に編集を 1 件適用した結果を返す。
pub fn apply_play_fact_edit(play: PlayFact, edit: PlayFactEdit) -> PlayFact {
    let mut play = play;
    match edit {
        PlayFactEdit::Note { text } => play.note = normalize_optional_text(text),
        PlayFactEdit::Title { text } => play.title = normalize_optional_text(text),
        PlayFactEdit::Player { player_id } => play.player_id = player_id,
        PlayFactEdit::Kind { kind } => play.kind = kind,
        PlayFactEdit::MatchClock { elapsed_seconds } => {
            play.anchor = FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: elapsed_seconds.max(0.0),
            });
        }
        PlayFactEdit::VideoClock { elapsed_seconds } => {
            let video_clock = VideoClock {
                elapsed_seconds: elapsed_seconds.max(0.0),
            };
            play.anchor = match play.anchor {
                FactAnchor::VideoClock(_) => FactAnchor::VideoClock(video_clock),
                FactAnchor::Both { match_clock, .. } => FactAnchor::Both {
                    match_clock,
                    video_clock,
                },
                // matchClock 単独の fact に videoClock だけ与えても sync 点は決まらないため触らない。
                FactAnchor::MatchClock(_) => play.anchor,
            };
        }
    }
    play
}

/// 任意テキストの正規化: 前後の空白・改行を除去し、空文字になったら None にする。
///
/// 移植元: `RecordingScreenStore` の
/// `let trimmed = text?.trimmingCharacters(in: .whitespacesAndNewlines)` +
/// `(trimmed?.isEmpty == false) ? trimmed : nil`。
fn normalize_optional_text(text: Option<String>) -> Option<String> {
    let trimmed = text?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
