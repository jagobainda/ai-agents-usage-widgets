use std::path::PathBuf;
use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont, point};
use resvg::{tiny_skia, usvg};

use crate::config::Logo;
use crate::models::Usage;
use crate::theme;
use crate::utils::round_percent;

pub const ICON_SIZE: u32 = 64;

const FONT_FILES: [&str; 4] = ["segoeuib.ttf", "arialbd.ttf", "seguisb.ttf", "arial.ttf"];
const FONT_FRACTIONS: [f64; 5] = [0.88, 0.75, 0.62, 0.50, 0.38];

/// Straight (non-premultiplied) RGBA8 pixels, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RgbaImage {
    fn transparent(size: u32) -> Self {
        Self {
            width: size,
            height: size,
            rgba: vec![0; (size * size * 4) as usize],
        }
    }
}

/// Pillow `textbbox` metrics: pen origin at (0, 0), box includes the pen line.
struct TextLayout {
    scale: PxScale,
    ascent: f32,
    glyphs: Vec<(GlyphId, f32)>,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

pub fn usage_icon(usage: &Usage, color: &str) -> RgbaImage {
    let utilization = usage.primary().map_or(0.0, |limit| limit.utilization);
    let percent = round_percent(utilization);
    make_icon(&percent.to_string(), color)
}

pub fn loading_icon() -> RgbaImage {
    make_icon("…", theme::NEUTRAL)
}

pub fn error_icon() -> RgbaImage {
    make_icon("!", theme::NEUTRAL)
}

pub fn render_logo(logo: &Logo, size: u32) -> RgbaImage {
    let mut image = RgbaImage::transparent(size);
    let Ok(tree) = usvg::Tree::from_data(logo.svg, &usvg::Options::default()) else {
        return image;
    };
    let Some(mut pixmap) = tiny_skia::Pixmap::new(size, size) else {
        return image;
    };
    let canvas = size as f32;
    let view = tree.size();
    let scale = canvas * logo.scale / view.width().max(view.height());
    let transform = tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (canvas - view.width() * scale) / 2.0,
        (canvas - view.height() * scale) / 2.0,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    for (out, pixel) in image
        .rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(pixmap.pixels())
    {
        let color = pixel.demultiply();
        out.copy_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    image
}

fn font() -> Option<&'static (FontVec, f32)> {
    static FONT: OnceLock<Option<(FontVec, f32)>> = OnceLock::new();
    FONT.get_or_init(|| {
        let dir =
            PathBuf::from(std::env::var_os("WINDIR").unwrap_or(r"C:\Windows".into())).join("Fonts");
        FONT_FILES.iter().find_map(|name| {
            let font = FontVec::try_from_vec(std::fs::read(dir.join(name)).ok()?).ok()?;
            let units_per_em = font.units_per_em()?;
            Some((font, units_per_em))
        })
    })
    .as_ref()
}

/// Rounded ink bounds stand in for FreeType's hinted, grid-fitted glyph boxes.
fn layout(font: &FontVec, units_per_em: f32, text: &str, em: f32) -> TextLayout {
    let px_per_unit = em / units_per_em;
    let scale = PxScale::from(font.height_unscaled() * px_per_unit);
    let scaled = font.as_scaled(scale);
    let (mut pen, mut left, mut right, mut top, mut bottom) =
        (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    let mut glyphs = Vec::new();
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(outline) = font.outline(id) {
            let px = |units: f32| (units * px_per_unit).round();
            left = left.min(pen + px(outline.bounds.min.x));
            right = right.max(pen + px(outline.bounds.max.x));
            top = top.min(-px(outline.bounds.min.y));
            bottom = bottom.max(-px(outline.bounds.max.y));
        }
        glyphs.push((id, pen));
        pen += scaled.h_advance(id).round();
        right = right.max(pen);
    }
    TextLayout {
        scale,
        ascent: (font.ascent_unscaled() * px_per_unit).ceil(),
        glyphs,
        left,
        top,
        width: right - left,
        height: bottom - top,
    }
}

fn make_icon(text: &str, color: &str) -> RgbaImage {
    let mut image = RgbaImage::transparent(ICON_SIZE);
    let Some((font, units_per_em)) = font() else {
        return image;
    };
    let size = ICON_SIZE as f32;
    let fits = |text: &TextLayout| text.width <= size - 2.0 && text.height <= size - 2.0;
    let ems = FONT_FRACTIONS.map(|fraction| (f64::from(ICON_SIZE) * fraction) as u32 as f32);
    let mut text_layout = layout(font, *units_per_em, text, ems[0]);
    for &em in &ems[1..] {
        if fits(&text_layout) {
            break;
        }
        text_layout = layout(font, *units_per_em, text, em);
    }
    // FreeType snaps the pen to whole pixels rounding half up in its y-up space.
    let x = ((size - text_layout.width) / 2.0 - text_layout.left + 0.5).floor();
    let y = ((size - text_layout.height) / 2.0 - text_layout.top - 0.5).ceil();
    // Pillow's text mask starts at int(y) of its ascender-anchored origin and crops what lies above.
    let origin_y = (size - text_layout.height) / 2.0 - text_layout.ascent - text_layout.top;
    let clip_top = (origin_y.trunc() + text_layout.ascent + text_layout.top) as i64;
    let [r, g, b] = theme::parse_hex(color);
    for (id, pen) in &text_layout.glyphs {
        let glyph = id.with_scale_and_position(text_layout.scale, point(x + pen, y));
        let Some(outlined) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outlined.px_bounds();
        outlined.draw(|gx, gy, coverage| {
            let px = bounds.min.x as i64 + i64::from(gx);
            let py = bounds.min.y as i64 + i64::from(gy);
            let alpha = (coverage.min(1.0) * 255.0).round() as u8;
            if alpha == 0
                || !(0..i64::from(ICON_SIZE)).contains(&px)
                || !(clip_top..i64::from(ICON_SIZE)).contains(&py)
            {
                return;
            }
            let i = ((py * i64::from(ICON_SIZE) + px) * 4) as usize;
            let pixel = &mut image.rgba[i..i + 4];
            pixel.copy_from_slice(&[r, g, b, pixel[3].max(alpha)]);
        });
    }
    image
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::models::UsageLimit;

    fn usage(utilization: f64) -> Usage {
        Usage {
            limits: vec![UsageLimit {
                key: "primary".into(),
                label: "Primary".into(),
                short_label: "Primary".into(),
                utilization,
                resets_at: None,
                severity: "normal".into(),
                is_active: false,
            }],
            fetched_at: Utc::now(),
            primary_key: None,
            details: Vec::new(),
        }
    }

    fn visible(image: &RgbaImage) -> Vec<&[u8; 4]> {
        image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 0)
            .collect()
    }

