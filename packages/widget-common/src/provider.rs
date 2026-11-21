use std::fmt;

use crate::models::Usage;

/// Error whose message is safe to show in the popup and the tray tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError(pub String);

impl ProviderError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProviderError {}

pub trait UsageProvider: Send + Sync {
    fn fetch_usage(&self) -> Result<Usage, ProviderError>;

    fn close(&self) {}
}
