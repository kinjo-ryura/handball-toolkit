package io.github.kinjoryura.handballtoolkit

import java.util.UUID

// TeamOption の安定識別子（iOS シムの Identifiable+Conformances.swift 相当）。
//
// iOS シムが持つ Identifiable / CaseIterable 適合そのものは移植しない:
//   - Identifiable は SwiftUI の ForEach 専用の概念で、Compose は `key = { it.id }` の
//     ラムダを取るため対応物が要らない。`id` フィールドを持つ生成型はそのまま使える
//   - CaseIterable（PlayEventKind）は Kotlin の enum が `entries` を標準で持つため不要
// 移植が要るのは「コアが供給しない識別子」だけで、それが下の TeamOption。

private val NEW_TEAM_OPTION_ID_VALUE: UUID = UUID.fromString("00000000-0000-0000-0000-00000000ffff")

/**
 * 「新規作成」候補の識別子。
 *
 * コアは UUID を生成しない（設計不変条件 2）ため、既存チームを持たない候補の識別子は
 * シムが供給する（ADR 0005 決定 2 追記 — handball-project#67）。1 つの候補リストに
 * 新規作成候補は必ず 1 件だけなので、固定値で一意になる。iOS シムと同じ値。
 */
val TeamOption.Companion.NEW_TEAM_OPTION_ID: UUID
    get() = NEW_TEAM_OPTION_ID_VALUE

/** リスト表示の安定キー。既存チームがあればその id、無ければ [NEW_TEAM_OPTION_ID]。 */
val TeamOption.id: UUID
    get() = existing?.id ?: NEW_TEAM_OPTION_ID_VALUE
