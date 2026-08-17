//! ポゼッション開始（handball-project#154）。移植元なし — Rust コアで新規に足した種別。

use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::ids::TeamId;

/// あるチームのポゼッションがそこから始まった、という点の事実。
///
/// `PlayFact` でも `ControlFact` でもない第 3 の fact 種別として置いている:
/// - **control ではない** — control fact は同期点を兼ねる定義だが、ポゼッションは試合タイマーを
///   止めないので segment を作らない
/// - **play ではない** — `PlayEventKind` の 6 種はいずれも選手が起こした離散事象で、
///   `player_id` / `related_player_id` / `title` / `note` を持つ。ポゼッションが使うのは team だけで、
///   `PlayFact` に載せると 4 フィールドが常に None になり「型が語れないことを validation で守る」形になる
///
/// **`team_id` は `Option` ではない。** ポゼッションは原則交互に移るので「最初の 1 件 + 交互性」から
/// 導出できるが、それが成り立つのは fact log に欠落が無いときだけ。供給源（動画解析）は棄権つきで
/// カバレッジ 44〜81% なので欠測が構造的に起き、導出方式だと 1 件の取りこぼしで以降の帰属が
/// **全部反転する**（しかも形式的には整合しているので validation に見えない）。各 fact が独立して
/// 正しい形にするため型で必須にしている（`PlayFact.team_id` が全 kind で `Option` なのとは意図的に非対称）。
///
/// **end を持たない。** 区間の終わりは次のポゼッション開始（無ければ phase の end）から導出できる
/// ため、「計算で導出できる値は冗長記録しない」に従う。同じチームの PossessionFact が連続するのは
/// 矛盾ではなく 2 件目が冗長なだけで、区間はチームが切り替わった所で区切る。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PossessionFact {
    /// ここからポゼッションを持つチーム。**必須**。
    pub team_id: TeamId,
    /// ボールを保持した瞬間。再開のスローが実行された瞬間でも、相手コートへ入った瞬間でもない。
    pub anchor: FactAnchor,
}
