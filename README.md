# handball-toolkit

ハンドボール試合データのツールキット。fact スキーマ + スコア / タイムライン導出（projections）+ validation を提供する Rust crate 群。

[HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder) のドメイン層 `RecorderDomain`（Swift、Foundation のみ依存の純粋計算）の移植であり、単一の共有コアを iOS / Android / Web (wasm) / CLI へ届けるための基盤。経緯と設計判断は [handball-project#49](https://github.com/kinjo-ryura/handball-project/issues/49) と `handball-project/docs/research/handballrecorder-rust-core.md` を参照。

## 設計方針

- **stateless 純粋関数コア** — 公開 API はすべて「fact 列 in → 導出結果 out」。状態を所有しない（fact ログの永続化は各 OS ネイティブ、UI 状態はシェル側）
- **決定性** — `now()` / UUID 生成はコアに置かず、シェルが発行して fact に載せて渡す
- **エラーは構造化** — エラーコード + パラメータのみを返す。ユーザー向け文言（日本語等）は各シェルが持つ
- **パリティ検証** — [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の実試合 JSON をゴールデンコーパスに、Swift 実装をオラクルとして projection 出力の一致を検証する

## 構成

```
crates/
  handball-toolkit/       — コア crate（facts / clocks / configuration / entities / validators / projections）
                            feature `uniffi`（default off）でドメイン全型の UniFFI 公開が有効になる（ADR 0004）
  handball-toolkit-cli/   — sample-matches 配信 JSON の検証 CLI（handball-project#58）
  handball-toolkit-ffi/   — FFI パッケージング crate（staticlib 化 + uniffi-bindgen CLI）
                            型・関数の公開面はコア crate の namespace に集約する（ADR 0004）
```

将来の拡張候補（必要になってから追加）: wasm バインディング、Kotlin バインディング。

## 検証 CLI

[handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の配信 JSON（SAMPLE_DTO_V2）をコアの validators で検証する。

```bash
# v2 ルートを一括検証（index ↔ ファイル突合 + スコア / factCount / hasVideo / date の転記整合）
cargo run -p handball-toolkit-cli -- validate ../handball-sample-matches/v2

# 単体ファイル（試合本体 / index をトップレベルキーで自動判別）。--json で機械可読出力
cargo run -p handball-toolkit-cli -- validate --json path/to/match.json
```

exit code は 0 = 指摘なし / 1 = 指摘あり / 2 = 使い方・パス誤り。指摘は境界ワイヤ形式
`{scope, code, params}`（ADR 0002）で表示する。handball-sample-matches の CI への組み込みは
本リポの OSS 公開後に行う（private リポのままだと public リポの Actions から PAT なしで
参照できないため。それまでは配信前に手元で本 CLI を実行する運用）。

## 開発

開発環境は Nix flake + direnv で宣言的に管理する（rustup は不使用）。ツールチェーンは
[`rust-toolchain.toml`](./rust-toolchain.toml) でバージョン固定し、[rust-overlay](https://github.com/oxalica/rust-overlay) が提供する。

前提: Nix / direnv / Xcode Command Line Tools。リンクは Nix の clang ではなく CLT の
`/usr/bin/cc` に任せる構成（将来の iOS ターゲット（UniFFI → XCFramework）ビルドで
xcrun 系と衝突させないため。詳細は `flake.nix` のコメントを参照）。

```bash
direnv allow        # 初回のみ。以降はディレクトリに入ると自動で環境が整う
cargo test          # 全テスト
cargo clippy        # lint
cargo fmt           # フォーマット
```

direnv を使わない場合は `nix develop` で同じシェルに入れる。ツールチェーンの更新は
`rust-toolchain.toml` の `channel` を書き換える（wasm / iOS 等のクロスターゲットも
同ファイルの `targets` に足していく）。

## ステータス

private。パリティ検証は完走済みで、iOS シェル（HandballRecorder）は本コアで動いている。OSS 公開は完走時点の判断で見送り（`docs/PORTING.md` P9）。公開意思が固まった時点で README の英語化とライセンス選定（MIT / Apache-2.0 デュアル想定）とセットで再判断する。
