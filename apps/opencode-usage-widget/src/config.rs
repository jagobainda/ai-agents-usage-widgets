use widget_common::config::DEFAULT_REFRESH_INTERVAL;
use widget_common::{Logo, WidgetConfig};

pub const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

pub static WIDGET_CONFIG: WidgetConfig = WidgetConfig {
    app_name: env!("CARGO_BIN_NAME"),
    tray_name: "opencode-usage",
    display_name: "OpenCode Usage Widget",
    popup_title: "OpenCode Usage Widget",
    tooltip_name: "OpenCode Usage Widget",
    accent: "#808080",
    accent_hover: "#3A3A3A",
    logo: Logo {
        svg: include_bytes!("../assets/opencode_icon.svg"),
        scale: 0.84,
    },
    refresh_interval: DEFAULT_REFRESH_INTERVAL,
};
