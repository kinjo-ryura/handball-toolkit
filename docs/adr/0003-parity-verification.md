# パリティ検証戦略 — Swift 実装をオラクルとするゴールデンコーパス検証

## Status

accepted（2026-07-12 起草、同日 grill 済み。handball-project#49）

## 文脈

移植の中核リスクは `SegmentResolver` や R3–R11 など「V2 で苦労して固めたセマンティクス」の写経ミス。安全網は二層で構成する:

1. **単体テスト移植** — Swift Testing の 144 テスト / 約 2,507 行を Rust に写す（`#expect` の即値が回帰ロックとして機能している）
2. **ゴールデンコーパス検証** — handball-sample-matches の実試合 JSON を入力に、Swift 実装の projection 出力と Rust 実装の出力の一致を検証する

Swift 側にはスナップショット/ゴールデンテスト資産は存在しない（調査済み）ため、オラクル出力の dump ツールを新設する。

## 決定

### 1. コーパス

| ソース | 件数 | configuration | 備考 |
|---|---|---|---|
| `handball-sample-matches/v2/matches/` | 2 | `.video` | 実 JHL 試合、各 100+ facts |
| `handball-sample-matches/v2/highlights/` | 6 | `.videoHighlight` | |
| `handball-sample-matches/pdf-matches/`（gitignore 済みローカル） | 可変 | `.timer` | JHA / JHL の PDF インポート産。**公開コーパスに `.timer` 試合が無い gap をここで補う**（ローカル実行専用の拡張コーパス） |

- 公開 8 件のゴールデンは handball-toolkit の `tests/golden/` にコミットする（入力 JSON のコピー + 期待出力）
- `.timer` の公開サンプル追加（gap の恒久解消）は別 Issue 候補とする
- 3 モード網羅の最小フィクスチャ（エッジケース: shootout / `.both` override / stoppage 隣接など）は実試合コーパスとは別に、移植した単体テストが担う

### 2. Sample DTO V2 パーサはコアの一部（`sample_dto` モジュール）

- `SAMPLE_DTO_V2.md` 準拠の serde 型（`kind` discriminator + null 兄弟フィールドの tagged union、anchor の flat end フィールド）+ domain 型への converter を `sample_dto` モジュールとして実装する
- 位置づけは「handball-sample-matches SCHEMA の型付き実装」（research メモの OSS 枠づけと一致）。将来の JSON 検証 CLI・wasm デモの土台
- コア crate 内モジュールとして置く（grill 確定 2026-07-12）。別 crate への分離は「パーサ抜きでコアだけ使いたい」需要が実在してから行う。**分離を機械作業に保つため、依存は `sample_dto` → domain 型の一方通行を厳守**（domain 側から sample_dto を参照しない）

### 3. ID の決定性 — 出力は「コーパス由来キー」で表現する

Swift の `SampleMatchConverterV2` は decode 時に player key → 新規 UUID を割り当てる（非決定的）。オラクルと Rust の出力を突き合わせるため、**ゴールデン出力では domain 内部 ID を使わず、コーパス由来の安定キーへ正規化する**:

- fact は JSON の `factID` で識別
- チームは `teamKey`（`home` / `away`）
- 選手は `playerKey`（JSON 内の文字列キー）

dump ツール・Rust 側テストハーネスの双方が「内部 ID → コーパスキー」の逆写像を持ち、正規化後の JSON を比較する。

### 4. オラクル dump ツール（Swift 側）

- **場所**: HandballRecorder リポに SPM executable target を新設（`Packages/RecorderDomain` に `recorder-domain-dump` executable を追加）。アプリターゲットには触れないため cycle-9 の計測凍結（アプリ本体のコア差し替え禁止）と非干渉（grill 確定 2026-07-12。既存の `MatchExporterV2` は入力側=生データの書き出しであり、projection 出力=模範解答を書き出す手段は本ツールが初）
- **DTO decode はツール内に自前で持つ**（アプリ層の `SampleMatchDTOsV2.swift` はアプリターゲット所属で import 不可。約 200 行の Codable struct の意図的な複製とし、複製である旨をコメントで明記）
- **出力**: コーパス JSON 1 件につき正規化 JSON 1 件

```jsonc
{
  "resolver": { "phases": [...], "segments": [...] },          // SegmentResolver.build の全出力
  "timeline": [ { "factID": "...", "resolvedMatchClock": 123.0, "resolvedVideoClock": 456.0 } ],
  "summary": { /* teamKey / playerKey ベース。phaseSummaries 含む */ },
  "scoreProgression": { /* points / phaseSpans / totalSeconds / maxAbsDiff。null 可 */ },
  "liveSamples": [ /* 各 segment 境界とその中点で buildVideoMode した結果の列 */ ]
}
```

- JSON の正規化: キーはアルファベット順、浮動小数は最短表現（Swift / Rust 両方で往復一致する形式）、配列順は仕様上の決定的順序
- `liveSamples` のサンプリング点は「各 segment の start / 中点 / end 直前」+ 全 phase の外（before / between / after）。境界判定（半開区間・degenerate 特例）を踏ませるため

### 5. 比較規約

- **f64 は完全一致（bit-exact）から始める**（grill 確定 2026-07-12）。移植が演算順を保存していれば IEEE 754 の決定性により一致するはず。ズレ = 写経ミスのシグナルとして扱う。破れたケースは原因を特定し、epsilon 許容に切り替える場合はこの ADR に判断を追記する
- 比較は Rust 側のテスト（`cargo test`）で実行: `tests/golden/` の期待出力と Rust 実装の出力の JSON 構造比較。差分はケース単位で報告

### 6. 完走判定

Issue #49 の「パリティ検証の完走」= 公開コーパス 8 件 + ローカル `.timer` コーパスの全件で、上記 5 系統（resolver / timeline / summary / scoreProgression / liveSamples）の出力が一致し、かつ移植済み単体テスト 144 件が green であること。OSS 公開判断はこの完走後（research メモ）。

## Considered options

- **オラクル出力をアプリ実行時に生成**（DEBUG メニュー等）→ 却下。手動操作が入り再現性がない。CLI 化で CI / 再生成が自動化できる
- **DTO を SPM パッケージへ移動して dump ツールと共有** → 却下（現時点）。アプリ側リファクタは計測凍結中に行わない。移植完了後の整理候補
- **ゴールデンを Swift 側リポにコミット** → 却下。比較を実行するのは Rust 側テストであり、handball-toolkit 単独 clone でパリティ検証が回る方が OSS 公開後も筋が良い
- **プロパティベーステスト（ランダム fact 列生成）** → 採用見送り（初期）。オラクルとの照合には Swift 側でも同一入力を流す配管が要る。ゴールデン + 移植テストで完走後、余力があれば検討

## Consequences

- Swift 実装の挙動が「期待出力ファイル」として固定される — HandballRecorder 側で RecorderDomain を変更したらゴールデン再生成が必要（dump ツールを再実行するだけ）
- `tests/golden/` に実試合データのコピーが入る（handball-sample-matches は public repo のため公開上の新規リスクなし。ローカル `.timer` コーパスはコミットしない）
- dump ツールの DTO 複製は SAMPLE_DTO_V2.md への追従義務を持つ（schema 変更時に 2 箇所修正）

## 参照

- ADR 0001（境界 API 目録）/ ADR 0002（構造化エラー）
- `apps/HandballRecorder/docs/redesign/SAMPLE_DTO_V2.md` — コーパスの schema 正典
- `apps/handball-sample-matches/`（`v2/` パス）
- `handball-project/docs/research/handballrecorder-rust-core.md`「パリティ検証（移植の安全網）」
