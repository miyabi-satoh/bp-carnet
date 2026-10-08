//! 入力値の検証ルール。
//!
//! 血圧の値域は `api::records` (登録・更新 API) と `ocr` (読み取り結果を「フォームに
//! 反映してよい値か」判定する際の基準) の両方から参照する。2箇所で別々の定数を持つと、
//! OCR は反映できたのに登録 API では値域外として弾かれる、という食い違いが起きるため、
//! 一箇所に集約している。朝/夜の閾値も、設定 UI と API で条件がずれないよう同じ方針で扱う。

use std::ops::RangeInclusive;

use jiff::tz::TimeZone;
use unicode_general_category::{GeneralCategory, get_general_category};

/// 収縮期血圧の許容範囲 (mmHg)。PoC のバリデーションルールを踏襲する。
pub const SYSTOLIC_RANGE: RangeInclusive<i64> = 60..=260;
/// 拡張期血圧の許容範囲 (mmHg)。
pub const DIASTOLIC_RANGE: RangeInclusive<i64> = 30..=180;
/// 脈拍の許容範囲 (回/分)。
pub const PULSE_RANGE: RangeInclusive<i64> = 30..=220;

/// 朝/夜の閾値として指定できる「その日の 0:00 からの経過分数」の範囲。
/// 24:00 (=1440) を終了時刻として指定できるよう上限を 1440 にしている。
pub const DAY_MINUTES_RANGE: RangeInclusive<i64> = 0..=1440;

/// 管理者が指定できる OCR 累計金額の上限 (円)。0 は「もう使えない」、上限の 100000 は、
/// 桁が違う入力 (0 の打ち間違い) を弾くためのもの。
pub const OCR_BUDGET_YEN_RANGE: RangeInclusive<i64> = 0..=100_000;

/// 無制限を表す値。API で受け付けるのはこの値だけにする。
pub const OCR_BUDGET_YEN_UNLIMITED: i64 = -1;

/// 管理者が指定した OCR 累計金額の上限として受け付けてよい値か。
pub fn is_valid_ocr_budget_yen(budget: i64) -> bool {
    budget == OCR_BUDGET_YEN_UNLIMITED || OCR_BUDGET_YEN_RANGE.contains(&budget)
}

/// ユーザーIDの最大長 (文字数)。メールアドレスをそのままIDにできるよう、RFC 5321 の
/// 上限 (254) に合わせる。
pub const USERNAME_MAX_CHARS: usize = 254;

/// ユーザーIDとして受け付けない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsernameError {
    Empty,
    TooLong,
    /// 空白や制御文字を含む。ログイン時に見えない差で弾かれるのを防ぐため受け付けない。
    InvalidCharacter,
}

/// 見た目では気づけない文字。ログインのたびに「入力したはずのIDで入れない」が起きるため、
/// ユーザーIDには使わせない。
///
/// Cf (Format) を丸ごと弾くのは、ゼロ幅スペース (U+200B) のように既存のIDと見分けが
/// つかない別IDを作れてしまうため。U+FEFF もここに含まれる (Unicode の White_Space では
/// ないので `char::is_whitespace()` だけでは拾えない)。
fn is_invisible(c: char) -> bool {
    c.is_whitespace() || c.is_control() || get_general_category(c) == GeneralCategory::Format
}

/// ユーザーIDを検証する。メールアドレスと任意のIDのどちらも受け付けるため、書式では
/// 分岐させず、確実に事故になるものだけを弾く (docs/authentication.md)。
pub fn validate_username(username: &str) -> Result<(), UsernameError> {
    if username.is_empty() {
        return Err(UsernameError::Empty);
    }
    if username.chars().count() > USERNAME_MAX_CHARS {
        return Err(UsernameError::TooLong);
    }
    if username.chars().any(is_invisible) {
        return Err(UsernameError::InvalidCharacter);
    }
    Ok(())
}

/// 血圧記録の値が満たすべき条件のうち、値域チェックでは表現できない部分。
/// `validate_bp_values` が返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpValuesError {
    SystolicOutOfRange,
    DiastolicOutOfRange,
    /// 血圧の定義上必ず成り立つはずの `収縮期 > 拡張期` が崩れている。
    SystolicNotGreaterThanDiastolic,
    PulseOutOfRange,
}

