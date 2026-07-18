# export オラクル fixture — Swift `MatchExporterV2` の encode 出力（バイト正）

Rust の export 方向（`sample_dto::export_match` + `encode_sample_match`）は移植ではなく
**新規実装**（ADR 0004 決定 2）。アプリ層 `MatchExporterV2` をオラクルに、決定的な
UUID / 日時で組んだ 3 試合の encode 出力を fixture として保存し、
`tests/sample_match_exporter_tests.rs` が**バイト一致**（文字列比較）で検証する。

バイト一致にした理由: JSON 構造比較では見えない Swift `JSONEncoder` の書式仕様
（UUID 大文字 / `.iso8601` の秒未満切り捨て / 整数値 double の `.0` 省略 /
`" : "` 区切り / nil キー省略 / 空配列の `[\n\n  ]` 形 / `/` 非エスケープ）を
すべて釘付けするため。SAMPLE_DTO_V2 の出力形式はこの fixture が正であり、
Swift 側 exporter 削除後も配信ファイルの形式が変わらないことを保証する。

## 出所（provenance）

| 項目 | 値 |
|---|---|
| オラクル | HandballRecorder **main** `b7cf57e861a9100ee3f721c8c477e2aae062c3f8` の `HandballRecorder/SampleMatches/V2/MatchExporterV2.swift` + `SampleMatchDTOsV2.swift`（無改変コピーで実行） |
| 生成ツール | 一時 SwiftPM パッケージ（RecorderDomain へ path 依存 + 上記 2 ファイルのコピー + 決定的試合を組む main.swift）。再生成手順は下記 |
| 生成日 | 2026-07-18 |

## fixture の内容

| ファイル | 釘付けする仕様 |
|---|---|
| `timer.json` | 全 6 play kind / stoppage 2 種（note 有無）/ phaseStart regular+shootout / matchClock anchor（整数・小数秒）/ recordedAt 昇順ソート（生成時は逆順投入）/ 秒未満切り捨て / 日本語・`/` を含む文字列 / jerseyNumber nil 省略 / relatedPlayerKey / teamKey・playerKey なしの freeNote |
| `video.json` | title nil（displayName キー省略）/ away 選手 0（空配列の形）/ `.local` provider / videoClock・both anchor（end の flatten 含む）/ playerKey なしの play |
| `video-highlight.json` | facts 0 件 / `.videoHighlight` + youtube / `.999` 秒の切り捨て（丸め上げしない） |
| `slugs.json` | `defaultSlug` の 3 形: ASCII 両チーム名 / 日本語→空 slug の shortID フォールバック / 記号・空白の `-` 折り畳み |

## 再生成手順（オラクル側の schema / encoder が変わったら）

Swift 側 exporter は本境界移行の完了で削除されるため、通常は再生成不要（fixture が凍結された正）。
削除前に再生成が必要になった場合:

```bash
# 1. 一時パッケージを作る（RecorderDomain へ path 依存する executableTarget "gen"）
# 2. HandballRecorder/SampleMatches/V2/{SampleMatchDTOsV2,MatchExporterV2}.swift を無改変コピー
#    （diff -q でコピーの無改変を確認する）
# 3. tests/sample_match_exporter_tests.rs の決定的試合と同一の main.swift で dump
swift run gen <出力先>   # {timer,video,video-highlight}.json + slugs.json
```

決定的試合の定義（UUID / 日時 / fact 列）は `tests/sample_match_exporter_tests.rs` 側と
1:1 対応させること — fixture と Rust テストの入力がズレるとバイト一致の意味がなくなる。
