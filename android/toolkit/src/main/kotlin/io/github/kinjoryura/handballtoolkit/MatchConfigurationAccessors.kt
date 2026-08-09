package io.github.kinjoryura.handballtoolkit

// MatchConfiguration の UI helper 4 種（ADR 0004 決定 4）。iOS シムの
// MatchConfiguration+Accessors.swift と同一挙動。旧 `contentKind` は
// 再提供しない（ADR 0001 — 公開面に出さないまま）。
//
// `phaseDurationSecondsOrNull` の OrNull は FactAnchorAccessors.kt と同じ理由 —
// `MatchConfiguration.Timer` に同名の non-null メンバがあるため。

/** configuration の種別。 */
val MatchConfiguration.kind: MatchConfigurationKind
    get() = when (this) {
        is MatchConfiguration.Timer -> MatchConfigurationKind.TIMER
        is MatchConfiguration.Video -> MatchConfigurationKind.VIDEO
        is MatchConfiguration.VideoHighlight -> MatchConfigurationKind.VIDEO_HIGHLIGHT
    }

/**
 * 試合の時計 source of truth（UI helper。source of truth 自体は常に
 * `MatchConfiguration` の case そのもの）。
 */
val MatchConfiguration.captureMethod: CaptureMethod
    get() = when (this) {
        is MatchConfiguration.Timer -> CaptureMethod.MANUAL_CLOCK
        is MatchConfiguration.Video, is MatchConfiguration.VideoHighlight -> CaptureMethod.VIDEO
    }

/** 動画 source（UI helper。`Timer` では null）。 */
val MatchConfiguration.videoSource: VideoSource?
    get() = when (this) {
        is MatchConfiguration.Timer -> null
        is MatchConfiguration.Video -> v1
        is MatchConfiguration.VideoHighlight -> v1
    }

/** `Timer` の phaseDurationSeconds（他の case では null）。 */
val MatchConfiguration.phaseDurationSecondsOrNull: Double?
    get() = when (this) {
        is MatchConfiguration.Timer -> phaseDurationSeconds
        is MatchConfiguration.Video, is MatchConfiguration.VideoHighlight -> null
    }
