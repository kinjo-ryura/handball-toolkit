#!/usr/bin/env bash
# wasm バインディングのビルド（handball-project#57）。
#
# 成果物（target/wasm/ 配下。コミットしない）:
#   - handball_toolkit_wasm.js       — ES module の JS グルー（`import init, { ... } from ...`）
#   - handball_toolkit_wasm_bg.wasm  — wasm 本体
#   - handball_toolkit_wasm.d.ts     — 型定義
#
# サイズ最適化はワークスペース Cargo.toml の [profile.release]（LTO / codegen-units=1 /
# panic=abort）。wasm-opt は通していない（必要になったら binaryen を flake に足す）。
#
# 前提: nix develop（または direnv）環境内で実行する。wasm-bindgen-cli は flake が入れる。
set -euo pipefail
cd "$(dirname "$0")/.."

readonly CRATE=handball-toolkit-wasm
readonly WASM=target/wasm32-unknown-unknown/release/handball_toolkit_wasm.wasm
readonly OUT=target/wasm

echo "==> 1/2 wasm ターゲットでビルド"
cargo build --release -p "$CRATE" --target wasm32-unknown-unknown

echo "==> 2/2 JS グルー生成（web ターゲット）"
rm -rf "$OUT"
wasm-bindgen "$WASM" --target web --out-dir "$OUT"

echo "==> 完了: $OUT"
ls -lh "$OUT"
