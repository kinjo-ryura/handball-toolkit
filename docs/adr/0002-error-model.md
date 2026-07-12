# 構造化エラー体系 — エラーコード + パラメータのみを返す

## Status

draft（2026-07-12 起草、grill 前。handball-project#49）

## 文脈

移植元の validation は既に「エラー enum（コード + payload）」と「日本語文言（`DomainValidationIssue.userMessage` → `DomainValidationMessage`）」が分離されている。Rust 移植ではこの分離を境界まで押し進め、**文言レイヤをコアから完全に外す**。これは移植で発生する唯一の意図的再設計ポイント（他はすべて忠実移植）。

理由（research メモより）: OSS に日本語を焼き込まない / 多言語化が各シェルで閉じる / API が「意味」だけを語る。

## 決定

### 1. エラー enum は Swift の 4 系統・全 37 ケースを 1:1 で写す

| Rust enum | Swift 対応 | ケース数 |
|---|---|---|
| `MatchValidationError` | 同名 | 3（`SameTeamOnBothSides` / `EmptyTitle` / `OverlappingRosterSelections { player_ids }`） |
| `ConfigurationValidationError` | 同名 | 2（`NonPositivePhaseDuration { seconds }` / `EmptyVideoExternalId`） |
| `FactValidationError` | 同名 | 20（anchor 系 3 / 文字列・参照 3 / Play kind 必須 2 / PhaseStart 3 / Stoppage 5 / 参照整合 4） |
| `TimelineValidationError` | 同名 | 12（R3 / R5 / R6 / R7 / R8 / R9 / R11 + shootout 順序 2 + phase 連続性 1 + stoppage 重複・範囲 2） |
| `DomainValidationIssue` | 同名 | 4（`Match` / `Configuration` / `Fact` / `Timeline` の集約） |

- payload（`invalidAnchorForConfiguration(configuration:actual:allowed:)` の 3 引数等）も 1:1 で写す
- **severity は持たない**（一律 blocking、Swift 設計踏襲）。warning が必要になったら struct ラッパー化する将来判断も踏襲
- 自動修復はコアで行わない（判定のみ。修復はシェルの責務）
- ルール ID（R3–R11、R1/R2/R4/R10 は歴史的欠番）は doc コメントに Swift 同様に明記し、`DOMAIN_VALIDATION_RULES.md` と相互参照可能にする

### 2. 境界のワイヤ形式は `(scope, code, params)`

FFI / JSON 境界でのエラー表現（serde 形式）:

```json
{ "scope": "fact",     "code": "negativeMatchClock" }
{ "scope": "fact",     "code": "invalidAnchorForConfiguration",
  "params": { "configuration": "timer", "actual": "videoClock", "allowed": ["matchClock"] } }
{ "scope": "timeline", "code": "playRecordedOutsidePhaseRange", "params": { "kind": "regular" } }
{ "scope": "match",    "code": "overlappingRosterSelections",
  "params": { "playerIds": ["..."] } }
```

- `scope` = `match` / `configuration` / `fact` / `timeline`（`DomainValidationIssue` の 4 系統）
- `code` = Swift の case 名そのまま（camelCase）。**シェルの文言テーブルの lookup key は `(scope, code)`**。Swift 実装の case 名と一致させることで、既存の文言正典 `DOMAIN_VALIDATION_MESSAGES.md` をそのまま対応表として使える
- `params` は payload のフィールドをそのまま camelCase キーで持つ（無ければ省略）
- code は**安定契約**: 一度出荷した code の改名は breaking change として扱う

### 3. 文言はシェルが所有する

- 日本語文言の正典は引き続き `apps/HandballRecorder/docs/redesign/DOMAIN_VALIDATION_MESSAGES.md`
- 各シェルが `(scope, code) → ローカライズ文言` の写像を所有する。「case 名 / 内部用語を UI に漏らさない」という Swift `userMessage` の責務は各シェルへ移る
- コアには文言・翻訳キー・ロケール処理を一切置かない

### 4. blocking 契約

`validate_append` / `validate_update` / `validate_delete` が非空の `Vec<DomainValidationIssue>` を返したら、シェルは書き込みを拒否する（Swift では repository が throw する契約に対応）。コアは Result ではなく issue の列を返す — 「複数の問題を一度に報告する」挙動を保存するため。

## Considered options

- **`code` を snake_case / dot 区切り（例 `fact.negative_match_clock`）にする** → 却下。Swift case 名との一致を崩すと文言正典・テスト・ドキュメントの相互参照に恒常的な変換層が要る。scope は別フィールドにあるため prefix も不要
- **`thiserror` で `Error` trait 実装 + Display 文言** → 却下。Display に英語文言を書くこと自体が「文言をコアに焼き込まない」に反する。validation issue は「エラー」というより検査結果の値であり、`Error` trait 準拠は不要
- **エラーを bitflags / 数値コード化** → 却下。可読性と params の表現力を失う。パフォーマンス要求も無い

## Consequences

- 新しいシェル（Android / wasm デモ）は `(scope, code)` 写像を書くだけで多言語化が閉じる
- Swift シェルが将来 Rust コアへ差し替わる場合、`DomainValidationMessage.swift` 相当の写像はシェル側に残る（現行と同じ場所・同じ責務）
- code の安定契約により、エラーケースの追加は自由だが改名・削除は semver major 扱い

## 参照

- ADR 0001（境界 API 目録）
- `apps/HandballRecorder/docs/redesign/DOMAIN_VALIDATION_RULES.md` / `DOMAIN_VALIDATION_ERRORS.md` / `DOMAIN_VALIDATION_MESSAGES.md`
- `handball-project/docs/research/handballrecorder-rust-core.md`「エラーは構造化」
