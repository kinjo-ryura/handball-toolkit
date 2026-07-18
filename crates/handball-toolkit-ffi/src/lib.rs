//! FFI パッケージング crate。
//!
//! uniffi の型公開・関数公開はすべてコア crate（feature `uniffi`）の namespace に
//! 集約されており（ADR 0004）、この crate は次の 2 つだけを担う:
//!
//! - **staticlib 化**: コアの scaffolding を含む `libhandball_toolkit_ffi.a` を作る
//!   （XCFramework の中身。scripts/build_xcframework.sh）
//! - **bindgen CLI**: feature `bindgen` で uniffi-bindgen を同居させる
//!
//! PoC の粗い境界「SAMPLE_DTO_V2 JSON in → summary JSON out」は本境界への移行に伴い
//! 廃止した（ADR 0004 決定 1）。

// staticlib にコアの uniffi scaffolding（メタデータ・extern "C" 関数）を含めるための再エクスポート。
pub use handball_toolkit::ffi_api;
