// ルートビルドスクリプト。プラグイン版は app モジュール側で適用する。
//
// バージョンは Gradle（flake の pkgs.gradle_9）と互換な組み合わせで固定する。
// **版はこのファイルと app/build.gradle.kts だけが持つ。** README にも、このファイルのコメントにも
// 版の数字を写さないこと（Dependabot は宣言の行しか上げないので、写した版は古いまま残る。
// CI の scripts/check_readme_versions.py が落とす。handball-project#783）。
plugins {
    id("com.android.application") version "9.5.1" apply false
    // AGP 9 の built-in Kotlin が Kotlin をコンパイルするので、app では apply しない
    // （apply すると AGP がエラーにする）。ここは Kotlin Gradle Plugin の版を決めるためだけに
    // 置いている — 理由は android/build.gradle.kts と同じ（handball-project#412）。
    id("org.jetbrains.kotlin.android") version "2.4.21" apply false
    id("com.google.devtools.ksp") version "2.3.13" apply false
}
