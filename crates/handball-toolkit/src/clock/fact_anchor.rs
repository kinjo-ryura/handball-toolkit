//! 移植元: `Clock/FactAnchor.swift`。

use serde::{Deserialize, Serialize};

use super::{MatchClock, VideoClock};

/// その事実を何基準で観測したか。
///
/// - `MatchClock`: 手動時計で記録（タイマーモード）
/// - `VideoClock`: 動画を見ながら記録（動画モード / ハイライト）
/// - `Both`: 強制 sync point（動画カット復旧などの override 専用、平常時は使わない）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FactAnchor {
    MatchClock(MatchClock),
    VideoClock(VideoClock),
    Both {
        match_clock: MatchClock,
        video_clock: VideoClock,
    },
}

/// `invalidAnchorForConfiguration` 等で actual / allowed の表現に使う raw kind。
///
/// Ord は `allowed: BTreeSet<FactAnchorKind>`（エラー payload の決定的順序 — ADR 0001 の
/// BTreeSet 方針と同じ理由）のための derive。順序は宣言順で意味を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum FactAnchorKind {
    MatchClock,
    VideoClock,
    Both,
}

impl FactAnchorKind {
    /// Swift `CaseIterable.allCases` 相当。
    pub const ALL_CASES: [FactAnchorKind; 3] = [
        FactAnchorKind::MatchClock,
        FactAnchorKind::VideoClock,
        FactAnchorKind::Both,
    ];
}

impl FactAnchor {
    pub fn kind(&self) -> FactAnchorKind {
        match self {
            FactAnchor::MatchClock(_) => FactAnchorKind::MatchClock,
            FactAnchor::VideoClock(_) => FactAnchorKind::VideoClock,
            FactAnchor::Both { .. } => FactAnchorKind::Both,
        }
    }

    pub fn match_clock(&self) -> Option<MatchClock> {
        match self {
            FactAnchor::MatchClock(clock)
            | FactAnchor::Both {
                match_clock: clock, ..
            } => Some(*clock),
            FactAnchor::VideoClock(_) => None,
        }
    }

    pub fn video_clock(&self) -> Option<VideoClock> {
        match self {
            FactAnchor::VideoClock(clock)
            | FactAnchor::Both {
                video_clock: clock, ..
            } => Some(*clock),
            FactAnchor::MatchClock(_) => None,
        }
    }

    /// matchClock があれば matchClock.elapsedSeconds、なければ None。
    pub fn match_elapsed_seconds(&self) -> Option<f64> {
        self.match_clock().map(|clock| clock.elapsed_seconds)
    }

    /// videoClock があれば videoClock.elapsedSeconds、なければ None。
    pub fn video_elapsed_seconds(&self) -> Option<f64> {
        self.video_clock().map(|clock| clock.elapsed_seconds)
    }

    /// 時刻編集で入力された累積秒を anchor へ書き戻す。
    ///
    /// `kind` は「今どちらの時計を編集しているか」。`Both` anchor は片側ずつ編集するので、
    /// 呼び出し側（時刻編集 UI）が `MatchClock` / `VideoClock` を明示して 2 回に分けて呼ぶ。
    ///
    /// **`Both` の片側だけを差し替え、もう片方を保つのがこの関数の中核** — `Both` は強制同期点
    /// （動画カット復旧の override）で、片側を編集しただけで同期点そのものを失ってはいけない。
    ///
    /// 遷移表（`kind` × 現 anchor）:
    ///
    /// | kind | 現 anchor | 結果 |
    /// |---|---|---|
    /// | MatchClock | MatchClock | MatchClock(s) |
    /// | MatchClock | Both{_, v} | Both{s, v} — 動画側を保つ |
    /// | MatchClock | VideoClock | MatchClock(s) — **動画側を捨てる** |
    /// | VideoClock | VideoClock | VideoClock(s) |
    /// | VideoClock | Both{m, _} | Both{m, s} — 試合側を保つ |
    /// | VideoClock | MatchClock | VideoClock(s) — **試合側を捨てる** |
    /// | Both | MatchClock | MatchClock(s) |
    /// | Both | VideoClock(v) | Both{s, v} |
    /// | Both | Both{_, v} | Both{s, v} |
    ///
    /// 時計の種類が食い違う 2 行（MatchClock × VideoClock とその逆）で捨てる側が出るのは、
    /// 「編集した時計だけを持つ anchor に倒す」ため。`Both` へ昇格させると、ユーザーが
    /// 入力していない側の時計を捏造することになる。
    ///
    /// `kind == Both` の 3 行は呼び出し側が片側を明示しなかったときの fallback で、
    /// 試合時計側を編集したものとして扱う（`Both` 以外の 2 行は実際には到達しない）。
    ///
    /// 負の秒は 0 に丸める（累積秒に負は無い）。NaN も 0 になる（`f64::max` は NaN でない側を
    /// 返す）ため、ここから非有限 anchor は生まれない。
    pub fn with_elapsed_seconds(&self, kind: FactAnchorKind, seconds: f64) -> FactAnchor {
        let clamped = 0.0_f64.max(seconds);
        match (kind, *self) {
            (FactAnchorKind::MatchClock, FactAnchor::MatchClock(_))
            | (FactAnchorKind::MatchClock, FactAnchor::VideoClock(_))
            | (FactAnchorKind::Both, FactAnchor::MatchClock(_)) => {
                FactAnchor::MatchClock(MatchClock {
                    elapsed_seconds: clamped,
                })
            }
            (FactAnchorKind::MatchClock, FactAnchor::Both { video_clock, .. })
            | (FactAnchorKind::Both, FactAnchor::VideoClock(video_clock))
            | (FactAnchorKind::Both, FactAnchor::Both { video_clock, .. }) => FactAnchor::Both {
                match_clock: MatchClock {
                    elapsed_seconds: clamped,
                },
                video_clock,
            },
            (FactAnchorKind::VideoClock, FactAnchor::VideoClock(_))
            | (FactAnchorKind::VideoClock, FactAnchor::MatchClock(_)) => {
                FactAnchor::VideoClock(VideoClock {
                    elapsed_seconds: clamped,
                })
            }
            (FactAnchorKind::VideoClock, FactAnchor::Both { match_clock, .. }) => {
                FactAnchor::Both {
                    match_clock,
                    video_clock: VideoClock {
                        elapsed_seconds: clamped,
                    },
                }
            }
        }
    }
}
