use chrono::Utc;
use serde_json::{Map, Value};
use ureq::http::StatusCode;
use widget_common::{ProviderError, Usage, UsageLimit, utils::parse_iso_datetime};

use crate::auth::{AuthError, OAuthToken, refresh_token};
use crate::config::USAGE_URL;
use crate::http::{ClaudeRequest, ClaudeResponse, usage_request};
use widget_common::utils::truthy;

pub fn fetch_usage(
    mut token: OAuthToken,
    send: &impl Fn(ClaudeRequest) -> Result<ClaudeResponse, ProviderError>,
) -> Result<(Usage, OAuthToken), AuthError> {
    if token.is_expired(Utc::now().timestamp_millis()) {
        token = refresh_token(token, send)?;
    }
    let mut response = send(usage_request(&token.access_token())?)?;
    if response.status == StatusCode::UNAUTHORIZED {
        token = refresh_token(token, send)?;
        response = send(usage_request(&token.access_token())?)?;
    }
    let limits = parse_limits(&response.json(USAGE_URL)?)?;
    Ok((
        Usage {
            limits,
            fetched_at: Utc::now(),
            primary_key: Some("session".into()),
            details: Vec::new(),
        },
        token,
    ))
}

fn parse_limits(payload: &Value) -> Result<Vec<UsageLimit>, ProviderError> {
    let payload = payload
        .as_object()
        .ok_or_else(|| ProviderError::new("TypeError: Expected a usage response object"))?;
    let mut limits = Vec::new();
    match payload.get("limits").filter(|value| truthy(value)) {
        Some(Value::Array(items)) => {
            for item in items.iter().filter_map(Value::as_object) {
                let kind = string_or(item.get("kind"), "")?;
                let utilization = percentage(item.get("percent"))?;
                let (label, short_label) = limit_labels(kind, item.get("scope"))?;
                limits.push(UsageLimit {
                    key: kind.to_owned(),
                    label,
                    short_label,
                    utilization,
                    resets_at: parse_iso_datetime(item.get("resets_at")),
                    severity: string_or(item.get("severity"), "normal")?.to_owned(),
                    is_active: item.get("is_active").is_some_and(truthy),
                });
            }
        }
        None | Some(Value::Object(_) | Value::String(_)) => {}
        _ => {
            return Err(ProviderError::new(
                "TypeError: Expected iterable usage limits",
            ));
        }
    }
    if !limits.is_empty() {
        return Ok(limits);
    }
    for (key, kind) in [("five_hour", "session"), ("seven_day", "weekly_all")] {
        let block = object_or_empty(payload.get(key))?;
        let (label, short_label) = limit_labels(kind, None)?;
        limits.push(UsageLimit {
            key: key.to_owned(),
            label,
            short_label,
            utilization: percentage(block.and_then(|block| block.get("utilization")))?,
            resets_at: parse_iso_datetime(block.and_then(|block| block.get("resets_at"))),
            severity: "normal".to_owned(),
            is_active: true,
        });
    }
    Ok(limits)
}

fn limit_labels(kind: &str, scope: Option<&Value>) -> Result<(String, String), ProviderError> {
    Ok(match kind {
        "session" => ("5-hour window".to_owned(), "5h".to_owned()),
        "weekly_all" => ("7-day window".to_owned(), "7d".to_owned()),
        "weekly_scoped" => {
            let scope = object_or_empty(scope)?;
            let model = object_or_empty(scope.and_then(|scope| scope.get("model")))?;
            let name = string_or(model.and_then(|model| model.get("display_name")), "scoped")?;
            (format!("7-day · {name}"), name.to_owned())
        }
        "" => ("Limit".to_owned(), "Limit".to_owned()),
        _ => {
            let pretty = title_case(&kind.replace('_', " "));
            (pretty.clone(), pretty)
        }
    })
}

fn title_case(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut previous_cased = false;
    for character in text.chars() {
        if !previous_cased && character == 'ß' {
            output.push_str("Ss");
        } else if previous_cased {
            output.extend(character.to_lowercase());
        } else {
            output.extend(character.to_uppercase());
        }
        previous_cased = character.is_lowercase() || character.is_uppercase();
    }
    output
}

