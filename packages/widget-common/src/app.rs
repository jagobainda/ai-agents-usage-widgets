use std::cell::Cell;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::Arc;

use tray_icon::menu::{ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetMessageW,
    GetWindowLongPtrW, HMENU, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    SetMenuDefaultItem, SetWindowLongPtrW, TranslateMessage, WM_APP, WNDCLASSW,
};

use crate::config::WidgetConfig;
use crate::icons::RgbaImage;
use crate::popup::{self, PopupSlot};
use crate::promote;
use crate::provider::UsageProvider;
use crate::state::AppState;
use crate::utils::encode_wide;

const WM_APP_STATE_CHANGED: u32 = WM_APP + 1;
const WM_APP_OPEN_POPUP: u32 = WM_APP + 2;
const WM_APP_REFRESH: u32 = WM_APP + 3;
const WM_APP_QUIT: u32 = WM_APP + 4;

struct UiContext {
    state: Arc<AppState>,
    slot: Arc<PopupSlot>,
    tray: TrayIcon,
    open_popup: Cell<bool>,
    in_popup: Cell<bool>,
    quitting: Cell<bool>,
}

impl UiContext {
    fn sync_tray(&self) {
        if let Some(icon) = to_tray_icon(self.state.tray_icon()) {
            let _ = self.tray.set_icon(Some(icon));
        }
        let _ = self.tray.set_tooltip(Some(self.state.tray_title()));
    }
}

fn to_tray_icon(image: RgbaImage) -> Option<Icon> {
    Icon::from_rgba(image.rgba, image.width, image.height).ok()
}

unsafe extern "system" fn notify_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const UiContext;
    if !ptr.is_null() {
        let ui = unsafe { &*ptr };
        match msg {
            WM_APP_STATE_CHANGED => {
                ui.sync_tray();
                return 0;
            }
            WM_APP_OPEN_POPUP => {
                if !ui.quitting.get() && !ui.in_popup.get() && !ui.slot.is_open() {
                    ui.open_popup.set(true);
                }
                return 0;
            }
            WM_APP_REFRESH => {
                ui.state.trigger_refresh();
                return 0;
            }
            WM_APP_QUIT => {
                ui.quitting.set(true);
                ui.open_popup.set(false);
                ui.state.stop();
                if ui.slot.is_open() {
                    ui.slot.close();
                } else if !ui.in_popup.get() {
                    unsafe { PostQuitMessage(0) };
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn create_notify_window() -> HWND {
    let class_name = encode_wide("UsageWidgetNotifyWindow");
    let hinstance = unsafe { GetModuleHandleW(null()) };
    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(notify_wnd_proc),
        hInstance: hinstance,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { zeroed() }
    };
    unsafe { RegisterClassW(&wnd_class) };
    unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            hinstance,
            null(),
        )
    }
}

pub fn run_widget(provider: Box<dyn UsageProvider>, config: &'static WidgetConfig) -> i32 {
    let state = AppState::new(provider, config);
    let slot = Arc::new(PopupSlot::default());

    let view_details_item = MenuItem::new("View details…", true, None);
    let refresh_item = MenuItem::new("Refresh now", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit_item = MenuItem::new("Quit", true, None);

    let view_details_id = view_details_item.id().clone();
    let refresh_id = refresh_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append_items(&[&view_details_item, &refresh_item, &separator, &quit_item]);
    unsafe {
        SetMenuDefaultItem(menu.hpopupmenu() as HMENU, 0, TRUE as u32);
    }

    let mut builder = TrayIconBuilder::new()
        .with_id(config.tray_name)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(true)
        .with_tooltip(state.tray_title());
    if let Some(icon) = to_tray_icon(state.tray_icon()) {
        builder = builder.with_icon(icon);
    }
    let Ok(tray) = builder.build() else {
        state.stop();
        return 1;
    };

    let notify_hwnd = unsafe { create_notify_window() };
    if notify_hwnd.is_null() {
        state.stop();
        return 1;
    }
    promote::spawn_promotion(config.app_name);
    let hwnd_raw = notify_hwnd as isize;

    let ui = UiContext {
        state: Arc::clone(&state),
        slot: Arc::clone(&slot),
        tray,
        open_popup: Cell::new(false),
        in_popup: Cell::new(false),
        quitting: Cell::new(false),
    };
    unsafe {
        SetWindowLongPtrW(notify_hwnd, GWLP_USERDATA, (&raw const ui) as isize);
    }

    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            unsafe {
                PostMessageW(hwnd_raw as HWND, WM_APP_OPEN_POPUP, 0, 0);
            }
        }
    }));

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let msg = if event.id == view_details_id {
            WM_APP_OPEN_POPUP
        } else if event.id == refresh_id {
            WM_APP_REFRESH
        } else if event.id == quit_id {
            WM_APP_QUIT
        } else {
            return;
        };
        unsafe {
            PostMessageW(hwnd_raw as HWND, msg, 0, 0);
        }
    }));

    let slot_for_listener = Arc::clone(&slot);
    state.add_listener(move || {
        slot_for_listener.request_repaint();
        unsafe {
            PostMessageW(hwnd_raw as HWND, WM_APP_STATE_CHANGED, 0, 0);
        }
    });

    state.start();

    unsafe {
        let mut msg: MSG = zeroed();
        while !ui.quitting.get() && GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);

            if ui.quitting.get() {
                break;
            }
            if ui.open_popup.replace(false) && !ui.in_popup.get() && !ui.slot.is_open() {
                ui.in_popup.set(true);
                popup::run_popup(&ui.state, &ui.slot);
                ui.in_popup.set(false);
                ui.open_popup.set(false);
            }
        }
    }

    state.stop();
    unsafe {
        SetWindowLongPtrW(notify_hwnd, GWLP_USERDATA, 0);
        DestroyWindow(notify_hwnd);
    }
    0
}
