#!/usr/bin/env bash
# UniFFI → XCFramework ビルド（handball-project#49 UniFFI PoC）。
#
# 成果物（target/xcframework/ 配下。コミットしない）:
#   - HandballToolkit.xcframework — 実機 + シミュレータの静的ライブラリ + C ヘッダ/modulemap
#   - HandballToolkit.swift       — 生成 Swift API 層（利用側がソースとして一緒にコンパイルする）
#
# 前提: nix develop（または direnv）環境内で実行する。リンクは Xcode CLT に任せる
# 構成のため、xcodebuild / xcrun は /usr/bin のものがそのまま使える。
set -euo pipefail
cd "$(dirname "$0")/.."

readonly LIB=libhandball_toolkit_ffi.a
readonly OUT=target/xcframework
readonly STAGING="$OUT/staging"

echo "==> 1/3 iOS 実機 + シミュレータの staticlib をビルド"
cargo build --release -p handball-toolkit-ffi \
  --target aarch64-apple-ios --target aarch64-apple-ios-sim

echo "==> 2/3 Swift バインディング生成（library mode: .a の uniffi メタデータから）"
rm -rf "$OUT"
mkdir -p "$STAGING/headers"
cargo run -q -p handball-toolkit-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "target/aarch64-apple-ios/release/$LIB" \
  --language swift --out-dir "$STAGING/bindings"

# XCFramework のヘッダディレクトリ。modulemap は module.modulemap の名前が必須
cp "$STAGING/bindings/HandballToolkitFFI.h" "$STAGING/headers/"
cp "$STAGING/bindings/HandballToolkitFFI.modulemap" "$STAGING/headers/module.modulemap"

echo "==> 3/3 XCFramework 作成"
xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/release/$LIB" -headers "$STAGING/headers" \
  -library "target/aarch64-apple-ios-sim/release/$LIB" -headers "$STAGING/headers" \
  -output "$OUT/HandballToolkit.xcframework"
cp "$STAGING/bindings/HandballToolkit.swift" "$OUT/"

echo "完了: $OUT/HandballToolkit.xcframework + HandballToolkit.swift"
