use chrono::Utc;
use serde_json::Value;
use widget_common::utils::parse_iso_datetime;
use widget_common::{Usage, UsageLimit};

use crate::config::USAGE_URL;
use crate::error::OpenCodeError;
use widget_common::http::{Client, DEFAULT_TIMEOUTS, Response};

const WINDOWS: [(&str, &str, &str); 3] = [
    ("rolling", "5-hour window", "5h"),
    ("weekly", "Weekly window", "7d"),
    ("monthly", "Monthly window", "month"),
];

pub fn parse_usage_response(payload: &Value) -> Result<Usage, OpenCodeError> {
    let container = payload.get("usage").and_then(Value::as_object);
    let mut limits = Vec::new();
    if let Some(container) = container {
        for (key, label, short_label) in WINDOWS {
            let Some(window) = container.get(key).and_then(Value::as_object) else {
                continue;
            };
            let Some(utilization) = percentage(window.get("percent")) else {
                continue;
            };
            limits.push(UsageLimit {
                key: format!("opencode-go:{key}"),
                label: label.to_string(),
                short_label: short_label.to_string(),
                utilization,
                resets_at: parse_iso_datetime(window.get("resetsAt")),
                severity: if window.get("status").and_then(Value::as_str) == Some("rate-limited") {
                    "reached"
                } else {
                    "normal"
                }
                .to_string(),
                is_active: true,
            });
        }
    }
    if limits.is_empty() {
        return Err(OpenCodeError::Payload(
            "OpenCode Go returned no usable usage windows.",
        ));
    }
    Ok(Usage {
        limits,
        fetched_at: Utc::now(),
        primary_key: Some("opencode-go:rolling".to_string()),
        details: vec!["Plan · OpenCode Go".to_string()],
    })
}

fn percentage(value: Option<&Value>) -> Option<f64> {
    let percent = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !percent.is_finite() {
        return None;
    }
    Some(percent.clamp(0.0, 100.0) / 100.0)
}

pub fn fetch_usage(client: &Client, api_key: &str) -> Result<Usage, OpenCodeError> {
    fetch_usage_with(api_key, |url, headers, _| {
        client.get(url, headers).map_err(Into::into)
    })
}

pub fn fetch_usage_with<F>(api_key: &str, mut get: F) -> Result<Usage, OpenCodeError>
where
    F: FnMut(&str, &[(&str, &str)], (u64, u64)) -> Result<Response, OpenCodeError>,
{
    let authorization = format!("Bearer {api_key}");
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Accept", "application/json"),
        ("User-Agent", "OpenCodeUsageWidget/1.0"),
    ];
    let response = get(USAGE_URL, &headers, DEFAULT_TIMEOUTS)?;
    if response.status_code != 200 {
        return Err(OpenCodeError::Api(response.status_code));
    }
    let payload = serde_json::from_str(&response.body)
        .map_err(|_| OpenCodeError::Payload("OpenCode Go returned invalid JSON."))?;
    parse_usage_response(&payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(status_code: u16, body: &str) -> Result<Response, OpenCodeError> {
        Ok(Response {
            status_code,
            body: body.to_string(),
        })
    }

    #[test]
    fn test_parses_all_windows_in_stable_order() {
        let payload = json!({
            "usage": {
                "monthly": {"status": "ok", "percent": 6, "resetsAt": "2026-10-01T00:00:00.000Z"},
                "rolling": {"status": "ok", "percent": 3, "resetsAt": "2026-09-22T16:00:00.000Z"},
                "weekly": {"status": "rate-limited", "percent": 100, "resetsAt": null}
            }
        });

        let usage = parse_usage_response(&payload).unwrap();
        let keys: Vec<_> = usage.limits.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "opencode-go:rolling",
                "opencode-go:weekly",
                "opencode-go:monthly"
            ]
        );
        assert_eq!(usage.primary().unwrap().key, "opencode-go:rolling");
        assert_eq!(usage.limits[1].severity, "reached");
        assert_eq!(usage.details, ["Plan · OpenCode Go"]);
        assert_eq!(
            usage.limits[0].resets_at.unwrap().to_rfc3339(),
            "2026-09-22T16:00:00+00:00"
        );
        assert_eq!(usage.limits[1].resets_at, None);
    }

    #[test]
    fn test_whole_percent_values_are_not_treated_as_fractions() {
        let usage = parse_usage_response(&json!({
            "usage": {
                "rolling": {"percent": 0},
                "weekly": {"percent": 1},
                "monthly": {"percent": 100}
            }
        }))
        .unwrap();
        let values: Vec<_> = usage.limits.iter().map(|item| item.utilization).collect();
        assert_eq!(values, [0.0, 0.01, 1.0]);
    }

    #[test]
    fn test_percentages_are_clamped_and_unknown_fields_ignored() {
        let usage = parse_usage_response(&json!({
            "usage": {
                "rolling": {"percent": -3, "future": true},
                "weekly": {"percent": 140},
                "monthly": {"percent": "invalid"},
                "futureWindow": {"percent": 50}
            },
            "newTopLevel": true
        }))
        .unwrap();
        let values: Vec<_> = usage.limits.iter().map(|item| item.utilization).collect();
        assert_eq!(values, [0.0, 1.0]);
    }

    #[test]
    fn test_no_usable_windows_is_an_incompatible_payload() {
        for payload in [
            Value::Null,
            json!({}),
            json!({"usage": {}}),
            json!({"usage": {"rolling": {"percent": true}}}),
        ] {
            assert!(matches!(
                parse_usage_response(&payload),
                Err(OpenCodeError::Payload(_))
            ));
        }
    }

    #[test]
    fn test_fetch_sends_bearer_key_and_timeouts() {
        let mut captured_url = None;
        let mut captured_headers = None;
        let mut captured_timeouts = None;
        let usage = fetch_usage_with("secret", |url, headers, timeouts| {
            captured_url = Some(url.to_string());
            captured_headers = Some(
                headers
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect::<Vec<_>>(),
            );
            captured_timeouts = Some(timeouts);
            response(200, r#"{"usage":{"rolling":{"percent":12}}}"#)
        })
        .unwrap();

        assert_eq!(usage.primary().unwrap().utilization, 0.12);
        assert_eq!(captured_url.as_deref(), Some(USAGE_URL));
        assert_eq!(captured_timeouts, Some((5, 15)));
        let headers = captured_headers.unwrap();
        assert!(headers.contains(&("Authorization".to_string(), "Bearer secret".to_string())));
        assert!(headers.contains(&("Accept".to_string(), "application/json".to_string())));
        assert!(headers.contains(&(
            "User-Agent".to_string(),
            "OpenCodeUsageWidget/1.0".to_string()
        )));
    }

    #[test]
    fn test_non_200_and_invalid_json_are_classified() {
        let error = fetch_usage_with("secret", |_, _, _| response(403, "")).unwrap_err();
        assert_eq!(error, OpenCodeError::Api(403));
        let error = fetch_usage_with("secret", |_, _, _| response(200, "bad json")).unwrap_err();
        assert_eq!(
            error,
            OpenCodeError::Payload("OpenCode Go returned invalid JSON.")
        );
    }
}
