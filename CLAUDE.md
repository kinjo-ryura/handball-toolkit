# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## リポジトリ概要

ハンドボール試合データのツールキット（Rust workspace）。[HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder) のドメイン層 `RecorderDomain`（Swift・Foundation のみ依存の純粋計算 約 2,700 行）の移植であり、単一の共有コアを iOS / Android / Web (wasm) / CLI へ届けるための基盤。[handball-project](https://github.com/kinjo-ryura/handball-project) の submodule（`apps/handball-toolkit/`）として管理される。

- 経緯・設計判断の一次資料: [handball-project#49](https://github.com/kinjo-ryura/handball-project/issues/49) と `handball-project/docs/research/handballrecorder-rust-core.md`
- 設計の正典: `docs/adr/`（0001 境界 API / 0002 エラー体系 / 0003 パリティ検証。accepted 2026-07-12）
- **移植作業の進め方・現在地・作業規律: [`docs/PORTING.md`](docs/PORTING.md)。移植作業のセッションは必ず冒頭でこれを読み、進捗があればチェックを更新すること**
- ドキュメント・コードコメントは日本語で書く（OSS 公開時の英語化は公開判断とセットで行う）

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

Cargo workspace。2 crate 構成:

- `crates/handball-toolkit/` — コア crate（facts / clocks / configuration / entities / validators / projections）。feature `uniffi`（default off）でドメイン全型の UniFFI derive と FFI 関数公開（`ffi_api` / `ffi_support`）が有効になる（ADR 0004。feature off の wasm / CLI ビルドでは uniffi が依存グラフごと消える）
- `crates/handball-toolkit-ffi/` — FFI パッケージング crate。staticlib 化（XCFramework の中身）と uniffi-bindgen CLI（feature `bindgen`）のみを担い、型・関数の公開面はコア crate の namespace に集約する（ADR 0004 決定 3 実装追記）

CLI（JSON 検証器）・wasm バインディング・Kotlin バインディングは将来の拡張候補で、必要になった時点で workspace member として追加する（先回りで作らない）。iOS シェル向けのドメイン全型 UniFFI 公開（本境界）は ADR 0004 で確定・実装済み。Kotlin バインディングは handball-project#59。

### iOS 向け XCFramework（UniFFI）

```bash
./scripts/build_xcframework.sh   # target/xcframework/ に HandballToolkit.xcframework + 生成 Swift API 層
./scripts/ios_poc/run.sh         # 本境界 smoke をビルドして iOS シミュレータ内で実行
```

XCFramework は ios / ios-sim / macos の 3 スライス構成（HandballRecorderMac も同じ枠組み）。サイズ最適化はワークスペース Cargo.toml の `[profile.release]`（LTO / codegen-units=1 / panic=abort。実測と代償は ADR 0004 実装追記）。生成 Swift（`HandballToolkit.swift`）は XCFramework に入らない。「バイナリ + C モジュール」が XCFramework、Swift API 層はソースとして利用側が一緒にコンパイルする 2 段構え（UniFFI の標準配布形）。

### 設計不変条件（コアに入れてよいもの / いけないもの）

1. **stateless 純粋関数コア** — 公開 API はすべて「fact 列 in → 導出結果 out」。状態を所有しない。fact ログの永続化は各 OS ネイティブ、UI 状態はシェル側。コアに状態を持たせる拡張（Store ロジックの昇格など）は移植完了後に別途判断であり、このリポの現フェーズではやらない
2. **決定性** — `now()` / UUID 生成をコアに置かない。timestamp / ID はシェルが発行して fact に載せて渡す（ゴールデンテストの安定と wasm 対応のため）
3. **エラーは構造化** — エラーコード + パラメータのみを返す。日本語等のユーザー向け文言をコアに焼き込まない（文言は各シェルが持つ）。移植元の `DomainValidationMessage` をそのまま写さないこと — ここは意図的な再設計ポイント
4. **境界は粗い粒度** — FFI / JNI / wasm 越えを前提に、細かい getter の応酬ではなく「fact 列 in → projection out」の同期バッチ形状を保つ

### 移植のオラクル（Swift 実装）とパリティ検証

移植元の Swift 実装が真実の仕様。セマンティクスに迷ったら移植元とそのテスト（約 2,500 行）を読む:

- Swift 実装: `HandballRecorder/Packages/RecorderDomain/Sources/RecorderDomain/`（Clock / Configuration / Entities / Facts / Projection / Validation / Validators）。handball-project の checkout 内では sibling submodule `../HandballRecorder/`
- 型仕様・validation ルール: 同リポの `docs/redesign/DOMAIN_TYPES_V1.md` / `DOMAIN_VALIDATION_RULES.md`、ドメイン語彙は `CONTEXT.md`
- パリティ検証: [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の実試合 JSON をゴールデンコーパスに、Swift 実装をオラクルとして projection 出力の一致を検証する。特に `SegmentResolver` と validation R3–R9 は移植で最も繊細な部分 — 挙動を「改善」せず一致させる
