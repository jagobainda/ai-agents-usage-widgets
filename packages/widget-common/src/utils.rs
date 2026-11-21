use std::ffi::OsStr;
use std::fmt::Write as _;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;

const WEEK: i64 = 7 * 24 * 60;
const DAY: i64 = 24 * 60;

pub fn parse_iso_datetime(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let text = value?.as_str().filter(|s| !s.is_empty())?;
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }
    let normalized = text.replacen(' ', "T", 1);
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M%:z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
    ] {
        if let Ok(parsed) = DateTime::parse_from_str(&normalized, format) {
            return Some(parsed.with_timezone(&Utc));
        }
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(&normalized, format) {
            return Some(Utc.from_utc_datetime(&parsed));
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|naive| Utc.from_utc_datetime(&naive))
}

pub fn datetime_from_unix(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let seconds = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor();
    let nanos = ((seconds - whole) * 1e9).round() as u32;
    DateTime::from_timestamp(whole as i64, nanos.min(999_999_999))
}

pub fn py_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.trunc() as i64)
        }),
        Value::String(text) => text.trim().replace('_', "").parse::<i64>().ok(),
        _ => None,
    }
}

pub fn format_until(value: Option<DateTime<Utc>>) -> String {
    format_until_at(value, Utc::now())
}

pub fn format_until_at(value: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    let total = (value - now).num_seconds();
    if total <= 0 {
        return "now".to_string();
    }
    let (hours, minutes) = (total / 3600, total % 3600 / 60);
    if hours >= 24 {
        return format!("{}d {}h {}m", hours / 24, hours % 24, minutes);
    }
    format!("{hours}h {minutes:02}m")
}

fn positive_minutes(minutes: Option<&Value>) -> Option<i64> {
    py_int(minutes).filter(|total| *total > 0)
}

pub fn duration_label(minutes: Option<&Value>) -> String {
    let Some(total) = positive_minutes(minutes) else {
        return "Usage window".to_string();
    };
    if total >= 2 * WEEK && total % WEEK == 0 {
        format!("{}-week window", total / WEEK)
    } else if total % DAY == 0 {
        format!("{}-day window", total / DAY)
    } else if total % 60 == 0 {
        format!("{}-hour window", total / 60)
    } else {
        format!("{total}-minute window")
    }
}

pub fn duration_short_label(minutes: Option<&Value>) -> String {
    let Some(total) = positive_minutes(minutes) else {
        return "usage".to_string();
    };
    if total >= 2 * WEEK && total % WEEK == 0 {
        format!("{}w", total / WEEK)
    } else if total % DAY == 0 {
        format!("{}d", total / DAY)
    } else if total % 60 == 0 {
        format!("{}h", total / 60)
    } else {
        format!("{total}m")
    }
}

pub fn home_dir() -> PathBuf {
    use std::env;

    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| {
            let mut home = PathBuf::from(env::var_os("HOMEDRIVE")?);
            home.push(env::var_os("HOMEPATH")?);
            Some(home)
        })
        .unwrap_or_default()
}

pub fn round_percent(utilization: f64) -> i64 {
    (utilization * 100.0).round_ties_even() as i64
}

pub fn encode_wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

pub fn truthy(value: &serde_json::Value) -> bool {
    use serde_json::Value;

    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(array) => !array.is_empty(),
        Value::Object(object) => !object.is_empty(),
    }
}

pub fn ascii_json(json: &str) -> String {
    let mut escaped = String::with_capacity(json.len());
    for character in json.chars() {
        if character < '\u{7f}' {
            escaped.push(character);
        } else {
            for unit in character.encode_utf16(&mut [0; 2]) {
                let _ = write!(escaped, "\\u{unit:04x}");
            }
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    #[test]
    fn parses_iso_variants() {
        let expected = Utc.with_ymd_and_hms(2026, 11, 21, 11, 0, 0).unwrap();
        for text in [
            "2026-11-21T11:00:00Z",
            "2026-11-21T11:00:00+00:00",
            "2026-11-21T12:00:00+01:00",
            "2026-11-21T11:00:00",
            "2026-11-21 11:00:00",
            "2026-11-21T11:00:00.000000Z",
        ] {
            assert_eq!(
                parse_iso_datetime(Some(&json!(text))),
                Some(expected),
                "{text}"
            );
        }
        assert_eq!(parse_iso_datetime(Some(&json!(""))), None);
        assert_eq!(parse_iso_datetime(Some(&json!(5))), None);
        assert_eq!(parse_iso_datetime(Some(&json!("nope"))), None);
        assert_eq!(parse_iso_datetime(None), None);
    }

    #[test]
    fn converts_unix_values() {
        assert_eq!(
            datetime_from_unix(Some(&json!(0))),
            Some(Utc.timestamp_opt(0, 0).unwrap())
        );
        assert_eq!(datetime_from_unix(Some(&json!("invalid"))), None);
        assert_eq!(datetime_from_unix(Some(&json!(true))), None);
        assert_eq!(datetime_from_unix(None), None);
    }

    #[test]
    fn formats_time_until_reset() {
        let now = Utc.with_ymd_and_hms(2026, 11, 21, 11, 0, 0).unwrap();
        assert_eq!(format_until_at(None, now), "—");
        assert_eq!(format_until_at(Some(now), now), "now");
        assert_eq!(
            format_until_at(Some(now + Duration::minutes(135)), now),
            "2h 15m"
        );
        assert_eq!(
            format_until_at(Some(now + Duration::minutes(5)), now),
            "0h 05m"
        );
        assert_eq!(
            format_until_at(Some(now + Duration::days(3)), now),
            "3d 0h 0m"
        );
    }

    #[test]
    fn labels_durations() {
        for (minutes, long, short) in [
            (json!(300), "5-hour window", "5h"),
            (json!(10080), "7-day window", "7d"),
            (json!(20160), "2-week window", "2w"),
            (json!(45), "45-minute window", "45m"),
            (json!(0), "Usage window", "usage"),
            (json!(true), "Usage window", "usage"),
            (json!("300"), "5-hour window", "5h"),
            (json!(null), "Usage window", "usage"),
        ] {
            assert_eq!(duration_label(Some(&minutes)), long);
            assert_eq!(duration_short_label(Some(&minutes)), short);
        }
    }
}
