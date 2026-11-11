#!/bin/bash
# Makes a stand-in archive to try the Archivio tab without games: files of a
# few bytes named and laid out as real ones, with no game inside.
#
#   fixtures/fake-archive/        the archive (pick it as the archive folder)
#     Super Mario Bros. Wonder [010015100B514000]/   base game + update v131072
#                                 (titledb knows v327680: «update mancante»)
#     Mario Kart 8 Deluxe [0100152000022000][v0].nsp  loose: «Metti in ordine»
#   fixtures/archive-inbox/       what to import (drop the folder on the tab)
#     mk8-update.zip              Mario Kart 8 Deluxe's update in a ZIP
#     kart-world.7z               a Switch 2 NUP (base + update) in a 7z
#     Bananza/                    a dump: base/ + dlc/ (titledb lists 1 DLC)
#     mystery.nsp                 no title ID in its name: needs prod.keys
#
# Run again to start over.
set -euo pipefail
cd "$(dirname "$0")/.."
archive=fixtures/fake-archive inbox=fixtures/archive-inbox tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
rm -rf "$archive" "$inbox"
mkdir -p "$archive" "$inbox"

wonder="$archive/Super Mario Bros. Wonder [010015100B514000]"
mkdir -p "$wonder"
printf 'stand-in' > "$wonder/Super Mario Bros. Wonder [010015100B514000][v0].nsp"
printf 'stand-in' > "$wonder/Super Mario Bros. Wonder [010015100B514800][v131072].nsp"
printf 'stand-in' > "$archive/Mario Kart 8 Deluxe [0100152000022000][v0].nsp"

mkdir -p "$tmp/zip"
printf 'stand-in' > "$tmp/zip/Mario Kart 8 Deluxe [0100152000022800][v1441792].nsp"
/usr/bin/tar -a -cf "$inbox/mk8-update.zip" -C "$tmp/zip" .

# A NUP's header and package records (scripts/pack_nup.py), and a dump's
# main.npdm with the program ID where the ACI0 section keeps it.
mkdir -p "$tmp/nup" "$inbox/Bananza/base/exefs" "$inbox/Bananza/base/romfs" "$inbox/Bananza/dlc/040074a01bf13001/0/romfs"
/usr/bin/python3 - "$tmp/nup/Mario Kart World.nup" "$inbox/Bananza/base/exefs/main.npdm" <<'PY'
import struct, sys
nup, npdm = sys.argv[1:]
pkgs = [(0x0400C3F00006E000, 0, 0), (0x0400C3F00006E000, 65536, 1)]   # (title ID, version, kind: 0 base, 1 update)
nacp = b'Mario Kart World'.ljust(0x4000, b'\0')
meta = struct.pack('<HHI', 2, 0, len(nacp)) + nacp
toc_off, toc_len = 0x80, 0x30 + 0x40 * len(pkgs)
meta_off = toc_off + toc_len
total = meta_off + len(meta)
b = bytearray(total)
struct.pack_into('<4sHHQQ', b, 0, b'NUP\x01', 0x80, 0, toc_off, toc_len)
struct.pack_into('<QI', b, 64, total, len(pkgs))
struct.pack_into('<4sI', b, toc_off, b'TOC\x01', len(pkgs))
for i, (tid, version, kind) in enumerate(pkgs):
    r = toc_off + 0x30 + i * 0x40
    struct.pack_into('<QIIIIB', b, r, tid, 0, 0, 0, version, kind)
    if i == 0: struct.pack_into('<QI', b, r + 32, meta_off, len(meta))
b[meta_off:] = meta
open(nup, 'wb').write(b)
n = bytearray(0x100)
struct.pack_into('<4s', n, 0, b'META'); struct.pack_into('<II', n, 0x70, 0x80, 0x40)
struct.pack_into('<4s', n, 0x80, b'ACI0'); struct.pack_into('<Q', n, 0x90, 0x040074A01BF12000)
open(npdm, 'wb').write(n)
PY
/usr/bin/tar -a -cf "$inbox/kart-world.7z" -C "$tmp/nup" .
printf 'stand-in' > "$inbox/Bananza/base/romfs/data.bin"
printf 'stand-in' > "$inbox/Bananza/dlc/040074a01bf13001/0/romfs/data.bin"
printf 'stand-in' > "$inbox/mystery.nsp"

echo "Archive: $PWD/$archive"
echo "To import: $PWD/$inbox"
