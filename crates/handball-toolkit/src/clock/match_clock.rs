//! 移植元: `Clock/MatchClock.swift`。

use serde::{Deserialize, Serialize};

/// 試合通算 matchClock 進行秒数（累積）。
///
/// 累積の定義: matchClock が動いた時間の累積。Stoppage 中は進まない。
/// regular phase 切替時は前 phase の end と次 phase の start が同じ累積秒数を共有
/// （matchClock は phase 境界で進まない）。
///
/// 例: phase 1 (regular) 0:00 = 0、phase 1 30:00 = 1800、phase 2 (regular) 0:00 = 1800、
/// phase 2 5:00 = 2100、phase 2 30:00 = 3600。
///
/// shootout は時計が動かない → shootout 開始時点で matchClock 累積秒は固定
/// （shootout 中の全 fact が同じ値）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchClock {
    pub elapsed_seconds: f64,
}
