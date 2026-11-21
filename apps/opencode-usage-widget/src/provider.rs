use std::path::PathBuf;
use widget_common::http::{Client, DEFAULT_TIMEOUTS};

use widget_common::{ProviderError, Usage, UsageProvider};

use crate::api::fetch_usage;
use crate::auth::{default_auth_path, load_api_key};
use crate::error::OpenCodeError;

pub struct OpenCodeProvider {
    auth_path: PathBuf,
    client: Client,
}

impl OpenCodeProvider {
    pub fn new() -> Self {
        Self {
            auth_path: default_auth_path(),
            client: Client::new(DEFAULT_TIMEOUTS),
        }
    }

    fn fetch_with<L, F>(&self, load: L, fetch: F) -> Result<Usage, ProviderError>
    where
        L: FnOnce() -> Result<String, OpenCodeError>,
        F: FnOnce(&str) -> Result<Usage, OpenCodeError>,
    {
        load()
            .and_then(|api_key| fetch(&api_key))
            .map_err(provider_error)
    }
}

impl UsageProvider for OpenCodeProvider {
    fn fetch_usage(&self) -> Result<Usage, ProviderError> {
        self.fetch_with(
            || load_api_key(&self.auth_path),
            |key| fetch_usage(&self.client, key),
        )
    }
}

fn provider_error(error: OpenCodeError) -> ProviderError {
    let message = match error {
        OpenCodeError::Credential(_) => {
            "OpenCode Go is not connected. Run OpenCode, use '/connect', and select OpenCode Go."
                .to_string()
        }
        OpenCodeError::Api(401) => {
            "OpenCode Go rejected the saved API key. Reconnect it with '/connect'.".to_string()
        }
        OpenCodeError::Api(403) => "An active OpenCode Go subscription is required.".to_string(),
        OpenCodeError::Api(429) => {
            "OpenCode Go usage is temporarily rate limited. The widget will retry.".to_string()
        }
        OpenCodeError::Api(status) if status >= 500 => {
            "OpenCode Go usage is temporarily unavailable. The widget will retry.".to_string()
        }
        OpenCodeError::Api(status) => {
            format!("OpenCode Go could not return usage data (HTTP {status}).")
        }
        OpenCodeError::Payload(_) => {
            "OpenCode Go returned an incompatible usage response.".to_string()
        }
        OpenCodeError::Timeout => "OpenCode Go usage timed out. The widget will retry.".to_string(),
        OpenCodeError::Transport => {
            "OpenCode Go usage could not be reached. The widget will retry.".to_string()
        }
    };
    ProviderError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maps_auth_and_endpoint_errors_to_safe_messages() {
        let provider = OpenCodeProvider::new();
        let cases = [
            (
                OpenCodeError::Credential("missing"),
                "OpenCode Go is not connected. Run OpenCode, use '/connect', and select OpenCode Go.",
            ),
            (
                OpenCodeError::Api(401),
                "OpenCode Go rejected the saved API key. Reconnect it with '/connect'.",
            ),
            (
                OpenCodeError::Api(403),
                "An active OpenCode Go subscription is required.",
            ),
            (
                OpenCodeError::Api(429),
                "OpenCode Go usage is temporarily rate limited. The widget will retry.",
            ),
            (
                OpenCodeError::Api(503),
                "OpenCode Go usage is temporarily unavailable. The widget will retry.",
            ),
            (
                OpenCodeError::Payload("bad"),
                "OpenCode Go returned an incompatible usage response.",
            ),
            (
                OpenCodeError::Timeout,
                "OpenCode Go usage timed out. The widget will retry.",
            ),
            (
                OpenCodeError::Transport,
                "OpenCode Go usage could not be reached. The widget will retry.",
            ),
        ];

        for (error, expected) in cases {
            let result = provider.fetch_with(|| Ok("secret".to_string()), |_| Err(error));
            assert_eq!(result.unwrap_err().to_string(), expected);
        }
    }
}
