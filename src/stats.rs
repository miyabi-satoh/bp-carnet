//! 朝/夜の血圧統計の集計。
//!
//! `measured_at` は UTC で保存しているため、朝/夜の判定にはユーザーのタイムゾーンでの
//! ローカル時刻が要る。SQLite 側では時刻の変換もユーザーごとの閾値の適用もできないため、
//! 取得したレコードに対してここで集計する。DB を介さず単体テストできるよう、純粋な
//! 関数として切り出している。

use std::collections::BTreeMap;

use jiff::civil::{Date, DateTime};

use crate::local_time::UserTimeZone;
use crate::validation::PeriodThresholds;

/// 集計の入力1件。`measured_at` は UTC の RFC3339 文字列。
#[derive(Debug, Clone)]
pub struct RecordPoint {
    pub measured_at: String,
    pub systolic: i64,
    pub diastolic: i64,
}

/// 上下の血圧の平均。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BpAverage {
    pub systolic: f64,
    pub diastolic: f64,
}

/// ローカル日1日分の朝/夜。記録が無い側は `None`。
#[derive(Debug, Clone, PartialEq)]
pub struct DailyAverages {
    /// ユーザーのタイムゾーンでの日付 (`YYYY-MM-DD`)。
    pub date: String,
    pub morning: Option<BpAverage>,
    pub evening: Option<BpAverage>,
}

/// 期間全体の統計と、グラフ用の日別系列。
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub morning: Option<BpAverage>,
    pub evening: Option<BpAverage>,
    /// 日付の昇順。朝夜どちらにも属さない記録しか無い日は含まれない。
    pub days: Vec<DailyAverages>,
}

/// 朝/夜のどちらに属するか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Morning,
    Evening,
}

/// 1日の中の位置 (0:00 からの経過分) を朝/夜に振り分ける。
/// 区間は `[start, end)` として扱うため、境界の記録が朝と夜の両方に数えられることはない。
fn classify(minutes: i64, t: PeriodThresholds) -> Option<Slot> {
    if (t.morning_start_min..t.morning_end_min).contains(&minutes) {
        Some(Slot::Morning)
    } else if (t.evening_start_min..t.evening_end_min).contains(&minutes) {
        Some(Slot::Evening)
    } else {
        None
    }
}

/// ユーザーのタイムゾーンと朝/夜の時間帯。記録1件ごとの判定 (一覧) と集計で同じ判定を使う。
#[derive(Debug, Clone)]
pub struct SlotClassifier {
    tz: UserTimeZone,
    thresholds: PeriodThresholds,
}

impl SlotClassifier {
    pub fn new(tz: UserTimeZone, thresholds: PeriodThresholds) -> Self {
        Self { tz, thresholds }
    }

    pub fn timezone(&self) -> &UserTimeZone {
        &self.tz
    }

    /// UTC の RFC3339 文字列 `measured_at` を、ユーザーのタイムゾーンでの日時にする。
    /// パースできない (登録時に正規化しているため通常は無い) 場合は `None`。
    pub fn local_datetime(&self, measured_at: &str) -> Option<DateTime> {
        self.tz.local_datetime(measured_at)
    }

    /// ユーザーのタイムゾーンでの日時が、朝/夜のどちらに属するか。
    pub fn slot(&self, local: DateTime) -> Option<Slot> {
        let minutes = i64::from(local.hour()) * 60 + i64::from(local.minute());
        classify(minutes, self.thresholds)
    }
}

/// 血圧値の合計と件数。平均を出すまでの中間状態。
#[derive(Debug, Default, Clone, Copy)]
struct Accumulator {
    systolic: i64,
    diastolic: i64,
    count: i64,
}

impl Accumulator {
    fn push(&mut self, systolic: i64, diastolic: i64) {
        self.systolic += systolic;
        self.diastolic += diastolic;
        self.count += 1;
    }

    fn average(self) -> Option<BpAverage> {
        (self.count > 0).then(|| BpAverage {
            systolic: self.systolic as f64 / self.count as f64,
            diastolic: self.diastolic as f64 / self.count as f64,
        })
    }
}

/// 日別平均をならして期間全体の平均にする。日ごとの測定回数の差が期間全体の平均に
/// 効かないよう、全件をまとめて平均するのではなく一度日別にならしてから平均する。
fn average_of(values: impl Iterator<Item = BpAverage>) -> Option<BpAverage> {
    let mut systolic = 0.0;
    let mut diastolic = 0.0;
    let mut count = 0.0;
    for value in values {
        systolic += value.systolic;
        diastolic += value.diastolic;
        count += 1.0;
    }
    (count > 0.0).then(|| BpAverage {
        systolic: systolic / count,
        diastolic: diastolic / count,
    })
}

