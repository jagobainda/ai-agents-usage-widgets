"""Generate the provider-specific multi-size Windows executable icon."""
from __future__ import annotations

import sys
from pathlib import Path

SIZES = [16, 32, 48, 64, 128, 256]

REPO_ROOT = Path(__file__).resolve().parent.parent


def _logo_factory(app: str):
    app_dir = REPO_ROOT / "apps" / f"{app}-usage-widget"
    common_dir = REPO_ROOT / "packages" / "widget-common"
    for path in (app_dir, common_dir):
        sys.path.insert(0, str(path))
    if app == "claude":
        from claude_widget.branding import claude_logo

        return claude_logo
    if app == "codex":
        from codex_widget.branding import codex_logo

        return codex_logo
    if app == "opencode":
        from opencode_widget.branding import opencode_logo

        return opencode_logo
    raise ValueError(f"unknown app: {app}")


def main() -> int:
    if len(sys.argv) == 2:
        app, output = "claude", sys.argv[1]
    elif len(sys.argv) == 4 and sys.argv[1] == "--app":
        app, output = sys.argv[2].lower(), sys.argv[3]
    else:
        print(
            "usage: _make_icon.py [--app claude|codex|opencode] <out.ico>",
            file=sys.stderr,
        )
        return 2

    out = Path(output)
    out.parent.mkdir(parents=True, exist_ok=True)
    logo_factory = _logo_factory(app)
    frames = [logo_factory(size=s).convert("RGBA") for s in SIZES]

    # Largest frame is the base; smaller frames are passed via append_images
    # so each size in the .ico is the natively-rendered raster (not a
    # downsample of the 256px one).
    frames.sort(key=lambda im: im.size[0], reverse=True)
    base, *extras = frames
    base.save(out, format="ICO", append_images=extras)
    print(f"wrote {out}  [{app} branding]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
