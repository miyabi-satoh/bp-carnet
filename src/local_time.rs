//! ユーザーのタイムゾーンでの日時・日付と、DB に保存する UTC の日時との変換。
//!
//! API はユーザーのタイムゾーン (`users.timezone`) での日時・日付をオフセット無しで受け渡すため、
//! UTC との変換をここに集める。

use jiff::{
    Timestamp,
    civil::{Date, DateTime},
    fmt::strtime,
    tz::TimeZone,
};

/// DB に保存する日時の形式 (UTC・秒精度)。期間の絞り込みは文字列の大小で比べるため、
/// 書き込む値は必ずこの形式に揃える。
const UTC_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// API で受け渡す日時の形式。`<input type="datetime-local">` の値と同じ分精度にする。
const LOCAL_DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M";

const LOCAL_DATE_FORMAT: &str = "%Y-%m-%d";

/// メールに書く日付 (`2026年10月12日`)。
pub fn date_label(date: Date) -> String {
    date.strftime("%Y年%-m月%-d日").to_string()
}

/// ユーザーの個人設定のタイムゾーン。
#[derive(Debug, Clone)]
pub struct UserTimeZone(TimeZone);

impl UserTimeZone {
    /// `name` が tzdb に無い名前なら `None`。DB の直接編集や tzdb の更新に伴うゾーンの廃止で
    /// 起きうるため、呼び出し側でエラーに変換できるようにしている。
    pub fn get(name: &str) -> Option<Self> {
        TimeZone::get(name).ok().map(Self)
    }

    /// ユーザーのタイムゾーンでの日時 (`YYYY-MM-DDTHH:MM`) を、保存形式の UTC にする。
    /// 形式が違えば `None`。オフセット付きの値も受け付けない (別のタイムゾーンの時刻を、
    /// 黙ってユーザーのタイムゾーンの時刻として読み替えないため)。
    ///
    /// 夏時間の切り替えで存在しない時刻は切り替え後に、2回ある時刻は1回目に寄せる。
    pub fn parse_local_datetime(&self, value: &str) -> Option<String> {
        let datetime = strtime::parse(LOCAL_DATETIME_FORMAT, value)
            .ok()?
            .to_datetime()
            .ok()?;
        Some(format_utc(self.0.to_zoned(datetime).ok()?.timestamp()))
    }

    /// 保存形式の UTC を、ユーザーのタイムゾーンでの日時にする。解釈できなければ `None`。
    pub fn local_datetime(&self, measured_at: &str) -> Option<DateTime> {
        let timestamp = measured_at.parse::<Timestamp>().ok()?;
        Some(timestamp.to_zoned(self.0.clone()).datetime())
    }

    /// `timestamp` の、ユーザーのタイムゾーンでの日時。
    pub fn to_local(&self, timestamp: Timestamp) -> DateTime {
        timestamp.to_zoned(self.0.clone()).datetime()
    }

    /// 今このときの、ユーザーのタイムゾーンでの日付 (`YYYY-MM-DD`)。
    pub fn local_today(&self) -> String {
        Timestamp::now()
            .to_zoned(self.0.clone())
            .date()
            .strftime(LOCAL_DATE_FORMAT)
            .to_string()
    }

    /// ユーザーのタイムゾーンでの `date` の始まりを、保存形式の UTC にする。0:00 が夏時間の
    /// 切り替えで存在しない日は、切り替え後の最初の時刻になる。
    pub fn start_of_day_utc(&self, date: Date) -> Option<String> {
        Some(format_utc(date.to_zoned(self.0.clone()).ok()?.timestamp()))
    }
}

/// API で返す日時の文字列 (`YYYY-MM-DDTHH:MM`)。
pub fn format_local_datetime(datetime: DateTime) -> String {
    datetime.strftime(LOCAL_DATETIME_FORMAT).to_string()
}

