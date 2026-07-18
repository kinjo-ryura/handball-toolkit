#!/usr/bin/env bash
# FFI 本境界 smoke をビルドして iOS シミュレータ内で実行する（ADR 0004）。
#
#   scripts/ios_poc/run.sh
#
# 事前に scripts/build_xcframework.sh を実行しておくこと（未実行なら自動で呼ぶ）。
set -euo pipefail
cd "$(dirname "$0")/../.."

readonly XC_DIR=target/xcframework
readonly SLICE="$XC_DIR/HandballToolkit.xcframework/ios-arm64-simulator"
readonly OUT=target/ios-poc

[ -d "$SLICE" ] || ./scripts/build_xcframework.sh

echo "==> 1/3 シミュレータ向け Swift 実行ファイルをビルド"
mkdir -p "$OUT"
xcrun -sdk iphonesimulator swiftc \
  -target arm64-apple-ios16.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" -lhandball_toolkit_ffi \
  "$XC_DIR/HandballToolkit.swift" scripts/ios_poc/main.swift \
  -o "$OUT/ios-poc"

echo "==> 2/3 シミュレータを起動（既に booted ならそのまま）"
UDID=$(xcrun simctl list devices available --json | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
booted = [d for ds in devices.values() for d in ds if d["state"] == "Booted"]
iphones = [d for ds in devices.values() for d in ds if "iPhone" in d["name"]]
print((booted or iphones)[0]["udid"])
')
xcrun simctl bootstatus "$UDID" -b > /dev/null

echo "==> 3/3 シミュレータ内で実行"
xcrun simctl spawn "$UDID" "$OUT/ios-poc"
