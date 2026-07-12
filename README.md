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
  handball-toolkit/   — コア crate（facts / clocks / configuration / entities / validators / projections）
```

将来の拡張候補（必要になってから追加）: CLI（JSON 検証器）、wasm バインディング、UniFFI バインディング（Swift / Kotlin）。

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

開発初期（private）。OSS 公開はパリティ検証の完走後に判断し、その際に README の英語化とライセンス選定（MIT / Apache-2.0 デュアル想定）を行う。
