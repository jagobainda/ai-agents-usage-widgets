use serde_json::{Value, json};
use widget_common::{ProviderError, Usage, UsageProvider};

use crate::app_server::{AppServerError, CodexAppServerClient};
use crate::usage::parse_rate_limits_response;

const SIGN_IN: &str = "Codex is not signed in. Run 'codex login' and try again.";

pub struct CodexProvider {
    client: CodexAppServerClient,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            client: CodexAppServerClient::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn with_client(client: CodexAppServerClient) -> Self {
        Self { client }
    }
}

impl UsageProvider for CodexProvider {
    fn fetch_usage(&self) -> Result<Usage, ProviderError> {
        let result = self
            .client
            .request("account/rateLimits/read", None, None)
            .map_err(provider_error)?;
        let usage = parse_rate_limits_response(&result);
        if !usage.limits.is_empty() {
            return Ok(usage);
        }

        let account = match self.client.request(
            "account/read",
            Some(json!({"refreshToken": false})),
            Some(0),
        ) {
            Ok(account) => Some(account),
            Err(AppServerError::Rpc { .. }) => None,
            Err(error) => return Err(provider_error(error)),
        };
        if let Some(account) = account
            && account.get("requiresOpenaiAuth").and_then(Value::as_bool) == Some(true)
            && account.get("account").is_none_or(|value| value.is_null())
        {
            return Err(ProviderError::new(SIGN_IN));
        }
        Err(ProviderError::new(
            "Codex App Server returned no usage windows.",
        ))
    }

    fn close(&self) {
        self.client.close();
    }
}

fn provider_error(error: AppServerError) -> ProviderError {
    let message = match error {
        AppServerError::NotFound => {
            "Codex CLI is not installed or is not in PATH. Install Codex CLI first."
        }
        AppServerError::Timeout(_) => "Codex App Server timed out. The widget will retry.",
        AppServerError::Transport(_) => {
            "Codex App Server could not start or stopped unexpectedly. \
             Verify or update Codex CLI; the widget will retry."
        }
        AppServerError::Closed => return ProviderError::new(error.to_string()),
        AppServerError::Rpc {
            unsupported: true, ..
        } => "This Codex CLI version does not support account usage. Update Codex CLI.",
        AppServerError::Rpc { sign_in: true, .. } => SIGN_IN,
        AppServerError::Rpc { code, .. } => {
            let code = code.map_or_else(|| "None".into(), |code| code.to_string());
            return ProviderError::new(format!(
                "Codex App Server could not return usage data (error {code})."
            ));
        }
    };
    ProviderError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_error_uses_safe_literal_messages() {
        for (error, expected) in [
            (
                AppServerError::Rpc {
                    code: Some((-32601).into()),
                    unsupported: true,
                    sign_in: true,
                },
                "This Codex CLI version does not support account usage. Update Codex CLI.",
            ),
            (
                AppServerError::Rpc {
                    code: Some((-32600).into()),
                    unsupported: false,
                    sign_in: true,
                },
                "Codex is not signed in. Run 'codex login' and try again.",
            ),
            (
                AppServerError::Rpc {
                    code: Some((-32600).into()),
                    unsupported: false,
                    sign_in: false,
                },
                "Codex App Server could not return usage data (error -32600).",
            ),
        ] {
            assert_eq!(provider_error(error).to_string(), expected);
        }
    }
}
