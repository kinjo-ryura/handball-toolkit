//! fact 列の永続化順（累積秒 → recordedAt → id）。
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

use crate::facts::MatchFact;

/// 整列キーの代表時刻。累積秒（matchClock）を優先し、無ければ動画秒を使う。
/// どちらも無い fact は末尾へ寄せる（読み出し側の `?? .infinity` と同じ扱い）。
fn order_seconds(fact: &MatchFact) -> f64 {
    let anchor = fact.anchor();
    anchor
        .match_elapsed_seconds()
        .or_else(|| anchor.video_elapsed_seconds())
        .unwrap_or(f64::INFINITY)
}

/// fact 列を永続化順へその場で整列する。
///
/// `f64` の比較は `total_cmp`（NaN を含んでも全順序が定まり、並びが実行ごとに揺れない）。
/// `FactId` の `Ord` は内包 `Uuid` のバイト順 = Swift `uuidString` 昇順と同順。
pub fn sort_by_persistence_order(facts: &mut [MatchFact]) {
    facts.sort_by(|lhs, rhs| {
        order_seconds(lhs)
            .total_cmp(&order_seconds(rhs))
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
