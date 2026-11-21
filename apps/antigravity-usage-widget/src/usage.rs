use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use widget_common::utils::parse_iso_datetime;
use widget_common::{Usage, UsageLimit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AntigravityUsageError {
    Payload(String),
    Empty(String),
}

impl fmt::Display for AntigravityUsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Payload(msg) | Self::Empty(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for AntigravityUsageError {}

fn fraction(value: Option<&Value>) -> Option<f64> {
    let raw = match value? {
        Value::Number(num) => num.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !raw.is_finite() {
        return None;
    }
    Some(raw.clamp(0.0, 1.0))
}

fn strip_word_suffix_ci<'a>(text: &'a str, suffix_word: &str) -> &'a str {
    let trimmed = text.trim();
    if trimmed.len() <= suffix_word.len() {
        return trimmed;
    }
    let split_idx = trimmed.len() - suffix_word.len();
    if !trimmed.is_char_boundary(split_idx) {
        return trimmed;
    }
    if !trimmed[split_idx..].eq_ignore_ascii_case(suffix_word) {
        return trimmed;
    }
    let prefix = &trimmed[..split_idx];
    if !prefix.chars().last().is_some_and(char::is_whitespace) {
        return trimmed;
    }
    let stripped = prefix.trim();
    if stripped.is_empty() {
        trimmed
    } else {
        stripped
    }
}

fn clean_group_name(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(strip_word_suffix_ci(raw, "models").to_string())
}

fn replace_whitespace_and_ci(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        let Some(ws_pos) = rest.find(char::is_whitespace) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..ws_pos]);
        let after_ws1 = rest[ws_pos..].trim_start_matches(char::is_whitespace);
        if after_ws1.len() >= 3
            && after_ws1.is_char_boundary(3)
            && after_ws1[..3].eq_ignore_ascii_case("and")
        {
            let after_and = &after_ws1[3..];
            if after_and.chars().next().is_some_and(char::is_whitespace) {
                out.push('/');
                rest = after_and.trim_start_matches(char::is_whitespace);
                continue;
            }
        }
        let ws_len = rest.len() - ws_pos - after_ws1.len();
        out.push_str(&rest[ws_pos..ws_pos + ws_len]);
        rest = after_ws1;
    }
    out
}

fn known_window(normalized: &str) -> Option<(u8, &'static str, &'static str)> {
    match normalized {
        "5h" => Some((0, "5-hour window", "5h")),
        "daily" | "1d" | "24h" => Some((1, "Daily window", "1d")),
        "weekly" | "7d" => Some((2, "Weekly window", "7d")),
        "monthly" | "30d" => Some((3, "Monthly window", "month")),
        _ => None,
    }
}

fn window_meta(window: Option<&Value>, bucket_name: Option<&Value>) -> (u8, String, String) {
    let normalized = window
        .and_then(Value::as_str)
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    if let Some((order, label, short)) = known_window(&normalized) {
        return (order, label.to_string(), short.to_string());
    }
    if let Some(raw_name) = bucket_name.and_then(Value::as_str) {
        let trimmed = raw_name.trim();
        if !trimmed.is_empty() {
            let cleaned = strip_word_suffix_ci(trimmed, "remaining");
            if !cleaned.is_empty() {
                let short = if normalized.is_empty() {
                    cleaned.to_string()
                } else {
                    normalized
                };
                return (4, cleaned.to_string(), short);
            }
        }
    }
    if !normalized.is_empty() {
        return (4, format!("{normalized} window"), normalized);
    }
    (4, "Usage window".to_string(), "usage".to_string())
}

fn bucket_remaining(bucket: &Map<String, Value>) -> Option<f64> {
    if let Some(value) = bucket.get("remaining_fraction") {
        return fraction(Some(value));
    }
    // Protobuf JSON omits 0.0 float values when a bucket is exhausted.
    if bucket
        .get("reset_time")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Some(0.0);
    }
    None
}

struct ParsedBucket {
    order: u8,
    bucket_index: usize,
    bucket_id: String,
    window_key: String,
    window_label: String,
    window_short: String,
    utilization: f64,
    resets_at: Option<DateTime<Utc>>,
}

