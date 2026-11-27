#!/bin/sh
# Promptagotchi hook — forwards Claude Code hook events to the Promptagotchi desk pet.
#
# Safe by design: if the pet isn't running, curl fails instantly, this prints nothing,
# and Claude Code carries on exactly as if the hook didn't exist.
#
#   sh promptagotchi-hook.sh event   # fire-and-forget status update (async hooks)
#   sh promptagotchi-hook.sh perm    # PermissionRequest: may hold while you answer from the pet
#   sh promptagotchi-hook.sh ask     # PreToolUse(AskUserQuestion): may hold while you pick an answer
#
# Reads the hook JSON on stdin. Only ever talks to 127.0.0.1.

MODE="${1:-event}"
DIR="${PROMPTAGOTCHI_HOME:-$HOME/.promptagotchi}"

[ -r "$DIR/token" ] || { cat >/dev/null; exit 0; }
TOKEN=$(cat "$DIR/token" 2>/dev/null)
PORT=$(cat "$DIR/port" 2>/dev/null)
PORT="${PORT:-47386}"

case "$MODE" in
  perm|ask) MAX=900 ;;  # the pet lets go long before this (see "Hold prompts" in the menu)
  *)        MAX=3   ;;
esac

OUT=$(curl -sf --connect-timeout 1 --max-time "$MAX" -X POST \
  -H "Content-Type: application/json" \
  -H "X-Promptagotchi-Token: $TOKEN" \
  -H "X-Host-Bundle: ${__CFBundleIdentifier:-}" \
  -H "X-Term-Program: ${TERM_PROGRAM:-}" \
  -H "X-Iterm-Session: ${ITERM_SESSION_ID:-}" \
  --data-binary @- \
  "http://127.0.0.1:$PORT/v1/hook?mode=$MODE" 2>/dev/null)

if [ "$MODE" != "event" ] && [ -n "$OUT" ] && [ "$OUT" != "{}" ]; then
  printf '%s\n' "$OUT"
fi
exit 0
