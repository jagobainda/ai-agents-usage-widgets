use std::io::ErrorKind;
use std::time::Duration;

use ureq::config::Config;
use ureq::config::ConfigBuilder;
use ureq::http::{HeaderValue, Request, StatusCode};
use ureq::tls::{RootCerts, TlsConfig};
use ureq::typestate::AgentScope;
use ureq::{Agent, Body, RequestBuilder};

/// Agent configuration that validates TLS against the Windows certificate store and
/// returns 4xx/5xx responses instead of errors; callers add their own timeouts.
pub fn agent_config() -> ConfigBuilder<AgentScope> {
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .http_status_as_error(false)
}

pub const DEFAULT_TIMEOUTS: (u64, u64) = (5, 15);

pub struct Client(Agent);

pub struct Response {
    pub status_code: u16,
    pub body: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Timeout { connect: bool },
    HostNotFound,
    Redirects,
    Tls,
    Proxy,
    Transport,
}

impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        match error {
            ureq::Error::Timeout(phase) => Self::Timeout {
                connect: matches!(phase, ureq::Timeout::Connect),
            },
            ureq::Error::Io(error) if error.kind() == ErrorKind::TimedOut => {
                Self::Timeout { connect: false }
            }
            ureq::Error::HostNotFound => Self::HostNotFound,
            ureq::Error::TooManyRedirects => Self::Redirects,
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) => Self::Tls,
            ureq::Error::InvalidProxyUrl | ureq::Error::ConnectProxyFailed(_) => Self::Proxy,
            _ => Self::Transport,
        }
    }
}

impl Client {
    pub fn new((connect, read): (u64, u64)) -> Self {
        let read = Some(Duration::from_secs(read));
        Self::with_config(
            agent_config()
                .timeout_connect(Some(Duration::from_secs(connect)))
                .timeout_recv_response(read)
                .timeout_recv_body(read)
                .build(),
        )
    }

    pub fn with_config(config: Config) -> Self {
        Self(config.into())
    }

    pub fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, Error> {
        let mut response = with_headers(self.0.get(url), headers)?.call()?;
        let status_code = response.status().as_u16();
        text_response(status_code, || {
            if status_code == 200 {
                response.body_mut().read_to_string()
            } else {
                Ok(String::new())
            }
        })
    }

    pub fn post_form(
        &self,
        url: &str,
        form: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> Result<Response, Error> {
        let mut response =
            with_headers(self.0.post(url), headers)?.send_form(form.iter().copied())?;
        text_response(response.status().as_u16(), || {
            response.body_mut().read_to_string()
        })
    }

    pub fn send(&self, request: Request<Option<String>>) -> Result<(StatusCode, Vec<u8>), Error> {
        let (parts, body) = request.into_parts();
        let mut response = match body {
            Some(body) => self.0.run(Request::from_parts(parts, body)),
            None => self.0.run(Request::from_parts(parts, ())),
        }?;
        Ok((response.status(), Body::read_to_vec(response.body_mut())?))
    }
}

pub fn authorization_header(value: &str) -> Result<HeaderValue, Error> {
    let mut value = HeaderValue::from_str(value).map_err(|_| Error::Transport)?;
    value.set_sensitive(true);
    Ok(value)
}

fn with_headers<B>(
    mut request: RequestBuilder<B>,
    headers: &[(&str, &str)],
) -> Result<RequestBuilder<B>, Error> {
    for (name, value) in headers {
        request = if name.eq_ignore_ascii_case("authorization") {
            request.header(*name, authorization_header(value)?)
        } else {
            request.header(*name, *value)
        };
    }
    Ok(request)
}

fn text_response(
    status_code: u16,
    read: impl FnOnce() -> Result<String, ureq::Error>,
) -> Result<Response, Error> {
    let body = match read() {
        Ok(body) => body,
        Err(_) if status_code != 200 => String::new(),
        Err(error) => return Err(error.into()),
    };
    Ok(Response { status_code, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_http_status_when_an_error_body_cannot_be_read() {
        for status in [401, 403, 404, 502] {
            let response = text_response(status, || {
                Err(std::io::Error::from(ErrorKind::InvalidData).into())
            })
            .unwrap();
            assert_eq!(response.status_code, status);
            assert!(response.body.is_empty());
        }
        let response = text_response(400, || Ok(r#"{"error":"invalid_grant"}"#.into())).unwrap();
        assert_eq!(response.body, r#"{"error":"invalid_grant"}"#);
        assert!(matches!(
            text_response(
                200,
                || Err(std::io::Error::from(ErrorKind::TimedOut).into())
            ),
            Err(Error::Timeout { .. })
        ));
    }

    #[test]
    fn authorizations_are_sensitive() {
        assert!(
            authorization_header("Bearer fixture")
                .unwrap()
                .is_sensitive()
        );
        assert_eq!(
            authorization_header("invalid\nheader"),
            Err(Error::Transport)
        );
    }
}
