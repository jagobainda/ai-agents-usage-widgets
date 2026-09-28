"""Shared pystray and Tkinter application shell."""

from __future__ import annotations

import threading
import tkinter as tk

import pystray

from .config import WidgetConfig
from .icons import loading_icon
from .popup import PopupWindow
from .provider import UsageProvider
from .state import AppState


def run_widget(provider: UsageProvider, config: WidgetConfig) -> int:
    state = AppState(provider, config)
    tk_root = tk.Tk()
    tk_root.withdraw()
    popup_open = threading.Event()

    def do_open_popup() -> None:
        if popup_open.is_set():
            return
        popup_open.set()
        PopupWindow(state, tk_root, on_close=popup_open.clear)

    def on_open(_icon, _item) -> None:
        tk_root.after(0, do_open_popup)

    def on_refresh(_icon, _item) -> None:
        state.trigger_refresh()

    def on_quit(_icon, _item) -> None:
        state.stop()
        tk_root.after(0, tk_root.quit)

    menu = pystray.Menu(
        pystray.MenuItem("View details…", on_open, default=True),
        pystray.MenuItem("Refresh now", on_refresh),
        pystray.Menu.SEPARATOR,
        pystray.MenuItem("Quit", on_quit),
    )
    icon = pystray.Icon(
        config.tray_name,
        icon=loading_icon(),
        title=f"{config.tooltip_name} – loading…",
        menu=menu,
    )
    state.icon = icon

    threading.Thread(target=icon.run, daemon=True, name=f"{config.tray_name}-tray").start()
    threading.Thread(
        target=state.worker_loop,
        daemon=True,
        name=f"{config.tray_name}-scheduler",
    ).start()
    try:
        tk_root.mainloop()
    finally:
        state.stop()
        try:
            tk_root.destroy()
        except Exception:
            pass
    return 0
