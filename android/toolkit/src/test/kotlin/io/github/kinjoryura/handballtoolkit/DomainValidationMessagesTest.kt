package io.github.kinjoryura.handballtoolkit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * 文言リソースの網羅性（handball-project#136）。
 *
 * DomainValidationMessages.kt の `when` は sealed 型に対して網羅なので、コアに case が
 * 増えると**既定ロケール側はコンパイルエラーで検出される**。コンパイラが見ないのは
 * 次の 2 つで、それをここで押さえる:
 *
 *  1. 追加ロケール（values-ja）への行の足し忘れ — 実行時に既定ロケールへ黙って落ちる
 *  2. 分岐が別 case の resource を指してしまう写経ミス — 型は合うので気付けない
 *
 * リソース XML を直接読むため Context も端末も要らない（JVM 単体テスト）。
 */
class DomainValidationMessagesTest {

    @Test
    fun `既定ロケールと日本語ロケールで string の集合が一致する`() {
        val default = stringNames("values")
        val japanese = stringNames("values-ja")

        assertEquals(
            emptySet(),
            default - japanese,
            "values-ja に無い string がある（日本語文言の足し忘れ）",
        )
        assertEquals(
            emptySet(),
            japanese - default,
            "values に無い string がある（既定ロケールの足し忘れ）",
        )
    }

    @Test
    fun `全ての string がライブラリの接頭辞を持つ`() {
        // build.gradle.kts の resourcePrefix と同じ値。利用側アプリの名前空間へ
        // マージされるため、接頭辞なしは衝突事故になる。
        val prefix = "handball_toolkit_"
        val offenders = (stringNames("values") + stringNames("values-ja")).filterNot { it.startsWith(prefix) }
        assertEquals(emptyList(), offenders, "接頭辞 $prefix が無い string がある")
    }

    @Test
    fun `validation の全 case に title と body がある`() {
        assertEveryCaseHasMessage(MatchValidationError::class.java, "match")
        assertEveryCaseHasMessage(ConfigurationValidationError::class.java, "configuration")
        assertEveryCaseHasMessage(FactValidationError::class.java, "fact")
        assertEveryCaseHasMessage(TimelineValidationError::class.java, "timeline")
    }

    @Test
    fun `write エラーの全 case に title と body がある`() {
        assertEveryCaseHasMessage(CoreWriteException::class.java, "write")
    }

    @Test
    fun `ケース数が ERROR_CODES 表と一致する`() {
        // docs/ERROR_CODES.md が公表している数。ここがずれたら同ドキュメントも直す。
        assertEquals(3, caseNames(MatchValidationError::class.java).size)
        assertEquals(2, caseNames(ConfigurationValidationError::class.java).size)
        assertEquals(22, caseNames(FactValidationError::class.java).size)
        assertEquals(12, caseNames(TimelineValidationError::class.java).size)
        assertEquals(7, caseNames(CoreWriteException::class.java).size)
    }

    // ── helper ──

    private fun assertEveryCaseHasMessage(type: Class<*>, scope: String) {
        val names = stringNames("values")
        val missing = caseNames(type)
            .flatMap { case ->
                val stem = "handball_toolkit_${scope}_${snakeCase(case)}"
                listOf("${stem}_title", "${stem}_body")
            }
            .filterNot { it in names }

        assertTrue(missing.isEmpty(), "${type.simpleName} の文言が足りない: $missing")
    }

    /**
     * sealed 型の case 名。companion（`Companion` / `ErrorHandler`）は入れ子クラスとして
     * 現れるが sealed のサブクラスではないので、代入可能性で弾く。
     */
    private fun caseNames(type: Class<*>): List<String> =
        type.declaredClasses.filter { type.isAssignableFrom(it) }.map { it.simpleName }

    private fun snakeCase(camel: String): String =
        camel.replace(Regex("(?<=.)([A-Z])"), "_$1").lowercase()

    private fun stringNames(valuesDir: String): Set<String> =
        Regex("<string\\s+name=\"([^\"]+)\"")
            .findAll(File(resDir(), "$valuesDir/strings.xml").readText())
            .map { it.groupValues[1] }
            .toSet()

    /** 単体テストの作業ディレクトリはビルド構成で変わるため、res を持つ親を探し当てる。 */
    private fun resDir(): File {
        var dir: File? = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (dir != null) {
            val candidate = File(dir, "src/main/res")
            if (File(candidate, "values/strings.xml").isFile) return candidate
            dir = dir.parentFile
        }
        error("src/main/res が見つかりません (user.dir=${System.getProperty("user.dir")})")
    }
}
