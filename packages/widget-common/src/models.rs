use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq)]
pub struct UsageLimit {
    pub key: String,
    pub label: String,
    pub short_label: String,
    pub utilization: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub severity: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    pub limits: Vec<UsageLimit>,
    pub fetched_at: DateTime<Utc>,
    pub primary_key: Option<String>,
    pub details: Vec<String>,
}

impl Usage {
    pub fn primary(&self) -> Option<&UsageLimit> {
        self.primary_key
            .as_deref()
            .and_then(|key| self.limits.iter().find(|limit| limit.key == key))
            .or_else(|| self.limits.first())
    }
}
