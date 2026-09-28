"""Run OpenCode Usage Widget from the monorepo source tree."""

from __future__ import annotations

import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
COMMON_PACKAGE = REPO_ROOT / "packages" / "widget-common"
if str(COMMON_PACKAGE) not in sys.path:
    sys.path.insert(0, str(COMMON_PACKAGE))

from opencode_widget import main  # noqa: E402


if __name__ == "__main__":
    sys.exit(main())
