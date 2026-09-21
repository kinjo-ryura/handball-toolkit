// ルートビルドスクリプト。プラグイン版は toolkit モジュール側で適用する。
//
// AGP / Kotlin / KSP のバージョンは examples/android と揃える（同じ Gradle = flake の
// pkgs.gradle_9 で動かすため）。上げるときは examples/android/build.gradle.kts と
// examples/android/README.md「バージョンの対応関係」も同時に直すこと。
plugins {
    id("com.android.library") version "9.4.0" apply false
    // AGP 9 からは Kotlin のコンパイルを AGP 自身が受け持つ（built-in Kotlin。
    // handball-project#412）。モジュールでこのプラグインを apply すると AGP がエラーにする。
    // ここで apply false で宣言しているのは Kotlin Gradle Plugin の版を決めるためだけ —
    // AGP が実行時に引く KGP（AGP 9.x は 2.2.10）よりこちらが新しければ、こちらが使われる。
    // Dependabot もこの行で Kotlin の版を追う。
    id("org.jetbrains.kotlin.android") version "2.4.20" apply false
}
