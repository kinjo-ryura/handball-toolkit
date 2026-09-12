//! 書き込み経路の計画層（純粋関数・feature 非依存 — ADR 0005 決定 1）。
//!
//! 「検証入力をどう組むか・何をどの順に保存すべきか」の判断を、fact 列 in → 導出結果 out の
//! 純粋関数として置く。永続化の発火（repository を await する orchestration）は
//! feature `uniffi` 配下の `ffi_write` が担う。

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};
use crate::configuration::{MatchConfiguration, MatchConfigurationKind, PhaseKind, VideoSource};
use crate::entities::Match;
use crate::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    PossessionFact, StoppageKind, StoppagePayload,
};
use crate::ids::{FactId, PlayerId, TeamId};
use crate::projection::SegmentResolver;
use crate::validators::{self, RosterContext};

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

/// 補完すべき regular phase 区間（昇順）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseCompletionSlot {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// `fact` をタイマーモードで永続化する直前に auto-create すべき regular phase 区間を返す。
///
/// **起点は「直前までの長さの累計」**（handball-project#352）。既存の regular phase の
/// 終端から鎖を伸ばし、記録時刻を含むところまで phase を並べる。
///
/// かつては `[(N-1)·D, N·D]` という「番号 × 規定長」で区間を作っていたが、phase の長さを
/// 編集できるようになると破綻する — 前半を 25 分に縮めた試合で後半の記録を積むと、補完が
/// `[30:00, 60:00]` を作って 25:00〜30:00 に隙間が開き、`PhaseStartNotContinuousFromPrevious`
/// で保存そのものが拒否される。起点を累計にすれば隙間は表現できない。
///
/// **新しく作る phase の長さは「直前 regular phase の長さ、無ければ試合設定の規定長」**。
/// 明示的に phase を開始する経路（シェルの `PhaseDefaults.duration`）と同じ規則にしてある —
/// 同じ場面でどちらの経路を通ったかによって phase の長さが変わらないようにするため。
///
/// - `.video` / `.videoHighlight`・D <= 0 は空（動画は videoClock 基準で明示 phase 開始）
/// - PhaseStart fact 自身は補完しない（明示 phase 管理はユーザーダイアログ経由 — `startPhase`）
/// - 記録時刻は `fact` の matchClock anchor から取る（無ければ 0 = 先頭 phase のみ確保）
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
        .unwrap_or(0.0)
        .max(0.0);

    // 既存 regular phase を開始順に取り出す。鎖の先端 = 終端の最大値（continuity が
    // 保たれていれば最後の phase の終端と同じ。崩れていても前へ戻さない安全側）。
    let resolver = SegmentResolver::build(existing_facts);
    let mut regular: Vec<(f64, f64)> = resolver
        .phases
        .iter()
        .filter(|phase| phase.kind == PhaseKind::Regular)
        .filter_map(|phase| Some((phase.match_elapsed_start?, phase.match_elapsed_end?)))
        .collect();
    regular.sort_by(|lhs, rhs| lhs.0.total_cmp(&rhs.0));

    let chain_end = regular
        .iter()
        .map(|(_, end)| *end)
        .fold(0.0_f64, |acc, end| acc.max(end));
    if seconds < chain_end {
        return Vec::new();
    }

    // 新しく作る phase の長さ。直前 regular phase の長さを踏襲し、無ければ試合設定の規定長。
    let slot_duration = regular
        .last()
        .map(|(start, end)| end - start)
        .filter(|length| length.is_finite() && *length > 0.0)
        .unwrap_or(duration);

    let missing = ((seconds - chain_end) / slot_duration).floor() as i64 + 1;
    (0..missing.max(0))
        .map(|k| {
            let start = chain_end + k as f64 * slot_duration;
            PhaseCompletionSlot {
                start_seconds: start,
                end_seconds: start + slot_duration,
            }
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

// ── タイマーモードの phase 長さ編集（handball-project#352）──

/// phase の長さを変えたときに書き換える fact の計画。
///
/// タイマーモードの phase は開始と終了の 2 つを持つが、**編集の入力は長さ 1 つ**にする。
/// 開始は直前までの長さの累計で決まるので、隙間も重なりも表現できない — つまり
/// `PhaseStartNotContinuousFromPrevious` を踏む余地が構造的に消える。開始と終了を
/// 別々に編集していた頃は、後続 phase ができた後だとどちらの順で直しても隣接ペアが
/// 一致せず、1 回の保存で 2 つの phase を動かす導線も無いので詰んでいた。
///
/// **保存形式は変えない**。v2 試合 JSON は配信サンプル・Android・サイトが読んでおり、
/// phase の表現を変えると全部が動く。変えるのは入力と、保存時に鎖を書き直す処理だけ。
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseDurationChangePlan {
    /// 書き換え後の fact（同 id の既存 fact を置き換える）。変化しない fact は載らない。
    ///
    /// **並びは発火順**: phase の鎖を開始順に並べ、そのあとに記録を置く。発火は逐次・
    /// 非 atomic なので、鎖が半分だけ書き換わった状態は連続性違反で以降その試合へ
    /// 何も保存できなくなる。phase は数件・記録は数百件ありうるため、危険な窓を
    /// 先頭の数件へ寄せてある。
    pub updated_facts: Vec<MatchFact>,
    /// 新しい終了へ寄せる記録の id。短縮によって phase の外へ出るぶんがここに載る。
    /// 空でないまま発火すると記録の時刻が変わるので、シェルは必ず件数を見せて承認を取る。
    pub clamped_fact_ids: Vec<FactId>,
}

/// 長さ編集の計画が成立しない理由（発火層が `CoreWriteError` へ写像する）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhaseDurationChangeError {
    /// タイマーモード以外。動画モードの phase は人が動画を見ながら開始と終了を打つので、
    /// 長さは結果であって入力ではない（#352 の方針 — 動画モードの編集は現状のまま）。
    NotTimerConfiguration { kind: MatchConfigurationKind },
    /// 指定 id の fact が無い。
    PhaseFactNotFound { fact_id: FactId },
    /// 指定 id が PhaseStart fact ではない。
    NotPhaseStartFact { fact_id: FactId },
    /// PhaseStart の anchor が matchClock を持たない（タイマーモードでは到達しない安全網）。
    PhaseAnchorHasNoMatchClock { fact_id: FactId },
    /// 長さが 0 以下 / 非有限。
    InvalidDuration { seconds: f64 },
}

/// `phase_fact_id` の phase を長さ `new_duration_seconds` にしたときの書き換え計画を返す。
///
/// 規則は 3 つだけで、すべて matchClock 累積秒に対して働く:
///
/// 1. 対象 phase の**開始は動かさない**。終了を `開始 + 長さ` に置き直す
/// 2. 対象 phase の元の終了**以降にあるものはすべて差分だけずらす** — 後続 phase も、
///    その中の記録も、phase の外にある記録も同じ量だけ動く。これにより
///    「後半 7:00 で記録したものは後半 7:00 のまま」が保たれる（記録した位置を保つ）
/// 3. 短縮して新しい終了を**超えてしまう記録**は新しい終了へ寄せ、`clamped_fact_ids`
///    に載せる。寄せると時刻が実際と変わるので、承認なしに発火してはいけない
///
/// 伸ばす場合（差分が正）は 3 が起きない — 新しい終了は元の終了より後なので、
/// 対象 phase の中の記録がはみ出しようがない。
pub fn phase_duration_change_plan(
    match_: &Match,
    facts: &[MatchFact],
    phase_fact_id: FactId,
    new_duration_seconds: f64,
) -> Result<PhaseDurationChangePlan, PhaseDurationChangeError> {
    if !matches!(match_.configuration, MatchConfiguration::Timer { .. }) {
        return Err(PhaseDurationChangeError::NotTimerConfiguration {
            kind: match_.configuration.kind(),
        });
    }
    if !new_duration_seconds.is_finite() || new_duration_seconds <= 0.0 {
        return Err(PhaseDurationChangeError::InvalidDuration {
            seconds: new_duration_seconds,
        });
    }

    let target = facts.iter().find(|fact| fact.id == phase_fact_id).ok_or(
        PhaseDurationChangeError::PhaseFactNotFound {
            fact_id: phase_fact_id,
        },
    )?;
    let MatchFactPayload::Control(ControlFact::PhaseStart(target_payload)) = &target.payload else {
        return Err(PhaseDurationChangeError::NotPhaseStartFact {
            fact_id: phase_fact_id,
        });
    };
    let (Some(old_start), Some(old_end)) = (
        target_payload.start_anchor.match_elapsed_seconds(),
        target_payload.end_anchor.match_elapsed_seconds(),
    ) else {
        return Err(PhaseDurationChangeError::PhaseAnchorHasNoMatchClock {
            fact_id: phase_fact_id,
        });
    };

    let new_end = old_start + new_duration_seconds;
    let delta = new_end - old_end;

    // phase の鎖を先に、記録を後に発火する。発火は逐次・非 atomic なので、途中で
    // repository が失敗したときにどこまで書けているかが結果を分ける — 鎖が半分だけ
    // 書き換わった状態は連続性違反で、**以降その試合へは何も保存できなくなる**。
    // phase は多くても数件、記録は数百件ありうるので、危険な窓を先頭の数件へ寄せる。
    let mut updated_phases: Vec<MatchFact> = Vec::new();
    let mut updated_records: Vec<MatchFact> = Vec::new();
    let mut clamped_fact_ids: Vec<FactId> = Vec::new();

    for fact in facts {
        if fact.id == phase_fact_id {
            if delta != 0.0 {
                let mut updated = fact.clone();
                if let MatchFactPayload::Control(ControlFact::PhaseStart(payload)) =
                    &mut updated.payload
                {
                    payload.end_anchor = payload
                        .end_anchor
                        .with_elapsed_seconds(FactAnchorKind::MatchClock, new_end);
                }
                updated_phases.push(updated);
            }
            continue;
        }
        if delta == 0.0 {
            continue;
        }

        let moved = match &fact.payload {
            // 対象より後ろの phase は鎖ごとずらす（各 phase 自身の長さは保つ）。
            // 対象の中に開始が入り込んでいる phase は触らない — 既に重なっている異常な
            // 状態で、寄せると異常を上書きしてしまう。結果の log 検証が拾う。
            MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
                if let Some(start) = payload.start_anchor.match_elapsed_seconds()
                    && start >= old_end
                {
                    let mut updated = fact.clone();
                    if let MatchFactPayload::Control(ControlFact::PhaseStart(next)) =
                        &mut updated.payload
                    {
                        shift_anchor(&mut next.start_anchor, delta);
                        shift_anchor(&mut next.end_anchor, delta);
                    }
                    updated_phases.push(updated);
                }
                None
            }
            MatchFactPayload::Control(ControlFact::Stoppage(payload)) => {
                let Some(start) = payload.start_anchor.match_elapsed_seconds() else {
                    continue;
                };
                let Some(new_start) = relocated_seconds(start, old_end, new_end, delta) else {
                    continue;
                };
                if new_start == new_end && start < old_end {
                    clamped_fact_ids.push(fact.id);
                }
                let mut updated = fact.clone();
                if let MatchFactPayload::Control(ControlFact::Stoppage(stoppage)) =
                    &mut updated.payload
                {
                    stoppage.start_anchor = stoppage
                        .start_anchor
                        .with_elapsed_seconds(FactAnchorKind::MatchClock, new_start);
                    // タイマーモードの Stoppage は end を持たない（開始のみの marker）。
                    // 動画モード由来の end が残っていても同じ規則で動かす。
                    if let Some(end_anchor) = stoppage.end_anchor
                        && let Some(end_seconds) = end_anchor.match_elapsed_seconds()
                        && let Some(relocated) =
                            relocated_seconds(end_seconds, old_end, new_end, delta)
                    {
                        stoppage.end_anchor = Some(
                            end_anchor.with_elapsed_seconds(FactAnchorKind::MatchClock, relocated),
                        );
                    }
                }
                Some(updated)
            }
            MatchFactPayload::Play(_) | MatchFactPayload::Possession(_) => {
                let Some(anchor) = fact.single_anchor() else {
                    continue;
                };
                let Some(seconds) = anchor.match_elapsed_seconds() else {
                    continue;
                };
                let Some(new_seconds) = relocated_seconds(seconds, old_end, new_end, delta) else {
                    continue;
                };
                if new_seconds == new_end && seconds < old_end {
                    clamped_fact_ids.push(fact.id);
                }
                let relocated_anchor =
                    anchor.with_elapsed_seconds(FactAnchorKind::MatchClock, new_seconds);
                let mut updated = fact.clone();
                if let Some(target_anchor) = updated.single_anchor_mut() {
                    *target_anchor = relocated_anchor;
                }
                Some(updated)
            }
        };

        if let Some(updated) = moved {
            updated_records.push(updated);
        }
    }

    // 鎖は開始順に並べる（発火順がそのまま鎖の順になり、途中で止まっても前から埋まる）。
    updated_phases.sort_by(|lhs, rhs| {
        lhs.anchor()
            .match_elapsed_seconds()
            .unwrap_or(0.0)
            .total_cmp(&rhs.anchor().match_elapsed_seconds().unwrap_or(0.0))
    });
    updated_phases.extend(updated_records);

    Ok(PhaseDurationChangePlan {
        updated_facts: updated_phases,
        clamped_fact_ids,
    })
}

