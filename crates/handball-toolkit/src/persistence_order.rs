//! fact 列の永続化順（動画秒を持つか → 時刻 → phase 開始か → recordedAt → id）。
//!
//! Rust 側の追加モジュール（移植元 `RecorderDomain` に対応物はない — ADR 0001 ミラー表の外）。
//! 規約の正典は読み出し側の `SwiftDataMatchRepository.factRecordOrder` で、
//! `validators` の入力契約（「`facts` は永続化順でソート済み」— ADR 0001）が要求する順序でもある。
//!
//! 同じ規約を import commit（`sample_import`）と検証 CLI が別々に実装していたため、
//! 規約が 1 箇所で決まるようここへ集約した（handball-project#87）。tie-break の変更が
//! 片方だけに入って静かに乖離するのを防ぐのが目的。
//!
//! FFI へは公開しない。Swift 側は SwiftData のクエリ順（`SortDescriptor`）で同じ並びを得ており、
//! コアを経由しないため（ADR 0001 関数目録の対象外）。
//!
//! **したがって同じ規約の実装はコアの外にも残る。** このモジュールのほか、HandballRecorder の
//! `SwiftDataMatchRepository.factRecordOrder`、Android の Room の `ORDER BY`（`examples/android` の
//! `ShellDao.factLog` と handball-recorder-android の同名メソッド）の計 4 箇所。Room のクエリは
//! この関数を呼べない。4 箇所とも共通 fixture（`tests/fixtures/persistence-order-cases.json`）を
//! 読むテストを持ち、どれか 1 つだけ変えると赤くなる（handball-project#405 — handball-project#401 の段が
//! Android に入らないまま残った）。**規約を変えるときは fixture を先に変える。**
//!
//! **時刻は動画秒を優先する**（handball-project#380）。タイマーから動画へ移行した試合では
//! phaseStart / stoppage が `Both`、play が `VideoClock` だけを持つ。累積秒を優先すると control は
//! 累積秒・play は動画秒で比べられ、後半の phaseStart（累積秒 1800）が前半終盤の play
//! （動画秒 1800 超）より前に来て、区間が交互に並んでいた。
//!
//! **動画秒を持たない fact は後ろにまとめる。** 移行が途中で止まった試合（`Video` なのに
//! `MatchClock` だけの fact が残る — handball-project#320）で、累積秒と動画秒を同じ数直線で
//! 比べないため。動画秒を持たない fact どうしは累積秒で並ぶので、タイマーの試合の並びは変わらない。
//!
//! **同じ時刻なら phase 開始が先**（handball-project#401）。タイマーモードの phase は記録した
//! 瞬間に auto-create される（ADR 0001 / `write::phase_completion_plan`）ので、その phase の
//! 最初の記録と phase 開始は必ず同じ累積秒を持つ。どちらが先かは種別で決まっていて、
//! 時刻からは決まらない。
//!
//! **この判定を `recorded_at` に任せない。** `recorded_at` は記録した実時刻で、シェルが発行する
//! スタンプの順が発火順と一致する保証は無い。実際 ADR 0005 で phase 自動補完をコア入口へ移した
//! とき、シェルが本 fact のスタンプを先・補完 phase のスタンプを後に発行するようになり、
//! 発火順（補完 phase → 本 fact）と逆転して 00:00 の得点が「前半の開始」より上に並んでいた。

use crate::facts::{ControlFact, MatchFact, MatchFactPayload};

/// 動画秒を持たない fact を後ろへ寄せる第 1 キー（`false` = 持つ が先）。
fn lacks_video_seconds(fact: &MatchFact) -> bool {
    fact.anchor().video_elapsed_seconds().is_none()
}

/// 同じ時刻では phase 開始を先に置く第 3 キー（`false` = phase 開始 が先）。
fn is_not_phase_start(fact: &MatchFact) -> bool {
    !matches!(
        fact.payload,
        MatchFactPayload::Control(ControlFact::PhaseStart(_))
    )
}

/// 整列キーの代表時刻。動画秒を優先し、無ければ累積秒（matchClock）を使う。
/// どちらも無い fact は末尾へ寄せる（読み出し側の `?? .infinity` と同じ扱い）。
fn order_seconds(fact: &MatchFact) -> f64 {
    let anchor = fact.anchor();
    anchor
        .video_elapsed_seconds()
        .or_else(|| anchor.match_elapsed_seconds())
        .unwrap_or(f64::INFINITY)
}

/// fact 列を永続化順へその場で整列する。
///
/// `f64` の比較は `total_cmp`（NaN を含んでも全順序が定まり、並びが実行ごとに揺れない）。
/// `FactId` の `Ord` は内包 `Uuid` のバイト順 = Swift `uuidString` 昇順と同順。
pub fn sort_by_persistence_order(facts: &mut [MatchFact]) {
    facts.sort_by(|lhs, rhs| {
        lacks_video_seconds(lhs)
            .cmp(&lacks_video_seconds(rhs))
            .then_with(|| order_seconds(lhs).total_cmp(&order_seconds(rhs)))
            .then_with(|| is_not_phase_start(lhs).cmp(&is_not_phase_start(rhs)))
            .then_with(|| lhs.recorded_at.cmp(&rhs.recorded_at))
            .then_with(|| lhs.id.cmp(&rhs.id))
    });
}

/// 借用した fact 列を永続化順に並べ直した新しい `Vec` を返す。
pub fn persistence_ordered(facts: &[MatchFact]) -> Vec<MatchFact> {
    let mut ordered = facts.to_vec();
    sort_by_persistence_order(&mut ordered);
    ordered
}
