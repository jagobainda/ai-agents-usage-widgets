use std::fmt;

#[derive(Debug)]
pub enum KimiError {
    Credential(&'static str),
    InvalidUrlType(&'static str),
    Refresh(&'static str),
    Api(u16),
    Payload(&'static str),
    Timeout,
    Transport,
    Io(std::io::Error),
}

impl fmt::Display for KimiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Credential(message) | Self::Refresh(message) | Self::Payload(message) => {
                formatter.write_str(message)
            }
            Self::InvalidUrlType(type_name) => {
                write!(
                    formatter,
                    "AttributeError: '{type_name}' object has no attribute 'rstrip'"
                )
            }
            Self::Api(status) => write!(formatter, "Kimi usage endpoint returned HTTP {status}."),
            Self::Timeout => formatter.write_str("Kimi Code request timed out."),
            Self::Transport => formatter.write_str("Kimi Code request failed."),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for KimiError {}

impl From<widget_common::http::Error> for KimiError {
    fn from(error: widget_common::http::Error) -> Self {
        match error {
            widget_common::http::Error::Timeout { .. } => Self::Timeout,
            _ => Self::Transport,
        }
    }
}
