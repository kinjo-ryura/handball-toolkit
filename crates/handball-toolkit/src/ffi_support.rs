//! UniFFI 向けの third-party 型ブリッジ（feature `uniffi` 時のみ — ADR 0004 決定 6）。
//!
//! Swift 側の見え方（typealias 先・変換式）は crate ルートの `uniffi.toml` が定める
//! （設定が無い型はブリッジ先組み込み型への typealias になる）。
//!
//! `remote` custom type の trait 実装はこの crate のタグ限定のため、他 crate が
//! これらの型を export 関数の引数・戻り値に直接使う場合は `uniffi::use_remote_type!` が要る。

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use jiff::SignedDuration;
use uuid::Uuid;

use crate::clock::FactAnchorKind;
use crate::ids::{FactId, MatchId, PlayerId, TeamId};
use crate::timestamp::Timestamp;

// Uuid は String でブリッジし、Swift 側では Foundation の UUID になる（uniffi.toml）。
uniffi::custom_type!(Uuid, String, {
    remote,
    try_lift: |val| Ok(Uuid::parse_str(&val)?),
    lower: |obj| obj.to_string(),
});

// ID newtype（handball-project#52）も Uuid と同じ String ブリッジ。Swift 側では
// uniffi.toml の写像によりすべて Foundation の UUID として見える（API 面は newtype 化前と不変）。
uniffi::custom_type!(MatchId, String, {
    try_lift: |val| Ok(MatchId(Uuid::parse_str(&val)?)),
    lower: |obj| obj.0.to_string(),
});
uniffi::custom_type!(TeamId, String, {
    try_lift: |val| Ok(TeamId(Uuid::parse_str(&val)?)),
    lower: |obj| obj.0.to_string(),
});
uniffi::custom_type!(PlayerId, String, {
    try_lift: |val| Ok(PlayerId(Uuid::parse_str(&val)?)),
    lower: |obj| obj.0.to_string(),
});
uniffi::custom_type!(FactId, String, {
    try_lift: |val| Ok(FactId(Uuid::parse_str(&val)?)),
    lower: |obj| obj.0.to_string(),
});

/// 日時（[`Timestamp`]）は SystemTime（uniffi 組み込み Timestamp）でブリッジし、Swift では Date、
/// Kotlin では `java.time.Instant` になる。別名は chrono を使っていた頃の `UtcDateTime` のまま
/// （生成されるバインディングの型名を変えない — handball-project#838）。
///
/// jiff の SystemTime との変換は `std` 配下（コアは `std` を外している — `timestamp` の doc）
/// なので、Unix 時刻からの差で書く。
pub type UtcDateTime = Timestamp;
uniffi::custom_type!(UtcDateTime, SystemTime, {
    remote,
    try_lift: |val| Ok(timestamp_from_system_time(val)?),
    lower: |obj| timestamp_to_system_time(obj),
});

/// jiff が表せる範囲（西暦 -9999〜9999 年）の外はエラー（シェルへ例外として返る）。
fn timestamp_from_system_time(at: SystemTime) -> Result<Timestamp, jiff::Error> {
    let since_epoch = match at.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => SignedDuration::try_from(after)?,
        Err(before) => -SignedDuration::try_from(before.duration())?,
    };
    Timestamp::from_duration(since_epoch)
}

/// 西暦 -9999〜9999 年は Unix 系の SystemTime（i64 秒）に収まるので、足し引きは溢れない。
fn timestamp_to_system_time(at: Timestamp) -> SystemTime {
    let since_epoch = at.as_duration();
    if since_epoch.is_negative() {
        SystemTime::UNIX_EPOCH - since_epoch.unsigned_abs()
    } else {
        SystemTime::UNIX_EPOCH + since_epoch.unsigned_abs()
    }
}

/// `BTreeSet<PlayerId>` はソート済み Vec でブリッジ（決定的順序を保存 — ADR 0004 決定 6）。
/// Swift 側の Set 変換 convenience はシムが提供する。
pub type PlayerIdSet = BTreeSet<PlayerId>;
uniffi::custom_type!(PlayerIdSet, Vec<PlayerId>, {
    remote,
    try_lift: |val| Ok(val.into_iter().collect()),
    lower: |obj| obj.into_iter().collect(),
});

/// `BTreeSet<FactAnchorKind>`（validation payload の `allowed`）も同様にソート済み Vec。
pub type FactAnchorKindSet = BTreeSet<FactAnchorKind>;
uniffi::custom_type!(FactAnchorKindSet, Vec<FactAnchorKind>, {
    remote,
    try_lift: |val| Ok(val.into_iter().collect()),
    lower: |obj| obj.into_iter().collect(),
});

