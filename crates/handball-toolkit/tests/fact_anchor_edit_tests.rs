//! 時刻編集の書き戻し規則 `FactAnchor::with_elapsed_seconds` の挙動固定
//! （handball-project#168）。
//!
//! 移植元は iOS `AnchorWheelPickerV2.writeBack` と Mac `AnchorEditorMac.writeBack`。
//! **両者の 9 分岐は完全に一致していた**ので、ここでは「どちらが正か」の判断はしていない。
//! 片方だけが直されて腐る構造を消すためにコアへ引き上げただけで、挙動は据え置き。
//!
//! 分岐が 9 本あるのは `kind`（今どちらの時計を編集しているか）と現 anchor の 3 case の
//! 直積のため。全 9 通りをここで固定する。

use handball_toolkit::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};

fn match_clock(seconds: f64) -> FactAnchor {
    FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: seconds,
    })
}

fn video_clock(seconds: f64) -> FactAnchor {
    FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: seconds,
    })
}

fn both(match_seconds: f64, video_seconds: f64) -> FactAnchor {
    FactAnchor::Both {
        match_clock: MatchClock {
            elapsed_seconds: match_seconds,
        },
        video_clock: VideoClock {
            elapsed_seconds: video_seconds,
        },
    }
}

// ── kind = MatchClock ──

#[test]
fn 試合時計の編集は試合時計_anchor_をそのまま更新する() {
    let updated = match_clock(10.0).with_elapsed_seconds(FactAnchorKind::MatchClock, 90.0);
    assert_eq!(updated, match_clock(90.0));
}

/// **`Both` の中核**: 編集していない動画側を保つ。ここが落ちると強制同期点が
/// 片側編集で失われる。
#[test]
fn 試合時計の編集は_both_の動画側を保つ() {
    let updated = both(10.0, 500.0).with_elapsed_seconds(FactAnchorKind::MatchClock, 90.0);
    assert_eq!(updated, both(90.0, 500.0));
}

/// 時計の種類が食い違う経路。`Both` へ昇格させず、編集した時計だけを持つ anchor に倒す
/// （入力されていない側の時計を捏造しないため）。
#[test]
fn 試合時計の編集は動画時計_anchor_を試合時計_anchor_へ倒す() {
    let updated = video_clock(500.0).with_elapsed_seconds(FactAnchorKind::MatchClock, 90.0);
    assert_eq!(updated, match_clock(90.0));
}

// ── kind = VideoClock ──

#[test]
fn 動画時計の編集は動画時計_anchor_をそのまま更新する() {
    let updated = video_clock(10.0).with_elapsed_seconds(FactAnchorKind::VideoClock, 500.0);
    assert_eq!(updated, video_clock(500.0));
}

#[test]
fn 動画時計の編集は_both_の試合側を保つ() {
    let updated = both(90.0, 10.0).with_elapsed_seconds(FactAnchorKind::VideoClock, 500.0);
    assert_eq!(updated, both(90.0, 500.0));
}

#[test]
fn 動画時計の編集は試合時計_anchor_を動画時計_anchor_へ倒す() {
    let updated = match_clock(90.0).with_elapsed_seconds(FactAnchorKind::VideoClock, 500.0);
    assert_eq!(updated, video_clock(500.0));
}

// ── kind = Both（呼び出し側が片側を明示しなかったときの fallback）──

#[test]
fn kind_both_は_both_anchor_の試合側を編集する() {
    let updated = both(10.0, 500.0).with_elapsed_seconds(FactAnchorKind::Both, 90.0);
    assert_eq!(updated, both(90.0, 500.0));
}

#[test]
fn kind_both_は試合時計_anchor_を試合時計のまま更新する() {
    let updated = match_clock(10.0).with_elapsed_seconds(FactAnchorKind::Both, 90.0);
    assert_eq!(updated, match_clock(90.0));
}

#[test]
fn kind_both_は動画時計_anchor_を_both_へ昇格させる() {
    let updated = video_clock(500.0).with_elapsed_seconds(FactAnchorKind::Both, 90.0);
    assert_eq!(updated, both(90.0, 500.0));
}

// ── クランプ ──

#[test]
fn 負の秒は_0_に丸める() {
    assert_eq!(
        match_clock(90.0).with_elapsed_seconds(FactAnchorKind::MatchClock, -1.0),
        match_clock(0.0)
    );
    assert_eq!(
        both(90.0, 500.0).with_elapsed_seconds(FactAnchorKind::VideoClock, -0.5),
        both(90.0, 0.0)
    );
}

/// 非有限値をここから通さない（非有限 anchor は validation が弾く対象 — handball-project#91。
/// 入口で 0 に倒しておけば、時刻編集からその経路に乗ることが無い）。
#[test]
fn nan_と負の無限大は_0_に丸める() {
    assert_eq!(
        match_clock(90.0).with_elapsed_seconds(FactAnchorKind::MatchClock, f64::NAN),
        match_clock(0.0)
    );
    assert_eq!(
        match_clock(90.0).with_elapsed_seconds(FactAnchorKind::MatchClock, f64::NEG_INFINITY),
        match_clock(0.0)
    );
}

/// 正の無限大は素通しする（クランプは下限だけ）。上限は試合時間の validation の担当で、
/// ここで勝手に決めると「何分までなら妥当か」の規則が 2 箇所に散る。
#[test]
fn 正の無限大は素通しする() {
    let updated = match_clock(90.0).with_elapsed_seconds(FactAnchorKind::MatchClock, f64::INFINITY);
    assert_eq!(updated.match_elapsed_seconds(), Some(f64::INFINITY));
}
