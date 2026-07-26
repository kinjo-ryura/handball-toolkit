# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## リポジトリ概要

ハンドボール試合データのツールキット（Rust workspace）。[HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder) のドメイン層 `RecorderDomain`（Swift・Foundation のみ依存の純粋計算 約 2,700 行）の移植であり、単一の共有コアを iOS / Android / Web (wasm) / CLI へ届けるための基盤。[handball-project](https://github.com/kinjo-ryura/handball-project) の submodule（`apps/handball-toolkit/`）として管理される。

- 経緯・設計判断の一次資料: [handball-project#49](https://github.com/kinjo-ryura/handball-project/issues/49) と `handball-project/docs/research/handballrecorder-rust-core.md`
- 設計の正典: `docs/adr/`（0001 境界 API / 0002 エラー体系 / 0003 パリティ検証 — accepted 2026-07-12。0004 iOS FFI 本境界 / 0005 write orchestration — accepted 2026-07-18）。**各 ADR の「実装追記」が実装の現況を持つ**
- 境界のエラーコード一覧は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md)（外部シェル実装者向けの英語ドキュメント）。**エラー case を追加・改名したらこの表も更新する**（code は安定契約 — ADR 0002 決定 2）
- 移植の経緯・作業規律: [`docs/PORTING.md`](docs/PORTING.md)。**移植は完走済みで、同ファイルは完了記録**（現在地の管理台帳ではない）。進行中・未着手の作業は GitHub Issues が正
- ドキュメント・コードコメントは日本語で書く。**例外は外部の利用者が最初に読む 2 本のみ**（handball-project#134）: `README.md` と `docs/ERROR_CODES.md` は英語で書き、更新時も英語を保つ。ADR・`docs/PORTING.md`・コードコメントは日本語のまま（設計根拠は自分の思考資産であり、翻訳コストが継続的に効くため）

## 開発コマンド

開発環境は Nix flake + direnv で宣言的に管理する（rustup 不使用）。ツールチェーンは `rust-toolchain.toml` でバージョン固定し、rust-overlay が提供する。

```bash
direnv allow          # 初回のみ。以降はディレクトリに入ると自動で環境が整う
                      # direnv を使わない場合は nix develop

cargo test            # 全テスト
cargo test <部分一致>  # 単一テスト（テスト関数名の部分一致で絞り込み）
cargo clippy          # lint
cargo fmt             # フォーマット
```

- ツールチェーン更新は `rust-toolchain.toml` の `channel` を書き換えて `direnv reload`。wasm / iOS などのクロスターゲットも同ファイルの `targets` に足す
- **flake.nix に Nix の clang / apple-sdk を入れないこと**: リンクは意図的に Xcode CLT の `/usr/bin/cc` に任せている（将来の iOS ターゲット / UniFFI → XCFramework ビルドで xcrun 系と衝突させないため）。rust-overlay の propagation を空にしている `overrideAttrs` を外さない（詳細は flake.nix のコメント）

## アーキテクチャ

Cargo workspace。4 crate 構成:

- `crates/handball-toolkit/` — コア crate（facts / clocks / configuration / entities / validators / projections）。feature `uniffi`（default off）でドメイン全型の UniFFI derive と FFI 関数公開（`ffi_api` / `ffi_support`）が有効になる（ADR 0004。feature off の wasm / CLI ビルドでは uniffi が依存グラフごと消える）
- `crates/handball-toolkit-cli/` — sample-matches 配信 JSON（SAMPLE_DTO_V2）の検証 CLI（handball-project#58）。コアの validators を呼ぶだけの薄いシェルで、コアには手を入れない。使い方は README「検証 CLI」
- `crates/handball-toolkit-ffi/` — FFI パッケージング crate。staticlib 化（XCFramework の中身）と uniffi-bindgen CLI（feature `bindgen`）のみを担い、型・関数の公開面はコア crate の namespace に集約する（ADR 0004 決定 3 実装追記）
- `crates/handball-toolkit-wasm/` — wasm パッケージング crate（handball-project#57）。JS 向けの粗粒度エントリ（`toolkitVersion` / `requiredIdCount` / `buildMatchView`）とマーシャリングだけを担い、コアには触れない。**ID 生成はシェル（JS の `crypto.randomUUID()`）が行う** — コアは UUID を生成しない（設計不変条件 2）ので、この crate も乱数を引かず `getrandom` の wasm バックエンド設定が不要

Kotlin バインディングは将来の拡張候補で、必要になった時点で workspace member として追加する（先回りで作らない。handball-project#59）。iOS シェル向けのドメイン全型 UniFFI 公開（本境界）は ADR 0004 で確定・実装済み。

### iOS 向け XCFramework（UniFFI）

```bash
./scripts/build_xcframework.sh   # target/xcframework/ に HandballToolkit.xcframework + 生成 Swift API 層
./scripts/ios_poc/run.sh         # 本境界 smoke をビルドして iOS シミュレータ内で実行
```

### Web 向け wasm（wasm-bindgen）

```bash
./scripts/build_wasm.sh   # target/wasm/ に .wasm + ES module の JS グルー + .d.ts
```

`wasm-bindgen` crate と `wasm-bindgen-cli`（flake が nixpkgs から入れる）は**バージョン完全一致**が必要。Cargo.toml 側は `=` でピン留めしてあるので、nixpkgs が上がったら両方を同時に合わせる（不一致は生成時の schema version mismatch で落ちる）。

XCFramework は ios / ios-sim / macos の 3 スライス構成（HandballRecorderMac も同じ枠組み）。サイズ最適化はワークスペース Cargo.toml の `[profile.release]`（LTO / codegen-units=1 / panic=abort。実測と代償は ADR 0004 実装追記）。生成 Swift（`HandballToolkit.swift`）は XCFramework に入らない。「バイナリ + C モジュール」が XCFramework、Swift API 層はソースとして利用側が一緒にコンパイルする 2 段構え（UniFFI の標準配布形）。

### 設計不変条件（コアに入れてよいもの / いけないもの）

1. **状態を所有しない stateless コア** — コアは DB ハンドル・保存実体・UI 状態を所有しない。判断・計画（何をどの順に保存すべきか）は「fact 列 in → 導出結果 out」の純粋関数として置く。ただし**永続化の発火 orchestration**（注入された repository を await する薄い export 関数）は feature `uniffi` 配下の境界層として持てる（ADR 0005）。repository を保持する long-lived object は作らない
2. **決定性** — `now()` / UUID 生成をコアに置かない。timestamp / ID はシェルが発行して fact に載せて渡す（ゴールデンテストの安定と wasm 対応のため）
3. **エラーは構造化** — エラーコード + パラメータのみを返す。日本語等のユーザー向け文言をコアに焼き込まない（文言は各シェルが持つ）。移植元の `DomainValidationMessage` をそのまま写さないこと — ここは意図的な再設計ポイント
4. **境界は粗い粒度** — FFI / JNI / wasm 越えを前提に、細かい getter の応酬ではなく「fact 列 in → projection out」の同期バッチ形状を保つ

### 移植のオラクル（Swift 実装）とパリティ検証

移植元の Swift 実装が真実の仕様。セマンティクスに迷ったら移植元とそのテスト（約 2,500 行）を読む:

- Swift 実装: `HandballRecorder/Packages/RecorderDomain/Sources/RecorderDomain/`（Clock / Configuration / Entities / Facts / Projection / Validation / Validators）。handball-project の checkout 内では sibling submodule `../HandballRecorder/`
- 型仕様・validation ルール: 同リポの `docs/redesign/DOMAIN_TYPES_V1.md` / `DOMAIN_VALIDATION_RULES.md`、ドメイン語彙は `CONTEXT.md`
- パリティ検証: [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の実試合 JSON をゴールデンコーパスに、Swift 実装をオラクルとして projection 出力の一致を検証する。特に `SegmentResolver` と validation R3–R9 は移植で最も繊細な部分 — 挙動を「改善」せず一致させる
