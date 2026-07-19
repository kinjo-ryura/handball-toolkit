//! 識別子の newtype 群。移植元: `Identifiers.swift`。
//!
//! 移植期間中は Swift 同様 type alias で忠実移植していたが（ADR 0001）、パリティ完走後の
//! 型安全化タスク（handball-project#52）で newtype へ切り替えた。`PlayerId` と `TeamId` の
//! 取り違えをコンパイラが検出できる。
//!
//! - serde は `#[serde(transparent)]` で内包 `Uuid` と同一表現（ゴールデン不変）
//! - `Ord` は内包 `Uuid` のバイト順（= hex 文字列順）をそのまま透過する
//!   （playerStats 等の決定的ソート — ADR 0001「保存すべきセマンティクス」9）

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

define_id!(MatchId);
define_id!(TeamId);
define_id!(PlayerId);
define_id!(FactId);
