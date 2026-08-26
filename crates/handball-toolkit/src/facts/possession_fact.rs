//! ポゼッション開始（handball-project#154）。移植元なし — Rust コアで新規に足した種別。

use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::ids::TeamId;

/// あるチームのポゼッションがそこから始まった、という点の事実（終わりは任意）。
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PossessionFact {
    /// ここからポゼッションを持つチーム。**必須**。
    pub team_id: TeamId,
    /// そのチームのプレーが動き出した瞬間。ターンオーバーやルーズボールの確保では保持した瞬間と
    /// 一致するが、得点・ボールアウトの後は**スローオフ / スローインが実行された瞬間**であって
    /// 被得点の瞬間ではない（handball-project#220 で改めた。主たる供給源の CV はカメラの向きしか
    /// 見ないので、カメラが動かない被得点の瞬間には何も検出できず、旧定義は原理的に満たせなかった）。
    pub anchor: FactAnchor,
    /// ポゼッションが終わった瞬間。**任意**（handball-project#220）。
    ///
    /// **必須にしない。** 必須化すると「終わり」を語る fact が 2 つ（この end と次のポゼッション開始）に
    /// なって食い違え（次の開始より後ろの end / 区間の重なり）、かつ終わりを出せない供給源の出力が
    /// 一切取り込めなくなる。無い場合は従来どおり `PossessionProjection` が導出する。
    ///
    /// **埋めるのは主に機械で、人（Mac）は例外側。** CV は向きしか見ないので終わりを直接は出せないが、
    /// goal の時刻区間とズーム（handball-project#221）を組み合わせれば出せる。
    ///
    /// **`stoppage` の end と同じ形**（anchor + 任意 end）で、新しいパターンではない。関係の整合
    /// （「end ≤ 次の開始」「end が phase 範囲内」）は validation ではなく `PossessionProjection` の
    /// クランプが担保する — 詳細は `DOMAIN_VALIDATION_RULES.md`「持たないルール」。
    // uniffi(default) は既存の呼び出し側（end を持たない従来の生成経路）を壊さないため。
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub end_anchor: Option<FactAnchor>,
}
