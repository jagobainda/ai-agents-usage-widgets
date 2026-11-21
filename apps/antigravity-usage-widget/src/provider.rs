use serde_json::Value;
use widget_common::{ProviderError, Usage, UsageProvider};

use crate::cli::{AntigravityCLIClient, AntigravityCLIError};
use crate::usage::{AntigravityUsageError, parse_usage_response};

pub trait CliClient: Send + Sync {
    fn fetch_usage_payload(&self) -> Result<Value, AntigravityCLIError>;
    fn close(&self) {}
}

impl CliClient for AntigravityCLIClient {
    fn fetch_usage_payload(&self) -> Result<Value, AntigravityCLIError> {
        AntigravityCLIClient::fetch_usage_payload(self)
    }

    fn close(&self) {
        AntigravityCLIClient::close(self);
    }
}

fn cli_provider_error(error: &AntigravityCLIError) -> ProviderError {
    let detail = format!("{} {}", error.message(), error.detail()).to_lowercase();
    if [
        "unauthorized",
        "unauthenticated",
        "authentication required",
        "not logged",
        "login",
        "sign in",
        "credential",
        "oauth",
    ]
    .iter()
    .any(|term| detail.contains(term))
    {
        return ProviderError::new(
            "Antigravity is not signed in. Run 'agy' to sign in and try again.",
        );
    }
    if ["429", "rate limit", "resource_exhausted"]
        .iter()
        .any(|term| detail.contains(term))
    {
        return ProviderError::new(
            "Antigravity usage is temporarily rate limited. The widget will retry.",
        );
    }
    if [
        "unknown flag",
        "flag provided but not defined",
        "unknown command",
        "unsupported output format",
    ]
    .iter()
    .any(|term| detail.contains(term))
    {
        return ProviderError::new(
            "This Antigravity CLI version does not support JSON usage output. Run 'agy update'.",
        );
    }
    if [
        "connection",
        "network",
        "dns",
        "offline",
        "unreachable",
        "dial tcp",
        "deadline exceeded",
        "timeout",
        "unavailable",
    ]
    .iter()
    .any(|term| detail.contains(term))
    {
        return ProviderError::new(
            "Antigravity usage could not be reached. The widget will retry.",
        );
    }
    ProviderError::new("Antigravity CLI could not return usage data. The widget will retry.")
}

pub struct AntigravityProvider {
    client: Box<dyn CliClient>,
}

impl AntigravityProvider {
    pub fn new() -> Self {
        Self::with_client(Box::new(AntigravityCLIClient::new()))
    }

    pub fn with_client(client: Box<dyn CliClient>) -> Self {
        Self { client }
    }
}

impl UsageProvider for AntigravityProvider {
    fn fetch_usage(&self) -> Result<Usage, ProviderError> {
        let payload = self.client.fetch_usage_payload().map_err(|err| match err {
            AntigravityCLIError::NotFound(_) => ProviderError::new(
                "Antigravity CLI is not installed or is not in PATH. Install 'agy' first.",
            ),
            AntigravityCLIError::Timeout(_) => {
                ProviderError::new("Antigravity CLI timed out. The widget will retry.")
            }
            AntigravityCLIError::Payload(_) => {
                ProviderError::new("Antigravity CLI returned an incompatible usage response.")
            }
            other @ (AntigravityCLIError::Cli { .. } | AntigravityCLIError::Closed(_)) => {
                cli_provider_error(&other)
            }
        })?;

        parse_usage_response(&payload).map_err(|err| match err {
            AntigravityUsageError::Payload(_) => {
                ProviderError::new("Antigravity CLI returned an incompatible usage response.")
            }
            AntigravityUsageError::Empty(_) => {
                ProviderError::new("Antigravity CLI returned no usage windows.")
            }
        })
    }

    fn close(&self) {
        self.client.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingClient {
        error: AntigravityCLIError,
    }

    impl CliClient for FailingClient {
        fn fetch_usage_payload(&self) -> Result<Value, AntigravityCLIError> {
            Err(self.error.clone())
        }
    }

    #[test]
    fn test_maps_cli_errors_to_safe_messages() {
        let cases = [
            (
                AntigravityCLIError::NotFound("missing".to_string()),
                "Install 'agy'",
            ),
            (
                AntigravityCLIError::Timeout("slow".to_string()),
                "timed out",
            ),
            (
                AntigravityCLIError::Payload("bad json".to_string()),
                "incompatible",
            ),
            (
                AntigravityCLIError::cli("exit 1", "Authentication required: token ABC123"),
                "not signed in",
            ),
            (
                AntigravityCLIError::cli("exit 1", "HTTP 429 RESOURCE_EXHAUSTED"),
                "rate limited",
            ),
            (
                AntigravityCLIError::cli("exit 1", "flag provided but not defined: -output-format"),
                "agy update",
            ),
            (
                AntigravityCLIError::cli("exit 1", "dial tcp: lookup failed"),
                "could not be reached",
            ),
            (
                AntigravityCLIError::cli("exit 1", "internal secret stack"),
                "could not return usage data",
            ),
        ];

        for (error, expected) in cases {
            let provider = AntigravityProvider::with_client(Box::new(FailingClient { error }));
            let raised = provider.fetch_usage().unwrap_err();
            let message = raised.to_string();
            assert!(
                message.contains(expected),
                "expected {message:?} to contain {expected:?}"
            );
            assert!(!message.contains("ABC123"));
            assert!(!message.contains("internal secret"));
        }
    }
}
