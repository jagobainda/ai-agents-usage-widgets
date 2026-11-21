use widget_common::http::{Client, DEFAULT_TIMEOUTS};
use widget_common::{ProviderError, Usage, UsageProvider};

use crate::api::fetch_usage;
use crate::auth::{KimiSettings, OAuthToken, load_settings, load_token};
use crate::error::KimiError;

pub struct KimiProvider {
    client: Client,
}

impl KimiProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(DEFAULT_TIMEOUTS),
        }
    }

    fn fetch_with<S, T, F>(
        &self,
        mut settings_loader: S,
        mut token_loader: T,
        mut usage_fetcher: F,
    ) -> Result<Usage, ProviderError>
    where
        S: FnMut() -> Result<KimiSettings, KimiError>,
        T: FnMut(&KimiSettings) -> Result<OAuthToken, KimiError>,
        F: FnMut(OAuthToken) -> Result<Usage, KimiError>,
    {
        let mut load = || token_loader(&settings_loader()?);
        let result = match load().and_then(&mut usage_fetcher) {
            Err(KimiError::Refresh(_)) => load().and_then(usage_fetcher),
            result => result,
        };
        result.map_err(provider_error)
    }
}

impl UsageProvider for KimiProvider {
    fn fetch_usage(&self) -> Result<Usage, ProviderError> {
        self.fetch_with(
            || load_settings(None),
            load_token,
            |token| fetch_usage(&self.client, token),
        )
    }
}

fn provider_error(error: KimiError) -> ProviderError {
    let message = match error {
        KimiError::Credential(_) => {
            "Kimi Code is not signed in. Run 'kimi login' and try again.".to_string()
        }
        KimiError::InvalidUrlType(_) => error.to_string(),
        KimiError::Refresh(_) => "Session expired. Sign in to Kimi Code again.".to_string(),
        KimiError::Api(401 | 403) => {
            "Kimi Code rejected the saved session. Sign in again.".to_string()
        }
        KimiError::Api(404) => "This Kimi Code account does not provide usage data.".to_string(),
        KimiError::Api(429) => {
            "Kimi Code usage is temporarily rate limited. The widget will retry.".to_string()
        }
        KimiError::Api(status) if status >= 500 => {
            "Kimi Code usage is temporarily unavailable. The widget will retry.".to_string()
        }
        KimiError::Api(status) => {
            format!("Kimi Code could not return usage data (HTTP {status}).")
        }
        KimiError::Payload(_) => "Kimi Code returned an incompatible usage response.".to_string(),
        KimiError::Timeout => "Kimi Code usage timed out. The widget will retry.".to_string(),
        KimiError::Transport => {
            "Kimi Code usage could not be reached. The widget will retry.".to_string()
        }
        KimiError::Io(error) => format!("OSError: {error}"),
    };
    ProviderError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tests::make_token;
    use crate::config::{DEFAULT_BASE_URL, DEFAULT_OAUTH_HOST};
    use std::path::Path;
    use tempfile::tempdir;

    fn settings(path: &Path) -> KimiSettings {
        KimiSettings {
            home: path.to_path_buf(),
            base_url: DEFAULT_BASE_URL.to_string(),
            oauth_host: DEFAULT_OAUTH_HOST.to_string(),
            credential_name: "kimi-code".to_string(),
        }
    }

    #[test]
    fn test_maps_auth_api_and_network_errors_to_safe_messages() {
        let directory = tempdir().unwrap();
        let provider = KimiProvider::new();
        type ErrorCase = (fn() -> KimiError, &'static str);
        let cases: [ErrorCase; 8] = [
            (
                || KimiError::Refresh("expired"),
                "Session expired. Sign in to Kimi Code again.",
            ),
            (
                || KimiError::Api(403),
                "Kimi Code rejected the saved session. Sign in again.",
            ),
            (
                || KimiError::Api(404),
                "This Kimi Code account does not provide usage data.",
            ),
            (
                || KimiError::Api(429),
                "Kimi Code usage is temporarily rate limited. The widget will retry.",
            ),
            (
                || KimiError::Api(503),
                "Kimi Code usage is temporarily unavailable. The widget will retry.",
            ),
            (
                || KimiError::Payload("bad"),
                "Kimi Code returned an incompatible usage response.",
            ),
            (
                || KimiError::Timeout,
                "Kimi Code usage timed out. The widget will retry.",
            ),
            (
                || KimiError::Transport,
                "Kimi Code usage could not be reached. The widget will retry.",
            ),
        ];
        for (make_error, expected) in cases {
            let result = provider.fetch_with(
                || Ok(settings(directory.path())),
                |_| Ok(make_token(directory.path())),
                |_| Err(make_error()),
            );
            assert_eq!(result.unwrap_err().to_string(), expected);
        }
    }

    #[test]
    fn test_missing_credentials_prompt_for_login() {
        let result = KimiProvider::new().fetch_with(
            || Err(KimiError::Credential("missing")),
            |_| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Kimi Code is not signed in. Run 'kimi login' and try again."
        );
    }
}
