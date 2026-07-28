# ゴールデンコーパス — Swift 実装をオラクルとするパリティ検証データ

ADR 0003 のゴールデンコーパス。`inputs/` は handball-sample-matches の実試合 JSON のコピー、
`expected/` は Swift 実装（RecorderDomain）の projection 出力を正規化した期待値。
Rust 実装の出力が `expected/` と一致することをパリティテスト（P8）で検証する。

## 出所（provenance）

| 項目 | 値 |
|---|---|
| オラクル（RecorderDomain） | HandballRecorder **main** `b7cf57e861a9100ee3f721c8c477e2aae062c3f8` |
| dump ツール | HandballRecorder `parity/oracle-dump` ブランチの `recorder-domain-dump`（Package: `Packages/RecorderDomain`。ブランチ削除後は tag `oracle-dump-final`） |
| 入力コーパス | handball-sample-matches `d64d8d5f7b8ec947fae1640a5f6b0fa40900e369`（`v2/matches/` 4 件 = `.video` 2 + `.timer` 2、`v2/highlights/` 6 件） |
| 生成日 | 2026-07-20（`.timer` 2 件を追加 — handball-project#53。期待値は昇格前に `local/` で生成済みのものを、入力 byte 一致のまま流用したので再 dump していない。highlights は #71 で再生成、`.video` matches は 2026-07-12 生成のまま） |

`2025-12-20-f352ea46` の `match.displayName` に「（前半のみ）」を付記した入力更新
（handball-project#89）を取り込んでいる。`displayName` は projection に現れず期待値に
含まれないため、再 dump はしていない。

`2026-04-19-bera-bera-vs-aula` / `2026-05-05-ohrid-vs-alkaloid` の `match.date` を試合日へ
修正した入力更新（handball-project#115。V1 → V2 backfill で記録日時が転記されていた退行）も
取り込んでいる。`match.date` も projection に現れず期待値に含まれないため、同様に再 dump は
していない。

- 出所ハッシュは **main のコミット**を記録する（`parity/oracle-dump` はパリティ完走後に削除されるため。
  ブランチの不変条件「RecorderDomain ソース不変」により、オラクルの中身 = main の RecorderDomain）
- 再生成手順（main の RecorderDomain が変わったら）: PORTING.md「オラクル側の運用 — 再同期の手順」

```bash
# HandballRecorder の parity/oracle-dump ブランチで
cd Packages/RecorderDomain
swift run recorder-domain-dump --out <出力先>/matches    <handball-sample-matches>/v2/matches/*.json
swift run recorder-domain-dump --out <出力先>/highlights <handball-sample-matches>/v2/highlights/2026-*.json
```

## 期待値の形式（正規化の規約）

1 入力 1 出力で、5 系統を 1 つの JSON に持つ:

```jsonc
{
  "resolver": { "phases": [...], "segments": [...] },   // SegmentResolver.build の全出力
  "timeline": [ { "factID", "resolvedMatchClock"?, "resolvedVideoClock"? } ],
  "summary": { /* teamKey / playerKey ベース。phaseSummaries 含む */ },
  "scoreProgression": { /* points / phaseSpans / totalSeconds / maxAbsDiff */ },  // 無い場合はキー省略
  "liveSamples": [ /* buildVideoMode の採取列 */ ]
}
```

- **ID はコーパス由来キー**: fact は `factID`（UUID 小文字表記 = Rust `Uuid::to_string()` と一致）、
  チームは `teamKey`（`home` / `away`）、選手は `playerKey`（JSON 内の文字列キー）
- **`summary.playerStats` は playerKey 昇順**（ASCII バイト順）に正規化済み。Swift 実装の内部順序
  （uuidString 昇順）は run ごとに変わるため、比較時は Rust 側も同じ順序に整列する
- **nil はキー省略**（Swift JSONEncoder の既定。Rust serde は missing = `None` で読む）
- **キーはアルファベット順・整形は pretty**（`.sortedKeys` / `.prettyPrinted`）。ただし比較は
  文字列一致ではなく **JSON 構造比較**（f64 は parse 後に bit-exact — ADR 0003 §5）
- **`liveSamples` は採取した video 位置（`videoElapsedSeconds`。無キー = `currentVideoClock` なし）を
  そのまま記録**しており、Rust 側はこの位置列を再生して `build_video_mode` するだけでよい
  （サンプリング規則の二重実装を避ける）。採取規則（dump ツール側）: nil + 各 segment の
  start / 中点 / end 直前（`.nextDown`）/ end + 全 phase の外（before / between / after）、
  負値除外・昇順・重複除去

## 構成

```
inputs/
  matches/     — フル試合 4 件: `.video` 2 件（実 JHL 試合を動画から手記録）
                 + `.timer` 2 件（JHA 公式ランニングスコア PDF 由来）
  highlights/  — ハイライト（.videoHighlight）6 件
expected/
  matches/     — 対応する期待出力
  highlights/  — 対応する期待出力
```

`.timer` 2 件は handball-project#53 で公開コーパスへ昇格した。同一試合の `.video` 版と
背番号別の得点内訳が全選手一致することを確認済み（手記録と公式 PDF という独立 2 経路の突き合わせ）。
これにより ADR 0003 §1 が挙げていた「公開コーパスに `.timer` 試合が無い」gap は解消し、
standalone clone でも 3 モードすべてのパリティ検証が再現できる。

未昇格の `.timer`（JHL 由来など）は引き続き `local/` に置きコミットしない（ADR 0003 §1）。

## ローカル `.timer` コーパス（`local/` — gitignore 済み）

`local/inputs/<group>/*.json` + `local/expected/<group>/*.json` を置くと、パリティテスト
（`tests/golden_parity_tests.rs` の `local_timer_corpus_matches_oracle_if_present`）が
自動で拾って照合する。無い環境（standalone clone / CI）ではスキップされる。

再生成手順（jha/jhl-pdf-importer が現行 SAMPLE_DTO_V2 を直接出力するため移行ステップは不要
— handball-project#54、2026-07-15）:

```bash
# 1. importer 産の pdf-matches JSON をそのまま入力に置く
cp <sample-matches>/pdf-matches/jha/<試合>.json \
  crates/handball-toolkit/tests/golden/local/inputs/timer/<試合>.json

# 2. Swift オラクルで期待値を dump（HandballRecorder の parity/oracle-dump。
#    ブランチ削除後は tag oracle-dump-final から checkout で復元）
swift run recorder-domain-dump --out .../tests/golden/local/expected/timer \
  .../tests/golden/local/inputs/timer/*.json
```

pdf-matches 自体の生成は親リポの `tools/jha-pdf-importer/` / `tools/jhl-pdf-importer/`
（`.venv/bin/python parse_jha_pdf.py <PDF> --out <pdf-matches>/jha/<試合>.json`）。

## 比較実行側の前提（重要）

パリティテストは serde_json の **`float_roundtrip` feature を必須**とする（dev-dependencies で
有効化済み）。serde_json の既定 float パースは高速だが正確丸めでなく **1 ulp の誤差**があり、
corpus anchor 値と期待値の両方が僅かにズレて bit-exact 比較が偽陽性の差分を報告する
（2026-07-12 に実際に発生。Swift 側 JSONDecoder / JSONEncoder は正確と実測確認済み — ADR 0003 §5 追記）。
