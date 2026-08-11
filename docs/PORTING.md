# 移植作業ガイド — RecorderDomain → Rust

> **状態: 移植完走（2026-07-19）。本ファイルは完了した移植作業の記録であり、現在地の管理台帳ではない。** P0〜P8 完了（移植テスト 140/140 green・パリティ bit-exact 一致）、P9 は起草時点で全項目見送りを判断（その後トリガー到来により一部を実施 — 下記参照）。
>
> **完走後の設計変更はここで追跡していない。** iOS シェル向けの FFI 本境界は [ADR 0004](adr/0004-ios-full-boundary.md)、保存・更新発火のコア移管は [ADR 0005](adr/0005-core-write-orchestration.md)（各 ADR の「実装追記」が進捗を持つ）。2026-07-15 の UniFFI PoC 境界（JSON in → JSON out）は ADR 0004 で廃止済み。**進行中・未着手の作業は GitHub Issues が正**（完走時点に挙げた残り #52 / #53 / #54 / #55 / #57 / #58 / #59 はすべて完了）。
>
> **オラクル（RecorderDomain）は凍結済み**。以下で参照している `../HandballRecorder/Packages/RecorderDomain/` は HandballRecorder main から削除された（`8aeffb8`、2026-07-18）。読む必要が出たら tag `oracle-dump-final` から取り出す（手順は CLAUDE.md「移植のオラクル」節）。**「Swift が真実の仕様」は移植面に限る** — 完走後に Rust 独自追加された挙動は Rust 実装 + ADR の実装追記が正。

セッションをまたいで移植を進めるための「現在地と次の一手」のファイル。**各セッションの冒頭でこれを読み、進捗があったらチェックを更新する**。設計判断の正典は `docs/adr/`（このファイルには理由を書かない）。

- 背景・経緯: handball-project#49 / `handball-project/docs/research/handballrecorder-rust-core.md`
- 設計: [ADR 0001](adr/0001-boundary-api.md)（境界 API 目録）/ [ADR 0002](adr/0002-error-model.md)（エラー体系）/ [ADR 0003](adr/0003-parity-verification.md)（パリティ検証）— すべて accepted（2026-07-12）。完走後に [ADR 0004](adr/0004-ios-full-boundary.md)（FFI 本境界）/ [ADR 0005](adr/0005-core-write-orchestration.md)（write orchestration）が accepted（2026-07-18）
- 移植元（真実の仕様）: `../HandballRecorder/Packages/RecorderDomain/`（sibling submodule）。挙動に迷ったら Swift 実装とそのテストを読む。**「改善」しない**

## 作業規律

- **忠実移植**: Swift ファイルと Rust ファイルを 1:1 対応させ（ADR 0001 のミラー表）、演算順も保存する。改善したくなったら Issue 化して完走後に回す
- **テストも同時移植**: モジュールを写したら、対応する Swift Testing のテストを同じセッションで移植する（後回しにしない）。`#expect` の即値はそのまま写す
- **ブランチ運用（このリポ）**: フェーズ単位の作業ブランチ（例: `port/p2-types`）で進め、フェーズ完了（`cargo test` green + このファイルのチェック更新）時に main へ merge する。main は常に green を保つ。push 済みコミットの rebase / squash はしない
- **親リポの pointer bump は main への merge 時のみ**（作業ブランチの中間状態を親リポが指さない）
- **コミット粒度**: 1 モジュール（実装 + テスト）= 1 コミット目安。メッセージ規約は親リポ commit skill（日本語 Conventional Commits 風）
- **push 順**: handball-toolkit → 親リポ。逆にすると親リポがリモートに存在しない commit を指す（2026-07-12 に実際に発生）
- **ブランチ保護（2026-07-26〜）**: main は ruleset `protect-main` で保護済み（**PR 必須 + CI green 必須**、bypass なし）。**手順の正典は CLAUDE.md「変更の出し方」** — ここには導入の経緯だけ残す（二重管理を作らないため）
  - 当初の再検討トリガーは「外部コントリビュータが現れたら PR + CI へ切替」だったが、到来を待たず public 化と同時に入れた。push 済みコミットを amend しかけた事例が実際に発生し、上の「push 済みコミットの rebase / squash はしない」を機械的に強制する価値が先に立ったため（handball-project#134）
  - force push が本当に必要になったら ruleset を一時的に無効化する。規約上まず起きない想定

