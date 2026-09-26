# Claude Usage Ultra

Your Claude Code plan usage in the VS Code status bar: the 5-hour session
window, the weekly all-models window, the fullest per-model weekly window,
reset countdowns and extra-usage spend. The numbers come from the Claude Code
CLI itself, so nothing here reads your credentials.

## Features

- **One status bar item, built from segments.** Each usage window renders as a
  coloured dot, a name and a percentage — `🟢 Session 7% / 3h 49m`,
  `🟠 Weekly 84% / 2d 6h` — with the countdown riding along beside the
  percentage it belongs to. Which segments appear, and in what order, is up to
  `claudeUsageUltra.show`: `plan`, `session`, `weekly`, `scoped` (the fullest
  per-model weekly window, for example Opus), `spend`, `reset`. Related
  segments sit tight together and a wider gap opens between the plan name, the
  windows, the money and the clock, so six numbers do not read as one run.

- **Per-window colour, from the dots.** A status bar item paints all of its
  text one colour, so severity is carried by emoji instead: green below
  `noticeAtPercent`, yellow past it, orange past `warnAtPercent`, red past
  `criticalAtPercent`. Claude Code's own `warning` and `critical` severities
  are honoured even when your thresholds have not been crossed.

- **Countdowns that are not written twice.** Per-model weekly windows ride the
  weekly cycle and reset with it, so a countdown is printed only the first time
  it appears. The standalone `reset` segment picks up whatever is left — the
  session and weekly clocks when no window has already shown them — and quietly
  disappears when there is nothing left to say. On a plan with no windows at
  all, where usage runs on credits, it counts down to the monthly credit
  renewal (midnight local time on the first of next month) instead.

- **Extra usage spend with its cap.** The `spend` segment shows what has been
  spent this month against the monthly limit, `$72.77 / $200`, so the number
  has a scale. Non-USD accounts get the currency code rather than a symbol that
  may not render, and an uncapped account shows the spend alone.

- **A tooltip with the full picture.** Hovering lists every window Claude Code
  reported — including ones no segment is showing — each with its dot,
  percentage, reset countdown and a note when it is the window currently
  limiting the account. Below that: the plan name, extra usage with its
  percentage, whether extra usage is switched off, when credits renew, and how
  long ago the reading was taken.

- **Click for details and actions.** Clicking the item opens the same summary
  as a notification with three buttons: **Refresh**, **Check on claude.ai**
  (opens `https://claude.ai/settings/usage`) and **Show log**.

- **Refreshes on a timer, on focus, and on demand.** A refresh runs at startup
  and then every `pollIntervalSeconds` (300 by default). Returning to the
  window refreshes too, if the reading is over a minute old and
  `refreshOnFocus` is on. Failed refreshes back off — the interval doubles per
  consecutive failure, up to 30 minutes — so a CLI that is broken or logged out
  is not hammered. Countdowns re-render every 30 seconds between refreshes,
  without running the CLI.

- **Stale readings say so.** When the last successful reading is older than
  `staleAfterMinutes`, `(stale)` is appended to the status bar text and the
  tooltip explains why, along with the error from the last failed attempt. The
  most recent good reading is remembered across restarts, so a reload does not
  start with a blank bar.

- **Live settings.** Changing `alignment` or `priority` moves the item
  immediately; changing the poll interval reschedules; everything else
  re-renders on the spot. No reload required.

## How it works, and what it does not touch

This extension never reads your OAuth token, `~/.claude/.credentials.json`, or
any other credential file, and it never calls the Anthropic API. Instead it
spawns the Claude Code CLI as a short-lived child process and speaks the CLI's
own stream-JSON control protocol over stdin and stdout: it sends an
`initialize` control request, and once the CLI answers, a `get_usage` request —
the same request the Claude Code extension makes on behalf of its own usage
panel. The CLI owns the credentials and makes the `/api/oauth/usage` call; this
extension only parses the reply and formats it.

The process is deliberately inert. It is started with an empty MCP
configuration (`--strict-mcp-config --mcp-config '{"mcpServers":{}}'`), which
skips connecting your MCP servers, and with `--no-session-persistence`, so no
session is written. It never runs a turn, and it is terminated as soon as the
usage response arrives. A refresh normally takes about a second; if the CLI has
not answered within `timeoutSeconds`, it is given up on.

The CLI it runs is always the binary bundled inside the official Claude Code
extension (`Anthropic.claude-code`), at `resources/native-binary/claude` —
`claude.exe` on Windows. There is no setting to point it elsewhere and no
`PATH` lookup, so a repository you open can never choose the program this
extension runs. It also runs from this extension's own storage folder rather
than your workspace, so a repository's `.claude/settings.json` is never
loaded into it.

If the Claude Code extension is not installed, the tooltip says so. Diagnostics
go to the **Claude Usage Ultra** output channel.

## Requirements

The official Claude Code extension, which ships the CLI this extension runs.
It is declared as a dependency, so VS Code installs it alongside this one. It
must be logged in with an account that has plan limits. Logins
without them (API key, Bedrock, Vertex) make the item read
`Claude usage n/a`, because there is no plan usage to report.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `claudeUsageUltra.label` | `"Claude"` | Text shown before the numbers, so the item is recognisable. Empty leaves the icon alone. |
| `claudeUsageUltra.show` | `["plan", "session", "weekly", "scoped", "spend", "reset"]` | Segments to render, in order. Any of `session`, `weekly`, `scoped`, `reset`, `spend`, `plan`. |
| `claudeUsageUltra.noticeAtPercent` | `50` | Yellow dot once a window passes this percentage. `100` drops this step. |
| `claudeUsageUltra.warnAtPercent` | `80` | Orange dot once a window passes this percentage. |
| `claudeUsageUltra.criticalAtPercent` | `95` | Red dot once a window passes this percentage. Claude Code's own critical windows always get one. |
| `claudeUsageUltra.errorAtPercent` | `95` | Deprecated, renamed to `criticalAtPercent`. A value still set here applies until the new setting is set. |
| `claudeUsageUltra.staleAfterMinutes` | `30` | Mark the reading stale this many minutes after the last successful refresh. |
| `claudeUsageUltra.pollIntervalSeconds` | `300` | How often to ask the CLI for usage. Minimum 30. Failed refreshes back off automatically. |
| `claudeUsageUltra.refreshOnFocus` | `true` | Refresh when the window regains focus and the reading is over a minute old. |
| `claudeUsageUltra.timeoutSeconds` | `45` | Give up on a refresh if the CLI has not answered in this long. |
| `claudeUsageUltra.alignment` | `"right"` | Which side of the status bar the item sits on. `left` or `right`. |
| `claudeUsageUltra.priority` | `100` | Status bar priority. Higher values sit further left within the chosen alignment. |

## Commands

- **Claude Usage Ultra: Show Usage Details**
  (`claudeUsageUltra.showDetails`) — the summary notification, with its
  Refresh, Check on claude.ai and Show log buttons. This is what clicking the
  status bar item runs.
- **Claude Usage Ultra: Refresh** (`claudeUsageUltra.refresh`) — query the CLI
  now, without waiting for the next poll.
- **Claude Usage Ultra: Open Usage Settings on claude.ai**
  (`claudeUsageUltra.openUsagePage`) — open `claude.ai/settings/usage`, the
  authority to check these numbers against.
- **Claude Usage Ultra: Show Log** (`claudeUsageUltra.showLog`) — reveal the
  output channel, which records each refresh, its duration and the windows it
  read, plus the reason for any failure.
