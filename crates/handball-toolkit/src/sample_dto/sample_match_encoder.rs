//! Swift `SampleMatchEncoderV2`（JSONEncoder: `.iso8601` / `.prettyPrinted` / `.sortedKeys` /
//! `.withoutEscapingSlashes`）と**バイト一致**する配信 JSON の書き出し。
//!
//! バイト正は `tests/golden/export/`（オラクル fixture）。Swift 側 exporter の削除後も
//! handball-sample-matches の配信ファイル形式を変えないための互換レイヤ。
//! 日時・UUID の文字列表現は DTO 側（`swift_wire`）、本ファイルは整形のみを担う。

use std::io;

use serde::Serialize;
use serde_json::ser::Formatter;

use super::sample_match_dtos::SampleMatchDtoV2;

/// DTO を配信 JSON 文字列へ encode する（Swift `MatchExporterV2.encode` 相当）。
///
/// `.sortedKeys` は serde_json::Value（BTreeMap）経由のバイト順整列で再現する
/// （本 schema のキーは全て ASCII camelCase で、決定文字に大小混在の対が無く
/// Foundation の並びと一致する）。
pub fn encode_sample_match(dto: &SampleMatchDtoV2) -> String {
    let value = serde_json::to_value(dto).expect("SAMPLE_DTO_V2 は常に JSON 化可能");
    let mut out = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut out, SwiftJsonFormatter::default());
    value
        .serialize(&mut serializer)
        .expect("Value の再 serialize は失敗しない");
    String::from_utf8(out).expect("serde_json の出力は UTF-8")
}

/// Foundation の pretty 出力を再現する Formatter。serde_json の `PrettyFormatter` との相違:
/// `"key" : value`（コロン前後に空白）/ 空コンテナは `[\n\n  ]` 形 / 整数値 double は `.0` 省略。
/// `/` は serde_json が元々エスケープしない（`.withoutEscapingSlashes` と同挙動）。
#[derive(Default)]
struct SwiftJsonFormatter {
    indent: usize,
    has_value: bool,
}

impl SwiftJsonFormatter {
    fn write_indent<W: ?Sized + io::Write>(&self, writer: &mut W) -> io::Result<()> {
        for _ in 0..self.indent {
            writer.write_all(b"  ")?;
        }
        Ok(())
    }

    fn end_container<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        close: &[u8],
    ) -> io::Result<()> {
        self.indent -= 1;
        if self.has_value {
            writer.write_all(b"\n")?;
        } else {
            // Foundation は空コンテナを `[\n\n<indent>]` / `{\n\n<indent>}` と書く（実測）。
            writer.write_all(b"\n\n")?;
        }
        self.write_indent(writer)?;
        writer.write_all(close)
    }
}

impl Formatter for SwiftJsonFormatter {
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        // Swift は整数値 double を `.0` なしで書く（1800.0 → "1800"）。それ以外は
        // 最短往復表現（Rust の Display / Swift とも shortest round-trip で一致）。
        if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
            write!(writer, "{}", value as i64)
        } else {
            write!(writer, "{value}")
        }
    }

    fn begin_array<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.indent += 1;
        self.has_value = false;
        writer.write_all(b"[")
    }

    fn end_array<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.end_container(writer, b"]")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        writer.write_all(if first { b"\n" } else { b",\n" })?;
        self.write_indent(writer)
    }

    fn end_array_value<W>(&mut self, _writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.indent += 1;
        self.has_value = false;
        writer.write_all(b"{")
    }

    fn end_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.end_container(writer, b"}")
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        writer.write_all(if first { b"\n" } else { b",\n" })?;
        self.write_indent(writer)
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        writer.write_all(b" : ")
    }

    fn end_object_value<W>(&mut self, _writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.has_value = true;
        Ok(())
    }
}