/// 累積秒 1 つの移動先を返す。動かさないなら `None`。
///
/// - 対象 phase の元の終了以降 → 差分だけずらす（後続すべてが同じ量だけ動く）
/// - 短縮で新しい終了を超えたぶん → 新しい終了へ寄せる
/// - それ以外（対象 phase の手前 / 新しい終了までに収まっている）→ 動かさない
fn relocated_seconds(seconds: f64, old_end: f64, new_end: f64, delta: f64) -> Option<f64> {
    if seconds >= old_end {
        Some(seconds + delta)
    } else if seconds > new_end {
        Some(new_end)
    } else {
        None
    }
}

/// matchClock 側だけを `delta` だけずらす（`Both` anchor の動画側は保つ）。
fn shift_anchor(anchor: &mut FactAnchor, delta: f64) {
    if let Some(seconds) = anchor.match_elapsed_seconds() {
        let shifted = anchor.with_elapsed_seconds(FactAnchorKind::MatchClock, seconds + delta);
        *anchor = shifted;
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
            // play / possession はどちらも anchor 1 本の点なので同じ変換に乗せる。
            MatchFactPayload::Play(_) | MatchFactPayload::Possession(_) => {
                plays_to_convert.push(fact.clone())
            }
        }
    }

    // 更新済み control 全部から SegmentResolver を構築し、単一 anchor fact の anchor を変換。
    let resolver = SegmentResolver::build(&updated_control);
    let mut updated_plays: Vec<MatchFact> = Vec::new();
    for mut fact in plays_to_convert {
        let fact_id = fact.id;
        let Some(anchor) = fact.single_anchor_mut() else {
            unreachable!("plays_to_convert は Play / Possession のみ");
        };
        let FactAnchor::MatchClock(match_clock) = *anchor else {
            // 既に videoClock / both なら触らない（安全側）。
            updated_plays.push(fact);
            continue;
        };
        let video = resolver.resolve_video_clock(match_clock).ok_or(
            VideoMigrationPlanError::CannotResolveVideoClock {
                fact_id,
                match_clock_seconds: match_clock.elapsed_seconds,
            },
        )?;
        *anchor = FactAnchor::VideoClock(video);
        updated_plays.push(fact);
    }

    updated_control.extend(updated_plays);
    Ok(updated_control)
}

