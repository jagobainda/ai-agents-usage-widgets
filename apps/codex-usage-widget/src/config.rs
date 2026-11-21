use std::time::Duration;

use widget_common::config::DEFAULT_REFRESH_INTERVAL;
use widget_common::{Logo, WidgetConfig};

pub const CLIENT_NAME: &str = "codex_usage_widget";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const REQUEST_RETRIES: u32 = 2;
pub const RETRY_BACKOFF: Duration = Duration::from_millis(250);

pub static WIDGET_CONFIG: WidgetConfig = WidgetConfig {
    app_name: env!("CARGO_BIN_NAME"),
    tray_name: "codex-usage",
    display_name: "Codex Usage Widget",
    popup_title: "Codex Usage Widget",
    tooltip_name: "Codex Usage Widget",
    accent: "#4CC9F0",
    accent_hover: "#176B87",
    logo: Logo {
        svg: include_bytes!("../assets/codex_icon.svg"),
        scale: 1.0,
    },
    refresh_interval: DEFAULT_REFRESH_INTERVAL,
};
