package io.github.kinjoryura.handballtoolkit

// RosterSelection の Set ビュー（ADR 0004 決定 6）。iOS シムの
// RosterSelection+PlayerIDSets.swift と同一挙動。
//
// FFI 上は決定的順序のソート済みリスト（Rust `BTreeSet<Uuid>` 由来 — バイト昇順）で
// 運ばれる。Kotlin 側で作る値も同じ正準順に揃えないと、data class の equals（List 比較）が
// 中身の等しい 2 値を不一致と判定する。

/** ベンチ入り選手の Set ビュー。 */
val RosterSelection.benchedPlayerIdSet: Set<PlayerId>
    get() = benchedPlayerIds.toSet()

/** 名簿外選手の Set ビュー。 */
val RosterSelection.outOfRosterPlayerIdSet: Set<PlayerId>
    get() = outOfRosterPlayerIds.toSet()

/**
 * Set から `RosterSelection` を作る。要素は正準順（バイト昇順）へ並べ替える。
 *
 * 生成コンストラクタは順序を保証しない `List` を素通しするため、Set を持っている
 * 呼び出し側はこちらを使う。
 */
fun RosterSelection.Companion.of(
    benchedPlayerIds: Set<PlayerId> = emptySet(),
    outOfRosterPlayerIds: Set<PlayerId> = emptySet(),
): RosterSelection = RosterSelection(
    canonicalOrder(benchedPlayerIds),
    canonicalOrder(outOfRosterPlayerIds),
)

/**
 * Rust `BTreeSet<Uuid>` と同じ順序（バイト昇順）に並べる。
 *
 * **`sorted()` を使わないこと。** `java.util.UUID.compareTo` は 2 本の long を
 * **符号付き**で比較するため、最上位ビットが立つ UUID（先頭が 8〜f）が負数として
 * 先頭に来てしまい、バイト昇順にならない。`toString()` は固定長 36 文字・小文字
 * hex・ダッシュ位置固定なので、文字列の辞書順がそのままバイト昇順になる。
 */
private fun canonicalOrder(ids: Set<PlayerId>): List<PlayerId> = ids.sortedBy { it.toString() }
