package io.github.kinjoryura.handballtoolkit

// Summary 系 projection の導出値（ADR 0004 決定 4 — 四則演算と自明な 0 除算ガードのみ）。
// iOS シムの Projections+Derived.swift と同一挙動。
//
// **戻り値は Long（iOS シムは Int）。** iOS 側の Int 化は移植元 Swift API の
// シグネチャに合わせるための narrowing で、Kotlin には合わせる先の既存 API が無い。
// 集計フィールドは FFI 上 Long なので、そのまま Long で返して情報を落とさない。

/** シュート試投数（ゴール + シュート失敗）。 */
val TeamSummaryLine.shotAttempts: Long
    get() = goals + shotMisses

/** シュート成功率。試投 0 のときは null（0 除算ガード）。 */
val TeamSummaryLine.scoringRate: Double?
    get() = scoringRate(goals, shotAttempts)

/** シュート試投数（ゴール + シュート失敗）。 */
val PlayerStatLine.shotAttempts: Long
    get() = goals + shotMisses

/** シュート成功率。試投 0 のときは null（0 除算ガード）。 */
val PlayerStatLine.scoringRate: Double?
    get() = scoringRate(goals, shotAttempts)

/** ホームのシュート試投数。 */
val PhaseSummaryLine.homeAttempts: Long
    get() = homeGoals + homeShotMisses

/** アウェイのシュート試投数。 */
val PhaseSummaryLine.awayAttempts: Long
    get() = awayGoals + awayShotMisses

/** ホームのシュート成功率。試投 0 のときは null。 */
val PhaseSummaryLine.homeRate: Double?
    get() = scoringRate(homeGoals, homeAttempts)

/** アウェイのシュート成功率。試投 0 のときは null。 */
val PhaseSummaryLine.awayRate: Double?
    get() = scoringRate(awayGoals, awayAttempts)

/** away − home。負がホームリード（チャート左）、正がアウェイリード（チャート右）。 */
val ScoreProgressionPoint.diff: Long
    get() = awayScore - homeScore

private fun scoringRate(goals: Long, attempts: Long): Double? =
    if (attempts > 0L) goals.toDouble() / attempts.toDouble() else null