// ── 動画ソースの差し替え計画（handball-project#267）──

/// 動画ソースの差し替えが成立しない理由（発火層が `CoreWriteError` へ写像する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSourceReplacementError {
    /// 差し替え元が動画ソースを持たない（`.timer`）。タイマー → 動画は同期点の指定が
    /// 要るので、この経路ではなく `commit_video_migration`（移行ウィザード）を使う。
    SourceConfigurationHasNoVideo { kind: MatchConfigurationKind },
}

/// 既存 configuration の **variant を保ったまま** 動画ソースだけを差し替えた
/// configuration を返す（handball-project#267）。
///
/// **fact は 1 件も対象にしない。** 差し替えるのは configuration だけで、記録済み fact の
/// `videoClock` は触らない — つまり「新しい動画は元の動画と同じ切り出し（同じ 0 秒起点・
/// 同じ尺）である」ことは**呼び出し側の責任**であり、コアには検証手段が無い。ずれた動画へ
/// 差し替えるとエラーは出ず全 fact の時刻だけが静かにずれるため、UI は必ずこの前提を示す。
///
/// `.timer` からの差し替えは拒否する。タイマーモードの fact は matchClock しか持たず、
/// 動画へ紐付けるには phase / stoppage ごとの同期点が要る（`video_migration_plan` の仕事）。
/// 「動画ソースを差し替えるだけ」に見えて実体は移行なので、型でも入口でも分ける。
///
/// `.video` / `.videoHighlight` は variant を保つ。ハイライト集をフル試合へ（あるいは逆へ）
/// 変える操作ではないため。provider の組み合わせ（YouTube ↔ ローカル）は制限しない。
pub fn video_source_replacement_plan(
    configuration: &MatchConfiguration,
    new_source: VideoSource,
) -> Result<MatchConfiguration, VideoSourceReplacementError> {
    match configuration {
        MatchConfiguration::Timer { .. } => {
            Err(VideoSourceReplacementError::SourceConfigurationHasNoVideo {
                kind: configuration.kind(),
            })
        }
        MatchConfiguration::Video(_) => Ok(MatchConfiguration::Video(new_source)),
        MatchConfiguration::VideoHighlight(_) => Ok(MatchConfiguration::VideoHighlight(new_source)),
    }
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

/// 移行ウィザードを開いてよいか（= 移行元として妥当か）と、再開に要る件数
/// （handball-project#351）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct VideoMigrationSourceState {
    /// 移行元として使えない理由。`None` ならウィザードを開ける。
    pub issue: Option<VideoMigrationDraftIssue>,
    /// configuration に合わない anchor を持つ記録の件数（= 移行が終わっていない件数）。
    /// `.video` でこれが 1 件以上なら「途中で止まった移行の再開」。
    pub unsynced_fact_count: u32,
    /// **既に動画上の位置を持つ点の記録**の件数（play / possession）。
    ///
    /// この記録は `video_migration_plan` の変換対象にならない（matchClock を持たないので
    /// 引き直す材料が無く、動画モードで新規に記録したものとも区別が付かない）。
    /// **再開で同期点を変えるとこの件数ぶんだけ古い動画位置のまま取り残される**ので、
    /// シェルはこれを警告に使う。
    pub video_anchored_fact_count: u32,
}

