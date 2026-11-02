#!/usr/bin/env python3
"""mk-initrd.py — génère un initrd cpio newc pour XILI (J3.5+)."""

import argparse
import os
import stat
import sys
from dataclasses import dataclass
from pathlib import Path


TRAILER = b"TRAILER!!!"
HEADER_FMT = (
    "{magic}"
    "{ino:08x}"
    "{mode:08x}"
    "{uid:08x}"
    "{gid:08x}"
    "{nlink:08x}"
    "{mtime:08x}"
    "{filesize:08x}"
    "{dev_major:08x}"
    "{dev_minor:08x}"
    "{rdev_major:08x}"
    "{rdev_minor:08x}"
    "{namesize:08x}"
    "{crc:08x}"
)


@dataclass(frozen=True)
class Entry:
    name: str
    data: bytes
    mode: int = 0o644
    uid: int = 0
    gid: int = 0


def _pad(data: bytes) -> bytes:
    rem = len(data) % 4
    return data + (b"\x00" * (4 - rem) if rem else b"")


def _crc32(data: bytes) -> int:
    import zlib

    return zlib.crc32(data) & 0xFFFFFFFF


def encode_entry(entry: Entry) -> bytes:
    name = entry.name.encode("utf-8") + b"\x00"
    namesize = len(name)
    filesize = len(entry.data)
    header = HEADER_FMT.format(
        magic="070701",
        ino=0,
        mode=entry.mode | stat.S_IFREG,
        uid=entry.uid,
        gid=entry.gid,
        nlink=1,
        mtime=0,
        filesize=filesize,
        dev_major=0,
        dev_minor=0,
        rdev_major=0,
        rdev_minor=0,
        namesize=namesize,
        crc=_crc32(
            b"070701"
            + b"\x00" * (8 * 13)
            + name
            + entry.data
        ),
    ).encode("ascii")
    return header + _pad(name) + _pad(entry.data)


def build_cpio(entries: list[Entry]) -> bytes:
    out = b""
    for entry in entries:
        out += encode_entry(entry)
    out += encode_entry(Entry(name=TRAILER.decode("utf-8"), data=b""))
    out += b"\x00" * 4  # align trailer data padding
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Génère un initrd cpio newc pour XILI.")
    ap.add_argument("out", type=Path, help="Chemin de sortie .cpio")
    ap.add_argument("files", nargs="*", type=Path, help="Fichiers à inclure")
    ap.add_argument(
        "--base-dir",
        type=Path,
        default=None,
        help="Répertoire de base pour les fichiers listés",
    )
    args = ap.parse_args(argv)

    entries: list[Entry] = []
    base = args.base_dir or Path.cwd()
    for path in args.files:
        full = path if path.is_absolute() else base / path
        if not full.exists():
            print(f"missing: {full}", file=sys.stderr)
            return 1
        entries.append(Entry(name=path.name, data=full.read_bytes()))

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(build_cpio(entries))
    print(f"initrd écrit : {args.out} ({len(entries)} fichier(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
