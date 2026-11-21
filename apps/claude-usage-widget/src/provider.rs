use std::sync::Mutex;

use widget_common::{ProviderError, Usage, UsageProvider};

use crate::api;
use crate::auth::{AuthError, Credentials, OAuthToken};
use crate::http::{self, ClaudeRequest, ClaudeResponse};

pub struct ClaudeProvider {
    token: Mutex<Option<OAuthToken>>,
    credentials: Credentials,
    client: widget_common::http::Client,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            token: Mutex::new(None),
            credentials: Credentials::new(),
            client: http::client(),
        }
    }

    fn fetch_with(
        &self,
        load: impl Fn() -> Result<OAuthToken, AuthError>,
        send: impl Fn(ClaudeRequest) -> Result<ClaudeResponse, ProviderError>,
    ) -> Result<Usage, AuthError> {
        let cached = self
            .token
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let token = match cached {
            Some(token) => token,
            None => load()?,
        };
        let (usage, token) = match api::fetch_usage(token, &send) {
            Err(AuthError::RefreshRejected) => api::fetch_usage(load()?, &send)?,
            result => result?,
        };
        *self.token.lock().unwrap_or_else(|error| error.into_inner()) = Some(token);
        Ok(usage)
    }
}

impl UsageProvider for ClaudeProvider {
    fn fetch_usage(&self) -> Result<Usage, ProviderError> {
        self.fetch_with(
            || self.credentials.load(),
            |request| http::send(&self.client, request),
        )
        .map_err(provider_error)
    }
}

