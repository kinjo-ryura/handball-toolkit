//! 移植元: `Clock/VideoClock.swift`。

use serde::{Deserialize, Serialize};

/// 動画再生位置の秒数。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoClock {
    pub elapsed_seconds: f64,
}
