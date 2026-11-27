# Contributing

Thanks for wanting to make the pet better!

## Run it

You need Rust and Node 20+ (Node only runs the Tauri command-line tool).

```bash
npm install
npm start        # debug build; open Debug → Start simulator for pretend sessions
npm test         # cargo test
```

`PROMPTAGOTCHI_DEBUG=1` prints what the windows and the core say to each other. `PROMPTAGOTCHI_HOME=/tmp/pg` uses a throwaway config, token and pets folder, and `PROMPTAGOTCHI_PORT=47399` another port, so a dev copy can run next to your real one.

## Where things live

| Path | What |
| --- | --- |
| `src-tauri/src/sessions.rs` | The state machine: hook events + status files → one state per session |
| `src-tauri/src/server.rs` | Localhost hook server (holds prompts while you answer from the pet) |
| `src-tauri/src/session_files.rs` | Watches `~/.claude/sessions` |
| `src-tauri/src/transcripts.rs` | "Did that turn end on a question for you?" |
| `src-tauri/src/hooks.rs` | Installs / removes hooks in `~/.claude/settings.json` |
| `src-tauri/src/focus.rs` | Jump-to-session (deep links, AppleScript) |
| `src-tauri/src/pets.rs` | Pet discovery, loading, Codex atlas support |
| `src-tauri/src/geometry.rs` | Pet size per screen, placement, where the list opens |
| `src-tauri/src/simulator.rs` | Pretend sessions for testing |
| `src-tauri/src/lib.rs` | Windows, tray and menus, shortcuts, click-through, the whip, the `ipc` command |
| `src/renderer/bridge.js` | How the pages talk to the Rust core |
| `src/renderer/pet.*`, `panel.*`, `whip.*` | The pet, the session list, the whip |
| `src/renderer/app.html`, `home.js`, `studio.*` | The app window: Run, Editor, Settings, Debug |
| `hooks/promptagotchi-hook.sh` | The hook script (POSIX sh + curl) |

## Pull requests

- Add a Rust test next to the code for state-machine or geometry changes.
- New bundled pets go in `pets/<id>/` with a `preview.png` (`npm run preview -- pets/<id>`), and need adding to `bundle.resources` in `src-tauri/tauri.conf.json`.
- Keep the hook script dependency-free and fail-safe: it must exit 0 and print nothing when the app isn't running.
