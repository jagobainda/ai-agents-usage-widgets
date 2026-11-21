<div align="center">

<img src="apps/claude-usage-widget/assets/claude_icon.svg" alt="Claude" width="56" height="56">
&nbsp;&nbsp;&nbsp;&nbsp;
<img src="apps/codex-usage-widget/assets/codex_icon.svg" alt="Codex" width="56" height="56">
&nbsp;&nbsp;&nbsp;&nbsp;
<img src="apps/opencode-usage-widget/assets/opencode_icon.svg" alt="OpenCode" width="45" height="56">
&nbsp;&nbsp;&nbsp;&nbsp;
<img src="apps/kimi-usage-widget/assets/kimi_icon.svg" alt="Kimi" width="54" height="56">
&nbsp;&nbsp;&nbsp;&nbsp;
<img src="apps/antigravity-usage-widget/assets/antigravity_icon.svg" alt="Antigravity" width="56" height="56">

# Windows Usage Widgets

Independent Windows system tray widgets for monitoring **Claude Code**,
**OpenAI Codex**, **OpenCode Go**, **Kimi Code**, and **Google Antigravity**
usage.

Each widget displays the percentage used directly in its tray icon. Open it to
view every available usage window, the time remaining until each reset, and
when the data was last updated.

<br>

<img src="assets/widgets-overview.gif" alt="Demo of the usage widgets">

</div>

## Download and install