fn provider_error(error: AuthError) -> ProviderError {
    match error {
        AuthError::Missing => {
            ProviderError::new("Claude Code is not signed in. Run 'claude login' and try again.")
        }
        AuthError::RefreshRejected => {
            ProviderError::new("Session expired. Sign in to Claude Code again.")
        }
        AuthError::Other(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::{fs, path::Path};

    use tempfile::tempdir;
    use ureq::http::{Method, StatusCode};

    use super::*;

    const CURRENT: &str = include_str!("../tests/fixtures/limits-current.json");
    const LEGACY: &str = include_str!("../tests/fixtures/limits-legacy.json");

    fn credentials(home: &Path) -> Credentials {
        Credentials::from_paths(home, home.as_os_str(), home.as_os_str())
    }

    fn write_credentials(path: &Path, access: &str, refresh: &str, expires_at: i64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "claudeAiOauth": {
                    "accessToken": access,
                    "refreshToken": refresh,
                    "expiresAt": expires_at
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    struct Exchange {
        method: Method,
        token: &'static str,
        status: StatusCode,
        body: &'static str,
    }

    impl Exchange {
        fn get(token: &'static str, status: StatusCode, body: &'static str) -> Self {
            Self {
                method: Method::GET,
                token,
                status,
                body,
            }
        }

        fn post(token: &'static str, status: StatusCode, body: &'static str) -> Self {
            Self {
                method: Method::POST,
                token,
                status,
                body,
            }
        }
    }

    struct Script(RefCell<VecDeque<Exchange>>);

    impl Script {
        fn new(exchanges: impl IntoIterator<Item = Exchange>) -> Self {
            Self(RefCell::new(exchanges.into_iter().collect()))
        }

        fn send(&self, request: ClaudeRequest) -> Result<ClaudeResponse, ProviderError> {
            let next = self.0.borrow_mut().pop_front().expect("unexpected request");
            let request = request.as_http();
            let headers = request.headers();
            assert_eq!(request.method(), next.method);
            assert!(!headers.contains_key("connection"));
            if next.method == Method::GET {
                assert_eq!(request.uri(), "https://api.anthropic.com/api/oauth/usage");
                assert_eq!(headers["anthropic-beta"], "oauth-2025-04-20");
                assert_eq!(headers["user-agent"], "claude-code/2.0.31");
                assert!(headers["authorization"].is_sensitive());
                assert_eq!(
                    headers["authorization"].to_str().unwrap(),
                    format!("Bearer {}", next.token)
                );
                assert!(request.body().is_none());
            } else {
                assert_eq!(
                    request.uri(),
                    "https://console.anthropic.com/v1/oauth/token"
                );
                assert_eq!(headers["content-type"], "application/json");
                assert!(!headers.contains_key("user-agent"));
                assert!(!headers.contains_key("authorization"));
                assert_eq!(
                    request.body().as_ref().unwrap(),
                    &format!(
                        r#"{{"grant_type": "refresh_token", "refresh_token": "{}", "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e"}}"#,
                        next.token
                    )
                );
            }
            Ok(ClaudeResponse {
                status: next.status,
                body: next.body.as_bytes().to_vec(),
            })
        }

        fn assert_done(&self) {
            assert!(self.0.borrow().is_empty());
        }
    }

    #[test]
    fn refreshes_expired_credentials_and_retries_the_usage_request_once_after_401() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/.credentials.json");
        write_credentials(&path, "expired", "first", 0);
        let credentials = credentials(dir.path());
        let provider = ClaudeProvider::new();
        let script = Script::new([
            Exchange::post(
                "first",
                StatusCode::OK,
                r#"{"access_token":"one","refresh_token":"second","expires_in":"3600"}"#,
            ),
            Exchange::get("one", StatusCode::UNAUTHORIZED, "{}"),
            Exchange::post("second", StatusCode::OK, r#"{"access_token":"two"}"#),
            Exchange::get("two", StatusCode::OK, LEGACY),
        ]);
        let usage = provider
            .fetch_with(
                || credentials.load_with("tester", |_, _| panic!("unexpected keyring read")),
                |request| script.send(request),
            )
            .map_err(provider_error)
            .unwrap();
        script.assert_done();
        assert_eq!(usage.primary_key.as_deref(), Some("session"));
        assert_eq!(usage.primary().unwrap().key, "five_hour");
        assert!(usage.details.is_empty());
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["claudeAiOauth"]["accessToken"], "two");
        assert_eq!(saved["claudeAiOauth"]["refreshToken"], "second");
    }

    #[test]
    fn uses_the_cached_token_until_refresh_is_rejected_then_reloads_the_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/.credentials.json");
        write_credentials(&path, "cached", "previous", 4_102_444_800_000);
        let credentials = credentials(dir.path());
        let provider = ClaudeProvider::new();
        let reads = Cell::new(0);
        let load = || {
            reads.set(reads.get() + 1);
            credentials.load_with("tester", |_, _| panic!("unexpected keyring read"))
        };
        let initial = Script::new([Exchange::get("cached", StatusCode::OK, CURRENT)]);
        assert!(
            provider
                .fetch_with(load, |request| initial.send(request))
                .is_ok()
        );
        initial.assert_done();
        assert_eq!(reads.get(), 1);

        write_credentials(&path, "updated", "rotated", 4_102_444_800_000);
        let retry = Script::new([
            Exchange::get("cached", StatusCode::UNAUTHORIZED, "{}"),
            Exchange::post(
                "previous",
                StatusCode::BAD_REQUEST,
                r#"{"error":"invalid_grant"}"#,
            ),
            Exchange::get("updated", StatusCode::OK, CURRENT),
        ]);
        let usage = provider
            .fetch_with(load, |request| retry.send(request))
            .map_err(provider_error)
            .unwrap();
        assert_eq!(usage.primary().unwrap().key, "session");
        retry.assert_done();
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn reports_the_v1_messages_for_missing_credentials_and_rejected_refresh() {
        let dir = tempdir().unwrap();
        let credentials = credentials(dir.path());
        let provider = ClaudeProvider::new();
        let load = || credentials.load_with("tester", |_, _| None);
        assert_eq!(
            provider
                .fetch_with(load, |_| panic!("unexpected network request"))
                .map_err(provider_error)
                .unwrap_err()
                .0,
            "Claude Code is not signed in. Run 'claude login' and try again."
        );

        let path = dir.path().join(".claude/.credentials.json");
        write_credentials(&path, "expired", "invalid", 0);
        let rejected = Script::new([
            Exchange::post(
                "invalid",
                StatusCode::BAD_REQUEST,
                r#"{"error_description":"a secret echoed by a fake server"}"#,
            ),
            Exchange::post("invalid", StatusCode::BAD_REQUEST, "invalid response JSON"),
        ]);
        assert_eq!(
            provider
                .fetch_with(load, |request| rejected.send(request))
                .map_err(provider_error)
                .unwrap_err()
                .0,
            "Session expired. Sign in to Claude Code again."
        );
        rejected.assert_done();
    }

    #[test]
    fn a_second_usage_401_is_returned_without_another_refresh() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/.credentials.json");
        write_credentials(&path, "initial", "refresh", 4_102_444_800_000);
        let credentials = credentials(dir.path());
        let provider = ClaudeProvider::new();
        let script = Script::new([
            Exchange::get("initial", StatusCode::UNAUTHORIZED, "{}"),
            Exchange::post(
                "refresh",
                StatusCode::OK,
                r#"{"access_token":"replacement"}"#,
            ),
            Exchange::get("replacement", StatusCode::UNAUTHORIZED, "{}"),
        ]);
        let error = provider
            .fetch_with(
                || credentials.load_with("tester", |_, _| panic!("unexpected keyring read")),
                |request| script.send(request),
            )
            .map_err(provider_error)
            .unwrap_err();
        assert_eq!(
            error.0,
            "HTTPError: 401 Client Error: Unauthorized for url: https://api.anthropic.com/api/oauth/usage"
        );
        script.assert_done();
    }
}
