use std::time::Duration;

use serde_json::Value;
use ureq::http::{Method, Request, StatusCode};
use widget_common::ProviderError;
use widget_common::http::{self, Client, Error};

use crate::config::{ANTHROPIC_BETA, TOKEN_URL, USAGE_URL, USAGE_USER_AGENT};

pub struct ClaudeRequest(Request<Option<String>>);

#[cfg(test)]
impl ClaudeRequest {
    pub fn as_http(&self) -> &Request<Option<String>> {
        &self.0
    }
}

pub struct ClaudeResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

impl ClaudeResponse {
    pub fn json(&self, url: &str) -> Result<Value, ProviderError> {
        if self.status.is_client_error() || self.status.is_server_error() {
            let kind = if self.status.is_client_error() {
                "Client"
            } else {
                "Server"
            };
            let reason = self.status.canonical_reason().unwrap_or("");
            return Err(ProviderError::new(format!(
                "HTTPError: {} {kind} Error: {reason} for url: {url}",
                self.status.as_u16(),
            )));
        }
        let bytes = self
            .body
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&self.body);
        serde_json::from_slice(bytes).map_err(|error| {
            ProviderError::new(if bytes.iter().all(u8::is_ascii_whitespace) {
                "JSONDecodeError: Expecting value: line 1 column 1 (char 0)".to_owned()
            } else {
                format!(
                    "JSONDecodeError: Invalid JSON at line {} column {}",
                    error.line(),
                    error.column()
                )
            })
        })
    }
}

pub fn client() -> Client {
    Client::with_config(
        http::agent_config()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_per_read(Some(Duration::from_secs(15)))
            .max_redirects(30)
            .redirect_auth_headers(ureq::config::RedirectAuthHeaders::SameHost)
            .build(),
    )
}

pub fn usage_request(access_token: &str) -> Result<ClaudeRequest, ProviderError> {
    let authorization = http::authorization_header(&format!("Bearer {access_token}"))
        .map_err(|_| ProviderError::new("InvalidHeader: Invalid Authorization header"))?;
    Request::builder()
        .method(Method::GET)
        .uri(USAGE_URL)
        .header("user-agent", USAGE_USER_AGENT)
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("authorization", authorization)
        .body(None)
        .map(ClaudeRequest)
        .map_err(|_| ProviderError::new("InvalidHeader: Invalid usage request"))
}

pub fn token_request(body: String) -> Result<ClaudeRequest, ProviderError> {
    Request::builder()
        .method(Method::POST)
        .uri(TOKEN_URL)
        .header("content-type", "application/json")
        .body(Some(body))
        .map(ClaudeRequest)
        .map_err(|_| ProviderError::new("RequestException: Invalid token request"))
}

pub fn send(client: &Client, request: ClaudeRequest) -> Result<ClaudeResponse, ProviderError> {
    let (status, body) = client.send(request.0).map_err(transport_error)?;
    Ok(ClaudeResponse { status, body })
}

fn transport_error(error: Error) -> ProviderError {
    let message = match error {
        Error::Timeout { connect: true } => "ConnectTimeout: Connection timed out",
        Error::Timeout { .. } => "ReadTimeout: The read operation timed out",
        Error::HostNotFound => "ConnectionError: Failed to resolve the host",
        Error::Redirects => "TooManyRedirects: Exceeded 30 redirects.",
        Error::Tls => "SSLError: TLS connection failed",
        Error::Proxy => "ProxyError: Proxy connection failed",
        Error::Transport => "ConnectionError: HTTP request failed",
    };
    ProviderError::new(message)
}
