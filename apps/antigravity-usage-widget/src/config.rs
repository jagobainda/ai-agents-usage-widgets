use widget_common::config::DEFAULT_REFRESH_INTERVAL;
use widget_common::{Logo, WidgetConfig};

pub static WIDGET_CONFIG: WidgetConfig = WidgetConfig {
    app_name: env!("CARGO_BIN_NAME"),
    tray_name: "antigravity-usage",
    display_name: "Antigravity Usage Widget",
    popup_title: "Antigravity · Usage",
    tooltip_name: "Antigravity",
    accent: "#34A853",
    accent_hover: "#1E5F30",
    logo: Logo {
        svg: include_bytes!("../assets/antigravity_icon.svg"),
        scale: 1.0,
    },
    refresh_interval: DEFAULT_REFRESH_INTERVAL,
};
