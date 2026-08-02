import com.vanniktech.maven.publish.AndroidSingleVariantLibrary
import org.gradle.plugins.signing.Sign

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.vanniktech.maven.publish")
}

// コア crate（ワークスペース Cargo.toml の workspace.package.version）と同じ値を置く。
// ずれると「配布された .aar がどのコアなのか」が追えなくなるため、scripts/build_aar.sh が
// ビルド前に Cargo.toml と照合して不一致なら止める。
val toolkitVersion = "0.1.0"

android {
    // 生成 Kotlin の package_name（crates/handball-toolkit/uniffi.toml）と揃える。
    namespace = "io.github.kinjoryura.handballtoolkit"
    compileSdk = 36
    // nix が提供する SDK には build-tools が 1 つしか入っていないため明示する。既定値
    // （AGP のバンドル値）を要求されると read-only な nix store へダウンロードしようと
    // して失敗する（examples/android/app と同じ理由）。
    buildToolsVersion = "37.0.0"

    defaultConfig {
        // ADR 0006 実装追記（2026-07-28）で確定した値。NDK リンカの API レベルと一致する。
        minSdk = 24

        // ADR 0006 決定 5: arm64-v8a 単独。
        ndk { abiFilters += "arm64-v8a" }

        // 消費側が R8 で minify したときに JNA / 生成コードが削られないようにする。
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        // 生成 Kotlin の API 面に java.time.Instant が出る（UtcDateTime）。java.time は
        // API 26 以降の標準ライブラリなので minSdk 24 では desugaring が要る。
        // **消費側アプリでも同じ設定が必要**（ライブラリ側の有効化は消費側に伝播しない）
        // — README の Android 節に利用者向けの注意として書いてある。
        isCoreLibraryDesugaringEnabled = true
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }

    // scripts/build_aar.sh が生成する Kotlin バインディング（生成物なのでコミットしない）。
    sourceSets["main"].kotlin.srcDir("src/generated/kotlin")

    packaging {
        jniLibs {
            // .so を strip しない。panic = "abort"（ADR 0006 決定 4）ではコアの panic が
            // Kotlin 例外ではなくネイティブ abort になるため、シンボルが残っているか
            // どうかがそのまま診断可否になる。サイズより診断性を採る。
            keepDebugSymbols += "**/*.so"
        }
    }
}

dependencies {
    // ── UniFFI 生成コードの実行時依存 ──
    // どちらも api にする。消費側のコードが直接触る型ではないが、版を差し替えたい
    // ケース（別ライブラリと JNA の版が衝突する等）で消費側から見えている必要がある。
    //
    // JNA: 生成コードが Native.register（direct mapping）で .so を dlopen する。
    // Android では aar 版（各 ABI のネイティブ支援ライブラリ同梱）が要る。
    api("net.java.dev.jna:jna:5.17.0@aar")
    // 生成コードの suspend 関数・GlobalScope.launch が依存する。
    api("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")

    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}

mavenPublishing {
    // Sonatype Central Portal（2024 以降の Maven Central 受け口）。
    publishToMavenCentral()

    // 署名は Central Portal の必須要件。鍵と認証情報は ~/.gradle/gradle.properties か
    // 環境変数から読む（リポには絶対にコミットしない）— 手順は README「配布」節。
    signAllPublications()

    coordinates("io.github.kinjo-ryura", "handball-toolkit", toolkitVersion)

    // release variant 単独で publish し、sources / javadoc jar も出す
    // （どちらも Maven Central の必須要件）。
    configure(
        AndroidSingleVariantLibrary(
            variant = "release",
            sourcesJar = true,
            publishJavadocJar = true,
        ),
    )

    pom {
        name.set("handball-toolkit")
        // POM は Maven Central の検索面に出るメタデータなので英語で書く
        // （docs/ERROR_CODES.md を英語にしたのと同じ「外部実装者向け」の論理）。
        description.set(
            "Handball match data toolkit for Android: fact schema, score and timeline " +
                "projections, and validation, backed by a shared Rust core.",
        )
        url.set("https://github.com/kinjo-ryura/handball-toolkit")
        inceptionYear.set("2026")

        licenses {
            license {
                name.set("MIT License")
                url.set("https://github.com/kinjo-ryura/handball-toolkit/blob/main/LICENSE")
                distribution.set("repo")
            }
        }
        developers {
            developer {
                id.set("kinjo-ryura")
                name.set("kinjo-ryura")
                url.set("https://github.com/kinjo-ryura")
            }
        }
        scm {
            url.set("https://github.com/kinjo-ryura/handball-toolkit")
            connection.set("scm:git:git://github.com/kinjo-ryura/handball-toolkit.git")
            developerConnection.set("scm:git:ssh://git@github.com/kinjo-ryura/handball-toolkit.git")
        }
    }
}

// 署名鍵が無い環境では署名タスクをスキップする。
//
// signAllPublications() は Maven Central publish の必須要件だが、鍵が無いと
// publishToMavenLocal まで落ちてしまい、.aar の中身をローカルで検証できなくなる
// （examples/android を publish 済み成果物へ切り替えて経路を通す検証 = #135 の要）。
// 鍵を設定し忘れたまま Maven Central へ上げてしまう事故は、Central Portal が
// 署名の無い bundle を reject するので手前で止まる。
tasks.withType<Sign>().configureEach {
    onlyIf {
        providers.gradleProperty("signingInMemoryKey").isPresent ||
            providers.gradleProperty("signing.keyId").isPresent
    }
}