| Widget | Information displayed | Installer |
| --- | --- | --- |
| **Claude Usage Widget** | Claude Code usage windows | [Download version 2.0.0](https://cdn.jagoba.dev/downloads/usage-widgets/ClaudeUsageWidget-Setup-2.0.0.exe) |
| **Codex Usage Widget** | Account limits exposed by Codex CLI | [Download version 2.0.0](https://cdn.jagoba.dev/downloads/usage-widgets/CodexUsageWidget-Setup-2.0.0.exe) |
| **OpenCode Usage Widget** | OpenCode Go five-hour, weekly, and monthly windows | [Download version 2.0.0](https://cdn.jagoba.dev/downloads/usage-widgets/OpenCodeUsageWidget-Setup-2.0.0.exe) |
| **Kimi Usage Widget** | Kimi Code five-hour, weekly, and monthly windows | [Download version 2.0.0](https://cdn.jagoba.dev/downloads/usage-widgets/KimiUsageWidget-Setup-2.0.0.exe) |
| **Antigravity Usage Widget** | Google Antigravity model group quotas exposed by `agy` | [Download version 2.0.0](https://cdn.jagoba.dev/downloads/usage-widgets/AntigravityUsageWidget-Setup-2.0.0.exe) |

To install a widget:

1. Download its installer.
2. Run the `.exe` file and follow the setup wizard.
3. Enable **Start with Windows** if you want the widget to remain readily
   available.
4. When setup finishes, a new icon will appear in the system tray.

Each widget has its own process, installation directory, and startup setting.
You can install and use all five at the same time. On first launch, each widget
automatically promotes its icon to the visible notification area of the Windows
taskbar.

## Requirements and account access

### Claude Usage Widget

You need [Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview)
installed and signed in:

```powershell
claude login
```

After signing in, open Claude Usage Widget from the Start menu. The widget uses
the existing Claude Code session on your computer.

### Codex Usage Widget

You need a recent version of
[Codex CLI](https://developers.openai.com/codex/cli/) installed on Windows,
available in `PATH`, and signed in:

```powershell
codex login
codex --version
```

The widget communicates with `codex app-server`, the official Codex protocol,
and never accesses the credential file (`~/.codex/auth.json`) directly.

### OpenCode Usage Widget

You need [OpenCode](https://opencode.ai/docs/) and an active **OpenCode Go**
subscription. In OpenCode, run:

```text
/connect
```

Select **OpenCode Go** and enter your API key. The widget reuses that credential
in read-only mode; it does not copy or modify it. The stored credential is
named `opencode-go`.

> [!NOTE]
>
> OpenCode Usage Widget only displays OpenCode Go limits. The available
> endpoint does not include your Zen credit balance or usage from external
> providers configured in OpenCode.

### Kimi Usage Widget

You need [Kimi Code](https://www.kimi.com/code/docs/en/) installed and signed in
with a membership that includes Kimi Code:

```powershell
kimi login
```

The widget reuses the existing Kimi Code OAuth session and displays the same
account windows as the CLI's `/usage` command. The weekly window only appears
for legacy plans that include it. The widget also reports five-hour and shared
monthly usage, respects `KIMI_CODE_HOME`, and supports both the `.com` and
`.ai` regions.

### Antigravity Usage Widget

You need [Google Antigravity CLI](https://antigravity.google/docs/getting-started)
(`agy`) installed on Windows and signed in:

```powershell
agy
```

The widget queries `agy --print /usage --output-format json`, which reuses your
existing Antigravity session without accessing credentials directly, creating
conversations, or consuming quota. The widget displays all returned model groups
and usage windows with their reset times.

## Usage

- The number in the tray icon shows the percentage used in the primary window.
- Open the icon to view every window and its reset time.
- Select **Refresh now** to request an immediate update.
- Use the tray menu to open the details or quit the widget.
- If an update fails, the popup keeps the last valid data and displays the
  reason for the error.

The widgets only retrieve usage information. They do not create conversations
or send prompts to any model.

## Update or uninstall

To update a widget, download the latest version and run the installer over the
existing installation. Windows startup settings and credentials managed by
each tool remain separate.

To uninstall a widget, open **Windows Settings → Apps → Installed apps**, find
its name, and select **Uninstall**. Removing a widget does not sign you out or
delete credentials belonging to Claude Code, Codex, OpenCode, Kimi Code, or
Google Antigravity.

## Troubleshooting

- **The icon does not appear:** check the hidden icons area in the Windows
  system tray, then reopen the widget from the Start menu.
- **Not signed in:** authenticate with the corresponding tool and select
  **Refresh now**.
- **Codex CLI not found:** verify that `codex --version` works in a new terminal.
- **Antigravity CLI not found:** verify that `agy --help` works in a new terminal.
- **OpenCode Go subscription required:** confirm that the API key belongs to a
  workspace with an active Go subscription.
- **Kimi Code membership required:** confirm that your membership includes
  Kimi Code and that `kimi login` completes successfully.
- **Data temporarily unavailable:** check your Internet connection. The widget
  will retry automatically.

## Provider notices

Claude Usage Widget queries the private endpoint used by Claude Code; Anthropic
may change or remove it without notice. Codex Usage Widget requires a Codex CLI
version compatible with the `codex app-server` account methods. OpenCode Usage
Widget uses the Go usage endpoint, which is not currently documented as a
stable public contract. Kimi Usage Widget uses the same usage endpoint and
OAuth client as Kimi Code; Moonshot AI may change them without notice.
Antigravity Usage Widget requires a Google Antigravity CLI (`agy`) version
supporting `--print /usage --output-format json`.

## Build from source

On Windows, install the Rust stable toolchain with the MSVC target, the Visual
Studio C++ Build Tools and a Windows SDK. Install Inno Setup 6 to build
installers. Run the following commands from the repository root:

```powershell
cargo build --workspace --locked
cargo run -p claude-usage-widget
.\scripts\build-release.ps1 -App all -Version "2.0.0" -Installer
```

Cargo builds the executables in `target\debug\`. The release script writes
portable executables and installers to `releases\`. For one widget, replace
`all` with `claude`, `codex`, `opencode`, `kimi` or `antigravity`. Omit
`-Installer` for a portable release. Pass `-Sign -CertThumbprint` and, if needed,
`-KeyInfoFile` to sign with Certum.

The committed icons in `apps/*/assets/` were generated from the SVGs with
`widget_common::icons::render_logo` at 16, 32, 48, 64, 128 and 256 px.

## License

Distributed under [LICENSE.txt](LICENSE.txt).
