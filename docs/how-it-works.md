# How Promptagotchi works

```
 Claude Code sessions                       Promptagotchi (Rust core, Tauri)
 ─────────────────────                      ─────────────────────────────────────────
 ~/.claude/sessions/<pid>.json  ──watch──▶  SessionFileWatcher ─┐
                                                                ├─▶ SessionStore ──▶ overlay window
 hooks → promptagotchi-hook.sh ──POST──▶  127.0.0.1:47386 ────┘     (state per       (pet, bubble,
                       ▲                                              session)        cards, answers)
                       └──────── hook output (allow / deny / answers) ◀── you click a button
```

## 1. Session status files (zero setup)

Claude Code (the CLI and the desktop app's Code tab) keeps one small JSON file per running session in `~/.claude/sessions/<pid>.json`. Fields used:

- `pid`
- `sessionId`
- `cwd`
- `name`: the session title
- `status`: `busy` / `idle` / …
- `entrypoint`
- `hostSessionId`

Promptagotchi polls that folder once a second (plus `fs.watch`), and only keeps files whose process is still alive. If a session's file says `busy` but neither the file nor any hook has said anything for 30 minutes, the pet stops typing for it and its card says it might be waiting on you. A desktop session sitting on a prompt can look like that. When hooks report a `Stop`, that beats a stale `busy` file. That's enough to list every session, show which ones are working, and notice when one finishes (busy → idle). It never opens the `*.key` files that sit next to the status files.

## 2. Hooks (optional, live detail)

**Connect hooks** adds these entries to `~/.claude/settings.json`:

| Event | Mode | Used for |
| --- | --- | --- |
| `SessionStart`, `SessionEnd` | async | Sessions appearing and disappearing |
| `UserPromptSubmit` | async | Working starts; the session title |
| `PreToolUse`, `PostToolUse`, `PostToolUseFailure` | async | "Running: npm test", "Editing app.ts"; plan steps from `TodoWrite` / `update_plan` |
| `SubagentStart`, `SubagentStop` | async | The subagent list, and what each subagent is doing |
| `Notification` | async | "Needs your permission" and similar |
| `Stop` | async | Done, with Claude's last message |
| `PermissionRequest` | sync* | Allow / Deny from the pet |
| `PreToolUse` matching `AskUserQuestion` | sync* | Picking an answer from the pet |

\* Only added when you choose **Connect + answering**.

Every hook runs `sh "$HOME/.promptagotchi/promptagotchi-hook.sh" <mode>`, which pipes the hook's JSON to the local server with `curl`:

- **Pet not running:** the connection is refused and the script exits 0 in a few milliseconds. Claude doesn't notice a thing.
- **Hook script missing:** the command ends in `|| true`, so Claude still carries on.
- **Async hooks** don't block Claude at all.

### Holding a prompt

For `PermissionRequest` and `AskUserQuestion`, the server decides:

1. **Is answering on, and is the pet visible?** If not, it answers `{}` straight away, and Claude shows its own prompt.
2. **Are you looking at the app running that session right now?** The frontmost app comes from `lsappinfo`, and the session's app from the hook's `__CFBundleIdentifier`. If you are, it answers `{}` straight away. You'll answer in Claude like normal.
3. **Otherwise it holds the request** and shows the question with buttons. The hold ends when you:
   - **answer**: the hook prints the decision (`{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}`, or `updatedInput.answers` for questions);
   - **switch to the session's app, click Open ↗, or the hold time runs out**: the hook prints nothing, and Claude shows its own prompt;
   - **interrupt Claude**, which kills the hook process: the server notices the closed connection and lets go.

## 3. Jumping to a session

| Session runs in | How Promptagotchi opens it |
| --- | --- |
| Claude desktop app (Code tab) | `claude://code/continue?session=local_…`, using the session file's `hostSessionId` |
| VS Code extension | `vscode://anthropic.claude-code/open?session=<id>` |
| Terminal.app / iTerm2 | Walks up the process tree from the session's pid to find its tty, then selects that tab with AppleScript (macOS asks once for Automation permission) |
| Anything else | Brings the hosting app to the front (`open -a`) |

## 4. Windows, size and placement

The pet and the session list are two separate transparent, always-on-top windows. They're non-activating macOS panels, so clicking them never steals focus from what you're doing. Each page reports where its clickable parts are (the pet, the cards), and the Rust core watches the cursor and turns click-through off only while you're over them:

- **The pet window** is just big enough for the sprite and its bubble, so it can sit flush against any edge or corner of any screen. It's kept below the menu bar and may sit over the Dock.
- **Size scales with the screen.** The pet's height is about 13% of the current screen's height, times the **Size** setting (Small 0.75×, Medium 1×, Large 1.3×, Huge 1.7×). Pixel pets snap to whole-number scales so they stay crisp. Drag the pet to a bigger monitor and it grows when you let go. The cards scale the same way.
- **The spot is remembered as a share of the screen** (`place: { displayId, fx, fy }` in `config.json`, measured at the pet's feet). It survives resolution changes and docking, and goes back to the right monitor when that monitor comes back.
- **The list window** opens above the pet when there's more room above, below it otherwise, and lines up with the pet's side of the screen.

The math is in [`src-tauri/src/geometry.rs`](../src-tauri/src/geometry.rs), with its tests alongside it.

## 5. The local server

- Listens on `127.0.0.1:47386` only. Override it with `PROMPTAGOTCHI_PORT`.
- Every request must carry `X-Promptagotchi-Token`, the random token in `~/.promptagotchi/token` (mode 600).
- Requests with an `Origin` header (that is, from a browser) are rejected.
- Endpoints:
  - `POST /v1/hook?mode=event|perm|ask`: what the hook script calls.
  - `GET /v1/sessions`: the current state as JSON. Handy for your own status-bar widgets:

    ```bash
    curl -s -H "X-Promptagotchi-Token: $(cat ~/.promptagotchi/token)" http://127.0.0.1:47386/v1/sessions
    ```

  - `GET /v1/ping`

## Files

| Path | What |
| --- | --- |
| `~/.promptagotchi/config.json` | Your settings: pet, size, position, hold time… |
| `~/.promptagotchi/token`, `port` | For the hook script |
| `~/.promptagotchi/promptagotchi-hook.sh` | The hook script (re-copied at every launch) |
| `~/.promptagotchi/pets/` | Your pets |
| `~/.claude/settings.json` | Hook entries, only if you connect them |
| `~/.claude/settings.json.promptagotchi-backup` | Your settings as they were before the last connect / disconnect |

## Uninstall

1. Right-click the pet → **Claude Code → Disconnect hooks**. This removes exactly the entries it added.
2. Quit the app.
3. Delete `~/.promptagotchi` and the app folder.
