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

// Uuid は String でブリッジし、Swift 側では Foundation の UUID になる（uniffi.toml）。
uniffi::custom_type!(Uuid, String, {
    remote,
    try_lift: |val| Ok(Uuid::parse_str(&val)?),
    lower: |obj| obj.to_string(),
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
pub type PlayerIdSet = BTreeSet<Uuid>;
uniffi::custom_type!(PlayerIdSet, Vec<Uuid>, {
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

/// `usize`（phase index 用途）は u32 でブリッジ（ADR 0004 決定 6）。
/// phase 数が u32 を越えることは構造的にない。Swift の Int 変換はシムが提供する。
pub type PhaseIndex = usize;
uniffi::custom_type!(PhaseIndex, u32, {
    remote,
    try_lift: |val| Ok(val as usize),
    lower: |obj| u32::try_from(obj).expect("phase index は u32 に収まる"),
});

/// `BTreeMap<PlayerId, TeamId>`（RosterContext の lookup）は HashMap でブリッジ
/// （Swift は `[UUID: UUID]`）。順序はコア側で再び BTreeMap に集約されるため失われない。
pub type PlayerTeamLookup = BTreeMap<Uuid, Uuid>;
uniffi::custom_type!(PlayerTeamLookup, std::collections::HashMap<Uuid, Uuid>, {
    remote,
    try_lift: |val| Ok(val.into_iter().collect()),
    lower: |obj| obj.into_iter().collect(),
});
