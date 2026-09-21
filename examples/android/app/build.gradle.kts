// Kotlin は AGP の built-in Kotlin がコンパイルする（org.jetbrains.kotlin.android は
// apply しない。版はルートの build.gradle.kts が決める — handball-project#412）。
plugins {
    id("com.android.application")
    // Room の DAO 実装生成（kapt ではなく KSP。kapt は built-in Kotlin と併用できない）。
    id("com.google.devtools.ksp")
}

android {
    namespace = "com.example.handballshell"
    compileSdk = 36
    // nix が提供する SDK には build-tools が 1 つしか入っていないため明示する。
    // 既定値（AGP のバンドル値）を要求されると read-only な nix store の SDK へ
    // ダウンロードしようとして失敗する。
    buildToolsVersion = "37.0.0"

    defaultConfig {
        applicationId = "com.example.handballshell"
        // ADR 0006 決定 2 の暫定値をそのまま確定値として採用する（NDK リンカの
        // API レベルと一致させる）。java.time が API 26 未満で使えない点は
        // coreLibraryDesugaring で解消する（下記）。
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"

        // ADR 0006 決定 5: arm64-v8a 単独。
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // 実測用に release も端末へ入れたいので debug 鍵で署名する（配布物ではない）。
            // **debug ビルドで性能を測らないこと**: debuggable なプロセスでは ART が
            // -Xcheck:jni を有効化し、JNI 往復ごとに検査が入って桁が変わる（README 参照）。
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        // 生成 Kotlin の API 面に java.time.Instant が出る（UtcDateTime = java.time.Instant）。
        // java.time は API 26 以降の標準ライブラリなので、minSdk 24 では desugaring が要る。
        // 外部シェル実装者向けの注意点として README にも記載している。
        isCoreLibraryDesugaringEnabled = true
    }
    // Kotlin の jvmTarget は書かない。built-in Kotlin では上の targetCompatibility（17）が
    // そのまま既定値になる（handball-project#412 で `kotlin { compilerOptions }` を外した）。

    packaging {
        jniLibs {
            // .aar が同梱してくる .so を strip しない。panic = "abort"（ADR 0006 決定 4）
            // ではコアの panic が Kotlin 例外ではなくネイティブ abort になるため、
            // シンボルが残っているかどうかがそのまま診断可否になる。
            keepDebugSymbols += "**/*.so"
        }
    }
}

dependencies {
    // ── コア ──
    // 配布された .aar を app/libs/ に置いて参照する。**外部利用者とまったく同じ経路**で、
    // Rust / Nix / NDK は要らない（handball-project#135）。入手方法は 2 通り:
    //   - 外部利用者と同じ: GitHub Release から .aar をダウンロードして libs/ へ置く
    //   - 手元でコアを直したとき: ./scripts/build_aar.sh の出力を libs/ へコピー
    // どちらも手順は examples/android/README.md「ビルドと実行」。
    //
    // 版はコア crate の version（android/toolkit/build.gradle.kts の toolkitVersion）と揃える。
    // CI はいまのソースから組んだ .aar を handball-toolkit-<toolkitVersion>.aar の名前で置いて
    // このサンプルをビルドするので、版を上げてここを直し忘れると CI が落ちる
    // （CI がビルドしていなかった間、ここは 0.2.0 のまま 0.11.0 までずれていた — handball-project#412）。
    implementation(files("libs/handball-toolkit-0.11.0.aar"))

    // .aar ファイル単体は依存情報を運ばない（運ぶのは Maven の POM で、ローカルファイル
    // 参照では POM が介在しない）。そのため利用側がこの 2 つを自分で宣言する必要がある。
    // 生成コードが Native.register で .so を dlopen するのに JNA、suspend 関数に coroutines。
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")

    // ── 永続化（シェルの責務。コアは DB を所有しない）──
    implementation("androidx.room:room-runtime:2.8.5")
    implementation("androidx.room:room-ktx:2.8.5")
    ksp("androidx.room:room-compiler:2.8.5")

    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")

    // ── 単体テスト（JVM 上で回る。testImplementation は APK に入らない）──
    // `factLog` の ORDER BY をコアと共有する fixture と突き合わせる（FactPersistenceOrderTest。
    // handball-project#405）。JVM には Android の SQLite が無いので sqlite-jdbc で同じ SQL を実行する。
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.xerial:sqlite-jdbc:3.53.4.0")
}
