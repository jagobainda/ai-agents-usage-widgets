"""Codex Usage Widget entry point."""

from __future__ import annotations

import sys

from widget_common.app import run_widget

from .config import WIDGET_CONFIG
from .provider import CodexProvider


def main() -> int:
    return run_widget(CodexProvider(), WIDGET_CONFIG)


if __name__ == "__main__":
    sys.exit(main())
