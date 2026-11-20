"""Subprocess client for the official Antigravity CLI ``/usage`` command."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any, Optional


class AntigravityCLIError(RuntimeError):
    def __init__(self, message: str, detail: str = "") -> None:
        super().__init__(message)
        self.detail = detail


class AntigravityNotFoundError(AntigravityCLIError):
    pass


class AntigravityTimeoutError(AntigravityCLIError):
    pass


class AntigravityPayloadError(AntigravityCLIError):
    pass


class AntigravityClosedError(AntigravityCLIError):
    pass


def _default_executable() -> Optional[str]:
    executable = shutil.which("agy")
    if executable:
        return executable
    local_app_data = os.environ.get("LOCALAPPDATA")
    if local_app_data:
        candidate = Path(local_app_data) / "agy" / "bin" / "agy.exe"
        if candidate.is_file():
            return str(candidate)
    return None


def _extract_json(stdout: str) -> Optional[dict[str, Any]]:
    text = stdout.strip()
    if not text:
        return None
    try:
        parsed = json.loads(text)
        return parsed if isinstance(parsed, dict) else None
    except ValueError:
        pass
    for line in reversed(text.splitlines()):
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            parsed = json.loads(line)
            if isinstance(parsed, dict):
                return parsed
        except ValueError:
            continue
    return None


class AntigravityCLIClient:
    def __init__(
        self,
        command: Optional[Sequence[str]] = None,
        *,
        timeout: float = 25.0,
        cli_timeout_seconds: int = 20,
        process_factory: Callable[..., subprocess.Popen] = subprocess.Popen,
    ) -> None:
        self._command = list(command) if command is not None else None
        self.timeout = timeout
        self.cli_timeout_seconds = max(1, cli_timeout_seconds)
        self._process_factory = process_factory
        self._lock = threading.Lock()
        self._active_process: Optional[subprocess.Popen] = None
        self._closed = False

    def _resolve_command(self) -> list[str]:
        if self._command is not None:
            return list(self._command)
        executable = _default_executable()
        if executable is None:
            raise AntigravityNotFoundError("Antigravity CLI was not found.")
        return [
            executable,
            "--print",
            "/usage",
            "--output-format",
            "json",
            "--print-timeout",
            f"{self.cli_timeout_seconds}s",
        ]

    @staticmethod
    def _terminate_process(process: subprocess.Popen) -> None:
        if process.poll() is not None:
            return
        try:
            process.terminate()
            process.wait(timeout=2.0)
        except (OSError, subprocess.TimeoutExpired):
            try:
                process.kill()
                process.wait(timeout=2.0)
            except (OSError, subprocess.TimeoutExpired):
                pass

    def fetch_usage_payload(self) -> dict[str, Any]:
        with self._lock:
            if self._closed:
                raise AntigravityClosedError("Antigravity CLI client is closed.")
            command = self._resolve_command()
            creation_flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
            try:
                process = self._process_factory(
                    command,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    encoding="utf-8",
                    errors="replace",
                    creationflags=creation_flags,
                )
            except FileNotFoundError as exc:
                raise AntigravityNotFoundError("Antigravity CLI was not found.") from exc
            except OSError as exc:
                raise AntigravityCLIError("Could not start Antigravity CLI.") from exc
            self._active_process = process

        try:
            stdout, stderr = process.communicate(timeout=self.timeout)
        except subprocess.TimeoutExpired as exc:
            self._terminate_process(process)
            raise AntigravityTimeoutError("Antigravity CLI timed out.") from exc
        except OSError as exc:
            self._terminate_process(process)
            raise AntigravityCLIError("Antigravity CLI failed while reading output.") from exc
        finally:
            with self._lock:
                if self._active_process is process:
                    self._active_process = None
            for stream in (process.stdout, process.stderr):
                if stream is not None:
                    try:
                        stream.close()
                    except (OSError, ValueError):
                        pass

        payload = _extract_json(stdout or "")
        if process.returncode != 0:
            detail_parts = []
            if isinstance(payload, dict):
                for key in ("error", "message", "response"):
                    value = payload.get(key)
                    if isinstance(value, str) and value.strip():
                        detail_parts.append(value.strip())
            if stderr and stderr.strip():
                detail_parts.append(stderr.strip())
            raise AntigravityCLIError(
                f"Antigravity CLI exited with code {process.returncode}.",
                detail=" ".join(detail_parts),
            )

        if payload is None:
            raise AntigravityPayloadError("Antigravity CLI returned invalid JSON.")

        status = payload.get("status")
        if isinstance(status, str) and status.upper() != "SUCCESS":
            detail = " ".join(
                str(payload.get(key)).strip()
                for key in ("error", "message", "response")
                if isinstance(payload.get(key), str) and str(payload.get(key)).strip()
            )
            raise AntigravityCLIError(
                f"Antigravity CLI returned status {status}.",
                detail=detail,
            )

        return payload

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
            process = self._active_process
            self._active_process = None
        if process is not None:
            self._terminate_process(process)
