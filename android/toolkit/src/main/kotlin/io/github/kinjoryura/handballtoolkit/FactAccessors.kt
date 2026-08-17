package io.github.kinjoryura.handballtoolkit

// MatchFact / ControlFact の代表 anchor アクセサ（ADR 0004 決定 4 の許可基準内 —
// case の場合分けとフィールド取り出しのみ）。iOS シムの Facts+Accessors.swift と同一挙動。

/**
 * payload を問わず代表 anchor を返す。
 * PlayFact / PossessionFact は唯一の anchor、ControlFact は startAnchor を返す。
 */
val MatchFact.anchor: FactAnchor
    get() = when (val payload = payload) {
        is MatchFactPayload.Play -> payload.v1.anchor
        is MatchFactPayload.Control -> payload.v1.startAnchor
        is MatchFactPayload.Possession -> payload.v1.anchor
    }

/**
 * anchor を 1 本だけ持つ fact（= range を持たない fact）の anchor。
 * ControlFact は start / end の range を持つので null。コアの
 * `MatchFact::single_anchor` と同一挙動（handball-project#154）。
 */
val MatchFact.singleAnchor: FactAnchor?
    get() = when (val payload = payload) {
        is MatchFactPayload.Play -> payload.v1.anchor
        is MatchFactPayload.Possession -> payload.v1.anchor
        is MatchFactPayload.Control -> null
    }

/** payload を問わず開始 anchor を返す。 */
val ControlFact.startAnchor: FactAnchor
    get() = when (this) {
        is ControlFact.PhaseStart -> v1.startAnchor
        is ControlFact.Stoppage -> v1.startAnchor
    }

/** payload を問わず終了 anchor を返す（PhaseStart は常に値あり、Stoppage は任意）。 */
val ControlFact.endAnchor: FactAnchor?
    get() = when (this) {
        is ControlFact.PhaseStart -> v1.endAnchor
        is ControlFact.Stoppage -> v1.endAnchor
    }
