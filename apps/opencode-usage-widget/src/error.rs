#[derive(Debug, PartialEq, Eq)]
pub enum OpenCodeError {
    Credential(&'static str),
    Api(u16),
    Payload(&'static str),
    Timeout,
    Transport,
}

impl From<widget_common::http::Error> for OpenCodeError {
    fn from(error: widget_common::http::Error) -> Self {
        match error {
            widget_common::http::Error::Timeout { .. } => Self::Timeout,
            _ => Self::Transport,
        }
    }
}
