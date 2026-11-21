use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::Local;
use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, FontData, FontDefinitions, FontFamily, FontId, Galley,
    Id, Key, Painter, PointerButton, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2,
    ViewportBuilder, ViewportCommand, emath::GuiRounding, pos2, vec2,
};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
use winit::platform::windows::{CornerPreference, WindowExtWindows};

use crate::config::WidgetConfig;
use crate::icons;
use crate::state::{AppState, Snapshot};
use crate::theme::{self, Font};
use crate::utils::{format_until, round_percent};

const WIDTH: f32 = 380.0;
const PAD: f32 = 16.0;
const TITLEBAR_HEIGHT: f32 = 36.0;
const LOGO_SIZE: f32 = 20.0;
const WRAP_WIDTH: f32 = WIDTH - 2.0 * PAD;
const BAR_WIDTH: f32 = WRAP_WIDTH - 28.0;
const BAR_HEIGHT: f32 = 10.0;
const CARD_PADX: f32 = 14.0;
const CARD_PADY: f32 = 12.0;
/// Tk label border (2 px) plus padding (1 px) on each side.
const LABEL_INSET: f32 = 3.0;
const OFFSCREEN: [f32; 2] = [9999.0, 9999.0];
const TICK: Duration = Duration::from_secs(30);
const BOLD: &str = "bold";

/// Handle to the popup while it is open; shared with the tray shell.
#[derive(Default)]
pub struct PopupSlot {
    ctx: Mutex<Option<egui::Context>>,
}

impl PopupSlot {
    pub fn is_open(&self) -> bool {
        self.lock().is_some()
    }

    pub fn request_repaint(&self) {
        if let Some(ctx) = &*self.lock() {
            ctx.request_repaint();
        }
    }

