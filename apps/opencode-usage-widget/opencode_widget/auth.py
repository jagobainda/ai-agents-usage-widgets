"""Read the OpenCode Go API key without modifying OpenCode credentials."""

from __future__ import annotations

import json
from pathlib import Path

from .config import AUTH_PATH


class OpenCodeCredentialError(RuntimeError):
    """The local OpenCode Go credential is unavailable or malformed."""


def load_api_key(path: Path | None = None) -> str:
    credential_path = path or AUTH_PATH
    try:
        with credential_path.open("r", encoding="utf-8") as handle:
            payload = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise OpenCodeCredentialError("OpenCode credentials could not be read.") from exc

    if not isinstance(payload, dict):
        raise OpenCodeCredentialError("OpenCode credentials are malformed.")
    credential = payload.get("opencode-go")
    if not isinstance(credential, dict) or credential.get("type") != "api":
        raise OpenCodeCredentialError("OpenCode Go is not connected.")
    key = credential.get("key")
    if not isinstance(key, str) or not key.strip():
        raise OpenCodeCredentialError("OpenCode Go API key is missing.")
    return key.strip()
