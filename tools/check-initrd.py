#!/usr/bin/env python3
"""check-initrd.py — valide une archive cpio newc comme le noyau la lit.

Reproduit exactement la logique de `kernel/src/initrd.rs` : le nom commence
a `off + 110`, la donnee a `(off + 110 + namesize + 3) & ~3`, et l'entree
suivante a `(data_off + filesize + 3) & ~3`.
"""

import sys
from pathlib import Path


def parse(data: bytes) -> list[tuple[str, int, int]]:
    entries: list[tuple[str, int, int]] = []
    off = 0
    while off + 110 <= len(data):
        if data[off : off + 6] != b"070701":
            break
        filesize = int(data[off + 54 : off + 62], 16)
        namesize = int(data[off + 94 : off + 102], 16)
        name_off = (off + 110 + 3) & ~3
        name = data[name_off : name_off + namesize]
        data_off = (name_off + namesize + 3) & ~3
        entries.append((name.split(b"\x00")[0].decode(), data_off, filesize))
        if name.startswith(b"TRAILER!!!"):
            break
        off = (data_off + filesize + 3) & ~3
    return entries


def main() -> int:
    path = Path(sys.argv[1] if len(sys.argv) > 1 else "assets/initrd.cpio")
    data = path.read_bytes()
    entries = parse(data)
    print(f"{path} : {len(data)} octets, {len(entries)} entree(s)")
    ok = True
    for name, doff, size in entries:
        end = doff + size
        in_bounds = end <= len(data)
        magic = data[doff : doff + 4] if in_bounds else b""
        is_elf = magic == b"\x7fELF"
        flag = "OK " if in_bounds else "HORS"
        extra = " ELF" if is_elf else ""
        print(f"  {flag} {name:<16} data@{doff:<9} size={size:<9}{extra}")
        if not in_bounds:
            ok = False
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
