#!/usr/bin/env python3
"""Plays a build's --progress json events without a game, to try the launcher's
build view (docs/frontends.md, "Building: the NSP script"). It takes the same
arguments as engine/scripts/build_ipa_from_nsp.py, emits the same kinds of
events with the real stage names, and at the end makes a stand-in app with
make-fake-game.sh where the real script would write the game's app.

The launcher runs it instead of the real script when NXL_BUILD_SCRIPT names it:
  open --env NXL_BUILD_SCRIPT=$PWD/scripts/fake-build.py "NX Launcher.app"

A source whose name contains "fail" stops at the shader stage with exit 1.
FAKE_SPEED=4 runs four times faster. Ctrl-C (SIGINT) exits with 130, as the
real script does.
"""
import argparse, json, os, subprocess, sys, time, zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEED = float(os.environ.get('FAKE_SPEED', '1'))


def ev(kind, /, **fields):   # positional-only: events have a `name` field of their own
    print(json.dumps({'ev': kind, **fields, 't': round(time.time(), 3)}, ensure_ascii=False), flush=True)


def wait(seconds):
    time.sleep(seconds / SPEED)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('nsp_dir')
    ap.add_argument('-k', '--keys')
    ap.add_argument('-o', '--out')
    ap.add_argument('--platform', choices=('ios', 'mac', 'mac-app'), default='ios')
    ap.add_argument('--progress', choices=('auto', 'bar', 'lines', 'json', 'off'), default='auto')
    ap.add_argument('--work-dir')
    ap.add_argument('--romfs', choices=('bundle', 'separate'), default='bundle')
    ap.add_argument('--seen-shaders', action='store_true')
    a = ap.parse_args()   # an argument error: usage on stderr, exit 2, no JSON

    src = Path(a.nsp_dir).absolute()
    folder = src.parent if src.is_file() else src
    name = src.stem
    tid = f'0100FA4E{zlib.crc32(name.encode()) & 0xFFFF:04X}0000'
    work = Path(a.work_dir) if a.work_dir else folder / f'{tid}_build'
    (work / 'logs').mkdir(parents=True, exist_ok=True)
    ios = a.platform == 'ios'
    out = Path(a.out) if a.out else folder / (f'{name}.ipa' if ios else f'{name}.app')
    stages = ['rewrite', 'shader split', 'Mac shaders', 'Mac host', 'Pipeline archive', 'IPA pack' if ios else 'Mac app']
    n = len(stages)

    try:
        ev('say', msg=f'{name}: FAKE build (scripts/fake-build.py), nothing is extracted')
        ev('stage', k=1, n=n, name='rewrite', step='main')
        for i in range(1, 6):
            wait(0.5)
            ev('progress', k=1, name='rewrite', step='main', done=i, total=5, unit='modules', est=False, note='',
               frac=min(i / 5, 0.99), eta=(5 - i) * 0.5 / SPEED, elapsed=i * 0.5 / SPEED)
        ev('stage_end', k=1, name='rewrite', ok=True, summary='5 modules rewritten', elapsed=2.5)

        icon = work / 'icon.jpg'
        subprocess.run(['/usr/bin/sips', '-s', 'format', 'jpeg', '-z', '256', '256',
                        '/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericApplicationIcon.icns',
                        '--out', str(icon)], capture_output=True)
        ev('game', title_id=tid, name=name, version='1.0.0', bundle_id=f'org.nxnative.{tid.lower()}', out=str(out),
           icon=str(icon) if icon.is_file() else None)
        ev('say', msg=f'{name} ({tid}) v1.0.0 → {out}')

        ev('stage', k=2, n=n, name='shader split', step='')
        wait(1.5)
        ev('stage_end', k=2, name='shader split', ok=True, summary='2,418 shaders, tables match', elapsed=1.5)

        ev('stage', k=3, n=n, name='Mac shaders', step='compile')
        ev('shaders_stale', reason='chain', old=['b-41'], new='b-42', count=2)
        total = 2418
        if a.seen_shaders:   # the profile's shader_seen.tsv instead of the whole map
            total = 312
            ev('say', msg=f'--seen-shaders: {total} of 2,418 shaders, the ones the playtests used (FAKE); '
                          'an area they do not cover draws without its shaders')
        for i in range(1, 21):
            wait(0.4)
            done = total * i // 20
            if 'fail' in name.lower() and i == 12:
                ev('say', msg='[RED] shader 0x7f3a1c: translation failed (FAKE): see logs/shaders.log')
                ev('stage_end', k=3, name='Mac shaders', ok=False, summary='1 shader failed', elapsed=i * 0.4)
                return 1
            ev('progress', k=3, name='Mac shaders', step='compile', done=done, total=total, unit='shaders', est=False,
               note='metal', frac=min(done / total, 0.99), eta=(20 - i) * 0.4 / SPEED, elapsed=i * 0.4 / SPEED)
        ev('stage_end', k=3, name='Mac shaders', ok=True, summary=f'{total:,} shaders', elapsed=8)

        ev('stage', k=4, n=n, name='Mac host', step='xcodebuild')
        for i in range(4):   # work that cannot be counted: frac null
            wait(0.75)
            ev('progress', k=4, name='Mac host', step='xcodebuild', done=None, total=None, unit='', est=False,
               note='', frac=None, eta=None, elapsed=(i + 1) * 0.75 / SPEED)
        ev('stage_end', k=4, name='Mac host', ok=True, summary='', elapsed=3)

        ev('stage', k=5, n=n, name='Pipeline archive', step='')
        wait(1)
        ev('say', msg='pipeline archive: binary archive failed for applegpu_g17p (FAKE); the app is packed without it')
        ev('stage_end', k=5, name='Pipeline archive', ok=True, warn=True, summary='', elapsed=1)

        if ios:   # an IPA is a zip of Payload/<app>.app: a stand-in one, with a fake app inside
            ev('stage', k=6, n=n, name='IPA pack', step='pack')
            payload = work / 'Payload'
            subprocess.run([str(HERE / 'make-fake-game.sh'), str(payload), tid, name, '0', '1'], capture_output=True)
            subprocess.run(['/usr/bin/ditto', '-c', '-k', '--keepParent', str(payload), str(out)], capture_output=True)
            wait(1)
            ev('stage_end', k=6, name='IPA pack', ok=out.is_file(), summary=out.name, elapsed=1)
            if a.romfs == 'separate':
                ev('say', msg=f'romfs separate: copy the game files to the device once with: '
                              f'scripts/push_romfs.sh {work / "romfs.img"} (FAKE)')
            ev('say', msg=f'done: {out}')
            return 0 if out.is_file() else 1

        ev('stage', k=6, n=n, name='Mac app', step='pack')
        r = subprocess.run([str(HERE / 'make-fake-game.sh'), str(out.parent), tid, out.stem, '0', '20'],
                           capture_output=True, text=True)
        (work / 'logs' / 'pack_app.log').write_text(r.stdout + r.stderr)
        if r.returncode:
            ev('say', msg=f'[RED] pack_app failed (FAKE): see {work / "logs" / "pack_app.log"}')
            ev('stage_end', k=6, name='Mac app', ok=False, summary='', elapsed=1)
            return 1
        wait(1)
        ev('stage_end', k=6, name='Mac app', ok=True, summary=out.name, elapsed=1)
        ev('say', msg=f'done: {out}')
        return 0
    except KeyboardInterrupt:
        ev('say', msg='interrupted')
        return 130


if __name__ == '__main__':
    sys.exit(main())
