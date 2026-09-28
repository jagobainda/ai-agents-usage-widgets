"""Claude Usage Widget entry point."""

from __future__ import annotations

import sys
from widget_common.app import run_widget

from .config import WIDGET_CONFIG
from .provider import ClaudeProvider


def _enable_system_trust_store() -> None:
    """Validate TLS against the OS certificate store instead of certifi's
    bundle. On corporate networks an SSL-inspection proxy re-signs HTTPS with
    a private root CA that IT installs into the Windows cert store but that
    certifi never sees, causing CERTIFICATE_VERIFY_FAILED. truststore patches
    ssl to use the platform store, where that CA is already trusted. Best
    effort: if truststore is unavailable we fall back to certifi silently.
    """
    try:
        import truststore

        truststore.inject_into_ssl()
    except Exception:
        pass


def main() -> int:
    _enable_system_trust_store()
    return run_widget(ClaudeProvider(), WIDGET_CONFIG)


if __name__ == "__main__":
    sys.exit(main())