/// 血圧記録の値域・整合性を検証する。
/// 通常登録・CSVインポート (`api::records`) と OCR取り込み (`ocr`) の両方から呼ぶ。
/// `pulse` は「未入力/読み取れなかった」を `None` として渡す。
pub fn validate_bp_values(
    systolic: i64,
    diastolic: i64,
    pulse: Option<i64>,
) -> Result<(), BpValuesError> {
    if !SYSTOLIC_RANGE.contains(&systolic) {
        return Err(BpValuesError::SystolicOutOfRange);
    }
    if !DIASTOLIC_RANGE.contains(&diastolic) {
        return Err(BpValuesError::DiastolicOutOfRange);
    }
    if systolic <= diastolic {
        return Err(BpValuesError::SystolicNotGreaterThanDiastolic);
    }
    if let Some(pulse) = pulse
        && !PULSE_RANGE.contains(&pulse)
    {
        return Err(BpValuesError::PulseOutOfRange);
    }
    Ok(())
}

/// 新規ユーザーの既定タイムゾーン。主な利用者が日本在住のため。
///
/// 本体コードは使わず、マイグレーションの列 DEFAULT との一致をテストで固定するためにある。
#[cfg(test)]
pub const DEFAULT_TIMEZONE: &str = "Asia/Tokyo";

/// 朝/夜の時間帯 (分単位、`start` 側を含み `end` 側を含まない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodThresholds {
    pub morning_start_min: i64,
    pub morning_end_min: i64,
    pub evening_start_min: i64,
    pub evening_end_min: i64,
}

impl PeriodThresholds {
    /// 新規ユーザーの既定値 (朝 4:00-10:00・夜 18:00-24:00)。
    ///
    /// マイグレーションの列 DEFAULT はこの値を写したもので、`db.rs` のテストが両者の一致を
    /// 固定している (SQL 側を直接の基準にすると、値の意味と検証ルールが別ファイルに散る)。
    /// 本体コードは使わないため、テストビルドに限る。
    #[cfg(test)]
    pub const DEFAULT: Self = Self {
        morning_start_min: 4 * 60,
        morning_end_min: 10 * 60,
        evening_start_min: 18 * 60,
        evening_end_min: 24 * 60,
    };
}

/// `PeriodThresholds` が満たさない条件。呼び出し側 (API 層) が表示用のメッセージに変換する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodThresholdsError {
    /// いずれかの値が `DAY_MINUTES_RANGE` の外。
    OutOfRange,
    /// 朝または夜の区間で `start >= end` になっている。
    NotAscending,
    /// 朝と夜の区間が重なっている。1つの記録が朝と夜の両方に数えられるのを防ぐ。
    Overlapping,
}

/// 指定タイムゾーンが tzdb に存在するか。タイムゾーンを受け付ける際の検証に使う。
pub fn is_known_timezone(name: &str) -> bool {
    TimeZone::get(name).is_ok()
}

/// 朝/夜の閾値が統計・グラフの集計に使える形かを検証する。
///
/// 血圧値の値域と同じく、経路ごとに条件がずれないよう判定をここに集約する。
pub fn validate_period_thresholds(t: PeriodThresholds) -> Result<(), PeriodThresholdsError> {
    let values = [
        t.morning_start_min,
        t.morning_end_min,
        t.evening_start_min,
        t.evening_end_min,
    ];
    if !values.iter().all(|v| DAY_MINUTES_RANGE.contains(v)) {
        return Err(PeriodThresholdsError::OutOfRange);
    }
    if t.morning_start_min >= t.morning_end_min || t.evening_start_min >= t.evening_end_min {
        return Err(PeriodThresholdsError::NotAscending);
    }
    // 区間は end を含まないため、境界が接するだけ (朝の終了 == 夜の開始) は重複としない。
    if t.morning_start_min < t.evening_end_min && t.evening_start_min < t.morning_end_min {
        return Err(PeriodThresholdsError::Overlapping);
    }
    Ok(())
}

