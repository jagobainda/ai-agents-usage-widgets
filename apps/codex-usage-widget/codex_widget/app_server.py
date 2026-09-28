"""Thread-safe JSONL client for the official ``codex app-server`` process."""

from __future__ import annotations

import json
import queue
import shutil
import subprocess
import threading
import time
from collections.abc import Callable, Sequence
from typing import Any, Optional


class AppServerError(RuntimeError):
    pass


class AppServerNotFoundError(AppServerError):
    pass


class AppServerTransportError(AppServerError):
    pass


class AppServerTimeoutError(AppServerError):
    pass


class AppServerClosedError(AppServerError):
    pass


class AppServerRPCError(AppServerError):
    def __init__(self, code: object, message: object) -> None:
        self.code = code
        self.server_message = str(message or "App Server request failed")
        super().__init__(f"App Server error {code}")


class MessageRouter:
    """Correlate JSONL responses while retaining interleaved notifications."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._pending: dict[object, queue.Queue] = {}
        self.notifications: queue.Queue[dict[str, Any]] = queue.Queue()

    def register(self, request_id: object) -> queue.Queue:
        response_queue: queue.Queue = queue.Queue(maxsize=1)
        with self._lock:
            self._pending[request_id] = response_queue
        return response_queue

    def cancel(self, request_id: object) -> None:
        with self._lock:
            self._pending.pop(request_id, None)

    def dispatch(self, message: object) -> bool:
        if not isinstance(message, dict):
            return False
        if "id" in message:
            with self._lock:
                response_queue = self._pending.pop(message.get("id"), None)
            if response_queue is not None:
                response_queue.put(message)
                return True
            return False
        if isinstance(message.get("method"), str):
            self.notifications.put(message)
            return True
        return False

    def fail_all(self, error: BaseException) -> None:
        with self._lock:
            queues = list(self._pending.values())
            self._pending.clear()
        for response_queue in queues:
            try:
                response_queue.put_nowait(error)
            except queue.Full:
                pass


class CodexAppServerClient:
    """Manage one local App Server and restart it after transport failures."""

    def __init__(
        self,
        command: Optional[Sequence[str]] = None,
        *,
        request_timeout: float = 10.0,
        retries: int = 2,
        backoff_seconds: float = 0.25,
        process_factory: Callable[..., subprocess.Popen] = subprocess.Popen,
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self._command = list(command) if command is not None else None
        self.request_timeout = request_timeout
        self.retries = max(0, retries)
        self.backoff_seconds = max(0.0, backoff_seconds)
        self._process_factory = process_factory
        self._sleep = sleep
        self._router = MessageRouter()
        self._lifecycle_lock = threading.RLock()
        self._send_lock = threading.Lock()
        self._id_lock = threading.Lock()
        self._next_id = 1
        self._process: Optional[subprocess.Popen] = None
        self._reader_thread: Optional[threading.Thread] = None
        self._stderr_thread: Optional[threading.Thread] = None
        self._initialized = False
        self._closed = False

    @property
    def is_running(self) -> bool:
        process = self._process
        return process is not None and process.poll() is None

    def _resolve_command(self) -> list[str]:
        if self._command is not None:
            return list(self._command)
        executable = shutil.which("codex")
        if executable is None:
            raise AppServerNotFoundError("Codex CLI was not found in PATH")
        return [executable, "app-server"]

    def _new_id(self) -> int:
        with self._id_lock:
            request_id = self._next_id
            self._next_id += 1
        return request_id

    def start(self) -> None:
        with self._lifecycle_lock:
            if self._closed:
                raise AppServerClosedError("App Server client is closed")
            if self.is_running and self._initialized:
                return
            self._stop_process_locked()
            command = self._resolve_command()
            creation_flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
            try:
                process = self._process_factory(
                    command,
                    stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    encoding="utf-8",
                    errors="replace",
                    bufsize=1,
                    creationflags=creation_flags,
                )
            except FileNotFoundError as exc:
                raise AppServerNotFoundError("Codex CLI was not found in PATH") from exc
            except OSError as exc:
                raise AppServerTransportError("Could not start Codex App Server") from exc
            self._process = process
            self._reader_thread = threading.Thread(
                target=self._read_stdout,
                args=(process,),
                daemon=True,
                name="codex-app-server-stdout",
            )
            self._stderr_thread = threading.Thread(
                target=self._drain_stderr,
                args=(process,),
                daemon=True,
                name="codex-app-server-stderr",
            )
            self._reader_thread.start()
            self._stderr_thread.start()
            try:
                self._request_once(
                    "initialize",
                    {
                        "clientInfo": {
                            "name": "codex_usage_widget",
                            "title": "Codex Usage Widget",
                            "version": "1.0.0",
                        },
                        "capabilities": {},
                    },
                    timeout=self.request_timeout,
                )
                self._send_message({"method": "initialized", "params": {}})
                self._initialized = True
            except Exception:
                self._stop_process_locked()
                raise

    def _read_stdout(self, process: subprocess.Popen) -> None:
        stream = process.stdout
        if stream is None:
            self._router.fail_all(
                AppServerTransportError("Codex App Server stdout is unavailable")
            )
            return
        try:
            for line in iter(stream.readline, ""):
                line = line.strip()
                if not line:
                    continue
                try:
                    message = json.loads(line)
                except (TypeError, ValueError):
                    continue
                self._router.dispatch(message)
        except (OSError, ValueError):
            pass
        finally:
            if process is self._process and not self._closed:
                self._router.fail_all(
                    AppServerTransportError("Codex App Server exited unexpectedly")
                )

    @staticmethod
    def _drain_stderr(process: subprocess.Popen) -> None:
        """Drain stderr to avoid deadlocks without logging potentially sensitive data."""
        stream = process.stderr
        if stream is None:
            return
        try:
            for _line in iter(stream.readline, ""):
                pass
        except (OSError, ValueError):
            pass

    def _send_message(self, message: dict[str, Any]) -> None:
        process = self._process
        if process is None or process.poll() is not None or process.stdin is None:
            raise AppServerTransportError("Codex App Server is not running")
        serialized = json.dumps(message, separators=(",", ":"), ensure_ascii=True)
        try:
            with self._send_lock:
                process.stdin.write(serialized + "\n")
                process.stdin.flush()
        except (BrokenPipeError, OSError, ValueError) as exc:
            raise AppServerTransportError("Codex App Server connection was lost") from exc

    def _request_once(
        self,
        method: str,
        params: Optional[dict[str, Any]] = None,
        *,
        timeout: Optional[float] = None,
    ) -> Any:
        request_id = self._new_id()
        response_queue = self._router.register(request_id)
        message: dict[str, Any] = {"method": method, "id": request_id}
        if params is not None:
            message["params"] = params
        try:
            self._send_message(message)
        except Exception:
            self._router.cancel(request_id)
            raise
        try:
            response = response_queue.get(
                timeout=self.request_timeout if timeout is None else timeout
            )
        except queue.Empty as exc:
            self._router.cancel(request_id)
            raise AppServerTimeoutError(
                f"Timed out waiting for App Server method {method}"
            ) from exc
        if isinstance(response, BaseException):
            raise response
        error = response.get("error")
        if isinstance(error, dict):
            raise AppServerRPCError(error.get("code"), error.get("message"))
        if "result" not in response:
            raise AppServerTransportError("App Server returned an invalid response")
        return response.get("result")

    def request(
        self,
        method: str,
        params: Optional[dict[str, Any]] = None,
        *,
        timeout: Optional[float] = None,
        retries: Optional[int] = None,
    ) -> Any:
        attempts = self.retries if retries is None else max(0, retries)
        last_error: Optional[BaseException] = None
        for attempt in range(attempts + 1):
            try:
                self.start()
                return self._request_once(method, params, timeout=timeout)
            except (AppServerRPCError, AppServerNotFoundError, AppServerClosedError):
                raise
            except (AppServerTimeoutError, AppServerTransportError) as exc:
                last_error = exc
                with self._lifecycle_lock:
                    self._stop_process_locked()
                if attempt < attempts:
                    self._sleep(self.backoff_seconds * (2**attempt))
        if last_error is not None:
            raise last_error
        raise AppServerTransportError("Codex App Server request failed")

    def get_notification(self, timeout: Optional[float] = None) -> dict[str, Any]:
        return self._router.notifications.get(timeout=timeout)

    def _stop_process_locked(self) -> None:
        process = self._process
        reader_thread = self._reader_thread
        stderr_thread = self._stderr_thread
        self._process = None
        self._reader_thread = None
        self._stderr_thread = None
        self._initialized = False
        self._router.fail_all(AppServerTransportError("Codex App Server stopped"))
        if process is None:
            return
        try:
            if process.stdin is not None:
                process.stdin.close()
        except (OSError, ValueError):
            pass
        if process.poll() is None:
            try:
                # App Server exits cleanly on stdin EOF. This also lets a .cmd
                # launcher reap its Node child before we terminate the wrapper.
                process.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                pass
        if process.poll() is None:
            try:
                process.terminate()
                process.wait(timeout=2.0)
            except (OSError, subprocess.TimeoutExpired):
                try:
                    process.kill()
                    process.wait(timeout=2.0)
                except (OSError, subprocess.TimeoutExpired):
                    pass
        for stream in (process.stdout, process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except (OSError, ValueError):
                    pass
        current_thread = threading.current_thread()
        for thread in (reader_thread, stderr_thread):
            if thread is not None and thread is not current_thread:
                thread.join(timeout=0.5)

    def close(self) -> None:
        with self._lifecycle_lock:
            if self._closed:
                return
            self._closed = True
            self._stop_process_locked()
