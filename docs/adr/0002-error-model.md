# 構造化エラー体系 — エラーコード + パラメータのみを返す

## Status

accepted（2026-07-12 起草、同日 grill 済み。handball-project#49）

## 文脈

移植元の validation は既に「エラー enum（コード + payload）」と「日本語文言（`DomainValidationIssue.userMessage` → `DomainValidationMessage`）」が分離されている。Rust 移植ではこの分離を境界まで押し進め、**文言レイヤをコアから完全に外す**。これは移植で発生する唯一の意図的再設計ポイント（他はすべて忠実移植）。

理由（research メモより）: OSS に日本語を焼き込まない / 多言語化が各シェルで閉じる / API が「意味」だけを語る。

## 決定

### 1. エラー enum は Swift の 4 系統・全 37 ケースを 1:1 で写す

| Rust enum | Swift 対応 | ケース数 |
|---|---|---|
| `MatchValidationError` | 同名 | 3（`SameTeamOnBothSides` / `EmptyTitle` / `OverlappingRosterSelections { player_ids }`） |
| `ConfigurationValidationError` | 同名 | 2（`NonPositivePhaseDuration { seconds }` / `EmptyVideoExternalId`） |
| `FactValidationError` | 同名 | 20（anchor 系 3 / 文字列・参照 3 / Play kind 必須 2 / PhaseStart 3 / Stoppage 5 / 参照整合 4）→ 現在 22（下の #91 追記） |
| `TimelineValidationError` | 同名 | 12（R3 / R5 / R6 / R7 / R8 / R9 / R11 + shootout 順序 2 + phase 連続性 1 + stoppage 重複・範囲 2） |
| `DomainValidationIssue` | 同名 | 4（`Match` / `Configuration` / `Fact` / `Timeline` の集約） |

- payload（`invalidAnchorForConfiguration(configuration:actual:allowed:)` の 3 引数等）も 1:1 で写す
- **severity は持たない**（一律 blocking、Swift 設計踏襲）。warning が必要になったら struct ラッパー化する将来判断も踏襲

**実装追記（2026-07-20、handball-project#91）**: 上の「1:1 で写す」は**移植中の忠実性制約**であり、移植完走後の case 追加を禁じるものではない。移植元 `Packages/RecorderDomain` は HandballRecorder 側で削除済み（コミット `8aeffb8`、Rust コアへ差し替え）で、対応を保つべき live な Swift 実装はもう存在しない。したがって「Swift に無い case は足せない」という読み方は取らない。

初回の適用が `NonFiniteMatchClock` / `NonFiniteVideoClock` の新設（`FactValidationError` は 20 → 22 ケース）。移植元の負値検査 `< 0.0` は NaN / ±∞ を素通りさせるが、これは移植すべき仕様ではなく**移植元から引き継いだ穴**である。決定 2 の「code は安定契約」に対しては**追加のみ**で、既存 code の意味も名前も変えない。

case を足すときの条件:

- **移植元の挙動を「改善」する変更ではないこと**。ADR 0003 のパリティ検証は正常系の projection 一致を守る枠組みであり、正常系で観測されない値（非有限秒）の扱いはその対象外。判断に迷う場合はゴールデンコーパスの出力が変わるか否かを基準にする（変わるなら改善であり、足してはいけない）
- **シェル側の文言を同時に用意すること**。`DomainValidationMessage.swift` の `switch` は網羅型なので、文言を書かない限りコンパイルが通らない（漏れは構造的に検出される）。文言正典 `DOMAIN_VALIDATION_MESSAGES.md` にも行を足す
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

### 5. `uniffi::Error` の `Display` は開発者診断のみ（Debug 表現を流用する）

**実装追記（2026-07-20、handball-project#70）**: 本 ADR 起草時は validation issue（値）だけを想定していたが、ADR 0004 / 0005 で **throw されるエラー型**（`CoreWriteError` / `SampleDtoError`）が境界に増えた。`uniffi::Error` derive は `Display` 実装を要求するため、ここに何を書くかの方針を決める。

