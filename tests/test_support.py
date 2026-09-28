"""Import-path setup for source-tree tests."""

from __future__ import annotations

import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
for relative in (
    "packages/widget-common",
    "apps/claude-usage-widget",
    "apps/codex-usage-widget",
    "apps/opencode-usage-widget",
):
    path = str(REPO_ROOT / relative)
    if path not in sys.path:
        sys.path.insert(0, path)
