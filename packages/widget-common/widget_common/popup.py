"""Shared dark popup window for provider usage limits."""

from __future__ import annotations

import tkinter as tk
from typing import Optional

from PIL import ImageTk

from .state import AppState
from .theme import Theme, status_color_hex
from .utils import format_until


class HoverButton(tk.Label):
    def __init__(
        self,
        master: tk.Misc,
        text: str,
        command,
        *,
        bg: str = Theme.BTN_BG,
        bg_hover: str = Theme.BTN_BG_HOV,
        fg: str = Theme.BTN_FG,
        font=Theme.FONT_BODY,
        padx: int = 14,
        pady: int = 6,
    ) -> None:
        super().__init__(
            master,
            text=text,
            bg=bg,
            fg=fg,
            font=font,
            padx=padx,
            pady=pady,
            cursor="hand2",
            borderwidth=0,
            highlightthickness=0,
        )
        self._bg = bg
        self._bg_hover = bg_hover
        self._command = command
        self.bind("<Enter>", lambda _event: self.configure(bg=self._bg_hover))
        self.bind("<Leave>", lambda _event: self.configure(bg=self._bg))
        self.bind("<Button-1>", self._on_click)

    def _on_click(self, _event) -> None:
        if callable(self._command):
            self._command()


class RoundedProgress(tk.Canvas):
    def __init__(
        self,
        master: tk.Misc,
        *,
        width: int = 360,
        height: int = 10,
        track: str = Theme.TRACK,
        fill: str = Theme.OK,
    ) -> None:
        super().__init__(
            master,
            width=width,
            height=height,
            bg=master["bg"],
            highlightthickness=0,
            borderwidth=0,
        )
        self._bar_width = width
        self._bar_height = height
        self._track = track
        self._fill = fill
        self._value = 0.0
        self.after_idle(self._draw)

    def _round_rect(self, x1, y1, x2, y2, radius, **kwargs):
        if x2 - x1 < 2 * radius:
            radius = max(0, (x2 - x1) // 2)
        if y2 - y1 < 2 * radius:
            radius = max(0, (y2 - y1) // 2)
        if radius <= 0:
            return self.create_rectangle(x1, y1, x2, y2, **kwargs)
        points = [
            x1 + radius, y1, x2 - radius, y1, x2, y1, x2, y1 + radius,
            x2, y2 - radius, x2, y2, x2 - radius, y2, x1 + radius, y2,
            x1, y2, x1, y2 - radius, x1, y1 + radius, x1, y1,
        ]
        return self.create_polygon(points, smooth=True, **kwargs)

    def set(self, value: float, fill: Optional[str] = None) -> None:
        self._value = max(0.0, min(1.0, value))
        if fill is not None:
            self._fill = fill
        self._draw()

    def _draw(self) -> None:
        self.delete("all")
        radius = self._bar_height // 2
        self._round_rect(
            0,
            0,
            self._bar_width,
            self._bar_height,
            radius,
            fill=self._track,
            outline="",
        )
        if self._value > 0:
            width = max(self._bar_height, int(self._bar_width * self._value))
            self._round_rect(
                0,
                0,
                width,
                self._bar_height,
                radius,
                fill=self._fill,
                outline="",
            )


class PopupWindow:
    WIDTH = 380
    PAD = 16

    def __init__(self, state: AppState, master: tk.Tk, on_close=None) -> None:
        self.state = state
        self._on_close = on_close
        self.root = tk.Toplevel(master)
        self.root.geometry("+9999+9999")
        self.root.title(self.state.config.popup_title)
        self.root.configure(bg=Theme.BG)
        self.root.overrideredirect(True)
        self.root.attributes("-topmost", True)
        self.root.resizable(False, False)

        self.outer = tk.Frame(self.root, bg=Theme.BORDER)
        self.outer.pack(fill="both", expand=True)
        self.body = tk.Frame(self.outer, bg=Theme.BG)
        self.body.pack(fill="both", expand=True, padx=1, pady=1)

        self._icon_imgref: Optional[ImageTk.PhotoImage] = None
        self._title_icon_label: Optional[tk.Label] = None
        self._build_titlebar()

        self.content = tk.Frame(self.body, bg=Theme.BG)
        self.content.pack(fill="both", expand=True, padx=self.PAD, pady=(8, self.PAD))
        self._content_widgets: list[tk.Widget] = []
        self._cards: list[tuple[tk.Label, RoundedProgress, tk.Label]] = []
        self._rendered_keys: tuple[str, ...] = ()
        self._rendered_error: Optional[str] = None
        self._fetched_label: Optional[tk.Label] = None
        self._render()

        self._listener = self._on_state_change
        self.state.add_listener(self._listener)
        self._tick_job: Optional[str] = None
        self._schedule_tick()

        self.root.bind("<Escape>", lambda _event: self.close())
        self.root.protocol("WM_DELETE_WINDOW", self.close)
        self.root.after(1, self._show)

    def _build_titlebar(self) -> None:
        bar = tk.Frame(self.body, bg=Theme.TITLEBAR, height=36)
        bar.pack(fill="x", side="top")
        bar.pack_propagate(False)

        self._title_icon_label = tk.Label(bar, bg=Theme.TITLEBAR, bd=0)
        self._title_icon_label.pack(side="left", padx=(10, 8), pady=4)
        self._update_title_icon()

        title = tk.Label(
            bar,
            text=self.state.config.popup_title,
            bg=Theme.TITLEBAR,
            fg=Theme.TEXT,
            font=Theme.FONT_TITLE,
        )
        title.pack(side="left")

        close_button = HoverButton(
            bar,
            text="✕",
            command=self.close,
            bg=Theme.TITLEBAR,
            bg_hover=self.state.config.accent_hover,
            fg=self.state.config.accent,
            font=(Theme.FONT_FAMILY, 11, "bold"),
            padx=12,
            pady=8,
        )
        close_button.pack(side="right")

        refresh_button = HoverButton(
            bar,
            text="⟳",
            command=self.state.trigger_refresh,
            bg=Theme.TITLEBAR,
            bg_hover=Theme.BTN_BG_HOV,
            fg=Theme.TEXT_MUTED,
            font=(Theme.FONT_FAMILY, 11, "bold"),
            padx=10,
            pady=8,
        )
        refresh_button.pack(side="right")

        for widget in (bar, title, self._title_icon_label):
            widget.bind("<ButtonPress-1>", self._start_move)
            widget.bind("<B1-Motion>", self._do_move)

    def _build_card(self, title_text: str):
        card = tk.Frame(self.content, bg=Theme.CARD)
        card.pack(fill="x", pady=(0, 10))
        inner = tk.Frame(card, bg=Theme.CARD)
        inner.pack(fill="x", padx=14, pady=12)
        head = tk.Frame(inner, bg=Theme.CARD)
        head.pack(fill="x")
        tk.Label(
            head,
            text=title_text,
            bg=Theme.CARD,
            fg=self.state.config.accent,
            font=Theme.FONT_HEAD,
        ).pack(side="left")
        percentage = tk.Label(
            head,
            text="—",
            bg=Theme.CARD,
            fg=Theme.TEXT,
            font=Theme.FONT_PCT,
        )
        percentage.pack(side="right")
        progress = RoundedProgress(
            inner,
            width=self.WIDTH - 2 * self.PAD - 28,
            height=10,
        )
        progress.pack(fill="x", pady=(10, 8))
        reset = tk.Label(
            inner,
            text="Resets in —",
            bg=Theme.CARD,
            fg=Theme.TEXT_MUTED,
            font=Theme.FONT_BODY,
            anchor="w",
        )
        reset.pack(fill="x")
        return card, percentage, progress, reset

    def _clear_content(self) -> None:
        for widget in self._content_widgets:
            widget.destroy()
        self._content_widgets.clear()
        self._cards.clear()
        self._rendered_keys = ()
        self._fetched_label = None

    def _render(self) -> None:
        self._clear_content()
        usage = self.state.usage
        error = self.state.last_error
        if usage is None:
            placeholder = tk.Label(
                self.content,
                text="Loading usage data…" if not error else "Could not fetch usage data.",
                bg=Theme.BG,
                fg=Theme.TEXT,
                font=Theme.FONT_HEAD,
                anchor="w",
            )
            placeholder.pack(fill="x", pady=(4, 6))
            self._content_widgets.append(placeholder)
            if error:
                error_label = tk.Label(
                    self.content,
                    text=str(error),
                    bg=Theme.BG,
                    fg=Theme.DANGER,
                    font=Theme.FONT_BODY,
                    wraplength=self.WIDTH - 2 * self.PAD,
                    justify="left",
                    anchor="w",
                )
                error_label.pack(fill="x", pady=(0, 6))
                self._content_widgets.append(error_label)
        else:
            for limit in usage.limits:
                card, percentage, progress, reset = self._build_card(limit.label)
                self._content_widgets.append(card)
                self._cards.append((percentage, progress, reset))
            self._rendered_keys = tuple(limit.key for limit in usage.limits)

            for detail in usage.details:
                label = tk.Label(
                    self.content,
                    text=detail,
                    bg=Theme.BG,
                    fg=Theme.TEXT_MUTED,
                    font=Theme.FONT_BODY,
                    anchor="w",
                )
                label.pack(fill="x", pady=(0, 2))
                self._content_widgets.append(label)

            if error:
                error_label = tk.Label(
                    self.content,
                    text=f"Last refresh failed · {error}",
                    bg=Theme.BG,
                    fg=Theme.DANGER,
                    font=Theme.FONT_BODY,
                    wraplength=self.WIDTH - 2 * self.PAD,
                    justify="left",
                    anchor="w",
                )
                error_label.pack(fill="x", pady=(2, 4))
                self._content_widgets.append(error_label)

            self._fetched_label = tk.Label(
                self.content,
                text="",
                bg=Theme.BG,
                fg=Theme.TEXT_DIM,
                font=Theme.FONT_SMALL,
                anchor="w",
            )
            self._fetched_label.pack(fill="x", pady=(2, 0))
            self._content_widgets.append(self._fetched_label)
            self._update_values(usage)
        self._rendered_error = error
        self.root.update_idletasks()

    def _update_values(self, usage) -> None:
        for (percentage, progress, reset), limit in zip(self._cards, usage.limits):
            color = status_color_hex(limit.utilization)
            percentage.configure(
                text=f"{int(round(limit.utilization * 100))}%",
                fg=color,
            )
            progress.set(limit.utilization, fill=color)
            reset.configure(text=f"Resets in {format_until(limit.resets_at)}")
        if self._fetched_label is not None:
            timestamp = usage.fetched_at.astimezone().strftime("%H:%M:%S")
            self._fetched_label.configure(text=f"Updated · {timestamp}")

    def _update_title_icon(self) -> None:
        image = self.state.config.logo_factory(20)
        self._icon_imgref = ImageTk.PhotoImage(image)
        if self._title_icon_label is not None:
            self._title_icon_label.configure(image=self._icon_imgref)

    def _on_state_change(self) -> None:
        try:
            self.root.after(0, self._refresh_ui)
        except Exception:
            pass

    def _refresh_ui(self) -> None:
        try:
            self._update_title_icon()
        except Exception:
            pass
        usage = self.state.usage
        keys = tuple(limit.key for limit in usage.limits) if usage is not None else ()
        if keys != self._rendered_keys or self.state.last_error != self._rendered_error:
            self._render()
        elif usage is not None:
            self._update_values(usage)

    def _schedule_tick(self) -> None:
        if self.state.usage is not None:
            self._update_values(self.state.usage)
        self._tick_job = self.root.after(30_000, self._schedule_tick)

    def _start_move(self, event) -> None:
        self._drag_x = event.x_root - self.root.winfo_x()
        self._drag_y = event.y_root - self.root.winfo_y()

    def _do_move(self, event) -> None:
        x = event.x_root - getattr(self, "_drag_x", 0)
        y = event.y_root - getattr(self, "_drag_y", 0)
        self.root.geometry(f"+{x}+{y}")

    def _show(self) -> None:
        self.root.update_idletasks()
        width = max(self.WIDTH, self.root.winfo_reqwidth())
        height = max(1, self.root.winfo_reqheight())
        x = max(0, self.root.winfo_screenwidth() - width - 20)
        y = max(0, self.root.winfo_screenheight() - height - 60)
        self.root.geometry(f"{width}x{height}+{x}+{y}")
        self.root.lift()
        self.root.focus_force()

    def close(self) -> None:
        try:
            self.state.remove_listener(self._listener)
        except Exception:
            pass
        if self._tick_job is not None:
            try:
                self.root.after_cancel(self._tick_job)
            except Exception:
                pass
        if self._on_close is not None:
            try:
                self._on_close()
            except Exception:
                pass
        try:
            self.root.destroy()
        except Exception:
            pass
