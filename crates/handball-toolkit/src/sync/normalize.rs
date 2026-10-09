//! 時刻をミリ秒に丸める（ADR 0007 決定 5）。
//!
//! シェルの時刻は端末の型（Swift の `Date` は 1970 年からの秒を倍精度で持つ — 今の時刻で約 240 ns
//! 刻み）から FFI の `SystemTime`（ns）へ変わる。**往復すると 100 ns 程度ずれる** — 同期した記録を
//! 相手が保存して読み直すと、同じ記録の時刻が ns の桁で食い違う。そのまま比べると、同期済みの同じ
//! 記録が次の同期で「`updated_at` が同じで中身（`recorded_at`）が違う」に見え、聞く必要の無い
//! 問いが出る。
//!
//! 比べる前に、スナップショットの時刻をすべて**最も近い**ミリ秒に丸める。切り捨てにしないのは、
//! 往復のずれが負の向きに出ると 1 ms 手前へ落ちるため（最近接なら ±0.5 ms まで吸収する）。
//! 秒には丸めない — 後勝ちの比較と、`recorded_at` を決め手に含む並び順を変えないため。

use chrono::{DateTime, Utc};

use super::{SyncSnapshot, SyncStamp};

/// 最も近いミリ秒に丸める。表せない範囲（実際には来ない）はそのまま返す。
pub(crate) fn round_to_millis(at: DateTime<Utc>) -> DateTime<Utc> {
    let millis =
        at.timestamp() * 1000 + (i64::from(at.timestamp_subsec_nanos()) + 500_000) / 1_000_000;
    DateTime::from_timestamp_millis(millis).unwrap_or(at)
}

fn round_stamp(stamp: SyncStamp) -> SyncStamp {
    SyncStamp {
        updated_at: round_to_millis(stamp.updated_at),
        deleted_at: stamp.deleted_at.map(round_to_millis),
    }
}

/// スナップショットの時刻（記録の `updated_at` / `deleted_at`・試合の `date`・fact の
/// `recorded_at`）をすべてミリ秒に丸めた写し。
pub(crate) fn normalized(snapshot: &SyncSnapshot) -> SyncSnapshot {
    let mut copy = snapshot.clone();
    for m in &mut copy.matches {
        m.stamp = round_stamp(m.stamp);
        m.match_.date = round_to_millis(m.match_.date);
    }
    for t in &mut copy.teams {
        t.stamp = round_stamp(t.stamp);
    }
    for p in &mut copy.players {
        p.stamp = round_stamp(p.stamp);
    }
    for f in &mut copy.facts {
        f.stamp = round_stamp(f.stamp);
        f.fact.recorded_at = round_to_millis(f.fact.recorded_at);
    }
    copy
}
