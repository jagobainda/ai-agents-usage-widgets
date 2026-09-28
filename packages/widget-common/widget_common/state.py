"""Shared application state and non-blocking refresh scheduler."""

from __future__ import annotations

import threading
import time
import traceback
from typing import TYPE_CHECKING, Callable, Optional

from .config import WidgetConfig
from .icons import error_icon, usage_icon
from .models import Usage
from .provider import ProviderError, UsageProvider
from .utils import format_until

if TYPE_CHECKING:
    import pystray


_FETCH_DEADLINE_SECONDS = 30.0
_RESULT_GRACE_SECONDS = 8.0


class AppState:
    def __init__(self, provider: UsageProvider, config: WidgetConfig) -> None:
        self.provider = provider
        self.config = config
        self.usage: Optional[Usage] = None
        self.last_error: Optional[str] = None
        self.icon: Optional["pystray.Icon"] = None
        self._refresh_event = threading.Event()
        self._stop_event = threading.Event()
        self._result_ready = threading.Event()
        self._listeners: list[Callable[[], None]] = []
        self._fetch_lock = threading.Lock()
        self._fetch_in_flight = False
        self._fetch_started_at = 0.0
        self._fetch_seq = 0
        self._force_dispatch = False
        self._close_lock = threading.Lock()
        self._closed = False

    def add_listener(self, callback: Callable[[], None]) -> None:
        self._listeners.append(callback)

    def remove_listener(self, callback: Callable[[], None]) -> None:
        try:
            self._listeners.remove(callback)
        except ValueError:
            pass

    def _notify(self) -> None:
        for callback in list(self._listeners):
            try:
                callback()
            except Exception:
                traceback.print_exc()

    def trigger_refresh(self) -> None:
        self._force_dispatch = True
        self._refresh_event.set()

    def stop(self) -> None:
        self._stop_event.set()
        self._refresh_event.set()
        if self.icon is not None:
            try:
                self.icon.stop()
            except Exception:
                pass
        with self._close_lock:
            if self._closed:
                return
            self._closed = True
        try:
            self.provider.close()
        except Exception:
            pass

    def _publish_success(self, usage: Usage) -> None:
        self.usage = usage
        self.last_error = None
        if self.icon is not None:
            self.icon.icon = usage_icon(
                usage,
                color=self.config.accent,
            )
            summary = "  ·  ".join(
                f"{limit.short_label}: {int(round(limit.utilization * 100))}%"
                for limit in usage.limits
            )
            lines = [self.config.tooltip_name]
            if summary:
                lines.append(summary)
            if usage.primary is not None:
                lines.append(
                    f"{usage.primary.short_label} reset: "
                    f"{format_until(usage.primary.resets_at)}"
                )
            self.icon.title = "\n".join(lines)
        self._result_ready.set()
        self._notify()

    def _publish_error(self, message: str) -> None:
        self.last_error = message
        if self.icon is not None:
            self.icon.icon = error_icon()
            self.icon.title = f"{self.config.tooltip_name} – error\n{message}"
        self._result_ready.set()
        self._notify()

    def _dispatch_fetch(self, force: bool = False) -> None:
        with self._fetch_lock:
            now = time.monotonic()
            in_flight = self._fetch_in_flight and (
                now - self._fetch_started_at
            ) < _FETCH_DEADLINE_SECONDS
            if in_flight and not force:
                return
            self._fetch_seq += 1
            sequence = self._fetch_seq
            self._fetch_in_flight = True
            self._fetch_started_at = now

        def finish_in_flight() -> bool:
            with self._fetch_lock:
                is_latest = sequence == self._fetch_seq
                if is_latest:
                    self._fetch_in_flight = False
                return is_latest

        def run() -> None:
            try:
                usage = self.provider.fetch_usage()
            except ProviderError as exc:
                if finish_in_flight():
                    self._publish_error(str(exc))
                return
            except Exception as exc:  # noqa: BLE001
                traceback.print_exc()
                if finish_in_flight():
                    self._publish_error(f"{type(exc).__name__}: {exc}")
                return
            if finish_in_flight():
                self._publish_success(usage)

        threading.Thread(
            target=run,
            daemon=True,
            name=f"{self.config.tray_name}-fetch-{sequence}",
        ).start()

    def worker_loop(self) -> None:
        error_backoff = 5.0
        while not self._stop_event.is_set():
            force = self._force_dispatch
            self._force_dispatch = False
            self._result_ready.clear()
            self._dispatch_fetch(force=force)
            self._result_ready.wait(timeout=_RESULT_GRACE_SECONDS)

            if self.last_error is None:
                wait_seconds = self.config.refresh_interval_seconds
                error_backoff = 5.0
            else:
                wait_seconds = min(
                    error_backoff,
                    self.config.refresh_interval_seconds,
                )
                error_backoff = min(
                    error_backoff * 2,
                    self.config.refresh_interval_seconds,
                )

            self._refresh_event.wait(timeout=wait_seconds)
            self._refresh_event.clear()
