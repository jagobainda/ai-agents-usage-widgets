use widget_common::config::DEFAULT_REFRESH_INTERVAL;
use widget_common::{Logo, WidgetConfig};

pub const DEFAULT_BASE_URL: &str = "https://api.kimi.com/coding/v1";
pub const DEFAULT_OAUTH_HOST: &str = "https://auth.kimi.com";
pub const OAUTH_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

pub static WIDGET_CONFIG: WidgetConfig = WidgetConfig {
    app_name: env!("CARGO_BIN_NAME"),
    tray_name: "kimi-usage",
    display_name: "Kimi Usage Widget",
    popup_title: "Kimi Code · Usage",
    tooltip_name: "Kimi Code",
    accent: "#FFFFFF",
    accent_hover: "#3A3A3A",
    logo: Logo {
        svg: include_bytes!("../assets/kimi_icon.svg"),
        scale: 1.0,
    },
    refresh_interval: DEFAULT_REFRESH_INTERVAL,
};