## オラクル（HandballRecorder）側の運用

仕分けの原則: **ブランチに隔離するのは「捨てる物」、main に入れるのは「残す物」**。パリティ検証の部品のうち、捨てる物は Swift 側の dump ツールだけ。Rust 側（ゴールデン + 比較テスト + 移植テスト）は完走後も回帰ロックとして永続するので toolkit の main に置く。

- dump ツール（P7）は HandballRecorder の **`parity/oracle-dump` ブランチに置き、main へは merge しない**
- **寿命はパリティ完走まで**。完走したらブランチ先端に tag（`oracle-dump-final`）を打ってからブランチを削除する（tag は消えないので万一の再利用時に checkout で復元できる。ツール自体は数百行なので作り直しも安い）
- **ブランチの不変条件: RecorderDomain のライブラリソースを一切変更しない**。追加してよいのは dump ツールのファイルと Package.swift の executable target 定義のみ。これにより「オラクルの中身 = main の RecorderDomain」が常に成り立つ
- **ゴールデンの出所記録**: `tests/golden/` に「どの HandballRecorder **main** コミットの RecorderDomain から生成したか」のハッシュを記録する。ブランチのハッシュは使わない（ブランチ削除後に消えるため。main のハッシュは永続する）
- **再同期の手順**（完走前に main の RecorderDomain へ変更が入ったら）: `parity/oracle-dump` に main を merge → dump 再実行 → `tests/golden/` と出所ハッシュを更新 → Rust 実装を追従 → 一括で commit。完走後に Swift 側が変わった場合はフル再生成ではなく「変更に対応する Swift テストの移植」で差分を担保する
- **Rust 移植のために HandballRecorder に切るブランチはこの 1 本のみ**。通常のアプリ開発（バグ修正含む）は従来通り main 運用。完走後にアプリのコアを Rust へ差し替える場合は専用 feature ブランチ + TestFlight 検証になるが、それは #49 の範囲外（その時に判断）

## 全体フローと現在地

- [x] P0 開発環境（Nix flake + direnv、rust-toolchain.toml）
- [x] P1 設計 ADR 3 本（起草 → grill → accepted、2026-07-12）
- [x] P2 型の移植（依存 DAG 順。各モジュール = 実装 + テスト同時。2026-07-12）
  - [x] `ids`（Identifiers.swift。type alias — ADR 0001）
  - [x] `clock`（MatchClock / VideoClock / FactAnchor / FactAnchorKind）
  - [x] `configuration`（MatchConfiguration ほか。**ContentKind は移植しない** — ADR 0001）
  - [x] `entities`（Match / Team / Player / PlayerPhoto / RosterSelection）
  - [x] `facts`（MatchFact / MatchFactPayload / PlayFact / ControlFact ほか）
- [x] P3 validation エラー型（4 enum・37 ケース + DomainValidationIssue + ワイヤ形式 — ADR 0002。2026-07-12）
- [x] P4 projection（**最難関**。ADR 0001「保存すべきセマンティクス」9 項目を常に参照。2026-07-12）
  - [x] `time_segment`
  - [x] `segment_resolver`（最重要・最繊細。baseline rolling forward / stoppage carve / 半開区間）
  - [x] `timeline`
  - [x] `summary`
  - [x] `score_progression`
  - [x] `live_match`
