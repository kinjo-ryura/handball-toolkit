//! 移植元: `Configuration/VideoSource.swift`。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum VideoProvider {
    Youtube,
    /// 端末「写真」(Photos) 内のローカル動画。`external_id` = PHAsset の localIdentifier
    /// （端末固有。別端末・復元後は無効。詳細は HandballRecorder の CONTEXT.md「動画ソース」）。
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct VideoSource {
    pub provider: VideoProvider,
    pub external_id: String,
}
