#!/usr/bin/env bash
# UniFFI → XCFramework ビルド（ADR 0004 本境界。旧 #49 PoC を 3 スライスへ拡張）。
#
# 成果物（target/xcframework/ 配下。コミットしない）:
#   - HandballToolkit.xcframework — ios / ios-sim / macos 3 スライスの静的ライブラリ
#                                    + C ヘッダ/modulemap（HandballRecorderMac も同じ枠組みで使う）
#   - HandballToolkit.swift       — 生成 Swift API 層（利用側がソースとして一緒にコンパイルする）
#
# サイズ最適化はワークスペース Cargo.toml の [profile.release]（LTO / codegen-units=1 /
# panic=abort。実測は ADR 0004 実装追記）。
#
# 前提: nix develop（または direnv）環境内で実行する。リンクは Xcode CLT に任せる
# 構成のため、xcodebuild / xcrun は /usr/bin のものがそのまま使える。
set -euo pipefail
cd "$(dirname "$0")/.."

readonly LIB=libhandball_toolkit_ffi.a
readonly OUT=target/xcframework
readonly STAGING="$OUT/staging"

echo "==> 1/3 iOS 実機 / シミュレータ / macOS の staticlib をビルド"
cargo build --release -p handball-toolkit-ffi \
  --target aarch64-apple-ios --target aarch64-apple-ios-sim --target aarch64-apple-darwin

echo "==> 2/3 Swift バインディング生成（library mode: .a の uniffi メタデータから）"
# --no-format: 決定性のため。生成 Swift は HandballRecorder にコミットされ、親リポの同期 CI
# （toolkit-generated-sync.yml）が未整形で再生成してバイト比較する。整形の有無が環境で揺れると
# 全 push で偽 NG になるので、両側とも整形を無効化して揃える（handball-project#192）。
# uniffi は xcrun swift-format → swift-format → swift format → swiftformat の順に試すため、
# 「開発機に整形器が無い」に頼れない（Xcode 同梱の swift-format に xcrun で到達する）。
rm -rf "$OUT"
mkdir -p "$STAGING/headers"
cargo run -q -p handball-toolkit-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "target/aarch64-apple-ios/release/$LIB" \
  --language swift --no-format --out-dir "$STAGING/bindings"

# XCFramework のヘッダディレクトリ。modulemap は module.modulemap の名前が必須
cp "$STAGING/bindings/HandballToolkitFFI.h" "$STAGING/headers/"
cp "$STAGING/bindings/HandballToolkitFFI.modulemap" "$STAGING/headers/module.modulemap"

echo "==> 3/3 XCFramework 作成"
xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/release/$LIB" -headers "$STAGING/headers" \
  -library "target/aarch64-apple-ios-sim/release/$LIB" -headers "$STAGING/headers" \
  -library "target/aarch64-apple-darwin/release/$LIB" -headers "$STAGING/headers" \
  -output "$OUT/HandballToolkit.xcframework"
cp "$STAGING/bindings/HandballToolkit.swift" "$OUT/"

echo "完了: $OUT/HandballToolkit.xcframework + HandballToolkit.swift"
