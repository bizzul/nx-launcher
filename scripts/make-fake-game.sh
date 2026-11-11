#!/bin/bash
# Makes a stand-in for a Mac app packed by nxnative (--platform mac-app), to try
# the launcher without building a real game. It holds no game: its launcher
# writes a log like the real one (dm/mac/launcher.c), waits, and exits with the
# code you choose.
#
#   scripts/make-fake-game.sh <folder> <title ID> <name> [exit code] [seconds]
#
#   exit code: 0 quit (default), 70 GPU revoked, 134 crash (abort), 1 launcher error
#   seconds:   how long the "game" runs (default 8)
set -euo pipefail

if [ $# -lt 3 ]; then
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi
out=$1 id=$(echo "$2" | tr '[:lower:]' '[:upper:]') name=$3 code=${4:-0} secs=${5:-8}
app="$out/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/dm"

icon=/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericApplicationIcon.icns
[ -f "$icon" ] && cp "$icon" "$app/Contents/Resources/AppIcon.icns"

lower=$(echo "$id" | tr '[:upper:]' '[:lower:]')
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleDisplayName</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>org.nxnative.$lower</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleVersion</key><string>1.0.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>nxnative</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>DMTitleID</key><string>$id</string>
  <key>DMLanguage</key><string>it</string>
  <key>DMFake</key><true/>
</dict></plist>
EOF

cat > "$app/Contents/MacOS/nxnative" <<EOF
#!/bin/bash
# Fake nxnative launcher: same folders and log as dm/mac/launcher.c, no game.
sup="\$HOME/Library/Application Support/nxnative/$id"
logs="\$HOME/Library/Logs/nxnative"
mkdir -p "\$sup/save" "\$sup/temp" "\$logs"
[ -f "\$logs/$id.log" ] && mv "\$logs/$id.log" "\$logs/$id.prev.log"
exec >"\$logs/$id.log" 2>&1
echo "launcher: $name ($id), FAKE game (scripts/make-fake-game.sh)"
[ "\${DM_LAUNCHER:-1}" = 0 ] && echo "launcher: no settings window (DM_LAUNCHER=0 or --no-launcher)"
trap 'echo "launcher: SIGTERM, stopping"; exit 143' TERM INT HUP
sleep $secs & wait \$!
echo "launcher: the host exited with $code"
exit $code
EOF
chmod +x "$app/Contents/MacOS/nxnative"
echo "$app"