/// 記録を朝/夜に振り分けて集計する。朝/夜のどちらにも属さない記録・パースできない
/// `measured_at` は集計から除外する。
pub fn summarize(records: &[RecordPoint], classifier: &SlotClassifier) -> Summary {
    // `Date` は日付順に並ぶため、BTreeMap に入れるだけで日別系列が日付の昇順に揃う。
    let mut by_date: BTreeMap<Date, (Accumulator, Accumulator)> = BTreeMap::new();
    for record in records {
        let Some(local) = classifier.local_datetime(&record.measured_at) else {
            continue;
        };
        let Some(slot) = classifier.slot(local) else {
            continue;
        };

        let entry = by_date.entry(local.date()).or_default();
        let accumulator = match slot {
            Slot::Morning => &mut entry.0,
            Slot::Evening => &mut entry.1,
        };
        accumulator.push(record.systolic, record.diastolic);
    }

    let days: Vec<DailyAverages> = by_date
        .into_iter()
        .map(|(date, (morning, evening))| DailyAverages {
            date: date.to_string(),
            morning: morning.average(),
            evening: evening.average(),
        })
        .collect();

    Summary {
        morning: average_of(days.iter().filter_map(|day| day.morning)),
        evening: average_of(days.iter().filter_map(|day| day.evening)),
        days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKYO: &str = "Asia/Tokyo";
    const NEW_YORK: &str = "America/New_York";

    fn thresholds() -> PeriodThresholds {
        PeriodThresholds::DEFAULT
    }

    fn record(measured_at: &str, systolic: i64, diastolic: i64) -> RecordPoint {
        RecordPoint {
            measured_at: measured_at.to_string(),
            systolic,
            diastolic,
        }
    }

    fn classifier(timezone: &str) -> SlotClassifier {
        let tz = UserTimeZone::get(timezone)
            .unwrap_or_else(|| panic!("{timezone} should be a known timezone"));
        SlotClassifier::new(tz, thresholds())
    }

    fn summarize_tokyo(records: &[RecordPoint]) -> Summary {
        summarize(records, &classifier(TOKYO))
    }

    fn average(systolic: f64, diastolic: f64) -> Option<BpAverage> {
        Some(BpAverage {
            systolic,
            diastolic,
        })
    }

    fn slot_of(classifier: &SlotClassifier, measured_at: &str) -> Option<Slot> {
        let local = classifier
            .local_datetime(measured_at)
            .expect("measured_at should be a valid timestamp");
        classifier.slot(local)
    }

    // 表示用の日時はユーザーのタイムゾーンのもの (08:30 JST = 前日 23:30 UTC)。
    #[test]
    fn local_datetime_is_in_the_user_timezone() {
        let tokyo = classifier(TOKYO);
        let local = tokyo.local_datetime("2026-09-07T23:30:00Z");
        assert_eq!(
            local.map(|datetime| datetime.to_string()).as_deref(),
            Some("2026-09-08T08:30:00")
        );
        assert_eq!(tokyo.local_datetime("not a timestamp"), None);
    }

    // 集計と同じくローカル時刻で判定する (08:30 JST = 前日 23:30 UTC、21:00 JST = 12:00 UTC)。
    #[test]
    fn slot_is_judged_by_local_time() {
        let tokyo = classifier(TOKYO);
        assert_eq!(slot_of(&tokyo, "2026-09-07T23:30:00Z"), Some(Slot::Morning));
        assert_eq!(slot_of(&tokyo, "2026-09-08T12:00:00Z"), Some(Slot::Evening));
    }

    // 12:00 JST (日中)・02:00 JST (深夜) は、どちらの区分にも入れない。
    #[test]
    fn slot_is_none_outside_morning_and_evening() {
        let tokyo = classifier(TOKYO);
        assert_eq!(slot_of(&tokyo, "2026-09-08T03:00:00Z"), None);
        assert_eq!(slot_of(&tokyo, "2026-09-07T17:00:00Z"), None);
    }

    #[test]
    fn no_records_yields_empty_summary() {
        let summary = summarize_tokyo(&[]);
        assert_eq!(summary.morning, None);
        assert_eq!(summary.evening, None);
        assert!(summary.days.is_empty());
    }

    // 08:30 JST = 前日 23:30 UTC。ローカル時刻で判定できていないと、この記録は深夜扱いに
    // なって集計から丸ごと落ちる。
    #[test]
    fn classifies_by_local_time_not_utc() {
        let summary = summarize_tokyo(&[record("2026-09-07T23:30:00Z", 130, 80)]);
        assert_eq!(
            summary.days,
            vec![DailyAverages {
                date: "2026-09-08".to_string(),
                morning: average(130.0, 80.0),
                evening: None,
            }]
        );
    }

    // 23:30 JST = 同日 14:30 UTC。UTC の日付でまとめると別の日に寄ってしまう。
    #[test]
    fn groups_by_local_date_not_utc_date() {
        let summary = summarize_tokyo(&[record("2026-09-08T14:30:00Z", 120, 78)]);
        assert_eq!(summary.days.len(), 1);
        assert_eq!(summary.days[0].date, "2026-09-08");
        assert_eq!(summary.days[0].evening, average(120.0, 78.0));
    }

    #[test]
    fn ignores_records_outside_morning_and_evening() {
        // 12:00 JST (日中) と 02:00 JST (深夜)。
        let summary = summarize_tokyo(&[
            record("2026-09-08T03:00:00Z", 130, 80),
            record("2026-09-07T17:00:00Z", 140, 90),
        ]);
        assert_eq!(summary.morning, None);
        assert_eq!(summary.evening, None);
        assert!(summary.days.is_empty());
    }

    // 区間は end を含まないため、朝の終了ちょうど (10:00 JST) は朝に入らない。
    #[test]
    fn excludes_the_record_at_the_end_boundary() {
        let summary = summarize_tokyo(&[record("2026-09-08T01:00:00Z", 130, 80)]);
        assert!(summary.days.is_empty());
    }

    // 朝の開始ちょうど (4:00 JST) は含む。
    #[test]
    fn includes_the_record_at_the_start_boundary() {
        let summary = summarize_tokyo(&[record("2026-09-07T19:00:00Z", 130, 80)]);
        assert_eq!(summary.days.len(), 1);
        assert_eq!(summary.days[0].morning, average(130.0, 80.0));
    }

    #[test]
    fn averages_multiple_records_in_the_same_slot() {
        let summary = summarize_tokyo(&[
            record("2026-09-07T23:00:00Z", 130, 80),
            record("2026-09-07T23:30:00Z", 140, 90),
        ]);
        assert_eq!(summary.days[0].morning, average(135.0, 85.0));
    }

    #[test]
    fn leaves_the_missing_slot_empty() {
        // 朝だけの日。
        let summary = summarize_tokyo(&[record("2026-09-07T23:00:00Z", 130, 80)]);
        assert_eq!(summary.days[0].morning, average(130.0, 80.0));
        assert_eq!(summary.days[0].evening, None);
        assert_eq!(summary.evening, None);
    }

    // 日ごとの測定回数の差が期間全体の平均に効かないことを確認する。1日目は朝2件
    // (120/140 → 130)、2日目は朝1件 (160)。全件平均なら140だが、日別平均をならすので
    // (130 + 160) / 2 = 145 になる。
    #[test]
    fn averages_daily_averages_not_every_record() {
        let summary = summarize_tokyo(&[
            record("2026-09-07T23:00:00Z", 120, 80),
            record("2026-09-07T23:30:00Z", 140, 90),
            record("2026-09-08T23:00:00Z", 160, 100),
        ]);
        assert_eq!(summary.days.len(), 2);
        assert_eq!(summary.morning, average(145.0, 92.5));
    }

    #[test]
    fn orders_days_ascending_regardless_of_input_order() {
        let summary = summarize_tokyo(&[
            record("2026-09-09T23:00:00Z", 130, 80),
            record("2026-09-07T23:00:00Z", 130, 80),
            record("2026-09-08T23:00:00Z", 130, 80),
        ]);
        let dates: Vec<&str> = summary.days.iter().map(|day| day.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-09-08", "2026-09-09", "2026-09-10"]);
    }

    // Asia/Tokyo には夏時間が無いため、UTC からの固定オフセットで実装しても通ってしまう。
    // 夏時間のあるゾーンで、同じ時計時刻が時期によって別の判定になることを確認する。
    // 14:30Z は EST (UTC-5) なら 09:30 で朝、EDT (UTC-4) なら 10:30 で対象外。
    #[test]
    fn respects_daylight_saving_time() {
        let new_york = classifier(NEW_YORK);

        let winter = summarize(&[record("2026-01-15T14:30:00Z", 130, 80)], &new_york);
        assert_eq!(winter.days.len(), 1, "EST では 09:30 なので朝に入る");

        let summer = summarize(&[record("2026-07-15T14:30:00Z", 130, 80)], &new_york);
        assert!(summer.days.is_empty(), "EDT では 10:30 なので朝に入らない");
    }

    #[test]
    fn skips_records_with_an_unparsable_measured_at() {
        let summary = summarize_tokyo(&[
            record("not a timestamp", 130, 80),
            record("2026-09-07T23:00:00Z", 140, 90),
        ]);
        assert_eq!(summary.days.len(), 1);
        assert_eq!(summary.days[0].morning, average(140.0, 90.0));
    }
}
