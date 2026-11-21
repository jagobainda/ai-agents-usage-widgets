pub const BG: &str = "#1b1c20";
pub const TITLEBAR: &str = "#131418";
pub const CARD: &str = "#24262c";

pub const TEXT: &str = "#e9eaee";
pub const TEXT_MUTED: &str = "#9aa0a8";
pub const TEXT_DIM: &str = "#6c7079";

pub const BTN_BG: &str = "#2c2f36";
pub const BTN_BG_HOV: &str = "#3a3e47";
pub const BTN_FG: &str = "#e9eaee";
pub const TRACK: &str = "#2c2f36";

pub const OK: &str = "#3ecf63";
pub const WARN_LOW: &str = "#f0c419";
pub const WARN_HIGH: &str = "#ff9933";
pub const DANGER: &str = "#e64960";
pub const NEUTRAL: &str = "#7a818c";

pub const FONT_FAMILY: &str = "Segoe UI";

/// Tk font spec: size in points (1 pt = 96/72 px at 100 % scaling).
#[derive(Clone, Copy, Debug)]
pub struct Font {
    pub size_pt: f32,
    pub bold: bool,
}

pub const FONT_TITLE: Font = Font {
    size_pt: 10.0,
    bold: true,
};
pub const FONT_BODY: Font = Font {
    size_pt: 10.0,
    bold: false,
};
pub const FONT_HEAD: Font = Font {
    size_pt: 11.0,
    bold: true,
};
pub const FONT_PCT: Font = Font {
    size_pt: 14.0,
    bold: true,
};
pub const FONT_SMALL: Font = Font {
    size_pt: 8.0,
    bold: false,
};

pub fn status_color_hex(utilization: f64) -> &'static str {
    if utilization >= 0.90 {
        DANGER
    } else if utilization >= 0.75 {
        WARN_HIGH
    } else if utilization >= 0.50 {
        WARN_LOW
    } else {
        OK
    }
}

/// Parses `#rrggbb`; invalid input yields black.
pub fn parse_hex(hex: &str) -> [u8; 3] {
    let channel = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .unwrap_or(0)
    };
    [channel(1), channel(3), channel(5)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_thresholds() {
        assert_eq!(status_color_hex(0.49), OK);
        assert_eq!(status_color_hex(0.50), WARN_LOW);
        assert_eq!(status_color_hex(0.75), WARN_HIGH);
        assert_eq!(status_color_hex(0.90), DANGER);
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_hex("#D97757"), [0xd9, 0x77, 0x57]);
        assert_eq!(parse_hex("#7a818c"), [122, 129, 140]);
    }
}
