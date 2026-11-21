use chrono::Utc;
use serde_json::Value;
use widget_common::utils::{datetime_from_unix, duration_label, duration_short_label};
use widget_common::{Usage, UsageLimit};

use crate::value_text::display_value;
use widget_common::utils::truthy;

fn percentage(value: Option<&Value>) -> f64 {
    let value = match value {
        Some(Value::Number(value)) => value.as_f64(),
        Some(Value::String(text)) => parse_float(text.trim()),
        _ => None,
    };
    match value {
        Some(value) if value.is_nan() => 1.0,
        Some(value) => value.clamp(0.0, 100.0) / 100.0,
        None => 0.0,
    }
}

fn parse_float(text: &str) -> Option<f64> {
    if text.contains('_') {
        let bytes = text.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'_'
                && (index == 0
                    || !bytes[index - 1].is_ascii_digit()
                    || !bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
            {
                return None;
            }
        }
        return text.replace('_', "").parse().ok();
    }
    text.parse().ok()
}

fn readable_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut output = String::new();
    let mut previous_cased = false;
    for ch in value.chars() {
        let ch = if matches!(ch, '_' | '-') { ' ' } else { ch };
        let cased =
            ch.is_lowercase() || ch.is_uppercase() || ch.to_lowercase().ne(ch.to_uppercase());
        if previous_cased {
            output.extend(ch.to_lowercase());
        } else {
            let titlecase = match ch {
                'Ǆ' | 'ǅ' | 'ǆ' => Some('ǅ'),
                'Ǉ' | 'ǈ' | 'ǉ' => Some('ǈ'),
                'Ǌ' | 'ǋ' | 'ǌ' => Some('ǋ'),
                'Ǳ' | 'ǲ' | 'ǳ' => Some('ǲ'),
                _ => None,
            };
            if let Some(titlecase) = titlecase {
                output.push(titlecase);
            } else {
                let mut uppercase = ch.to_uppercase();
                output.extend(uppercase.next());
                output.extend(uppercase.flat_map(char::to_lowercase));
            }
        }
        previous_cased = cased;
    }
    Some(output)
}

fn snapshots(payload: &Value) -> Vec<(String, &Value)> {
    if let Some(buckets) = payload
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|buckets| !buckets.is_empty())
    {
        return buckets
            .iter()
            .filter(|(_, snapshot)| snapshot.as_object().is_some())
            .map(|(key, snapshot)| {
                let fallback = if key.is_empty() { "limit" } else { key };
                (limit_id(snapshot, fallback), snapshot)
            })
            .collect();
    }
    payload
        .get("rateLimits")
        .filter(|snapshot| snapshot.as_object().is_some())
        .map(|snapshot| (limit_id(snapshot, "codex"), snapshot))
        .into_iter()
        .collect()
}

fn limit_id(snapshot: &Value, fallback: &str) -> String {
    snapshot
        .get("limitId")
        .filter(|value| truthy(value))
        .map_or_else(|| fallback.to_owned(), display_value)
}

fn credits_detail(credits: &Value) -> Option<String> {
    if credits.get("unlimited").and_then(Value::as_bool) == Some(true) {
        return Some("Credits · Unlimited".into());
    }
    if credits.get("hasCredits").and_then(Value::as_bool) == Some(true) {
        let balance = credits.get("balance")?;
        if !balance.is_null() {
            return Some(format!("Credits · {}", display_value(balance)));
        }
    }
    None
}

