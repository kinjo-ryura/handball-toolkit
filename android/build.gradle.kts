// ルートビルドスクリプト。プラグイン版は toolkit モジュール側で適用する。
//
// AGP / Kotlin / KSP のバージョンは examples/android と揃える（同じ Gradle = flake の
// pkgs.gradle_9 で動かすため）。上げるときは examples/android/build.gradle.kts も同時に直すこと
// （examples/android/README.md には版を書かないので直さなくてよい。handball-project#783）。
plugins {
    id("com.android.library") version "9.5.1" apply false
    // AGP 9 からは Kotlin のコンパイルを AGP 自身が受け持つ（built-in Kotlin。
    // handball-project#412）。モジュールでこのプラグインを apply すると AGP がエラーにする。
    // ここで apply false で宣言しているのは Kotlin Gradle Plugin の版を決めるためだけ —
    // AGP が実行時に引く KGP（AGP 9.x は 2.2.10）よりこちらが新しければ、こちらが使われる。
    // Dependabot もこの行で Kotlin の版を追う。
    id("org.jetbrains.kotlin.android") version "2.4.21" apply false
}
