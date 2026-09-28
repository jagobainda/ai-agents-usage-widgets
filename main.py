"""Compatibility entry point for the original Claude widget command."""

from __future__ import annotations

import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent
CLAUDE_APP = REPO_ROOT / "apps" / "claude-usage-widget"
COMMON_PACKAGE = REPO_ROOT / "packages" / "widget-common"
for path in (CLAUDE_APP, COMMON_PACKAGE):
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))

from claude_widget import main  # noqa: E402


if __name__ == "__main__":
    sys.exit(main())
