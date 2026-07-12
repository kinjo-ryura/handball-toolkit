//! ハンドボール試合データのツールキット。
//!
//! HandballRecorder の `RecorderDomain`（Swift）を移植した stateless 純粋関数コア。
//! 公開 API はすべて「fact 列 in → 導出結果 out」の純粋関数で、
//! 時間・乱数・I/O・永続化は持たない（timestamp / ID はシェルが発行して fact に載せて渡す）。
//!
//! 設計方針の背景: handball-project#49 と
//! `handball-project/docs/research/handballrecorder-rust-core.md` を参照。

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(1 + 1, 2);
    }
}