#[cfg(test)]
mod ocr_budget_tests {
    use super::*;

    #[test]
    fn accepts_unlimited_and_the_range_boundaries_only() {
        for (budget, valid) in [
            (-1, true),
            (0, true),
            (80, true),
            (100_000, true),
            (-2, false),
            (100_001, false),
        ] {
            assert_eq!(is_valid_ocr_budget_yen(budget), valid, "{budget}");
        }
    }
}

#[cfg(test)]
mod username_tests {
    use super::*;

    #[test]
    fn accepts_an_email_address_and_a_plain_id() {
        assert_eq!(validate_username("kyoko"), Ok(()));
        assert_eq!(validate_username("hahaue@example.com"), Ok(()));
    }

    #[test]
    fn rejects_empty_whitespace_and_control_characters() {
        assert_eq!(validate_username(""), Err(UsernameError::Empty));
        for username in ["with space", "tab\there", "null\u{0}char"] {
            assert_eq!(
                validate_username(username),
                Err(UsernameError::InvalidCharacter),
                "{username:?} は弾かれるべき"
            );
        }
    }

    /// 見た目で気づけない Cf (Format) は、White_Space でなくても弾く。
    #[test]
    fn rejects_invisible_format_characters() {
        for username in [
            "ky\u{FEFF}oko", // ZERO WIDTH NO-BREAK SPACE
            "a\u{200B}b",    // ZERO WIDTH SPACE
            "a\u{200C}b",    // ZERO WIDTH NON-JOINER
            "a\u{200D}b",    // ZERO WIDTH JOINER
            "a\u{2060}b",    // WORD JOINER
        ] {
            assert_eq!(
                validate_username(username),
                Err(UsernameError::InvalidCharacter),
                "{username:?} は弾かれるべき"
            );
        }
    }

    /// 日本語のユーザーIDは引き続き使える (弾くのは見えない文字だけ)。
    #[test]
    fn accepts_non_ascii_letters() {
        assert_eq!(validate_username("きょうこ"), Ok(()));
    }

    #[test]
    fn rejects_a_username_longer_than_the_limit() {
        let limit = "a".repeat(USERNAME_MAX_CHARS);
        assert_eq!(validate_username(&limit), Ok(()));
        assert_eq!(
            validate_username(&format!("{limit}a")),
            Err(UsernameError::TooLong)
        );
    }

