plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    // Room の DAO 実装生成（kapt ではなく KSP）。
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

    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }

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
    // publish 済みの .aar を、外部利用者とまったく同じ経路で引く（handball-project#135）。
    // Rust / Nix / NDK は要らない。中身は「生成 Kotlin + arm64-v8a の .so + 依存宣言」で、
    // JNA と kotlinx-coroutines は .aar が api 依存として連れてくるのでここには書かない。
    //
    // ローカルで検証するときは、コアを直したあとに
    //   ./scripts/build_aar.sh && gradle -p android :toolkit:publishToMavenLocal
    // を回して ~/.m2 を更新する（settings.gradle.kts の mavenLocal() が拾う）。
    implementation("io.github.kinjo-ryura:handball-toolkit:0.1.0")

    // ── 永続化（シェルの責務。コアは DB を所有しない）──
    implementation("androidx.room:room-runtime:2.7.2")
    implementation("androidx.room:room-ktx:2.7.2")
    ksp("androidx.room:room-compiler:2.7.2")

    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}
