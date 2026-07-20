//! UniFFI 向けの third-party 型ブリッジ（feature `uniffi` 時のみ — ADR 0004 決定 6）。
//!
//! Swift 側の見え方（typealias 先・変換式）は crate ルートの `uniffi.toml` が定める
//! （設定が無い型はブリッジ先組み込み型への typealias になる）。
//!
//! `remote` custom type の trait 実装はこの crate のタグ限定のため、他 crate が
//! これらの型を export 関数の引数・戻り値に直接使う場合は `uniffi::use_remote_type!` が要る。

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::clock::FactAnchorKind;
use crate::ids::{FactId, MatchId, PlayerId, TeamId};

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

/// `DateTime<Utc>` は SystemTime（uniffi 組み込み Timestamp）でブリッジし、Swift では Date になる。
pub type UtcDateTime = DateTime<Utc>;
uniffi::custom_type!(UtcDateTime, SystemTime, {
    remote,
    try_lift: |val| Ok(DateTime::<Utc>::from(val)),
    lower: |obj| SystemTime::from(obj),
});

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
