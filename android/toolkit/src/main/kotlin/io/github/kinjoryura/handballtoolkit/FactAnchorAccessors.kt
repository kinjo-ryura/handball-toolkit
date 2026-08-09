package io.github.kinjoryura.handballtoolkit

// FactAnchor の自明アクセサ 5 種（ADR 0004 決定 4 — シム再実装の許可基準
// 「self のみ / ループなし / ドメイン規則なし」を満たす）。iOS シムの
// FactAnchor+Accessors.swift と同一挙動。
//
// **`matchClockOrNull` / `videoClockOrNull` の名前は iOS シムと意図的に違う。**
// Kotlin の sealed subclass は Swift の enum case と違ってメンバを持つため、
// `FactAnchor.Both` に `matchClock` / `videoClock` というメンバが既にある。
// 同名の拡張プロパティを基底型に置くと「受け手の静的型で戻り値型が変わる」
// （Both 型なら non-null メンバ、FactAnchor 型なら nullable 拡張）という
// 静かな罠になるため、null を返しうる側に OrNull を付けて衝突を避ける。

/** anchor の種別。 */
val FactAnchor.kind: FactAnchorKind
    get() = when (this) {
        is FactAnchor.MatchClock -> FactAnchorKind.MATCH_CLOCK
        is FactAnchor.VideoClock -> FactAnchorKind.VIDEO_CLOCK
        is FactAnchor.Both -> FactAnchorKind.BOTH
    }

/** matchClock を持つなら返す（`Both` は matchClock 側）。持たないなら null。 */
val FactAnchor.matchClockOrNull: MatchClock?
    get() = when (this) {
        is FactAnchor.MatchClock -> v1
        is FactAnchor.Both -> matchClock
        is FactAnchor.VideoClock -> null
    }

/** videoClock を持つなら返す（`Both` は videoClock 側）。持たないなら null。 */
val FactAnchor.videoClockOrNull: VideoClock?
    get() = when (this) {
        is FactAnchor.VideoClock -> v1
        is FactAnchor.Both -> videoClock
        is FactAnchor.MatchClock -> null
    }

/** matchClock があれば `elapsedSeconds`、なければ null。 */
val FactAnchor.matchElapsedSeconds: Double?
    get() = matchClockOrNull?.elapsedSeconds

/** videoClock があれば `elapsedSeconds`、なければ null。 */
val FactAnchor.videoElapsedSeconds: Double?
    get() = videoClockOrNull?.elapsedSeconds
