"""OAuth token loading, persistence and refresh."""

from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import requests

from .config import OAUTH_CLIENT_ID, TOKEN_PATH_CANDIDATES, TOKEN_URL


class RefreshTokenError(Exception):
    """The OAuth server rejected the stored refresh token (HTTP 400).

    Anthropic rotates refresh tokens on every use, so the copy held in memory
    goes stale whenever another client (typically the Claude Code CLI)
    refreshes first and rewrites the credentials file. Callers should reload
    the credentials from disk and retry once; if it still fails the refresh
    token is genuinely dead and the user must sign in again.
    """


@dataclass
class OAuthToken:
    access_token: str
    refresh_token: str
    expires_at_ms: int
    source_path: Path

    @property
    def is_expired(self) -> bool:
        return (self.expires_at_ms / 1000.0) <= (time.time() + 60)


def _find_token_file() -> Optional[Path]:
    for p in TOKEN_PATH_CANDIDATES:
        try:
            if p and p.is_file():
                return p
        except OSError:
            continue
    return None


def _try_keyring() -> Optional[OAuthToken]:
    try:
        import keyring  # type: ignore
    except ImportError:
        return None
    for service in ("Claude Code-credentials", "Claude Code", "Claude"):
        for user in ("default", os.environ.get("USERNAME", "")):
            try:
                raw = keyring.get_password(service, user)
            except Exception:
                raw = None
            if not raw:
                continue
            try:
                data = json.loads(raw)
                oauth = data.get("claudeAiOauth") or data
                return OAuthToken(
                    access_token=oauth["accessToken"],
                    refresh_token=oauth["refreshToken"],
                    expires_at_ms=int(oauth["expiresAt"]),
                    source_path=Path(f"keyring://{service}/{user}"),
                )
            except Exception:
                continue
    return None


def load_token() -> OAuthToken:
    path = _find_token_file()
    if path is not None:
        with path.open("r", encoding="utf-8") as fh:
            data = json.load(fh)
        oauth = data.get("claudeAiOauth") or data
        return OAuthToken(
            access_token=oauth["accessToken"],
            refresh_token=oauth["refreshToken"],
            expires_at_ms=int(oauth["expiresAt"]),
            source_path=path,
        )
    kr = _try_keyring()
    if kr:
        return kr
    raise FileNotFoundError(
        "Could not find Claude Code credentials. Tried: "
        + ", ".join(str(p) for p in TOKEN_PATH_CANDIDATES)
    )


def save_token(tok: OAuthToken) -> None:
    if str(tok.source_path).startswith("keyring://"):
        return
    try:
        with tok.source_path.open("r", encoding="utf-8") as fh:
            data = json.load(fh)
    except Exception:
        data = {}
    oauth = data.get("claudeAiOauth") or {}
    oauth["accessToken"] = tok.access_token
    oauth["refreshToken"] = tok.refresh_token
    oauth["expiresAt"] = tok.expires_at_ms
    data["claudeAiOauth"] = oauth
    tmp = tok.source_path.with_suffix(tok.source_path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(data, fh, indent=2)
    os.replace(tmp, tok.source_path)


def refresh_token(tok: OAuthToken) -> OAuthToken:
    body = {
        "grant_type": "refresh_token",
        "refresh_token": tok.refresh_token,
        "client_id": OAUTH_CLIENT_ID,
    }
    r = requests.post(TOKEN_URL, json=body, timeout=(5, 15))
    if r.status_code == 400:
        # invalid_grant / invalid_request: the refresh token is no longer
        # accepted. Surface it as a distinct, recoverable-by-reload signal
        # rather than a generic HTTPError the caller can't reason about.
        detail = ""
        try:
            j = r.json()
            detail = j.get("error_description") or j.get("error") or ""
        except Exception:
            pass
        raise RefreshTokenError(detail or "refresh token rejected")
    r.raise_for_status()
    j = r.json()
    expires_in = int(j.get("expires_in", 3600))
    new = OAuthToken(
        access_token=j["access_token"],
        refresh_token=j.get("refresh_token", tok.refresh_token),
        expires_at_ms=int((time.time() + expires_in) * 1000),
        source_path=tok.source_path,
    )
    save_token(new)
    return new
