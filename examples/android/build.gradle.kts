// ルートビルドスクリプト。プラグイン版は app モジュール側で適用する。
//
// バージョンは Gradle（flake の pkgs.gradle_9）と互換な組み合わせで固定する。
// 上げるときは README「バージョンの対応関係」も同時に直すこと。
plugins {
    id("com.android.application") version "9.4.1" apply false
    // AGP 9 の built-in Kotlin が Kotlin をコンパイルするので、app では apply しない
    // （apply すると AGP がエラーにする）。ここは Kotlin Gradle Plugin の版を決めるためだけに
    // 置いている — 理由は android/build.gradle.kts と同じ（handball-project#412）。
    id("org.jetbrains.kotlin.android") version "2.4.20" apply false
    id("com.google.devtools.ksp") version "2.3.12" apply false
}