/// 書き出す CSV の日時 (`YYYY-MM-DD HH:MM`)。表計算ソフトで日時として読まれる形にする (docs/import-export.md)。
pub fn format_csv_datetime(datetime: DateTime) -> String {
    datetime.strftime("%Y-%m-%d %H:%M").to_string()
}

/// ユーザーのタイムゾーンでの日付 (`YYYY-MM-DD`) を読む。形式が違えば `None`。
pub fn parse_local_date(value: &str) -> Option<Date> {
    strtime::parse(LOCAL_DATE_FORMAT, value)
        .ok()?
        .to_date()
        .ok()
}

fn format_utc(timestamp: Timestamp) -> String {
    timestamp.strftime(UTC_FORMAT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-03-04T15:30Z は東京では翌日の 0:30。月・日はゼロ埋めしない。
    #[test]
    fn date_label_is_the_date_in_the_user_timezone() {
        let tokyo = UserTimeZone::get("Asia/Tokyo").expect("既知のタイムゾーンのはず");
        let local = tokyo
            .local_datetime("2026-03-04T15:30:00Z")
            .expect("解釈できるはず");
        assert_eq!(date_label(local.date()), "2026年3月5日");
    }

    fn tz(name: &str) -> UserTimeZone {
        UserTimeZone::get(name).unwrap_or_else(|| panic!("{name} should be a known timezone"))
    }

    fn date(value: &str) -> Date {
        parse_local_date(value).unwrap_or_else(|| panic!("{value} should be a valid date"))
    }

    #[test]
    fn unknown_timezone_is_rejected() {
        assert!(UserTimeZone::get("Not/AZone").is_none());
    }

    // 07:15 JST は UTC では前日の 22:15。
    #[test]
    fn local_datetime_is_converted_to_utc() {
        assert_eq!(
            tz("Asia/Tokyo").parse_local_datetime("2026-09-07T07:15"),
            Some("2026-09-06T22:15:00Z".to_string())
        );
    }

    #[test]
    fn local_datetime_with_an_offset_or_seconds_is_rejected() {
        let tokyo = tz("Asia/Tokyo");
        for value in [
            "2026-09-07T07:15+09:00",
            "2026-09-07T07:15Z",
            "2026-09-07T07:15:00",
            "2026-09-07",
            "not a datetime",
        ] {
            assert_eq!(tokyo.parse_local_datetime(value), None, "{value}");
        }
    }

    // America/New_York は 2026-03-08 02:00 に 03:00 へ進む。存在しない 02:30 は 03:30 EDT。
    #[test]
    fn a_time_in_the_daylight_saving_gap_moves_after_the_gap() {
        assert_eq!(
            tz("America/New_York").parse_local_datetime("2026-03-08T02:30"),
            Some("2026-03-08T07:30:00Z".to_string())
        );
    }

    // America/New_York は 2026-11-01 02:00 に 01:00 へ戻る。2回ある 01:30 は1回目 (EDT)。
    #[test]
    fn a_repeated_time_uses_the_first_occurrence() {
        assert_eq!(
            tz("America/New_York").parse_local_datetime("2026-11-01T01:30"),
            Some("2026-11-01T05:30:00Z".to_string())
        );
    }

    #[test]
    fn stored_utc_is_shown_in_the_user_timezone() {
        let local = tz("Asia/Tokyo").local_datetime("2026-09-06T22:15:00Z");
        assert_eq!(
            local.map(format_local_datetime).as_deref(),
            Some("2026-09-07T07:15")
        );
        assert_eq!(tz("Asia/Tokyo").local_datetime("not a timestamp"), None);
    }

    #[test]
    fn start_of_day_is_midnight_in_the_user_timezone() {
        assert_eq!(
            tz("Asia/Tokyo").start_of_day_utc(date("2026-09-07")),
            Some("2026-09-06T15:00:00Z".to_string())
        );
    }

    #[test]
    fn local_date_must_be_a_plain_date() {
        assert!(parse_local_date("2026-09-07T00:00").is_none());
        assert!(parse_local_date("2026-02-30").is_none());
    }
}