fn percentage(value: Option<&Value>) -> Result<f64, ProviderError> {
    let value = match value.filter(|value| truthy(value)) {
        None => Some(0.0),
        Some(Value::Bool(flag)) => Some(f64::from(u8::from(*flag))),
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    value
        .map(|value| value / 100.0)
        .ok_or_else(|| ProviderError::new("ValueError: Invalid usage percentage"))
}

fn object_or_empty(value: Option<&Value>) -> Result<Option<&Map<String, Value>>, ProviderError> {
    value
        .filter(|value| truthy(value))
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| ProviderError::new("TypeError: Expected a usage limit object"))
        })
        .transpose()
}

fn string_or<'a>(value: Option<&'a Value>, default: &'a str) -> Result<&'a str, ProviderError> {
    match value.filter(|value| truthy(value)) {
        None => Ok(default),
        Some(value) => value
            .as_str()
            .ok_or_else(|| ProviderError::new("TypeError: Expected a usage limit string")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(payload: &str) -> Vec<UsageLimit> {
        parse_limits(&serde_json::from_str(payload).unwrap()).unwrap()
    }

    #[test]
    fn parses_current_limits_array() {
        let limits = fixture(include_str!("../tests/fixtures/limits-current.json"));
        assert_eq!(
            limits.iter().map(|v| v.key.as_str()).collect::<Vec<_>>(),
            ["session", "weekly_scoped"]
        );
        assert_eq!(limits[0].label, "5-hour window");
        assert_eq!(limits[0].short_label, "5h");
        assert!((limits[0].utilization - 0.37).abs() < f64::EPSILON);
        assert_eq!(
            limits[0].resets_at.unwrap().to_rfc3339(),
            "2026-09-21T18:30:00+00:00"
        );
        assert!(limits[0].is_active);
        assert_eq!(limits[1].label, "7-day · Opus");
        assert_eq!(limits[1].short_label, "Opus");
        assert_eq!(limits[1].resets_at, None);
        assert!(!limits[1].is_active);
        assert_eq!(limits[1].severity, "normal");
    }

    #[test]
    fn parses_legacy_blocks() {
        let limits = fixture(include_str!("../tests/fixtures/limits-legacy.json"));
        assert_eq!(
            limits.iter().map(|v| v.key.as_str()).collect::<Vec<_>>(),
            ["five_hour", "seven_day"]
        );
        assert!((limits[0].utilization - 0.125).abs() < f64::EPSILON);
        assert_eq!(limits[1].resets_at, None);
    }

    #[test]
    fn unknown_limit_kind_is_kept() {
        let limits = fixture(include_str!("../tests/fixtures/limits-unknown.json"));
        let [limit] = limits.as_slice() else {
            panic!("expected one limit");
        };
        assert_eq!(limit.label, "Future Quota");
        assert_eq!(limit.utilization, 0.0);
    }

    #[test]
    fn skips_non_objects_and_keeps_unclamped_limits() {
        let limits = parse_limits(&json!({
            "limits": [
                false,
                {"kind": "weekly_all", "percent": "140.5", "severity": "reached", "is_active": "yes"},
                {"kind": "weekly_scoped", "scope": {"model": {}}},
                {}
            ]
        }))
        .unwrap();
        assert_eq!(limits.len(), 3);
        assert_eq!(limits[0].label, "7-day window");
        assert_eq!(limits[0].utilization, 1.405);
        assert_eq!(limits[0].severity, "reached");
        assert!(limits[0].is_active);
        assert_eq!(limits[1].label, "7-day · scoped");
        assert_eq!(limits[2].label, "Limit");

        let legacy = parse_limits(&json!({"limits": [null, false]})).unwrap();
        assert_eq!(legacy[0].key, "five_hour");
        assert_eq!(legacy[1].key, "seven_day");
        assert!(legacy.iter().all(|limit| limit.is_active));
    }
}
