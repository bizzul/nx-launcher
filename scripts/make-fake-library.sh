#!/bin/bash
# Makes three stand-in games in fixtures/fake-games (no game inside: see
# make-fake-game.sh), with settings lists written by the pinned engine's own
# dm/games/settings.py from real game profiles, as a Mac build would carry:
#
#   Avventura di Prova  Breath of the Wild's list, plus the render scale row a
#                       real build gets (with its base size); quits after 20 s
#   Corsa GPU           Mario Kart World's list, fullscreen baked into the
#                       app's env.txt; stops with code 70 (GPU revoked)
#   Crash Test          no settings list; aborts (code 134) after 3 s
#
# Needs engine/ and engine-venv/ (PLAN.md, 3.2).
set -euo pipefail
cd "$(dirname "$0")/.."
out=fixtures/fake-games tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$out" "$tmp/empty"

list() {   # <profile title ID> <out.json>: the Mac settings list of a real profile
  SETTINGS_PLATFORM=mac engine-venv/bin/python engine/dm/games/settings.py "$1" "$tmp/empty" /dev/null "$2" >/dev/null
}

list 01007EF00011E000 "$tmp/adventure.json"
/usr/bin/python3 - "$tmp/adventure.json" <<'PY'
import json, sys
path = sys.argv[1]; d = json.load(open(path))
# The row settings.py adds when the rewriter has a render scale stub (fixups:RENDER_SCALE), with
# Breath of the Wild's render_base: the dump-free list cannot know the stub is there.
row = {'env': 'DM_RENDER_SCALE', 'kind': 'range', 'section': 'Game', 'label': 'Render scale', 'note': '',
       'default': '1.0', 'level': 'basic', 'base': {'docked': [1600, 900], 'handheld': [1280, 720]},
       'min': 0.5, 'max': 1.0, 'step': 0.05}
d['settings'].insert(1, row)
json.dump(d, open(path, 'w'), indent=1)
PY
list 0400C3F00006E000 "$tmp/race.json"
printf 'DM_FULLSCREEN=1\n' > "$tmp/race.env"

scripts/make-fake-game.sh "$out" 0100FA4E00001000 "Avventura di Prova" 0 20 "$tmp/adventure.json"
scripts/make-fake-game.sh "$out" 0100FA4E00002000 "Corsa GPU" 70 4 "$tmp/race.json" "$tmp/race.env"
scripts/make-fake-game.sh "$out" 0100FA4E00003000 "Crash Test" 134 3