/// 移行ウィザードの移行元として妥当かを判定する（handball-project#351）。
///
/// **fact 列が要るので draft 検証とは別の入口にする。** draft 検証は「次へ」の活性判定のため
/// 入力のたびに走るが、ここは試合を読み込んだ 1 回だけでよい。1 本にまとめると記録全量が
/// 毎キーストローク FFI を渡る（境界は粗い粒度 — 設計不変条件 4）。
///
/// 判定:
/// - `.timer` — 通常の移行。常に開ける
/// - `.video` で未同期の記録が残る — **途中で止まった移行の再開**として開ける。移行 commit は
///   非 atomic（ADR 0005 決定 7）なので、configuration だけ `.video` になって記録が matchClock の
///   まま残る試合が実在する
/// - `.video` で未同期が無い / `.videoHighlight` — 移行済み。`SourceConfigurationNotTimer`
pub fn video_migration_source_state(
    configuration: &MatchConfiguration,
    facts: &[MatchFact],
) -> VideoMigrationSourceState {
    let unsynced_fact_count = facts
        .iter()
        .filter(|fact| validators::has_anchor_mismatched_with_configuration(fact, configuration))
        .count() as u32;
    let video_anchored_fact_count = facts
        .iter()
        .filter(|fact| {
            fact.single_anchor().is_some_and(|anchor| {
                matches!(
                    anchor.kind(),
                    FactAnchorKind::VideoClock | FactAnchorKind::Both
                )
            })
        })
        .count() as u32;

    let resumable =
        matches!(configuration, MatchConfiguration::Video(_)) && unsynced_fact_count > 0;
    let issue = if matches!(configuration, MatchConfiguration::Timer { .. }) || resumable {
        None
    } else {
        Some(VideoMigrationDraftIssue::SourceConfigurationNotTimer)
    };

    VideoMigrationSourceState {
        issue,
        unsynced_fact_count,
        video_anchored_fact_count,
    }
}

