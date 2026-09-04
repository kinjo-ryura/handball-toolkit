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

# ビルド機の絶対パスを成果物に埋めない（handball-project#285）。
#
# panic の位置情報として依存クレートのソースパスが .wasm に残るため、素直にビルドすると
# 配信物から開発機の macOS ユーザー名が読める（2026-09-02 のセキュリティ監査で検出）。
# この成果物は handball-apps-site へコミットして公開配信するので、ビルド時点で畳んでおく。
#
# 2 つの前置きを両方畳む。rustc が複数の指定をどの順に当てるかに関わらず、
# CARGO_HOME が HOME 配下にある既定構成ではどちらが勝ってもユーザー名は消える。
readonly CARGO_HOME_DIR="${CARGO_HOME:-${HOME}/.cargo}"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${CARGO_HOME_DIR}=/cargo --remap-path-prefix=${HOME}=~"

echo "==> 1/3 wasm ターゲットでビルド"
cargo build --release -p "$CRATE" --target wasm32-unknown-unknown

echo "==> 2/3 JS グルー生成（web ターゲット）"
rm -rf "$OUT"
wasm-bindgen "$WASM" --target web --out-dir "$OUT"

# 検査を手順書ではなくスクリプトに置く（漏れたまま公開されるのを防ぐため）。
echo "==> 3/3 埋め込みパスの検査"
if [[ -z "${HOME:-}" || "$HOME" == "/" ]]; then
  echo "エラー: HOME が空 / ルートで、パスの畳み込みを検査できない" >&2
  exit 1
fi
if LC_ALL=C grep -a -l -e "$HOME" -e "$CARGO_HOME_DIR" "$OUT"/*; then
  echo "エラー: 上の成果物にビルド機の絶対パスが残っている（RUSTFLAGS の畳み込みが効いていない）" >&2
  exit 1
fi
echo "OK: ビルド機の絶対パスは埋まっていない"

echo "==> 完了: $OUT"
ls -lh "$OUT"
