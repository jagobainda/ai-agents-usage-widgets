use widget_common::config::DEFAULT_REFRESH_INTERVAL;
use widget_common::{Logo, WidgetConfig};

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
pub const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
pub const USAGE_USER_AGENT: &str = "claude-code/2.0.31";

pub static WIDGET_CONFIG: WidgetConfig = WidgetConfig {
    app_name: env!("CARGO_BIN_NAME"),
    tray_name: "claude-usage",
    display_name: "Claude Usage Widget",
    popup_title: "Claude Code · Usage",
    tooltip_name: "Claude Code",
    accent: "#D97757",
    accent_hover: "#c0613f",
    logo: Logo {
        svg: include_bytes!("../assets/claude_icon.svg"),
        scale: 1.0,
    },
    refresh_interval: DEFAULT_REFRESH_INTERVAL,
};
