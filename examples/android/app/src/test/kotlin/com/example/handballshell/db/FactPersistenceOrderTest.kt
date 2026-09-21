package com.example.handballshell.db

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.sql.Connection
import java.sql.DriverManager

/**
 * [FACT_PERSISTENCE_ORDER]（`factLog` の並び）を、コアと共有する fixture と突き合わせる
 * （handball-project#405）。
 *
 * 同じ規約はコアの `persistence_order` / iOS / Android の Room に別々に実装されていて
 * （Room のクエリはコアの関数を呼べない）、4 箇所が同じ fixture を読む。どれか 1 つだけ
 * 変えると赤くなる。
 *
 * **Room ではなく素の SQLite（sqlite-jdbc）で回す。** JVM の単体テストには Android の SQLite が
 * 無い。`@Query` と同じ定数を実行するので、検査しているのは `ORDER BY` 句そのもの（Room の写像は
 * 対象外）。fixture も SQLite の JSON 関数で読み、JSON ライブラリを足さない。
 */
class FactPersistenceOrderTest {

    @Test
    fun `共通 fixture の並びと一致する`() {
        val fixture = FIXTURE.readText()
        DriverManager.getConnection("jdbc:sqlite::memory:").use { db ->
            db.createStatement().use { it.execute(CREATE_FACT) }
            val cases = cases(db, fixture)
            // 空の fixture で緑にならないようにする（件数は書かない — 足すたびにずれる）。
            assertTrue("共通 fixture にケースが無い", cases.isNotEmpty())
            for ((index, name) in cases) {
                assertEquals(name, expected(db, fixture, index), ordered(db, fixture, index))
            }
        }
    }

    private fun cases(db: Connection, fixture: String): List<Pair<Int, String>> =
        db.prepareStatement(
            "SELECT key, json_extract(value, '$.name') FROM json_each(?, '$.cases') ORDER BY key",
        ).use { statement ->
            statement.setString(1, fixture)
            statement.executeQuery().use { rows ->
                buildList { while (rows.next()) add(rows.getInt(1) to rows.getString(2)) }
            }
        }

    /** その case の facts を入れ直し、[FACT_PERSISTENCE_ORDER] で読み出した id の列。 */
    private fun ordered(db: Connection, fixture: String, index: Int): List<String> {
        db.createStatement().use { it.execute("DELETE FROM fact") }
        db.prepareStatement(
            """
            INSERT INTO fact (id, payloadKind, startMatchSeconds, startVideoSeconds,
                              recordedAtEpochSecond, recordedAtNano)
            SELECT json_extract(value, '$.id'),
                   json_extract(value, '$.kind'),
                   json_extract(value, '$.matchSeconds'),
                   json_extract(value, '$.videoSeconds'),
                   json_extract(value, '$.recordedAt'),
                   0
            FROM json_each(?, ?)
            """,
        ).use { statement ->
            statement.setString(1, fixture)
            statement.setString(2, "$.cases[$index].facts")
            statement.executeUpdate()
        }
        return db.createStatement().use { statement ->
            statement.executeQuery("SELECT id FROM fact ORDER BY $FACT_PERSISTENCE_ORDER").use { rows ->
                buildList { while (rows.next()) add(rows.getString(1)) }
            }
        }
    }

    private fun expected(db: Connection, fixture: String, index: Int): List<String> =
        db.prepareStatement("SELECT value FROM json_each(?, ?) ORDER BY key").use { statement ->
            statement.setString(1, fixture)
            statement.setString(2, "$.cases[$index].expected")
            statement.executeQuery().use { rows ->
                buildList { while (rows.next()) add(rows.getString(1)) }
            }
        }

    private companion object {
        /**
         * コア側の fixture をそのまま読む（写しを持たない）。単体テストの作業ディレクトリは
         * モジュール（`examples/android/app`）。
         */
        val FIXTURE = File("../../../crates/handball-toolkit/tests/fixtures/persistence-order-cases.json")

        /**
         * `ORDER BY` が見る列だけを持つ `fact` 表。列名は [FactRow] と揃える — 改名すると
         * `@Query` は Room のコンパイルで、ここはこのテストで落ちる。
         */
        const val CREATE_FACT = """
            CREATE TABLE fact (
              id TEXT PRIMARY KEY NOT NULL,
              payloadKind TEXT NOT NULL,
              startMatchSeconds REAL,
              startVideoSeconds REAL,
              recordedAtEpochSecond INTEGER NOT NULL,
              recordedAtNano INTEGER NOT NULL
            )
        """
    }
}
