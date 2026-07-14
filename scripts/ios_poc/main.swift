// UniFFI PoC の検証ハーネス（handball-project#49）。
// XCFramework + 生成 Swift バインディング経由で Rust コアを iOS から呼ぶ。
// ビルド・実行は同ディレクトリの run.sh。
import Foundation

guard CommandLine.arguments.count > 1 else {
    print("usage: ios-poc <sample-match.json>")
    exit(64)
}

// 1) 疎通確認の最小関数
print("== toolkitVersion() ==")
print(toolkitVersion())

// 2) 実試合 JSON → SummaryProjection（Rust コアの実パイプライン）
print("\n== summarizeSampleMatch(実試合 JSON) ==")
let json = try String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
print(try summarizeSampleMatch(sampleJson: json))

// 3) エラー経路: Rust の Result::Err が Swift の throws に写ることの確認
print("\n== summarizeSampleMatch(壊れた JSON) ==")
do {
    _ = try summarizeSampleMatch(sampleJson: "{")
    print("NG: エラーになるべき入力が成功した")
    exit(1)
} catch let error as ToolkitError {
    print("OK: ToolkitError を捕捉 -> \(error)")
}

print("\nPoC 完了: Rust コア → UniFFI → XCFramework → iOS の経路を確認")
