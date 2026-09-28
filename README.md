<div align="center">

<img src="apps/claude-usage-widget/claude_widget/claude_icon.svg" alt="Claude" width="56" height="56">
&nbsp;&nbsp;&nbsp;
<img src="apps/codex-usage-widget/codex_widget/codex_icon.svg" alt="Codex" width="56" height="56">
&nbsp;&nbsp;&nbsp;
<img src="apps/opencode-usage-widget/opencode_widget/opencode_icon.svg" alt="OpenCode" width="56" height="56">

# Windows Usage Widgets

Independent Windows system tray widgets for monitoring **Claude Code**,
**OpenAI Codex**, and **OpenCode Go** usage.

Each widget displays the percentage used directly in its tray icon. Open it to
view every available usage window, the time remaining until each reset, and
when the data was last updated.

<br>

<img src="assets/widgets-overview.gif" alt="Demo of the three usage widgets">

</div>

## Download and install

| Widget | Information displayed | Installer |
| --- | --- | --- |
| **Claude Usage Widget** | Claude Code usage windows | [Download version 1.0.5](https://cdn.jagoba.dev/downloads/usage-widgets/ClaudeUsageWidget-Setup-1.0.5.exe) |
| **Codex Usage Widget** | Account limits exposed by Codex CLI | [Download version 1.0.5](https://cdn.jagoba.dev/downloads/usage-widgets/CodexUsageWidget-Setup-1.0.5.exe) |
| **OpenCode Usage Widget** | OpenCode Go five-hour, weekly, and monthly windows | [Download version 1.0.5](https://cdn.jagoba.dev/downloads/usage-widgets/OpenCodeUsageWidget-Setup-1.0.5.exe) |

To install a widget:

1. Download its installer.
2. Run the `.exe` file and follow the setup wizard.
3. Enable **Start with Windows** if you want the widget to remain readily
   available.
4. When setup finishes, a new icon will appear in the system tray.

Each widget has its own process, installation directory, and startup setting.
You can install and use all three at the same time.

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
and never accesses the credential file directly.

### OpenCode Usage Widget

You need [OpenCode](https://opencode.ai/docs/) and an active **OpenCode Go**
subscription. In OpenCode, run:

```text
/connect
```

Select **OpenCode Go** and enter your API key. The widget reuses that credential
in read-only mode; it does not copy or modify it.

> [!NOTE]
>
> OpenCode Usage Widget only displays OpenCode Go limits. The available
> endpoint does not include your Zen credit balance or usage from external
> providers configured in OpenCode.

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
delete credentials belonging to Claude Code, Codex, or OpenCode.

## Troubleshooting

- **The icon does not appear:** check the hidden icons area in the Windows
  system tray, then reopen the widget from the Start menu.
- **Not signed in:** authenticate with the corresponding tool and select
  **Refresh now**.
- **Codex CLI not found:** verify that `codex --version` works in a new terminal.
- **OpenCode Go subscription required:** confirm that the API key belongs to a
  workspace with an active Go subscription.
- **Data temporarily unavailable:** check your Internet connection. The widget
  will retry automatically.

## Provider notices

Claude Usage Widget queries the private endpoint used by Claude Code; Anthropic
may change or remove it without notice. Codex Usage Widget requires a Codex CLI
version compatible with the `codex app-server` account methods. OpenCode Usage
Widget uses the Go usage endpoint, which is not currently documented as a
stable public contract.

## License

Distributed under [LICENSE.txt](LICENSE.txt).