    /// 長さはコードポイント数で数える (UTF-8 のバイト数でも UTF-16 の符号単位数でもない)。
    /// 絵文字は UTF-16 では2単位になるため、上限ちょうどで境界を確かめる。
    #[test]
    fn counts_length_in_characters() {
        assert_eq!(validate_username(&"あ".repeat(USERNAME_MAX_CHARS)), Ok(()));
        assert_eq!(validate_username(&"😀".repeat(USERNAME_MAX_CHARS)), Ok(()));
        assert_eq!(
            validate_username(&"😀".repeat(USERNAME_MAX_CHARS + 1)),
            Err(UsernameError::TooLong)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> PeriodThresholds {
        PeriodThresholds::DEFAULT
    }

    /// `pulse` が `None` (未入力/読み取れなかった) の場合は値域チェックをスキップする。
    #[test]
    fn accepts_valid_values_without_pulse() {
        assert_eq!(validate_bp_values(120, 80, None), Ok(()));
    }

    #[test]
    fn accepts_range_boundaries() {
        assert_eq!(validate_bp_values(60, 30, None), Ok(()));
        assert_eq!(validate_bp_values(260, 180, Some(30)), Ok(()));
        assert_eq!(validate_bp_values(260, 180, Some(220)), Ok(()));
    }

    #[test]
    fn rejects_systolic_out_of_range() {
        assert_eq!(
            validate_bp_values(59, 30, None),
            Err(BpValuesError::SystolicOutOfRange)
        );
        assert_eq!(
            validate_bp_values(261, 80, None),
            Err(BpValuesError::SystolicOutOfRange)
        );
    }

    #[test]
    fn rejects_diastolic_out_of_range() {
        assert_eq!(
            validate_bp_values(120, 29, None),
            Err(BpValuesError::DiastolicOutOfRange)
        );
        assert_eq!(
            validate_bp_values(120, 181, None),
            Err(BpValuesError::DiastolicOutOfRange)
        );
    }

    /// 血圧の定義上必ず成り立つはずの関係が崩れている場合、値域内でも不採用にする。
    #[test]
    fn rejects_systolic_not_greater_than_diastolic() {
        assert_eq!(
            validate_bp_values(80, 80, None),
            Err(BpValuesError::SystolicNotGreaterThanDiastolic)
        );
    }

    #[test]
    fn rejects_pulse_out_of_range() {
        assert_eq!(
            validate_bp_values(120, 80, Some(29)),
            Err(BpValuesError::PulseOutOfRange)
        );
        assert_eq!(
            validate_bp_values(120, 80, Some(221)),
            Err(BpValuesError::PulseOutOfRange)
        );
    }

    /// tzdb を同梱している (Cargo.toml の `tzdb-bundle-always`) ことの確認も兼ねる。
    #[test]
    fn known_timezones_are_recognized() {
        assert!(is_known_timezone(DEFAULT_TIMEZONE));
        assert!(is_known_timezone("America/New_York"));
        assert!(!is_known_timezone("Not/AZone"));
    }

    #[test]
    fn default_thresholds_are_valid() {
        assert_eq!(validate_period_thresholds(defaults()), Ok(()));
    }

    #[test]
    fn midnight_to_end_of_day_is_valid() {
        let t = PeriodThresholds {
            morning_start_min: 0,
            morning_end_min: 1,
            evening_start_min: 1439,
            evening_end_min: 1440,
        };
        assert_eq!(validate_period_thresholds(t), Ok(()));
    }

    #[test]
    fn rejects_minutes_past_end_of_day() {
        let t = PeriodThresholds {
            evening_end_min: 1441,
            ..defaults()
        };
        assert_eq!(
            validate_period_thresholds(t),
            Err(PeriodThresholdsError::OutOfRange)
        );
    }

    #[test]
    fn rejects_negative_minutes() {
        let t = PeriodThresholds {
            morning_start_min: -1,
            ..defaults()
        };
        assert_eq!(
            validate_period_thresholds(t),
            Err(PeriodThresholdsError::OutOfRange)
        );
    }

    #[test]
    fn rejects_interval_that_ends_before_it_starts() {
        let t = PeriodThresholds {
            morning_start_min: 10 * 60,
            morning_end_min: 4 * 60,
            ..defaults()
        };
        assert_eq!(
            validate_period_thresholds(t),
            Err(PeriodThresholdsError::NotAscending)
        );
    }

    #[test]
    fn rejects_empty_interval() {
        let t = PeriodThresholds {
            morning_start_min: 4 * 60,
            morning_end_min: 4 * 60,
            ..defaults()
        };
        assert_eq!(
            validate_period_thresholds(t),
            Err(PeriodThresholdsError::NotAscending)
        );
    }

    #[test]
    fn rejects_morning_and_evening_that_overlap() {
        let t = PeriodThresholds {
            morning_end_min: 19 * 60,
            ..defaults()
        };
        assert_eq!(
            validate_period_thresholds(t),
            Err(PeriodThresholdsError::Overlapping)
        );
    }

    /// 区間は end を含まないため、朝の終了と夜の開始が同じ値でも重複ではない。
    #[test]
    fn accepts_intervals_that_only_touch_at_the_boundary() {
        let t = PeriodThresholds {
            morning_start_min: 4 * 60,
            morning_end_min: 18 * 60,
            evening_start_min: 18 * 60,
            evening_end_min: 24 * 60,
        };
        assert_eq!(validate_period_thresholds(t), Ok(()));
    }

    /// 夜が朝より前の時間帯に設定されていても、重ならなければ有効とする。
    #[test]
    fn accepts_evening_earlier_than_morning() {
        let t = PeriodThresholds {
            morning_start_min: 18 * 60,
            morning_end_min: 20 * 60,
            evening_start_min: 4 * 60,
            evening_end_min: 6 * 60,
        };
        assert_eq!(validate_period_thresholds(t), Ok(()));
    }
}