pub fn parse_rate_limits_response(payload: &Value) -> Usage {
    let payload = payload
        .get("result")
        .filter(|result| result.as_object().is_some())
        .unwrap_or(payload);
    let snapshots = snapshots(payload);
    let mut limits = Vec::new();
    let mut primary_key = None;
    let mut plan = None;
    let mut credits = None;
    let mut any_limit_reached = false;
    for (index, (limit_id, snapshot)) in snapshots.iter().enumerate() {
        let is_codex = limit_id.eq_ignore_ascii_case("codex");
        let mut name = snapshot
            .get("limitName")
            .and_then(Value::as_str)
            .and_then(readable_name);
        if name.is_none() && snapshots.len() > 1 && !is_codex {
            name = readable_name(limit_id).or_else(|| Some(format!("Limit {}", index + 1)));
        }
        let reached = snapshot.get("rateLimitReachedType").is_some_and(truthy);
        any_limit_reached |= reached;

        if plan.is_none() {
            plan = snapshot
                .get("planType")
                .and_then(Value::as_str)
                .filter(|plan| !plan.is_empty())
                .map(|plan| format!("Plan · {}", readable_name(plan).as_deref().unwrap_or(plan)));
        }
        if credits.is_none() {
            credits = snapshot.get("credits").and_then(credits_detail);
        }

        let known_slots = ["primary", "secondary"];
        let known_windows = known_slots
            .into_iter()
            .filter_map(|slot| snapshot.get(slot).map(|window| (slot, window)));
        let future_windows =
            snapshot
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(slot, window)| {
                    (!known_slots.contains(&slot.as_str()) && window.get("usedPercent").is_some())
                        .then_some((slot.as_str(), window))
                });
        for (slot, window) in known_windows.chain(future_windows) {
            if window.as_object().is_none() {
                continue;
            }
            let minutes = window.get("windowDurationMins");
            let mut label = duration_label(minutes);
            let mut short_label = duration_short_label(minutes);
            if let Some(name) = &name {
                label.push_str(" · ");
                label.push_str(name);
                short_label.push(' ');
                short_label.push_str(name);
            }
            let key = format!("{limit_id}:{slot}");
            if slot == "primary" && (primary_key.is_none() || is_codex) {
                primary_key = Some(key.clone());
            }
            limits.push(UsageLimit {
                key,
                label,
                short_label,
                utilization: percentage(window.get("usedPercent")),
                resets_at: datetime_from_unix(window.get("resetsAt")),
                severity: if reached { "reached" } else { "normal" }.into(),
                is_active: true,
            });
        }
    }
    let mut details: Vec<_> = [plan, credits].into_iter().flatten().collect();
    if any_limit_reached {
        details.push("A usage limit has been reached".into());
    }
    Usage {
        limits,
        fetched_at: Utc::now(),
        primary_key,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(fixture: &str) -> Usage {
        parse_rate_limits_response(&serde_json::from_str(fixture).unwrap())
    }

    #[test]
    fn primary_and_secondary_windows() {
        let usage = parse(include_str!("../tests/fixtures/primary-and-secondary.json"));
        let labels: Vec<_> = usage
            .limits
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["5-hour window", "7-day window"]);
        assert_eq!(usage.primary().unwrap().key, "codex:primary");
        assert!((usage.limits[1].utilization - 0.60).abs() < f64::EPSILON);
        assert_eq!(usage.details, ["Plan · Plus"]);
    }

    #[test]
    fn multiple_limit_ids_are_not_duplicated_by_legacy_view() {
        let usage = parse(include_str!("../tests/fixtures/multiple-limit-ids.json"));
        assert_eq!(usage.limits.len(), 3);
        assert_eq!(usage.limits[0].utilization, 0.31);
        assert_eq!(usage.limits[2].label, "1-hour window · Fast Models");
    }

    #[test]
    fn missing_and_unknown_fields_are_tolerated() {
        let usage = parse(include_str!(
            "../tests/fixtures/missing-and-unknown-fields.json"
        ));
        assert_eq!(usage.limits.len(), 1);
        assert_eq!(usage.limits[0].label, "Usage window");
        assert_eq!(usage.limits[0].utilization, 0.0);
        assert!(usage.limits[0].resets_at.is_none());
    }

    #[test]
    fn future_window_field_is_rendered() {
        let usage = parse(include_str!("../tests/fixtures/future-window.json"));
        let keys: Vec<_> = usage.limits.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(keys, ["codex:primary", "codex:burst"]);
        assert_eq!(usage.limits[1].label, "30-minute window");
    }

    #[test]
    fn unix_timestamp_conversion() {
        let converted = datetime_from_unix(Some(&json!(0))).unwrap();
        assert_eq!(converted.timezone(), Utc);
        assert_eq!(converted.to_rfc3339(), "1970-01-01T00:00:00+00:00");
        assert_eq!(datetime_from_unix(Some(&json!("invalid"))), None);
    }

    #[test]
    fn window_labels_are_derived_from_duration() {
        let cases = [
            (Some(json!(15)), "15-minute window", "15m"),
            (Some(json!(60)), "1-hour window", "1h"),
            (Some(json!(300)), "5-hour window", "5h"),
            (Some(json!(1_440)), "1-day window", "1d"),
            (Some(json!(10_080)), "7-day window", "7d"),
            (Some(json!(20_160)), "2-week window", "2w"),
            (None, "Usage window", "usage"),
        ];
        for (minutes, long, short) in cases {
            assert_eq!(duration_label(minutes.as_ref()), long);
            assert_eq!(duration_short_label(minutes.as_ref()), short);
        }
    }

    #[test]
    fn preserves_bucket_and_future_window_order_and_prefers_codex_primary() {
        let usage = parse(include_str!("../tests/fixtures/ordered-buckets.json"));
        let keys: Vec<_> = usage.limits.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "z-first:primary",
                "z-first:z-future",
                "z-first:a-future",
                "codex:primary"
            ]
        );
        assert_eq!(usage.limits[0].label, "5-hour window · Z First");
        assert_eq!(usage.limits[1].utilization, 1.0);
        assert_eq!(usage.limits[2].utilization, 0.0);
        let primary = usage.primary().unwrap();
        assert_eq!(primary.key, "codex:primary");
        assert_eq!(primary.utilization, 0.15);
        assert_eq!(primary.severity, "reached");
        assert_eq!(
            usage.details,
            [
                "Plan · Plus",
                "Credits · 3",
                "A usage limit has been reached"
            ]
        );
    }
}