    #[test]
    fn usage_digits_use_only_the_provider_color_at_every_usage_level() {
        for color in ["#D97757", "#4CC9F0", "#808080", "#FFFFFF", "#34A853"] {
            let expected = theme::parse_hex(color);
            for utilization in [0.0, 0.5, 0.95, 1.0] {
                let image = usage_icon(&usage(utilization), color);
                let pixels = visible(&image);
                assert!(!pixels.is_empty(), "{color} {utilization}");
                assert!(
                    pixels.iter().all(|pixel| pixel[..3] == expected),
                    "{color} {utilization}"
                );
            }
        }
    }

    #[test]
    fn state_icons_are_square_and_not_empty() {
        let empty_usage = Usage {
            limits: Vec::new(),
            ..usage(0.0)
        };
        for image in [
            loading_icon(),
            error_icon(),
            usage_icon(&empty_usage, "#D97757"),
        ] {
            assert_eq!((image.width, image.height), (ICON_SIZE, ICON_SIZE));
            assert_eq!(image.rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
            assert!(!visible(&image).is_empty());
        }
    }

    #[test]
    fn usage_text_rounds_half_to_even() {
        assert_eq!(
            usage_icon(&usage(0.125), "#D97757"),
            usage_icon(&usage(0.12), "#D97757")
        );
        assert_eq!(
            usage_icon(&usage(0.375), "#D97757"),
            usage_icon(&usage(0.38), "#D97757")
        );
    }

    #[test]
    fn logos_render_at_every_size() {
        let logos = [
            Logo {
                svg: include_bytes!("../../../apps/claude-usage-widget/assets/claude_icon.svg"),
                scale: 1.0,
            },
            Logo {
                svg: include_bytes!("../../../apps/codex-usage-widget/assets/codex_icon.svg"),
                scale: 1.0,
            },
            Logo {
                svg: include_bytes!("../../../apps/opencode-usage-widget/assets/opencode_icon.svg"),
                scale: 0.84,
            },
            Logo {
                svg: include_bytes!("../../../apps/kimi-usage-widget/assets/kimi_icon.svg"),
                scale: 1.0,
            },
            Logo {
                svg: include_bytes!(
                    "../../../apps/antigravity-usage-widget/assets/antigravity_icon.svg"
                ),
                scale: 1.0,
            },
        ];
        for logo in &logos {
            for size in [16, 32, 64, 256] {
                let image = render_logo(logo, size);
                assert_eq!((image.width, image.height), (size, size));
                assert_eq!(image.rgba.len(), (size * size * 4) as usize);
                assert!(!visible(&image).is_empty());
            }
        }
    }

    #[test]
    fn antigravity_logo_renders_official_multicolor_gradient() {
        let logo = Logo {
            svg: include_bytes!(
                "../../../apps/antigravity-usage-widget/assets/antigravity_icon.svg"
            ),
            scale: 1.0,
        };
        let image = render_logo(&logo, 64);
        assert_eq!((image.width, image.height), (64, 64));
        let opaque: Vec<_> = image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 200)
            .collect();
        assert!(!opaque.is_empty());
        let any = |test: fn(u8, u8, u8) -> bool| opaque.iter().any(|p| test(p[0], p[1], p[2]));
        assert!(any(|r, g, b| r > 180 && g < 120 && b < 120));
        assert!(any(|r, g, b| r > 190 && g > 170 && b < 110));
        assert!(any(|r, g, b| g > 150 && r < 110 && b < 140));
        assert!(any(|r, _, b| b > 200 && r < 140));
    }
}
