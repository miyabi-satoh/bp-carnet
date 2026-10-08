//! 設定画面で手動で選べるタイムゾーン。
//!
//! CLDR `windowsZones.xml` (release-48-2 時点の main、2026-09-16 に取得) の代表ゾーン
//! (`territory="001"`) 139件。tzdb 全件 (約600件) は選択肢として多すぎるため、Windows の
//! タイムゾーン一覧と同じ粒度に絞る。`metaZones.xml` は時刻の決まりが違うゾーン (夏時間の有無など) を
//! まとめてしまうため使わない。
//! 古い名前は tzdb 2026d の `backward` で今の名前に読み替えた (例: `Asia/Calcutta` → `Asia/Kolkata`)。
//!
//! 保存・検証は tzdb 全件のまま (`validation::is_known_timezone`)。自動設定ではこの一覧に無い名前も入る。

/// 手動で選べるタイムゾーン (IANA 名)。並びは `windowsZones.xml` のまま (表示の順は frontend が決める)。
pub(crate) const TIMEZONE_CHOICES: &[&str] = &[
    "Etc/GMT+12",
    "Etc/GMT+11",
    "America/Adak",
    "Pacific/Honolulu",
    "Pacific/Marquesas",
    "America/Anchorage",
    "Etc/GMT+9",
    "America/Tijuana",
    "Etc/GMT+8",
    "America/Los_Angeles",
    "America/Phoenix",
    "America/Mazatlan",
    "America/Denver",
    "America/Whitehorse",
    "America/Guatemala",
    "America/Chicago",
    "Pacific/Easter",
    "America/Mexico_City",
    "America/Regina",
    "America/Bogota",
    "America/Cancun",
    "America/New_York",
    "America/Port-au-Prince",
    "America/Havana",
    "America/Indiana/Indianapolis",
    "America/Grand_Turk",
    "America/Asuncion",
    "America/Halifax",
    "America/Caracas",
    "America/Cuiaba",
    "America/La_Paz",
    "America/Santiago",
    "America/St_Johns",
    "America/Araguaina",
    "America/Sao_Paulo",
    "America/Cayenne",
    "America/Argentina/Buenos_Aires",
    "America/Nuuk",
    "America/Montevideo",
    "America/Punta_Arenas",
    "America/Miquelon",
    "America/Bahia",
    "Etc/GMT+2",
    "Atlantic/Azores",
    "Atlantic/Cape_Verde",
    "Etc/UTC",
    "Europe/London",
    "Africa/Abidjan",
    "Africa/Sao_Tome",
    "Africa/Casablanca",
    "Europe/Berlin",
    "Europe/Budapest",
    "Europe/Paris",
    "Europe/Warsaw",
    "Africa/Lagos",
    "Asia/Amman",
    "Europe/Bucharest",
    "Asia/Beirut",
    "Africa/Cairo",
    "Europe/Chisinau",
    "Asia/Damascus",
    "Asia/Hebron",
    "Africa/Johannesburg",
    "Europe/Kyiv",
    "Asia/Jerusalem",
    "Africa/Juba",
    "Europe/Kaliningrad",
    "Africa/Khartoum",
    "Africa/Tripoli",
    "Africa/Windhoek",
    "Asia/Baghdad",
    "Europe/Istanbul",
    "Asia/Riyadh",
    "Europe/Minsk",
    "Europe/Moscow",
    "Africa/Nairobi",
    "Asia/Tehran",
    "Asia/Dubai",
    "Europe/Astrakhan",
    "Asia/Baku",
    "Europe/Samara",
    "Indian/Mauritius",
    "Europe/Saratov",
    "Asia/Tbilisi",
    "Europe/Volgograd",
    "Asia/Yerevan",
    "Asia/Kabul",
    "Asia/Tashkent",
    "Asia/Yekaterinburg",
    "Asia/Karachi",
    "Asia/Qyzylorda",
    "Asia/Kolkata",
    "Asia/Colombo",
    "Asia/Kathmandu",
    "Asia/Bishkek",
    "Asia/Dhaka",
    "Asia/Omsk",
    "Asia/Yangon",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Hovd",
    "Asia/Krasnoyarsk",
    "Asia/Novosibirsk",
    "Asia/Tomsk",
    "Asia/Shanghai",
    "Asia/Irkutsk",
    "Asia/Singapore",
    "Australia/Perth",
    "Asia/Taipei",
    "Asia/Ulaanbaatar",
    "Australia/Eucla",
    "Asia/Chita",
    "Asia/Tokyo",
    "Asia/Pyongyang",
    "Asia/Seoul",
    "Asia/Yakutsk",
    "Australia/Adelaide",
    "Australia/Darwin",
    "Australia/Brisbane",
    "Australia/Sydney",
    "Pacific/Port_Moresby",
    "Australia/Hobart",
    "Asia/Vladivostok",
    "Australia/Lord_Howe",
    "Pacific/Bougainville",
    "Asia/Srednekolymsk",
    "Asia/Magadan",
    "Pacific/Norfolk",
    "Asia/Sakhalin",
    "Pacific/Guadalcanal",
    "Asia/Kamchatka",
    "Pacific/Auckland",
    "Etc/GMT-12",
    "Pacific/Fiji",
    "Pacific/Chatham",
    "Etc/GMT-13",
    "Pacific/Tongatapu",
    "Pacific/Apia",
    "Pacific/Kiritimati",
];

#[cfg(test)]
mod tests {
    use super::TIMEZONE_CHOICES;
    use crate::validation::is_known_timezone;
    use std::collections::HashSet;

    /// 同梱 tzdb が知らない名前は `GET /settings/timezones` が黙って落とすため、
    /// tzdb を上げてゾーンが改名されると選択肢から静かに消える。それを検知する。
    #[test]
    fn every_choice_is_known_to_tzdb() {
        let unknown: Vec<&str> = TIMEZONE_CHOICES
            .iter()
            .copied()
            .filter(|zone| !is_known_timezone(zone))
            .collect();
        assert!(unknown.is_empty(), "tzdb に無いタイムゾーン: {unknown:?}");
    }

    #[test]
    fn choices_have_no_duplicates() {
        let mut seen = HashSet::new();
        let duplicates: Vec<&str> = TIMEZONE_CHOICES
            .iter()
            .copied()
            .filter(|zone| !seen.insert(*zone))
            .collect();
        assert!(
            duplicates.is_empty(),
            "重複したタイムゾーン: {duplicates:?}"
        );
    }
}
