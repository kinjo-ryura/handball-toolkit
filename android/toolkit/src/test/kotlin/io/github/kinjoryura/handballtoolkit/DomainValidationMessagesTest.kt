package io.github.kinjoryura.handballtoolkit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * 文言リソースの網羅性と、docs/ERROR_CODES.md の表の追随（handball-project#136 / #358）。
 *
 * DomainValidationMessages.kt の `when` は sealed 型に対して網羅なので、コアに case が
 * 増えると**既定ロケール側はコンパイルエラーで検出される**。コンパイラが見ないのは
 * 次の 3 つで、それをここで押さえる:
 *
 *  1. 追加ロケール（values-ja）への行の足し忘れ — 実行時に既定ロケールへ黙って落ちる
 *  2. 分岐が別 case の resource を指してしまう写経ミス — 型は合うので気付けない
 *  3. docs/ERROR_CODES.md の表への行の足し忘れ・改名漏れ — 外部シェル実装者向けの
 *     正典（ADR 0002 で code は安定契約）なので、黙って古びると公開契約がずれる
 *
 * **3 は件数ではなく code 名の集合で突き合わせる。** 期待する件数をテストへ書き写すと
 * 表と 2 箇所が同じ数を持ち、case を足すたびに両方を直すことになる（handball-project#358。
 * 実際に #352 で CI を 1 回落としている）。表の行そのものを読めば書き写す値は無くなり、
 * 件数の一致より強く改名・写経ミス・表への追加漏れまで捕まる。
 *
 * リソース XML と Markdown を直接読むため Context も端末も要らない（JVM 単体テスト）。
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
    fun `ERROR_CODES 表の code と sealed 型の case が一致する`() {
        assertCodesMatchDocument("scope: `match`", MatchValidationError::class.java)
        assertCodesMatchDocument("scope: `configuration`", ConfigurationValidationError::class.java)
        assertCodesMatchDocument("scope: `fact`", FactValidationError::class.java)
        assertCodesMatchDocument("scope: `timeline`", TimelineValidationError::class.java)
        assertCodesMatchDocument("`CoreWriteError`", CoreWriteException::class.java)
    }

    /**
     * 見出しが名乗る件数を、その見出しの表の行数だけで検算する。ここは Kotlin の型を
     * 見ないので、シムが扱わない `SampleDtoError` / `SampleMatchDecodeErrorV2` も含む
     * ドキュメント全体が対象になる。
     */
    @Test
    fun `ERROR_CODES 見出しの件数が自分の表の行数と一致する`() {
        val mismatches = sections.mapNotNull { (heading, body) ->
            val declared = Regex("""\((\d+)\)\s*$""").find(heading)?.groupValues?.get(1)?.toInt()
            val actual = tableCodes(body).size
            if (declared == null || declared == actual) null else "$heading → 表は $actual 行"
        }

        assertEquals(emptyList(), mismatches, "見出しの件数が表の行数と合っていない")
    }

    // ── helper ──

    /**
     * 表の code と case 名を集合として突き合わせる。
     *
     * 比較前に小文字化するのは、表が搬送される wire code を載せているのに対し case 名は
     * Rust の variant 名で、1 箇所だけ大小がずれるため（`emptyVideoExternalID` は Swift 期の
     * 綴りを `#[serde(rename)]` で保っており、variant は `EmptyVideoExternalId`）。
     * 大小しか違わない別 code は作れないので、これで取りこぼしは出ない。
     */
    private fun assertCodesMatchDocument(heading: String, type: Class<*>) {
        val body = sections.firstOrNull { it.first.contains(heading) }?.second
            ?: error("docs/ERROR_CODES.md に見出し「$heading」がありません")
        val documented = tableCodes(body).associateBy { it.lowercase() }
        val declared = caseNames(type).associateBy { it.lowercase() }

        assertEquals(
            emptyList(),
            (declared - documented.keys).values.sorted(),
            "${type.simpleName}: 表に無い case がある（docs/ERROR_CODES.md へ行を足す）",
        )
        assertEquals(
            emptyList(),
            (documented - declared.keys).values.sorted(),
            "${type.simpleName}: 表にあるが実在しない code がある（docs/ERROR_CODES.md から行を消す）",
        )
    }

    /** `##` / `###` 見出しごとに (見出し行, 次の見出しまでの本文) を返す。 */
    private val sections: List<Pair<String, String>> by lazy {
        val text = errorCodesDocument()
        val headings = Regex("""^#{2,3} .*$""", RegexOption.MULTILINE).findAll(text).toList()
        headings.mapIndexed { index, heading ->
            val end = headings.getOrNull(index + 1)?.range?.first ?: text.length
            heading.value.trim() to text.substring(heading.range.last + 1, end)
        }
    }

    /** 表の 1 列目に置かれた `code`。見出し行と区切り行は backtick が無いので素通りする。 */
    private fun tableCodes(body: String): List<String> =
        Regex("""^\|\s*`([^`]+)`""", RegexOption.MULTILINE)
            .findAll(body)
            .map { it.groupValues[1] }
            .toList()

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

    private fun resDir(): File = File(ancestorHolding("src/main/res/values/strings.xml"), "src/main/res")

    private fun errorCodesDocument(): String =
        File(ancestorHolding("docs/ERROR_CODES.md"), "docs/ERROR_CODES.md").readText()

    /**
     * 単体テストの作業ディレクトリはビルド構成で変わるため、目的のファイルを持つ親
     * ディレクトリを探し当てる。res は toolkit モジュール直下、ERROR_CODES.md は
     * リポジトリ直下にあり深さが違うので、固定の相対パスでは両方に届かない。
     */
    private fun ancestorHolding(relativePath: String): File {
        var dir: File? = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (dir != null) {
            if (File(dir, relativePath).isFile) return dir
            dir = dir.parentFile
        }
        error("$relativePath が見つかりません (user.dir=${System.getProperty("user.dir")})")
    }
}
