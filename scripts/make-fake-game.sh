#!/bin/bash
# Makes a stand-in for a Mac app packed by nxnative (--platform mac-app), to try
# the launcher without building a real game. It holds no game: its launcher
# writes a log like the real one (dm/mac/launcher.c), resolves the settings in
# the same order (environment, env.txt files, saved choices, defaults), logs
# them, waits, and exits with the code you choose.
#
#   scripts/make-fake-game.sh <folder> <title ID> <name> [exit code] [seconds] [launcher.json] [env.txt]
#
#   exit code:     0 quit (default), 70 GPU revoked, 134 crash (abort), 1 launcher error
#   seconds:       how long the "game" runs (default 8)
#   launcher.json: the settings list to carry (none: the game has no settings)
#   env.txt:       switches baked into the app (Contents/Resources/dm/env.txt)
#
# FAKE_BOOT=<seconds> also plays a start like the engine's, with its log lines
# (src-tauri/src/gamelog.rs): a few seconds before the game's code runs, the
# graphics preparation behind the progress screen (that many seconds on the
# first start, 2 on later ones), three missing shaders, the first frame, and the
# window covered for a while. Before the game's own seconds.
#
# scripts/make-fake-library.sh makes a set of three.
set -euo pipefail

if [ $# -lt 3 ]; then
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi
out=$1 id=$(echo "$2" | tr '[:lower:]' '[:upper:]') name=$3 code=${4:-0} secs=${5:-8}
settings=${6:-} envfile=${7:-} boot=${FAKE_BOOT:-0}
app="$out/$name.app"
res="$app/Contents/Resources"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$res/dm"

icon=/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericApplicationIcon.icns
[ -f "$icon" ] && cp "$icon" "$res/AppIcon.icns"
[ -n "$settings" ] && cp "$settings" "$res/dm/launcher.json"
[ -n "$envfile" ] && cp "$envfile" "$res/dm/env.txt"

# Info.plist as dm/mac/pack_app.py writes it, DMSettings included.
/usr/bin/python3 - "$app" "$id" "$name" <<'PY'
import json, plistlib, sys
from pathlib import Path
app, tid, name = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
p = {'CFBundleName': name, 'CFBundleDisplayName': name, 'CFBundleIdentifier': f'org.nxnative.{tid.lower()}',
     'CFBundleShortVersionString': '1.0.0', 'CFBundleVersion': '1.0.0', 'CFBundlePackageType': 'APPL',
     'CFBundleExecutable': 'nxnative', 'CFBundleIconFile': 'AppIcon', 'DMTitleID': tid, 'DMLanguage': 'it', 'DMFake': True}
listed = app / 'Contents/Resources/dm/launcher.json'
rows = []
if listed.is_file():
    for s in json.loads(listed.read_text())['settings']:
        if s['kind'] == 'range': p.setdefault('DMSettings', []).append({'env': s['env'], 'min': float(s['min']), 'max': float(s['max'])})
        elif s['kind'] == 'choice': p.setdefault('DMSettings', []).append({'env': s['env'], 'values': [o[0] for o in s['options']]})
        elif s['kind'] == 'toggle': p.setdefault('DMSettings', []).append({'env': s['env'], 'values': [s['on'], s['off']]})
        rows.append(f"{s['env']}\t{s['default']}")
(app / 'Contents/Info.plist').write_bytes(plistlib.dumps(p))
(app / 'Contents/Resources/dm/fake_settings.tsv').write_text(''.join(r + '\n' for r in rows))
PY

# The rewriter's header with main's build ID (patch_nso.py writes it into main.fixups.tsv);
# cheat files are named after its first 16 hex digits.
bid=$(printf '%s' "$id" | shasum | cut -c1-32)
printf '# BUILD_ID\t%s\n' "$bid" > "$res/dm/main.fixups.tsv"
BID=$(echo "${bid:0:16}" | tr '[:lower:]' '[:upper:]')

cat > "$app/Contents/MacOS/nxnative" <<EOF
#!/bin/bash
# Fake nxnative launcher: same folders, log and settings order as dm/mac/launcher.c, no game.
res="\$(cd "\$(dirname "\$0")/../Resources" && pwd)"
sup="\$HOME/Library/Application Support/nxnative/$id"
logs="\$HOME/Library/Logs/nxnative"
mkdir -p "\$sup/save" "\$sup/temp" "\$logs"
[ -f "\$logs/$id.log" ] && mv "\$logs/$id.log" "\$logs/$id.prev.log"
exec >"\$logs/$id.log" 2>&1
echo "launcher: $name ($id), FAKE game (scripts/make-fake-game.sh)"
[ "\${DM_LAUNCHER:-1}" = 0 ] && echo "launcher: no settings window (DM_LAUNCHER=0 or --no-launcher)"
while IFS=\$'\t' read -r env def; do
  [ -z "\$env" ] && continue
  val=\$def src=default
  if [ -n "\${!env:-}" ]; then val=\${!env} src=environment
  elif line=\$(grep -m1 "^\$env=" "\$sup/env.txt" 2>/dev/null); then val=\${line#*=} src="player env.txt"
  elif line=\$(grep -m1 "^\$env=" "\$res/dm/env.txt" 2>/dev/null); then val=\${line#*=} src="app env.txt"
  elif saved=\$(defaults read org.nxnative.$(echo "$id" | tr '[:upper:]' '[:lower:]') "launcher.\$env" 2>/dev/null); then val=\$saved src="saved choice"
  fi
  echo "launcher: env \$env=\$val (\$src)"
done < "\$res/dm/fake_settings.tsv"
[ -n "\${DM_HOME_QUIT_TEXT:-}" ] && echo "launcher: Home prompt title: \$(printf '%s\n' "\$DM_HOME_QUIT_TEXT" | sed -n 2p)"
cs=\$(defaults read org.nxnative.$(echo "$id" | tr '[:upper:]' '[:lower:]') cloudSaves.folder 2>/dev/null) && echo "launcher: cloud saves folder: \$cs"
ch="\$sup/cheats"
if [ -f "\$ch/.cheats-off-$id" ]; then echo "launcher: cheats off (master switch)"
elif [ -f "\$ch/$BID.txt" ]; then echo "launcher: cheats for build $BID, on: \$(tr '\n' ',' < "\$ch/$BID.enabled" 2>/dev/null)"
fi
trap 'echo "launcher: SIGTERM, stopping"; exit 143' TERM INT HUP
pause() { sleep "\$1" & wait \$!; }
if [ $boot -gt 0 ]; then   # FAKE_BOOT: a start like the engine's (nvn_mtl.m, front_mac.m)
  pause 3; echo "[dm] >>> nnMain"; pause 1
  prep=2
  if [ ! -f "\$sup/pso_user.tsv" ]; then
    echo "[dm] [nvn-mtl] pipeline prewarm: this device's user list does not exist yet: \$sup/pso_user.tsv"; prep=$boot
  fi
  echo "[dm] front end: window visible"
  echo "[dm] [nvn-mtl] pipeline prepare: 783 of 14748 lines done after 0.5 s, the progress screen shows"
  for k in 3be3604ea84c019e f588d206e9bcae62 3be3604ea84c019e 8e97a49724315293; do
    echo "[dm] [nvn-mtl] [RED] failed to load shader \${k}_b: cannot open \$res/msl/b-fake/\${k}_b.metallib"
  done
  pause \$prep
  echo "[dm] [nvn-mtl] pipeline prepare: 14748 lines (9546 shaders, 2418 render pipelines, 287 compute pipelines) in \$prep.00 s on 10 workers before the first frame, progress screen shown; FAKE"
  echo "fake" > "\$sup/pso_user.tsv"
  pause 1; echo "[dm] [nvn-mtl] present: picture shape 1920x1080 (1.778:1) for the front end's layout"
  pause 3; echo "[dm] front end: window covered (its display link fires about once a second: frame times measure nothing)"
  pause 10; echo "[dm] front end: window visible"
fi
pause $secs
echo "launcher: the host exited with $code"
exit $code
EOF
chmod +x "$app/Contents/MacOS/nxnative"
echo "$app"