- [x] P5 validators（fact / fact_log / match_write / configuration / match の 5 種。2026-07-12）
- [x] P6 `sample_dto` モジュール（SAMPLE_DTO_V2 準拠の serde 型 + converter。依存は domain への一方通行厳守 — ADR 0003。ID 供給はシェル注入・変換結果に逆写像同梱 — ADR 0003 §2 追記。2026-07-12）
- [x] P7 オラクル dump ツール（HandballRecorder の `parity/oracle-dump` ブランチ。main へ merge しない — 上記「オラクル側の運用」）+ `tests/golden/` 整備 — ADR 0003（2026-07-12。公開 8 件の golden 生成済み。出所・正規化規約・再生成手順は `crates/handball-toolkit/tests/golden/README.md`）
- [x] P8 パリティ検証完走（完走時点で公開 8 件 + ローカル `.timer` 2 件 × 5 系統 bit-exact 一致、移植テスト 140 件 green — 分母補正は下記。2026-07-12）。**現在のコーパス件数は `tests/golden_parity_tests.rs` の assert が正**（列挙漏れ検知を兼ねる。ローカル分は gitignore なので手元の `tests/golden/local/` を見る）
  - ハーネス: `tests/golden_parity_tests.rs`。**serde_json の `float_roundtrip` feature 必須**（ADR 0003 §5 追記）
  - ローカル `.timer` は pdf-matches（当時は旧 V2 形式）を一時スクリプトで移行して使用（ADR 0003 §1 追記）。その後 importer の現行スキーマ化（handball-project#54、2026-07-15）で移行は不要になり、スクリプトは削除済み（handball-project#55、2026-07-19）
  - オラクル側の後始末実施済み: `parity/oracle-dump` 先端に tag `oracle-dump-final` を打ちブランチ削除（push は手動: `git push origin oracle-dump-final`）
- [x] P9 完走後の判断（2026-07-12 実施 — **全項目見送り**。トリガー到来時に再判断）
  - OSS 公開判断: 見送り → **2026-07-26 に公開へ転換**（handball-project#134）。トリガーだった「公開意思が固まったとき」は、Android 実装者を迎える方針の確定をもって到来した。ライセンスは **MIT 単独**（フォーク・競合実装を許容し、還元の強制力は求めない方針。permissive の中でデュアルを採らなかった経緯も #134 のコメントに残す）。README に Getting Started を追加し、エラーコード表（`docs/ERROR_CODES.md`）を英語で新設、CI（`cargo test` / `clippy` / `fmt --check`）を整備した。**英語で持つのはエラーコード表 1 本のみ**（README を含む他は日本語 — 翻訳の二重管理を作らないため）
  - ID の newtype 化: 見送り（handball-project#52 で管理）→ **2026-07-19 に実施済み**（ADR 0001 の追記を参照）
  - `.timer` 公開サンプル追加: 見送り（handball-project#53）。importer 現行スキーマ化（handball-project#54）の後に正規生成で進める
  - 境界拡張候補: 見送り（ADR 0001「将来の境界拡張候補」どおり）。トリガー: Android シェル実装時

P2〜P5 の順序は Swift のモジュール依存 DAG（Identifiers → Clock → Configuration → Facts / Entities → Validation → Projection → Validators）に従う。P3（エラー型）を要求するのは P5 だけなので、P3 と P4 は入れ替えてもよい。

## テスト移植のメモ

- 移植元テスト: `../HandballRecorder/Packages/RecorderDomain/Tests/RecorderDomainTests/`（Swift Testing 144 テスト / 約 2,507 行）
- フィクスチャヘルパーは Rust 側 `tests` 内の `mod fixtures` に集約する
- 移植したテスト数はパリティ完走判定（P8）の分子になるので、モジュール完了時にここへ累計を記録する: **現在 140 / 140（完了）**
  （RecorderDomainTests 8 / SegmentResolverAdvanced 21 / TimelineProjection 12 / SummaryProjection 15 / ScoreProgressionProjection 7 / LiveMatchProjection 10 / MatchValidator 5 / ConfigurationValidator 10 / FactValidator 25 / RosterReferenceValidation 4 / FactLogValidator 23）
- 分母の補正（144 → 140）: `DomainValidationMessageTests.swift`（4 件）は文言レイヤのテストで移植対象外（ADR 0002 — 文言はシェル所有）。lookup key `(scope, code)` の安定性は Rust 新設の `tests/validation_wire_format_tests.rs`（6 件。分子には数えない）が担保する
- P6 `sample_dto` の移植元はアプリ層（`HandballRecorderTests/SampleMatchConverterV2Tests.swift`）のため分母 140 の外。converter テスト 31 件を `tests/sample_match_converter_tests.rs` に移植し、serde 表現（`factID` / `externalID` の明示 rename・明示 null 耐性・RFC 3339 日時）は Rust 新設の `tests/sample_dto_serde_tests.rs`（7 件。分子に数えない）で固定する