    pub fn close(&self) {
        if let Some(ctx) = &*self.lock() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<egui::Context>> {
        self.ctx.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Opens the popup and blocks until it is closed. Always called from the same (UI) thread.
pub fn run_popup(state: &Arc<AppState>, slot: &Arc<PopupSlot>) {
    if slot.is_open() {
        return;
    }
    let config = state.config();
    let ctx = egui::Context::default();
    *slot.lock() = Some(ctx.clone());
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(config.popup_title)
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_position(OFFSCREEN)
            .with_inner_size([WIDTH, TITLEBAR_HEIGHT]),
        dithering: false,
        ..Default::default()
    };
    let popup = Popup {
        state: Arc::clone(state),
        logo: None,
        placed: false,
    };
    let _ = eframe::run_native_ext(
        config.app_name,
        options,
        Some(ctx),
        Box::new(|cc| {
            if let Some(window) = cc.winit_window() {
                window.set_undecorated_shadow(false);
                window.set_corner_preference(CornerPreference::DoNotRound);
                window.set_border_color(None);
            }
            cc.egui_ctx.set_fonts(fonts());
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            Ok(Box::new(popup))
        }),
    );
    *slot.lock() = None;
}

struct Popup {
    state: Arc<AppState>,
    logo: Option<(f32, TextureHandle)>,
    placed: bool,
}

impl eframe::App for Popup {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        color(theme::BG).to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let config = self.state.config();
        let content = Content::layout(ui.painter(), config, &self.state.snapshot());
        let size = vec2(
            content.width + 2.0 * (PAD + 1.0),
            content.height + TITLEBAR_HEIGHT + 2.0,
        );
        self.fit_window(&ctx, size.ceil());

        let window = ui.max_rect();
        ui.painter().rect_filled(window, 0, color(theme::BORDER));
        let body = window.shrink(1.0);
        ui.painter().rect_filled(body, 0, color(theme::BG));
        let bar = Rect::from_min_size(body.min, vec2(body.width(), TITLEBAR_HEIGHT));
        self.titlebar(ui, bar, config);
        content.paint(
            ui.painter(),
            pos2(body.left() + PAD, bar.bottom() + 8.0),
            body.width() - 2.0 * PAD,
        );

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        ctx.request_repaint_after(TICK);
    }
}

impl Popup {
    /// Sizes the window to its content off screen, then shows it at v1's spot; later
    /// size changes keep the bottom edge in place.
    fn fit_window(&mut self, ctx: &egui::Context, size: Vec2) {
        let (inner, outer) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect));
        let Some(inner) = inner else { return };
        if (inner.size() - size).abs().max_elem() >= 1.0 {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            if self.placed
                && let Some(outer) = outer
            {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(
                    outer.left(),
                    outer.bottom() - size.y,
                )));
            }
        } else if !self.placed {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(home_position(ctx, size)));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.placed = true;
        }
    }

    fn titlebar(&mut self, ui: &Ui, bar: Rect, config: &WidgetConfig) {
        let ctx = ui.ctx();
        ui.painter().rect_filled(bar, 0, color(theme::TITLEBAR));

        let logo = Rect::from_min_size(
            bar.min + vec2(10.0, (TITLEBAR_HEIGHT - LOGO_SIZE) / 2.0),
            Vec2::splat(LOGO_SIZE),
        )
        .round_to_pixels(ctx.pixels_per_point());
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        let texture = self.logo_texture(ctx, config);
        ui.painter().image(texture, logo, uv, Color32::WHITE);

        let title = text(
            ui.painter(),
            config.popup_title,
            theme::FONT_TITLE,
            theme::TEXT,
            None,
        );
        let title_pos = pos2(
            logo.right() + 8.0 + LABEL_INSET,
            bar.center().y - title.size().y / 2.0,
        );
        ui.painter().galley(title_pos, title, Color32::PLACEHOLDER);

        let (close, left) = hover_button(
            ui,
            bar,
            bar.right(),
            "✕",
            42.0,
            config.accent,
            config.accent_hover,
        );
        let (refresh, refresh_left) = hover_button(
            ui,
            bar,
            left,
            "⟳",
            34.0,
            theme::TEXT_MUTED,
            theme::BTN_BG_HOV,
        );
        let drag_area = Rect::from_min_max(bar.min, pos2(refresh_left, bar.bottom()));
        if ui
            .interact(drag_area, Id::unique("titlebar"), Sense::drag())
            .drag_started_by(PointerButton::Primary)
        {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if close {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if refresh {
            self.state.trigger_refresh();
        }
    }

    fn logo_texture(&mut self, ctx: &egui::Context, config: &WidgetConfig) -> egui::TextureId {
        let scale = ctx.pixels_per_point();
        if self.logo.as_ref().is_some_and(|(s, _)| *s != scale) {
            self.logo = None;
        }
        let (_, texture) = self.logo.get_or_insert_with(|| {
            let image = icons::render_logo(&config.logo, (LOGO_SIZE * scale).round() as u32);
            let size = [image.width as usize, image.height as usize];
            let image = ColorImage::from_rgba_unmultiplied(size, &image.rgba);
            (
                scale,
                ctx.load_texture("logo", image, TextureOptions::LINEAR),
            )
        });
        texture.id()
    }
}

/// Tk `HoverButton` packed to the right of `right`: returns whether it was pressed and its left edge.
fn hover_button(
    ui: &Ui,
    bar: Rect,
    right: f32,
    label: &str,
    width: f32,
    fg: &str,
    bg_hover: &str,
) -> (bool, f32) {
    let galley = text(ui.painter(), label, theme::FONT_HEAD, fg, None);
    let rect = Rect::from_x_y_ranges(right - width..=right, bar.y_range());
    let response = ui
        .interact(rect, Id::unique(label), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let hovered = response.hovered();
    let bg = if hovered { bg_hover } else { theme::TITLEBAR };
    ui.painter().rect_filled(rect, 0, color(bg));
    let bold_offset = vec2(1.0, 0.0);
    let pos = rect.center() - galley.mesh_bounds.center().to_vec2() - bold_offset / 2.0;
    ui.painter()
        .galley(pos, Arc::clone(&galley), Color32::PLACEHOLDER);
    ui.painter()
        .galley(pos + bold_offset, galley, Color32::PLACEHOLDER);
    let pressed = hovered && ui.input(|i| i.pointer.primary_pressed());
    (pressed, rect.left())
}

/// Content below the title bar, measured like Tk's packer before painting.
struct Content {
    items: Vec<Item>,
    width: f32,
    height: f32,
}

enum Item {
    Label {
        galley: Arc<Galley>,
        pady: (f32, f32),
    },
    Card {
        label: Arc<Galley>,
        percent: Arc<Galley>,
        reset: Arc<Galley>,
        utilization: f64,
    },
}

impl Content {
    fn layout(painter: &Painter, config: &WidgetConfig, snapshot: &Snapshot) -> Self {
        let error = snapshot.last_error.as_deref().filter(|e| !e.is_empty());
        let label = |s: &str, font, fg, wrap, pady| Item::Label {
            galley: text(painter, s, font, fg, wrap),
            pady,
        };
        let mut items = Vec::new();
        match &snapshot.usage {
            None => {
                let placeholder = match error {
                    None => "Loading usage data…",
                    Some(_) => "Could not fetch usage data.",
                };
                items.push(label(
                    placeholder,
                    theme::FONT_HEAD,
                    theme::TEXT,
                    None,
                    (4.0, 6.0),
                ));
                if let Some(error) = error {
                    let wrap = Some(WRAP_WIDTH);
                    items.push(label(
                        error,
                        theme::FONT_BODY,
                        theme::DANGER,
                        wrap,
                        (0.0, 6.0),
                    ));
                }
            }
            Some(usage) => {
                for limit in &usage.limits {
                    let status = theme::status_color_hex(limit.utilization);
                    let percent = round_percent(limit.utilization);
                    let reset = format!("Resets in {}", format_until(limit.resets_at));
                    items.push(Item::Card {
                        label: text(painter, &limit.label, theme::FONT_HEAD, config.accent, None),
                        percent: text(
                            painter,
                            &format!("{percent}%"),
                            theme::FONT_PCT,
                            status,
                            None,
                        ),
                        reset: text(painter, &reset, theme::FONT_BODY, theme::TEXT_MUTED, None),
                        utilization: limit.utilization,
                    });
                }
                for detail in &usage.details {
                    items.push(label(
                        detail,
                        theme::FONT_BODY,
                        theme::TEXT_MUTED,
                        None,
                        (0.0, 2.0),
                    ));
                }
                if let Some(error) = error {
                    let failed = format!("Last refresh failed · {error}");
                    let wrap = Some(WRAP_WIDTH);
                    items.push(label(
                        &failed,
                        theme::FONT_BODY,
                        theme::DANGER,
                        wrap,
                        (2.0, 4.0),
                    ));
                }
                let updated = usage.fetched_at.with_timezone(&Local).format("%H:%M:%S");
                let updated = format!("Updated · {updated}");
                items.push(label(
                    &updated,
                    theme::FONT_SMALL,
                    theme::TEXT_DIM,
                    None,
                    (2.0, 0.0),
                ));
            }
        }
        let width = items
            .iter()
            .map(Item::width)
            .fold(WIDTH - 2.0 * (PAD + 1.0), f32::max);
        let height = 8.0 + items.iter().map(Item::height).sum::<f32>() + PAD;
        Self {
            items,
            width,
            height,
        }
    }

    fn paint(self, painter: &Painter, origin: Pos2, width: f32) {
        let mut y = origin.y;
        for item in self.items {
            let height = item.height();
            match item {
                Item::Label { galley, pady } => {
                    let pos = pos2(origin.x, y + pady.0) + Vec2::splat(LABEL_INSET);
                    painter.galley(pos, galley, Color32::PLACEHOLDER);
                }
                Item::Card {
                    label,
                    percent,
                    reset,
                    utilization,
                } => {
                    let card = Rect::from_min_size(pos2(origin.x, y), vec2(width, height - 10.0));
                    painter.rect_filled(card, 0, color(theme::CARD));
                    let inner = card.shrink2(vec2(CARD_PADX, CARD_PADY));
                    let head = label_height(&label).max(label_height(&percent));
                    let centered = |g: &Galley| inner.top() + (head - label_height(g)) / 2.0;
                    let label_pos = pos2(inner.left(), centered(&label));
                    let percent_x = inner.right() - percent.size().x - 2.0 * LABEL_INSET;
                    let percent_pos = pos2(percent_x, centered(&percent));
                    let bar_y = inner.top() + head + 10.0;
                    let reset_pos = pos2(inner.left(), bar_y + BAR_HEIGHT + 8.0);
                    paint_bar(painter, pos2(inner.left(), bar_y), utilization);
                    for (pos, galley) in [
                        (label_pos, label),
                        (percent_pos, percent),
                        (reset_pos, reset),
                    ] {
                        painter.galley(
                            pos + Vec2::splat(LABEL_INSET),
                            galley,
                            Color32::PLACEHOLDER,
                        );
                    }
                }
            }
            y += height;
        }
    }
}

impl Item {
    fn width(&self) -> f32 {
        match self {
            Item::Label { galley, .. } => label_width(galley),
            Item::Card {
                label,
                percent,
                reset,
                ..
            } => {
                let head = label_width(label) + label_width(percent);
                BAR_WIDTH.max(head).max(label_width(reset)) + 2.0 * CARD_PADX
            }
        }
    }

    fn height(&self) -> f32 {
        match self {
            Item::Label { galley, pady } => pady.0 + label_height(galley) + pady.1,
            Item::Card {
                label,
                percent,
                reset,
                ..
            } => {
                let head = label_height(label).max(label_height(percent));
                let bar = 10.0 + BAR_HEIGHT + 8.0;
                2.0 * CARD_PADY + head + bar + label_height(reset) + 10.0
            }
        }
    }
}

/// Tk `RoundedProgress`: 320×10 pill track, fill at least as wide as it is tall when > 0.
fn paint_bar(painter: &Painter, origin: Pos2, utilization: f64) {
    let radius = BAR_HEIGHT / 2.0;
    let track = Rect::from_min_size(origin, vec2(BAR_WIDTH, BAR_HEIGHT));
    painter.rect_filled(track, radius, color(theme::TRACK));
    let value = utilization.clamp(0.0, 1.0) as f32;
    if value > 0.0 {
        let width = (BAR_WIDTH * value).floor().max(BAR_HEIGHT);
        let fill = Rect::from_min_size(origin, vec2(width, BAR_HEIGHT));
        painter.rect_filled(fill, radius, color(theme::status_color_hex(utilization)));
    }
}

fn label_width(galley: &Galley) -> f32 {
    galley.size().x + 2.0 * LABEL_INSET
}

fn label_height(galley: &Galley) -> f32 {
    galley.size().y + 2.0 * LABEL_INSET
}

fn text(painter: &Painter, s: &str, font: Font, fg: &str, wrap: Option<f32>) -> Arc<Galley> {
    let wrap = wrap.unwrap_or(f32::INFINITY);
    painter.layout(s.to_owned(), font_id(font), color(fg), wrap)
}

fn font_id(font: Font) -> FontId {
    let family = if font.bold {
        FontFamily::Name(BOLD.into())
    } else {
        FontFamily::Proportional
    };
    FontId::new(font.size_pt * 96.0 / 72.0, family)
}

fn color(hex: &str) -> Color32 {
    let [r, g, b] = theme::parse_hex(hex);
    Color32::from_rgb(r, g, b)
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    let dir = PathBuf::from(windir).join("Fonts");
    let mut load = |name: &str, file: &str, index| {
        let data = std::fs::read(dir.join(file)).ok()?;
        let mut font = FontData::from_owned(data);
        font.index = index;
        fonts.font_data.insert(name.to_owned(), Arc::new(font));
        Some(name.to_owned())
    };
    let regular = load("segoeui", "segoeui.ttf", 0);
    let bold = load("segoeuib", "segoeuib.ttf", 0);
    let math = load("cambria-math", "cambria.ttc", 1);
    let symbols = load("seguisym", "seguisym.ttf", 0);
    let defaults = fonts.families[&FontFamily::Proportional].clone();
    for (family, primary) in [
        (FontFamily::Proportional, regular),
        (FontFamily::Name(BOLD.into()), bold),
    ] {
        let chain = primary
            .into_iter()
            .chain(math.clone())
            .chain(symbols.clone())
            .chain(defaults.iter().cloned())
            .collect();
        fonts.families.insert(family, chain);
    }
    fonts
}

/// v1's spot (`screen - size - (20, 60)` on the primary monitor), in this window's points.
fn home_position(ctx: &egui::Context, size: Vec2) -> Pos2 {
    let (screen_px, dpi) = unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let (mut dpi_x, mut dpi_y) = (96, 96);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let screen = vec2(
            GetSystemMetrics(SM_CXSCREEN) as f32,
            GetSystemMetrics(SM_CYSCREEN) as f32,
        );
        (screen, dpi_x as f32)
    };
    let scale = dpi / 96.0;
    let pos = (screen_px / scale - size - vec2(20.0, 60.0)).max(Vec2::ZERO);
    (pos * scale / ctx.pixels_per_point()).to_pos2()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Logo, ProviderError, Usage, UsageProvider};

    #[test]
    fn titlebar_drag_excludes_refresh_and_close() {
        struct IdleProvider;
        impl UsageProvider for IdleProvider {
            fn fetch_usage(&self) -> Result<Usage, ProviderError> {
                unreachable!()
            }
        }
        static CONFIG: WidgetConfig = WidgetConfig {
            app_name: "Test",
            tray_name: "Test",
            display_name: "Test",
            popup_title: "Test",
            tooltip_name: "Test",
            accent: "#dddddd",
            accent_hover: "#eeeeee",
            logo: Logo {
                svg: b"",
                scale: 1.0,
            },
            refresh_interval: Duration::from_secs(60),
        };

        for (x, drag, close) in [
            (100.0, true, false),
            (320.0, false, false),
            (360.0, false, true),
        ] {
            let state = AppState::new(Box::new(IdleProvider), &CONFIG);
            let mut popup = Popup {
                state,
                logo: None,
                placed: false,
            };
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts());
            let bar = Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, TITLEBAR_HEIGHT));
            let pos = pos2(x, bar.center().y);
            let mut frame = |events| {
                let input = egui::RawInput {
                    events,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| popup.titlebar(ui, bar, &CONFIG));
                output.textures_delta.clear();
                std::mem::take(
                    &mut output
                        .viewport_output
                        .get_mut(&egui::ViewportId::ROOT)
                        .unwrap()
                        .commands,
                )
            };
            frame(vec![egui::Event::PointerMoved(pos)]);
            let commands = frame(vec![egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(
                commands.contains(&ViewportCommand::StartDrag),
                drag,
                "x={x}"
            );
            assert_eq!(commands.contains(&ViewportCommand::Close), close, "x={x}");
        }
    }
}