- **`Display` は `{self:?}`（Debug 表現）を流用する**。決定 3「文言はシェルが所有する」により、コアはユーザー向け文言を持てない。かといって `Display` を空にすると開発時のログが無価値になるため、**開発者向け診断としてのみ**機能させる
- **この `Display` の出力をユーザーに見せてはならない**。シェルはエラー種別（Swift の `case` パターン）で分岐して自前の文言を出す。`Repository { message }` / `MigrationPlanInfeasible { message }` / `ImportDecodeFailed { message }` の `message` も同様に診断文字列であり、UI へそのまま流さない
- 根拠をコード側に重複させない。各 `Display` 実装のコメントは本項を参照する（従来は同一のコメントが `ffi_api.rs` と `ffi_write.rs` に一字一句複製されていた）

### 6. panic 境界 — コアの panic は「到達不能」を根拠に許容し、根拠を明文化する

**実装追記（2026-07-20、handball-project#70）**: `[profile.release]` は `panic = abort`（ADR 0004 決定 7）なので、**コアの panic はアプリの abort に直結する**。write 入口は `From<UnexpectedUniFFICallbackError>`（シェル実装が投げた例外）を `CoreWriteError::Repository` へ畳むが、これは「シェル側の失敗」の受け口であって、コア自身の panic を救うものではない。

方針:

- **コア自身の panic は救済しない**。`Result` へ畳むと「起きるはずのないこと」を型に載せることになり、シェルに無意味な分岐を強いる
- 代わりに **panic を残してよいのは「到達不能である根拠を、コードかテストで示せるもの」に限る**。根拠は `expect` のメッセージではなく、下表のいずれかで担保する
- 新しい `expect` / `unwrap` / `unreachable!` を FFI から到達可能なコードへ足すときは、この表に行を追加できることを条件とする

現行の全 panic 箇所と根拠（2026-07-20 時点）:

| 箇所 | 根拠 |
|---|---|
| `ffi_api.rs` `convert_sample_match` の `ids.next().expect` | 直前の `required_id_count` 検査で不足を `InsufficientNewIds` に落とす。数える側と消費する側の一致は `sample_match_exporter_tests.rs` がコーパス横断で assert |
| `ffi_api.rs` `decode_sample_fact` の `fallback.take().expect` | `decode_fact` が `new_id()` を呼ぶのは `fact_id` が None の 1 回だけ（`sample_match_converter.rs`）。`Option::take` が上限を型で保証 |
| `ffi_write.rs` `commit_sample_match_import` の `ids.next().expect` | 同上（`required_import_id_count` 検査 + `sample_import_tests.rs` の一致 assert） |
| `ffi_support.rs` `u32::try_from(obj).expect` | 対象は phase 数とコアが数える ID 個数。いずれも u32 を越えない構造 |
| `write.rs` `unreachable!("plays_to_convert は Play のみ")` | 直前に `MatchFactPayload::Play` で filter 済み |
| `sample_match_encoder.rs` の `expect` 3 箇所 | `SampleMatchDtoV2` は derive `Serialize` の plain struct（String / f64 / Option / Vec）で `to_value` は失敗しない。**非有限 f64 も Err ではなく `null` になる**（実測）。`Value` の再 serialize も失敗せず、serde_json の出力は常に UTF-8 |
| （wasm）`lib.rs` `build_match_view` の `ids.next().expect` | `ffi_api.rs` の同型（直前の `required_id_count` 検査で `InsufficientNewIds` に落とす）。個数一致は `wasm_binding_tests.rs` の `insufficient_ids_boundary` が required-1 / required の両側で assert |
| （wasm）`lib.rs` `build_match_view_js` の `to_string().expect` | `MatchView` はコアの derive `Serialize` 型を束ねた plain struct。根拠は `sample_match_encoder.rs` の行と同じ |

**注**: 「非有限 f64 は `null` になる」は panic しない代わりに **export を静かに壊す**（読み戻せない JSON を成功として書く）。これは panic 境界ではなく validation の穴であり、handball-project#91 で `NonFiniteMatchClock` / `NonFiniteVideoClock` を新設して**書き込み時点で弾く**ことで塞いだ（決定 1 の #91 追記）。encoder 側は引き続き非有限を弾かない — 弾くべき地点は validation であり、encoder に二重の防御を置くと責務が分散するため。

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
