#!/usr/bin/env bash
# UniFFI PoC をビルドして iOS シミュレータ内で実行する（handball-project#49）。
#
#   scripts/ios_poc/run.sh [sample-match.json]
#
# 事前に scripts/build_xcframework.sh を実行しておくこと（未実行なら自動で呼ぶ）。
# 引数省略時はコアのゴールデンコーパス 1 件目を使う。
set -euo pipefail
cd "$(dirname "$0")/../.."

readonly XC_DIR=target/xcframework
readonly SLICE="$XC_DIR/HandballToolkit.xcframework/ios-arm64-simulator"
readonly OUT=target/ios-poc
SAMPLE="${1:-crates/handball-toolkit/tests/golden/inputs/matches/2025-12-20-f352ea46.json}"
SAMPLE="$(cd "$(dirname "$SAMPLE")" && pwd)/$(basename "$SAMPLE")"  # simctl spawn に絶対パスで渡す

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
xcrun simctl spawn "$UDID" "$OUT/ios-poc" "$SAMPLE"
