"""Codex account usage provider backed exclusively by App Server."""

from __future__ import annotations

from widget_common.models import Usage
from widget_common.provider import ProviderError

from .app_server import (
    AppServerNotFoundError,
    AppServerRPCError,
    AppServerTimeoutError,
    AppServerTransportError,
    CodexAppServerClient,
)
from .usage import parse_rate_limits_response


def _rpc_provider_error(error: AppServerRPCError) -> ProviderError:
    message = error.server_message.lower()
    if error.code == -32601 or "method not found" in message:
        return ProviderError(
            "This Codex CLI version does not support account usage. Update Codex CLI."
        )
    if any(
        term in message
        for term in ("unauthorized", "not logged", "login", "sign in", "auth", "credential")
    ):
        return ProviderError(
            "Codex is not signed in. Run 'codex login' and try again."
        )
    return ProviderError(
        f"Codex App Server could not return usage data (error {error.code})."
    )


class CodexProvider:
    def __init__(self, client: CodexAppServerClient | None = None) -> None:
        self.client = client or CodexAppServerClient()

    def fetch_usage(self) -> Usage:
        try:
            result = self.client.request("account/rateLimits/read")
            usage = parse_rate_limits_response(result)
            if usage.limits:
                return usage

            try:
                account = self.client.request(
                    "account/read",
                    {"refreshToken": False},
                    retries=0,
                )
            except AppServerRPCError:
                account = None
            if isinstance(account, dict):
                if account.get("requiresOpenaiAuth") is True and account.get("account") is None:
                    raise ProviderError(
                        "Codex is not signed in. Run 'codex login' and try again."
                    )
            raise ProviderError("Codex App Server returned no usage windows.")
        except ProviderError:
            raise
        except AppServerNotFoundError as exc:
            raise ProviderError(
                "Codex CLI is not installed or is not in PATH. Install Codex CLI first."
            ) from exc
        except AppServerRPCError as exc:
            raise _rpc_provider_error(exc) from exc
        except AppServerTimeoutError as exc:
            raise ProviderError("Codex App Server timed out. The widget will retry.") from exc
        except AppServerTransportError as exc:
            raise ProviderError(
                "Codex App Server could not start or stopped unexpectedly. "
                "Verify or update Codex CLI; the widget will retry."
            ) from exc

    def close(self) -> None:
        self.client.close()