/// 移行ウィザードの draft 全体を検証する（移植元: `VideoModeMigrationValidator.validate`。
/// 放出順まで同セマンティクス）。
///
/// 検証ルール:
/// 1. video source が確定していること（有無のみ。URL 解析はシェル）
/// 2. PhaseSync: videoStart / videoEnd 入力済み・end > start・2 phase の範囲が overlap しない
/// 3. StoppageSync: 同上 + 範囲がいずれかの phase 範囲内に収まること
///
/// **移行元が妥当か（`SourceConfigurationNotTimer`）はここでは見ない** —
/// 判定に fact 列が要るので `video_migration_source_state` が持つ（handball-project#351）。
/// シェルは両方の結果を合わせて wizard の「次へ」活性を決める。
///
/// commit 時の安全網は `video_migration_plan`（存在・導出可否）と逐次 validation が担い、
/// 本関数は wizard の「次へ」活性・フィールド hint のための事前検証を持つ。
pub fn validate_video_migration_draft(
    video_source: Option<&VideoSource>,
    phase_syncs: &[VideoSyncDraftInput],
    stoppage_syncs: &[VideoSyncDraftInput],
) -> Vec<VideoMigrationDraftIssue> {
    let mut issues: Vec<VideoMigrationDraftIssue> = Vec::new();

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
///
/// さらに **オフセットは phase 境界と stoppage 区間を越えない**（handball-project#101）。
/// 越えさせると害が出る:
///
/// - 動画モード: anchor が phase 範囲外 / stoppage の内側に落ち、R7 / R8 で保存が拒否される。
///   ユーザーのタップは正しいのに、アプリ自身が加えた補正で記録そのものが失われる。
/// - タイマーモード: phase は matchClock 上で連続する（前 phase の end == 次 phase の start）ため、
///   後半開始直後の記録が前半の領域に入り、前半の得点として静かに集計される。
///
/// 下限を 0 で止める既存の考え方を、「いま有効な区間の開始」まで広げたもの。
pub fn capture_play_anchor(
    base_seconds: f64,
    recording_offset_seconds: f64,
    clock_kind: CaptureClockKind,
    facts: &[MatchFact],
) -> FactAnchor {
    let raw_seconds = (base_seconds - recording_offset_seconds).max(0.0);
    let elapsed_seconds = clamp_into_recordable_range(raw_seconds, base_seconds, clock_kind, facts);
    match clock_kind {
        CaptureClockKind::MatchClock => FactAnchor::MatchClock(MatchClock { elapsed_seconds }),
        CaptureClockKind::VideoClock => FactAnchor::VideoClock(VideoClock { elapsed_seconds }),
    }
}

/// オフセットで戻した秒を、記録可能な範囲の下限まで押し戻す。
///
/// 下限は 2 つ。いずれも `base_seconds`（＝ボタンを押した位置）を基準に「どの区間にいるか」を
/// 決めてから、`raw_seconds` がその手前に出ていたら引き戻す:
///
/// 1. 押した位置が属する PhaseStart fact の開始位置
/// 2. 押した位置より手前で閉じている Stoppage fact の終了位置
///
/// `clock_kind` と異なる時計しか持たない anchor は評価対象外（例: タイマーモードの
/// matchClock 記録に対して、videoClock だけの fact は無関係）。
fn clamp_into_recordable_range(
    raw_seconds: f64,
    base_seconds: f64,
    clock_kind: CaptureClockKind,
    facts: &[MatchFact],
) -> f64 {
    let seconds_of = |anchor: &FactAnchor| match clock_kind {
        CaptureClockKind::MatchClock => anchor.match_elapsed_seconds(),
        CaptureClockKind::VideoClock => anchor.video_elapsed_seconds(),
    };

    let mut lower_bound = raw_seconds;

    for fact in facts {
        match &fact.payload {
            MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
                let (Some(start), Some(end)) = (
                    seconds_of(&payload.start_anchor),
                    seconds_of(&payload.end_anchor),
                ) else {
                    continue;
                };
                // phase 境界ちょうどで押した場合は両 phase が該当しうる。max を取ることで
                // 「いま開始したばかりの phase」側に寄せる。
                if base_seconds >= start && base_seconds <= end && lower_bound < start {
                    lower_bound = start;
                }
            }
            MatchFactPayload::Control(ControlFact::Stoppage(payload)) => {
                let Some(start) = seconds_of(&payload.start_anchor) else {
                    continue;
                };
                let Some(end) = payload.end_anchor.and_then(|anchor| seconds_of(&anchor)) else {
                    continue;
                };
                // 停止明けに押したのに、戻した先が停止区間の内側に入ってしまう場合のみ引き戻す。
                // R8 の判定（strict inside）と不等号を揃える。
                if base_seconds >= end && lower_bound > start && lower_bound < end {
                    lower_bound = end;
                }
            }
            _ => {}
        }
    }

    lower_bound
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
            // possession はタイマーの頭出し基準にしない（動画解析由来の読み取り専用 fact で、
            // 記録者がタイマーモードで積むものではない — handball-project#154）。
            MatchFactPayload::Control(_) | MatchFactPayload::Possession(_) => None,
        })
        .flatten()
        .map(|clock| clock.elapsed_seconds)
        .unwrap_or(0.0)
}

