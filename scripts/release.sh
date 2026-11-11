#!/bin/bash
# Publishes a launcher version, which installed apps update to on their own
# (src-tauri/src/autoupdate.rs):
#
#   scripts/release.sh 0.2.0 "What changed" [--install]
#
# 1. Sets the version in tauri.conf.json, Cargo.toml and package.json.
# 2. Builds the Mac app and its updater package (NX Launcher.app.tar.gz),
#    signed with ~/.tauri/nx-launcher.key: the private key stays on this Mac,
#    never in the repo; installed apps check the signature against the public
#    key in tauri.conf.json. Losing the key means installed apps cannot take
#    new versions: they must be installed again by hand.
# 3. Commits "Release v<version>", tags it, pushes main and the tag, and makes
#    the GitHub release with latest.json (what the apps read) and the package.
# 4. --install also puts the app in /Applications (quitting the running one).
#
# Needs a clean tree on main, gh logged in to the repo's owner, the key.
set -euo pipefail
cd "$(dirname "$0")/.."

REPO=bizzul/nx-launcher
KEY=${NXL_SIGNING_KEY:-$HOME/.tauri/nx-launcher.key}
OUT=src-tauri/target/release/bundle/macos
APP="$OUT/NX Launcher.app"
VERSION_FILES=(src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json)

install=0 args=()
for a in "$@"; do
  if [[ $a == --install ]]; then install=1; else args+=("$a"); fi
done
version=${args[0]:-}
notes=${args[1]:-"NX Launcher $version"}

fail() { echo "release: $*" >&2; exit 1; }
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "usage: scripts/release.sh <x.y.z> [\"notes\"] [--install]"
[[ -f $KEY ]] || fail "no signing key at $KEY (pnpm tauri signer generate --ci -w $KEY)"
[[ -z $(git status --porcelain) ]] || fail "commit or stash your changes first"
[[ $(git branch --show-current) == main ]] || fail "release from main"
git remote get-url origin >/dev/null 2>&1 || fail "no origin remote (github.com/$REPO)"
! git rev-parse -q --verify "refs/tags/v$version" >/dev/null || fail "v$version exists already"
gh auth status >/dev/null 2>&1 || fail "gh is not logged in"

# 1. The version, in the three files that carry it. Put back if the build fails.
restore() { git checkout -- "${VERSION_FILES[@]}" src-tauri/Cargo.lock 2>/dev/null || true; }
trap restore ERR
/usr/bin/python3 -I - "$version" <<'PY'
import json, pathlib, re, sys
v = sys.argv[1]
for f in ('src-tauri/tauri.conf.json', 'package.json'):
    p = pathlib.Path(f)
    d = json.loads(p.read_text())
    d['version'] = v
    p.write_text(json.dumps(d, indent=2, ensure_ascii=False) + '\n')
p = pathlib.Path('src-tauri/Cargo.toml')
p.write_text(re.sub(r'(?m)^version = "[^"]*"', f'version = "{v}"', p.read_text(), count=1))
PY

# 2. The app and its signed updater package.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":true}}'
[[ -f "$APP.tar.gz" && -f "$APP.tar.gz.sig" ]] || fail "the build made no updater package"

dist=$(mktemp -d)
cp "$APP.tar.gz" "$dist/NX-Launcher.app.tar.gz"   # no space: GitHub would rename it
/usr/bin/python3 -I - "$version" "$notes" "$(cat "$APP.tar.gz.sig")" \
  "https://github.com/$REPO/releases/download/v$version/NX-Launcher.app.tar.gz" "$dist/latest.json" <<'PY'
import datetime, json, sys
version, notes, sig, url, out = sys.argv[1:]
json.dump({
    'version': version,
    'notes': notes,
    'pub_date': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
    'platforms': {'darwin-aarch64': {'signature': sig, 'url': url}},
}, open(out, 'w'), indent=2)
PY

# 3. Commit, tag, push, release.
git add "${VERSION_FILES[@]}" src-tauri/Cargo.lock
git commit -q -m "Release v$version" ${RELEASE_TRAILER:+-m "$RELEASE_TRAILER"}
trap - ERR
git tag -a "v$version" -m "NX Launcher $version"
git push -q origin main "v$version"
gh release create "v$version" "$dist/latest.json" "$dist/NX-Launcher.app.tar.gz" \
  --repo "$REPO" --title "NX Launcher $version" --notes "$notes"

# 4. This Mac's copy.
if (( install )); then
  osascript -e 'quit app "NX Launcher"' >/dev/null 2>&1 || true
  rm -rf "/Applications/NX Launcher.app"
  ditto "$APP" "/Applications/NX Launcher.app"
  echo "Installed /Applications/NX Launcher.app"
fi
echo "Released v$version: https://github.com/$REPO/releases/tag/v$version"
