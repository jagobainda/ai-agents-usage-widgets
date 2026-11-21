use chrono::Utc;
use serde_json::Value;
use widget_common::utils::{parse_iso_datetime, round_percent};
use widget_common::{Usage, UsageLimit};

use crate::auth::{OAuthToken, refresh_token_with};
use crate::error::KimiError;
use widget_common::http::{Client, DEFAULT_TIMEOUTS, Response};

const WINDOWS: [(&str, &str, &str); 3] = [
    ("limit_5h", "5-hour window", "5h"),
    ("limit_7d", "Weekly window", "7d"),
    ("limit_month_total", "Monthly window", "month"),
];

pub fn parse_usage_response(payload: &Value) -> Result<Usage, KimiError> {
    let container = payload.get("usages").and_then(Value::as_object);
    let mut limits = Vec::new();
    if let Some(container) = container {
        for (key, label, short_label) in WINDOWS {
            let Some(window) = container.get(key).and_then(Value::as_object) else {
                continue;
            };
            let Some(utilization) = ratio(window.get("used_ratio")) else {
                continue;
            };
            limits.push(UsageLimit {
                key: format!("kimi:{key}"),
                label: label.to_string(),
                short_label: short_label.to_string(),
                utilization,
                resets_at: parse_iso_datetime(window.get("reset_time")),
                severity: if utilization >= 1.0 {
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
        return Err(KimiError::Payload(
            "Kimi Code returned no usable usage windows.",
        ));
    }
    let mut details = Vec::new();
    let month_ratio = container
        .and_then(|container| container.get("limit_month_code"))
        .and_then(Value::as_object)
        .and_then(|month_code| ratio(month_code.get("used_ratio")));
    if let Some(ratio) = month_ratio
        && limits
            .iter()
            .any(|item| item.key == "kimi:limit_month_total")
    {
        let percent = round_percent(ratio);
        details.push(format!("Monthly · Kimi Code {percent}% used"));
    }
    Ok(Usage {
        limits,
        fetched_at: Utc::now(),
        primary_key: Some("kimi:limit_5h".to_string()),
        details,
    })
}

fn ratio(value: Option<&Value>) -> Option<f64> {
    let ratio = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !ratio.is_finite() {
        return None;
    }
    Some(ratio.clamp(0.0, 1.0))
}

pub fn fetch_usage(client: &Client, token: OAuthToken) -> Result<Usage, KimiError> {
    fetch_usage_with(
        token,
        current_unix_time(),
        |url, headers, _| client.get(url, headers).map_err(Into::into),
        |url, form, headers, _| client.post_form(url, form, headers).map_err(Into::into),
    )
}

pub fn fetch_usage_with<G, P>(
    mut token: OAuthToken,
    now: f64,
    mut get: G,
    mut post: P,
) -> Result<Usage, KimiError>
where
    G: FnMut(&str, &[(&str, &str)], (u64, u64)) -> Result<Response, KimiError>,
    P: FnMut(&str, &[(&str, &str)], &[(&str, &str)], (u64, u64)) -> Result<Response, KimiError>,
{
    if token.is_expired_at(now) {
        token = refresh_token_with(&token, now as i64, &mut post)?;
    }
    let mut response = request_usage(&token, &mut get)?;
    if response.status_code == 401 {
        token = refresh_token_with(&token, now as i64, &mut post)?;
        response = request_usage(&token, &mut get)?;
    }
    if response.status_code != 200 {
        return Err(KimiError::Api(response.status_code));
    }
    let payload = serde_json::from_str(&response.body)
        .map_err(|_| KimiError::Payload("Kimi Code returned invalid JSON."))?;
    parse_usage_response(&payload)
}

fn request_usage<G>(token: &OAuthToken, get: &mut G) -> Result<Response, KimiError>
where
    G: FnMut(&str, &[(&str, &str)], (u64, u64)) -> Result<Response, KimiError>,
{
    let authorization = format!("Bearer {}", token.access_token);
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Accept", "application/json"),
        ("User-Agent", "KimiUsageWidget/1.0"),
    ];
    get(&token.usage_url, &headers, DEFAULT_TIMEOUTS)
}

fn current_unix_time() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tests::make_token;
    use serde_json::json;
    use tempfile::tempdir;

    fn response(status_code: u16, body: &str) -> Result<Response, KimiError> {
        Ok(Response {
            status_code,
            body: body.to_string(),
        })
    }

    #[test]
    fn test_parses_available_windows_in_stable_order() {
        let usage = parse_usage_response(&json!({
            "usages": {
                "limit_month_total": {"used_ratio": 0.25, "reset_time": "2026-10-01T00:00:00Z"},
                "limit_month_code": {"used_ratio": 0.1},
                "limit_7d": {"used_ratio": 1.2, "reset_time": null},
                "limit_5h": {"used_ratio": 0.125, "reset_time": "2026-09-22T16:00:00Z"}
            }
        }))
        .unwrap();
        let keys: Vec<_> = usage.limits.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(
            keys,
            ["kimi:limit_5h", "kimi:limit_7d", "kimi:limit_month_total"]
        );
        assert_eq!(usage.primary().unwrap().utilization, 0.125);
        assert_eq!(usage.limits[1].utilization, 1.0);
        assert_eq!(usage.limits[1].severity, "reached");
        assert_eq!(usage.details, ["Monthly · Kimi Code 10% used"]);
        assert_eq!(
            usage.limits[0].resets_at.unwrap().to_rfc3339(),
            "2026-09-22T16:00:00+00:00"
        );
    }

    #[test]
    fn test_omits_missing_and_invalid_windows() {
        let usage = parse_usage_response(&json!({
            "usages": {"limit_5h": {"used_ratio": "0.4"}, "limit_7d": {"used_ratio": true}}
        }))
        .unwrap();
        assert_eq!(usage.limits.len(), 1);
        assert_eq!(usage.primary().unwrap().utilization, 0.4);
        for payload in [
            Value::Null,
            json!({}),
            json!({"usages": {}}),
            json!({"usages": {"limit_5h": {}}}),
        ] {
            assert!(matches!(
                parse_usage_response(&payload),
                Err(KimiError::Payload(_))
            ));
        }
    }

    #[test]
    fn test_fetch_sends_bearer_token_and_timeouts() {
        let directory = tempdir().unwrap();
        let token = make_token(directory.path());
        let mut captured_url = None;
        let mut captured_headers = None;
        let mut captured_timeouts = None;
        let usage = fetch_usage_with(
            token,
            1_700_000_000.0,
            |url, headers, timeouts| {
                captured_url = Some(url.to_string());
                captured_headers = Some(
                    headers
                        .iter()
                        .map(|(name, value)| (name.to_string(), value.to_string()))
                        .collect::<Vec<_>>(),
                );
                captured_timeouts = Some(timeouts);
                response(200, r#"{"usages":{"limit_5h":{"used_ratio":0.12}}}"#)
            },
            |_, _, _, _| unreachable!(),
        )
        .unwrap();

        assert_eq!(usage.primary().unwrap().utilization, 0.12);
        assert_eq!(
            captured_url.as_deref(),
            Some("https://api.kimi.com/coding/v1/usages")
        );
        assert_eq!(captured_timeouts, Some((5, 15)));
        let headers = captured_headers.unwrap();
        assert!(headers.contains(&("Authorization".to_string(), "Bearer access".to_string())));
        assert!(headers.contains(&("Accept".to_string(), "application/json".to_string())));
        assert!(headers.contains(&("User-Agent".to_string(), "KimiUsageWidget/1.0".to_string())));
    }

    #[test]
    fn test_unauthorized_refreshes_and_retries_once() {
        let directory = tempdir().unwrap();
        let token = make_token(directory.path());
        std::fs::create_dir(token.source_path.parent().unwrap()).unwrap();
        let mut calls = 0;
        let mut authorizations = Vec::new();
        let usage = fetch_usage_with(
            token,
            1_700_000_000.0,
            |_, headers, _| {
                calls += 1;
                authorizations.push(headers[0].1.to_string());
                if calls == 1 {
                    response(401, "")
                } else {
                    response(200, r#"{"usages":{"limit_5h":{"used_ratio":0.2}}}"#)
                }
            },
            |_, _, _, _| {
                response(
                    200,
                    r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600}"#,
                )
            },
        )
        .unwrap();

        assert_eq!(calls, 2);
        assert_eq!(authorizations, ["Bearer access", "Bearer new-access"]);
        assert_eq!(usage.primary().unwrap().utilization, 0.2);
    }

    #[test]
    fn test_non_200_and_invalid_json_are_classified() {
        let directory = tempdir().unwrap();
        let error = fetch_usage_with(
            make_token(directory.path()),
            1_700_000_000.0,
            |_, _, _| response(403, ""),
            |_, _, _, _| unreachable!(),
        )
        .unwrap_err();
        assert!(matches!(error, KimiError::Api(403)));
        let error = fetch_usage_with(
            make_token(directory.path()),
            1_700_000_000.0,
            |_, _, _| response(200, "bad json"),
            |_, _, _, _| unreachable!(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            KimiError::Payload("Kimi Code returned invalid JSON.")
        ));
    }
}