pub fn parse_usage_response(payload: &Value) -> Result<Usage, AntigravityUsageError> {
    let Some(payload_obj) = payload.as_object() else {
        return Err(AntigravityUsageError::Payload(
            "Antigravity CLI returned an invalid payload.".to_string(),
        ));
    };

    let Some(command) = payload_obj.get("command").and_then(Value::as_object) else {
        return Err(AntigravityUsageError::Payload(
            "Antigravity CLI response is missing command data.".to_string(),
        ));
    };

    if let Some(command_name) = command.get("name")
        && !command_name.is_null()
        && command_name.as_str() != Some("usage")
    {
        return Err(AntigravityUsageError::Payload(
            "Antigravity CLI returned an unexpected command.".to_string(),
        ));
    }

    let Some(data) = command.get("data").and_then(Value::as_object) else {
        return Err(AntigravityUsageError::Payload(
            "Antigravity CLI response is missing usage data.".to_string(),
        ));
    };

    let Some(groups) = data.get("groups").and_then(Value::as_array) else {
        return Err(AntigravityUsageError::Payload(
            "Antigravity CLI response is missing usage groups.".to_string(),
        ));
    };

    let mut parsed_groups: Vec<(Option<String>, Vec<ParsedBucket>)> = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        let Some(group_map) = group.as_object() else {
            continue;
        };
        let Some(raw_buckets) = group_map.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        let group_name = clean_group_name(group_map.get("name"));
        let mut entries = Vec::new();
        for (bucket_index, bucket) in raw_buckets.iter().enumerate() {
            let Some(bucket_map) = bucket.as_object() else {
                continue;
            };
            let Some(remaining) = bucket_remaining(bucket_map) else {
                continue;
            };
            let (order, window_label, window_short) =
                window_meta(bucket_map.get("window"), bucket_map.get("name"));
            let bucket_id = bucket_map
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("group-{group_index}-{window_short}-{bucket_index}"));
            let window_key = bucket_map
                .get("window")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_lowercase())
                .unwrap_or_default();
            let utilization = (1.0 - remaining).clamp(0.0, 1.0);
            let resets_at = parse_iso_datetime(bucket_map.get("reset_time"));
            entries.push(ParsedBucket {
                order,
                bucket_index,
                bucket_id,
                window_key,
                window_label,
                window_short,
                utilization,
                resets_at,
            });
        }
        if !entries.is_empty() {
            entries.sort_by_key(|item| (item.order, item.bucket_index));
            parsed_groups.push((group_name, entries));
        }
    }

    if parsed_groups.is_empty() {
        return Err(AntigravityUsageError::Empty(
            "Antigravity CLI returned no usage windows.".to_string(),
        ));
    }

    let multiple_groups = parsed_groups.len() > 1;
    let mut limits = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut primary_key: Option<String> = None;

    for (group_name, entries) in parsed_groups {
        let short_group = group_name.as_deref().map(replace_whitespace_and_ci);
        for entry in entries {
            let mut key = format!("antigravity:{}", entry.bucket_id);
            let mut suffix = 2usize;
            while seen_keys.contains(&key) {
                key = format!("antigravity:{}-{suffix}", entry.bucket_id);
                suffix += 1;
            }
            seen_keys.insert(key.clone());

            let (label, short_label) = match (
                multiple_groups,
                group_name.as_deref(),
                short_group.as_deref(),
            ) {
                (true, Some(g_name), Some(s_group)) => (
                    format!("{} · {g_name}", entry.window_label),
                    format!("{s_group} {}", entry.window_short),
                ),
                _ => (entry.window_label, entry.window_short),
            };

            if primary_key.is_none() && entry.window_key == "5h" {
                primary_key = Some(key.clone());
            }

            limits.push(UsageLimit {
                key,
                label,
                short_label,
                utilization: entry.utilization,
                resets_at: entry.resets_at,
                severity: if entry.utilization >= 1.0 {
                    "reached".to_string()
                } else {
                    "normal".to_string()
                },
                is_active: true,
            });
        }
    }

    let primary_key = primary_key.or_else(|| limits.first().map(|limit| limit.key.clone()));

    Ok(Usage {
        limits,
        fetched_at: Utc::now(),
        primary_key,
        details: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    const SAMPLE_PAYLOAD_JSON: &str = include_str!("../tests/fixtures/sample_usage.json");

    fn sample_payload() -> Value {
        serde_json::from_str(SAMPLE_PAYLOAD_JSON).unwrap()
    }

    fn assert_approx_eq(left: f64, right: f64) {
        assert!(
            (left - right).abs() < 1e-4,
            "assertion failed: {left} != {right}"
        );
    }

    #[test]
    fn test_parses_multi_group_usage_and_orders_short_windows_first() {
        let usage = parse_usage_response(&sample_payload()).unwrap();

        assert_eq!(
            usage
                .limits
                .iter()
                .map(|item| item.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "antigravity:gemini-5h",
                "antigravity:gemini-weekly",
                "antigravity:3p-5h",
                "antigravity:3p-weekly",
                "antigravity:gemini-argon-wtu-weekly",
            ]
        );
        assert_eq!(
            usage
                .limits
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "5-hour window · Gemini",
                "Weekly window · Gemini",
                "5-hour window · Claude and GPT",
                "Weekly window · Claude and GPT",
                "Weekly window · Gemini 4 Argon",
            ]
        );
        assert_eq!(
            usage
                .limits
                .iter()
                .map(|item| item.short_label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Gemini 5h",
                "Gemini 7d",
                "Claude/GPT 5h",
                "Claude/GPT 7d",
                "Gemini 4 Argon 7d",
            ]
        );
        assert_eq!(usage.primary().unwrap().key, "antigravity:gemini-5h");
        assert_approx_eq(usage.limits[0].utilization, 0.0263);
        assert_approx_eq(usage.limits[1].utilization, 0.0075);
        assert_approx_eq(usage.limits[2].utilization, 0.6);
        assert_eq!(usage.limits[3].utilization, 0.0);
        assert_eq!(
            usage.limits[0].resets_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 20, 19, 32, 20).unwrap())
        );
    }

    #[test]
    fn test_single_group_omits_group_suffix_and_falls_back_primary_to_first() {
        let usage = parse_usage_response(&json!({
            "status": "SUCCESS",
            "command": {
                "name": "usage",
                "data": {
                    "groups": [
                        {
                            "name": "Gemini Models",
                            "buckets": [
                                {
                                    "id": "gemini-weekly",
                                    "window": "weekly",
                                    "remaining_fraction": 0.25,
                                    "reset_time": "2026-11-27T14:32:20Z"
                                }
                            ]
                        }
                    ]
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.limits.len(), 1);
        assert_eq!(usage.limits[0].label, "Weekly window");
        assert_eq!(usage.limits[0].short_label, "7d");
        assert_eq!(usage.primary().unwrap().key, "antigravity:gemini-weekly");
        assert_approx_eq(usage.primary().unwrap().utilization, 0.75);
    }

    #[test]
    fn test_omitted_remaining_fraction_with_reset_time_means_exhausted() {
        let usage = parse_usage_response(&json!({
            "status": "SUCCESS",
            "command": {
                "name": "usage",
                "data": {
                    "groups": [
                        {
                            "name": "Gemini Models",
                            "buckets": [
                                {
                                    "id": "gemini-5h",
                                    "window": "5h",
                                    "reset_time": "2026-11-20T19:32:20Z"
                                }
                            ]
                        }
                    ]
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.primary().unwrap().utilization, 1.0);
        assert_eq!(usage.primary().unwrap().severity, "reached");
    }

    #[test]
    fn test_rejects_incompatible_and_empty_payloads() {
        for bad in [
            json!(null),
            json!({}),
            json!({"command": "usage"}),
            json!({"command": {"name": "credits", "data": {"groups": []}}}),
            json!({"command": {"name": "usage", "data": {}}}),
        ] {
            let err = parse_usage_response(&bad).unwrap_err();
            assert!(
                matches!(err, AntigravityUsageError::Payload(_)),
                "expected Payload error for {bad:?}, got {err:?}"
            );
        }

        for empty in [
            json!({"command": {"name": "usage", "data": {"groups": []}}}),
            json!({"command": {"name": "usage", "data": {"groups": [{"name": "G", "buckets": [{"remaining_fraction": true}]}]}}}),
        ] {
            let err = parse_usage_response(&empty).unwrap_err();
            assert!(
                matches!(err, AntigravityUsageError::Empty(_)),
                "expected Empty error for {empty:?}, got {err:?}"
            );
        }
    }
}
