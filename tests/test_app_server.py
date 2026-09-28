from __future__ import annotations

import json
import queue
import subprocess
import sys
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

import test_support  # noqa: F401
from codex_widget.app_server import (
    AppServerNotFoundError,
    AppServerTimeoutError,
    CodexAppServerClient,
    MessageRouter,
)


SERVER_CODE = textwrap.dedent(
    r"""
    import json
    import pathlib
    import sys

    mode = sys.argv[1]
    marker = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else None
    for raw in sys.stdin:
        message = json.loads(raw)
        method = message.get("method")
        request_id = message.get("id")
        if method == "initialize":
            print(json.dumps({"id": request_id, "result": {"userAgent": "test"}}), flush=True)
        elif method == "initialized":
            continue
        elif mode == "interleave":
            print(json.dumps({"method": "account/rateLimits/updated", "params": {"test": True}}), flush=True)
            print(json.dumps({"id": request_id, "result": {"ok": True}}), flush=True)
        elif mode == "hang":
            continue
        elif mode == "restart":
            if not marker.exists():
                marker.write_text("first process exited", encoding="utf-8")
                sys.exit(7)
            print(json.dumps({"id": request_id, "result": {"restarted": True}}), flush=True)
        else:
            print(json.dumps({"id": request_id, "result": {}}), flush=True)
    """
)


def server_command(mode: str, marker: Path | None = None) -> list[str]:
    command = [sys.executable, "-u", "-c", SERVER_CODE, mode]
    if marker is not None:
        command.append(str(marker))
    return command


class MessageRouterTests(unittest.TestCase):
    def test_correlates_out_of_order_responses_by_id(self) -> None:
        router = MessageRouter()
        first = router.register(1)
        second = router.register(2)
        router.dispatch({"id": 2, "result": "second"})
        router.dispatch({"id": 1, "result": "first"})
        self.assertEqual(first.get_nowait()["result"], "first")
        self.assertEqual(second.get_nowait()["result"], "second")

    def test_keeps_interleaved_notification(self) -> None:
        router = MessageRouter()
        response = router.register(9)
        notification = {"method": "account/rateLimits/updated", "params": {}}
        router.dispatch(notification)
        router.dispatch({"id": 9, "result": {"ok": True}})
        self.assertEqual(router.notifications.get_nowait(), notification)
        self.assertTrue(response.get_nowait()["result"]["ok"])


class AppServerLifecycleTests(unittest.TestCase):
    def test_missing_codex_executable_is_reported(self) -> None:
        client = CodexAppServerClient(request_timeout=0.1, retries=0)
        with patch("codex_widget.app_server.shutil.which", return_value=None):
            with self.assertRaises(AppServerNotFoundError):
                client.request("account/rateLimits/read")
        client.close()

    def test_notification_can_arrive_before_response(self) -> None:
        client = CodexAppServerClient(
            server_command("interleave"),
            request_timeout=1.0,
            retries=0,
        )
        try:
            self.assertEqual(client.request("test/read"), {"ok": True})
            notification = client.get_notification(timeout=1.0)
            self.assertEqual(notification["method"], "account/rateLimits/updated")
        finally:
            client.close()

    def test_request_timeout_stops_process(self) -> None:
        client = CodexAppServerClient(
            server_command("hang"),
            request_timeout=0.2,
            retries=0,
        )
        with self.assertRaises(AppServerTimeoutError):
            client.request("test/hang")
        self.assertFalse(client.is_running)
        client.close()

    def test_dead_process_is_restarted_with_backoff(self) -> None:
        marker = test_support.REPO_ROOT / "tests" / ".restart-test.marker"
        marker.unlink(missing_ok=True)
        try:
            sleeps: list[float] = []
            client = CodexAppServerClient(
                server_command("restart", marker),
                request_timeout=1.0,
                retries=1,
                backoff_seconds=0.01,
                sleep=sleeps.append,
            )
            try:
                self.assertEqual(client.request("test/restart"), {"restarted": True})
                self.assertTrue(marker.exists())
                self.assertEqual(sleeps, [0.01])
            finally:
                client.close()
        finally:
            marker.unlink(missing_ok=True)

    def test_close_terminates_subprocess(self) -> None:
        client = CodexAppServerClient(
            server_command("normal"),
            request_timeout=1.0,
            retries=0,
        )
        client.start()
        process = client._process
        self.assertIsNotNone(process)
        self.assertIsNone(process.poll())
        client.close()
        self.assertFalse(client.is_running)
        self.assertIsNotNone(process.poll())


if __name__ == "__main__":
    unittest.main()
