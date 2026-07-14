//! uniffi-bindgen CLI（`cargo run -p handball-toolkit-ffi --features bindgen --bin uniffi-bindgen`）。
//! ビルド済みライブラリからバインディングを生成する library mode で使う。

fn main() {
    uniffi::uniffi_bindgen_main()
}
