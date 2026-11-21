use std::time::Duration;

pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// Brand SVG and the fraction of the square canvas its fitted, centred box occupies.
#[derive(Debug)]
pub struct Logo {
    pub svg: &'static [u8],
    pub scale: f32,
}

#[derive(Debug)]
pub struct WidgetConfig {
    pub app_name: &'static str,
    pub tray_name: &'static str,
    pub display_name: &'static str,
    pub popup_title: &'static str,
    pub tooltip_name: &'static str,
    pub accent: &'static str,
    pub accent_hover: &'static str,
    pub logo: Logo,
    pub refresh_interval: Duration,
}