/// `usize` は u32 でブリッジし、Swift では `Int` になる（ADR 0004 決定 6 / uniffi.toml）。
///
/// この別名は crate 内の `usize` 全体に効く。用途は phase index（`currentPhaseIndex` /
/// `regularIndex`）と、コアが数える個数（`InsufficientNewIds` の required / provided、
/// `count_phase_completion_facts` 等）の 2 系統。旧名は `PhaseIndex` だったが、
/// index 以外にも効くため実態に合わせて改名した（handball-project#70）。
///
/// どちらの用途も u32 を越えることは構造的にない（phase 数・1 回の記録で必要な ID 数）。
///
/// `u32` を直接使わず custom_type を挟んでいるのは、**言語ごとに慣習の整数型へ写像する余地を
/// 残すため**。組み込み型の `u32` には再写像のフックが無く、UniFFI の既定で Swift は
/// `UInt32`、Kotlin は `UInt` に固定される。Kotlin バインディング（handball-project#59）を
/// 足すときは `uniffi.toml` に `[bindings.kotlin.custom_types.CoreInt]` を追加して
/// `Int`（Kotlin の `List.size` と同じ型）へ写すこと — 足さないと `UInt` になる。
pub type CoreInt = usize;
uniffi::custom_type!(CoreInt, u32, {
    remote,
    try_lift: |val| Ok(val as usize),
    lower: |obj| u32::try_from(obj).expect("phase index / 個数は u32 に収まる"),
});

/// `BTreeMap<PlayerId, TeamId>`（RosterContext の lookup）は HashMap でブリッジ
/// （Swift は `[UUID: UUID]`）。順序はコア側で再び BTreeMap に集約されるため失われない。
pub type PlayerTeamLookup = BTreeMap<PlayerId, TeamId>;
uniffi::custom_type!(PlayerTeamLookup, std::collections::HashMap<PlayerId, TeamId>, {
    remote,
    try_lift: |val| Ok(val.into_iter().collect()),
    lower: |obj| obj.into_iter().collect(),
});

/// `BTreeMap<String, TeamId>` / `BTreeMap<String, PlayerId>`（sample_dto の
/// teamKey / playerKey → 内部 ID 写像）も HashMap でブリッジ（Swift は `[String: UUID]`）。
/// ID newtype 化（handball-project#52）で TeamId / PlayerId の 2 型に分かれた。
pub type TeamKeyLookup = BTreeMap<String, TeamId>;
uniffi::custom_type!(TeamKeyLookup, std::collections::HashMap<String, TeamId>, {
    remote,
    try_lift: |val| Ok(val.into_iter().collect()),
    lower: |obj| obj.into_iter().collect(),
});
pub type PlayerKeyLookup = BTreeMap<String, PlayerId>;
uniffi::custom_type!(PlayerKeyLookup, std::collections::HashMap<String, PlayerId>, {
    remote,
    try_lift: |val| Ok(val.into_iter().collect()),
    lower: |obj| obj.into_iter().collect(),
});

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    /// 秒未満・1970 年より前も、SystemTime を往復して同じ瞬間に戻る（handball-project#838）。
    #[test]
    fn timestamp_round_trips_through_system_time() {
        for s in [
            "1970-01-01T00:00:00Z",
            "2026-11-28T12:34:56.123456789Z",
            "1969-12-31T23:59:59.5Z",
            "0001-01-01T00:00:00Z",
            // jiff の上限（`Timestamp::MAX`）。
            "9999-12-30T22:00:00.999999999Z",
        ] {
            let system = timestamp_to_system_time(at(s));
            assert_eq!(timestamp_from_system_time(system).unwrap(), at(s), "{s}");
        }
    }

    #[test]
    fn system_time_maps_to_the_same_instant() {
        let after = SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 250_000_000);
        assert_eq!(
            timestamp_from_system_time(after).unwrap(),
            at("2023-11-14T22:13:20.25Z")
        );
        let before = SystemTime::UNIX_EPOCH - Duration::from_millis(500);
        assert_eq!(
            timestamp_from_system_time(before).unwrap(),
            at("1969-12-31T23:59:59.5Z")
        );
    }

    /// jiff が表せない範囲（西暦 9999 年より後）は abort せずエラーで返す。
    #[test]
    fn system_time_outside_jiff_range_is_an_error() {
        let far = SystemTime::UNIX_EPOCH + Duration::from_secs(400_000_000_000);
        assert!(timestamp_from_system_time(far).is_err());
    }
}
