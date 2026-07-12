# 移植作業ガイド — RecorderDomain → Rust

セッションをまたいで移植を進めるための「現在地と次の一手」のファイル。**各セッションの冒頭でこれを読み、進捗があったらチェックを更新する**。設計判断の正典は `docs/adr/`（このファイルには理由を書かない）。

- 背景・経緯: handball-project#49 / `handball-project/docs/research/handballrecorder-rust-core.md`
- 設計: [ADR 0001](adr/0001-boundary-api.md)（境界 API 目録）/ [ADR 0002](adr/0002-error-model.md)（エラー体系）/ [ADR 0003](adr/0003-parity-verification.md)（パリティ検証）— すべて accepted（2026-07-12）
- 移植元（真実の仕様）: `../HandballRecorder/Packages/RecorderDomain/`（sibling submodule）。挙動に迷ったら Swift 実装とそのテストを読む。**「改善」しない**

## 作業規律

- **忠実移植**: Swift ファイルと Rust ファイルを 1:1 対応させ（ADR 0001 のミラー表）、演算順も保存する。改善したくなったら Issue 化して完走後に回す
- **テストも同時移植**: モジュールを写したら、対応する Swift Testing のテストを同じセッションで移植する（後回しにしない）。`#expect` の即値はそのまま写す
- **ブランチ運用（このリポ）**: フェーズ単位の作業ブランチ（例: `port/p2-types`）で進め、フェーズ完了（`cargo test` green + このファイルのチェック更新）時に main へ merge する。main は常に green を保つ。push 済みコミットの rebase / squash はしない
- **親リポの pointer bump は main への merge 時のみ**（作業ブランチの中間状態を親リポが指さない）
- **コミット粒度**: 1 モジュール（実装 + テスト）= 1 コミット目安。メッセージ規約は親リポ commit skill（日本語 Conventional Commits 風）
- **push 順**: handball-toolkit → 親リポ。逆にすると親リポがリモートに存在しない commit を指す（2026-07-12 に実際に発生）
- **再検討トリガー**: OSS 公開で外部コントリビュータが現れたら PR + CI（`cargo test` ゲート）へ切替

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
- [ ] P6 `sample_dto` モジュール（SAMPLE_DTO_V2 準拠の serde 型 + converter。依存は domain への一方通行厳守 — ADR 0003）
- [ ] P7 オラクル dump ツール（HandballRecorder の `feat/rust-domain-core` ブランチ。main へ merge しない — 上記「オラクル側の運用」）+ `tests/golden/` 整備 — ADR 0003
- [ ] P8 パリティ検証完走（公開 8 件 + ローカル `.timer` × 5 系統 bit-exact 一致、移植テスト 144 件 green。完走判定の定義は ADR 0003）
- [ ] P9 完走後: OSS 公開判断（README 英語化・ライセンス選定）/ ID の newtype 化（handball-project#52）/ `.timer` 公開サンプル追加（handball-project#53）/ 境界拡張候補（ADR 0001「将来の境界拡張候補」）

P2〜P5 の順序は Swift のモジュール依存 DAG（Identifiers → Clock → Configuration → Facts / Entities → Validation → Projection → Validators）に従う。P3（エラー型）を要求するのは P5 だけなので、P3 と P4 は入れ替えてもよい。

## テスト移植のメモ

- 移植元テスト: `../HandballRecorder/Packages/RecorderDomain/Tests/RecorderDomainTests/`（Swift Testing 144 テスト / 約 2,507 行）
- フィクスチャヘルパーは Rust 側 `tests` 内の `mod fixtures` に集約する
- 移植したテスト数はパリティ完走判定（P8）の分子になるので、モジュール完了時にここへ累計を記録する: **現在 140 / 140（完了）**
  （RecorderDomainTests 8 / SegmentResolverAdvanced 21 / TimelineProjection 12 / SummaryProjection 15 / ScoreProgressionProjection 7 / LiveMatchProjection 10 / MatchValidator 5 / ConfigurationValidator 10 / FactValidator 25 / RosterReferenceValidation 4 / FactLogValidator 23）
- 分母の補正（144 → 140）: `DomainValidationMessageTests.swift`（4 件）は文言レイヤのテストで移植対象外（ADR 0002 — 文言はシェル所有）。lookup key `(scope, code)` の安定性は Rust 新設の `tests/validation_wire_format_tests.rs`（6 件。分子には数えない）が担保する