/// 新規 play fact を組む（移植元: `confirmPlayEvent` / `confirmPendingFreeNote` /
/// `recordFreeNote` の fact 生成）。
///
/// `title` / `note` は `apply_play_fact_edit` と同じ規則で正規化する（前後空白除去 →
/// 空文字なら None）。移植元は新規記録経路だけ正規化しておらず、「同じ文字列でも
/// 新規記録か編集かで保存される中身が変わる」非対称があった（handball-project#69）。
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
            title: normalize_optional_text(title),
            note: normalize_optional_text(note),
        }),
    }
}

/// 新規 stoppage fact を組む（移植元: `recordTimeout` / `recordTimerPause` /
/// `recordVideoStoppage` の fact 生成）。
///
/// `end_anchor` はタイマーモードでは None（開始のみの marker）、動画モードでは区間の終端。
/// `note` の扱いは `build_play_fact` と同じ（正規化する）。
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
            note: normalize_optional_text(note),
        })),
    }
}

/// 新規ポゼッション開始 fact を組む（handball-project#184。移植元なし — #154 で足した第 3 の
/// fact 種別に、Mac の記録経路が初めて書き込む）。
///
/// `team_id` は `PossessionFact` の型で必須なので `Option` を受けない（`build_play_fact` と
/// 意図的に非対称 — 交互性からの導出も禁じている。`facts/possession_fact.rs`）。anchor は
/// 呼び出し側が `capture_play_anchor` で組む（記録オフセット + phase / stoppage 境界クランプは
/// play fact と同じ規則。「ボールを保持した瞬間」を押し遅れ補正込みで取る）。
///
/// `end_anchor` は任意（handball-project#220）。記録キー（`B`）は始まりだけを打つので None を渡し、
/// 終わりを入れるのはフォームからの編集と、終わりを出せた供給源の import。`build_stoppage_fact` が
/// 記録方法（Timer / Video）で end の要否を分けるのとは**非対称** — ポゼッションの end は記録方法
/// ではなく「供給源が終わりを出せたか」で決まる。
///
/// 正規化対象のテキストは持たない。`start_phase` のように shell が `MatchFact` を直接組む手も
/// あるが、新規 fact の組立はコアに寄せる（`build_play_fact` / `build_stoppage_fact` と同じ列）。
pub fn build_possession_fact(
    stamp: NewFactStamp,
    team_id: TeamId,
    anchor: FactAnchor,
    end_anchor: Option<FactAnchor>,
) -> MatchFact {
    MatchFact {
        id: stamp.id,
        recorded_at: stamp.recorded_at,
        payload: MatchFactPayload::Possession(PossessionFact {
            team_id,
            anchor,
            end_anchor,
        }),
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
