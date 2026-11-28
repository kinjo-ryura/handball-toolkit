//! 日時（handball-project#838）。
//!
//! コアの日時は [`Timestamp`]（jiff。UTC の瞬間・ナノ秒精度・西暦 -9999〜9999 年）。jiff は
//! `std` を外して取り込むので、`Timestamp::now()` はコンパイルの時点で存在しない（設計不変条件 2
//! 「コアは now() を持たない」）。**cli / wasm / ffi も jiff を直接の依存にせず、ここの再公開を
//! 使う** — どれか 1 つが jiff の `std` を足すと、feature の統合でコアにも now() が生える。
//!
//! 書き出しの形は chrono を使っていた頃のものを保つ（`tests/timestamp_wire_tests.rs` が固定する）。
//! jiff の既定の serde は小数秒の末尾の 0 を削る（`.1234`）が、chrono は 3 / 6 / 9 桁で書いていた
//! （`.123400`）。同期で運ぶ形（ADR 0007）と wasm へ渡す JSON がこの形を持つ。

use jiff::tz::Offset;

pub use jiff::Timestamp;

/// RFC 3339 の UTC 表記（`Z`）。小数秒は 0 / 3 / 6 / 9 桁のうち値を失わない最短
/// （chrono の `DateTime<Utc>` の serde と同じ形）。
pub fn to_rfc3339(at: Timestamp) -> String {
    let nanos = Offset::UTC.to_datetime(at).subsec_nanosecond();
    let fraction = if nanos == 0 {
        String::new()
    } else if nanos % 1_000_000 == 0 {
        format!(".{:03}", nanos / 1_000_000)
    } else if nanos % 1_000 == 0 {
        format!(".{:06}", nanos / 1_000)
    } else {
        format!(".{nanos:09}")
    };
    format!("{}{fraction}Z", date_time_to_seconds(at))
}

/// 秒までの UTC 表記（`Z`）。秒未満は切り捨てる（過去へ寄せる。Swift `.iso8601` と同じ）。
pub(crate) fn to_iso8601_seconds(at: Timestamp) -> String {
    format!("{}Z", date_time_to_seconds(at))
}

/// UTC の日付（`YYYY-MM-DD`。chrono の `format("%Y-%m-%d")` と同じ）。slug の先頭に使う。
pub fn to_utc_date(at: Timestamp) -> String {
    let date = Offset::UTC.to_datetime(at).date();
    let year = date.year();
    // 年が 0〜9999 の外なら符号を付ける（chrono と同じ）。
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+05}")
    };
    format!("{year}-{:02}-{:02}", date.month(), date.day())
}

/// `YYYY-MM-DDTHH:MM:SS`（UTC）。
fn date_time_to_seconds(at: Timestamp) -> String {
    let time = Offset::UTC.to_datetime(at).time();
    format!(
        "{}T{:02}:{:02}:{:02}",
        to_utc_date(at),
        time.hour(),
        time.minute(),
        time.second()
    )
}

/// Unix 秒（1970 年より前も過去へ寄せる）。`Timestamp::as_second` は 0 へ寄せるので、
/// 負の時刻で chrono の `timestamp()` と食い違う。
pub(crate) fn unix_seconds(at: Timestamp) -> i64 {
    at.as_second() - i64::from(at.subsec_nanosecond() < 0)
}

/// [`unix_seconds`] から数えた秒未満（0〜999_999_999。chrono の `timestamp_subsec_nanos()` と同じ）。
pub(crate) fn subsec_nanos(at: Timestamp) -> u32 {
    at.subsec_nanosecond()
        .rem_euclid(1_000_000_000)
        .unsigned_abs()
}

/// コアの型の日時フィールドに付ける serde（`#[serde(with = "crate::timestamp::rfc3339")]`）。
/// 読むほうは jiff の既定（RFC 3339。オフセット付きも UTC に直して読む）。
pub(crate) mod rfc3339 {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::Timestamp;

    pub fn serialize<S: Serializer>(value: &Timestamp, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::to_rfc3339(*value))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Timestamp, D::Error> {
        Timestamp::deserialize(deserializer)
    }
}

/// [`rfc3339`] の `Option` 版（`None` は `null`）。
pub(crate) mod rfc3339_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::Timestamp;

    pub fn serialize<S: Serializer>(
        value: &Option<Timestamp>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(at) => serializer.serialize_some(&super::to_rfc3339(*at)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Timestamp>, D::Error> {
        Option::<Timestamp>::deserialize(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn unix_seconds_floor_like_chrono() {
        assert_eq!(unix_seconds(at("1970-01-01T00:00:00Z")), 0);
        assert_eq!(unix_seconds(at("1970-01-01T00:00:00.5Z")), 0);
        assert_eq!(unix_seconds(at("1969-12-31T23:59:59.5Z")), -1);
        assert_eq!(subsec_nanos(at("1969-12-31T23:59:59.5Z")), 500_000_000);
        assert_eq!(unix_seconds(at("1969-12-31T23:59:59Z")), -1);
        assert_eq!(subsec_nanos(at("1969-12-31T23:59:59Z")), 0);
        assert_eq!(subsec_nanos(at("2026-01-01T00:00:00.000000001Z")), 1);
    }

    #[test]
    fn years_outside_four_digits_carry_a_sign() {
        assert_eq!(
            to_rfc3339(at("-000001-01-01T00:00:00Z")),
            "-0001-01-01T00:00:00Z"
        );
        assert_eq!(
            to_iso8601_seconds(at("0000-01-01T00:00:00.5Z")),
            "0000-01-01T00:00:00Z"
        );
    }
}
