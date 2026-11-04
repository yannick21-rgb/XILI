#!/usr/bin/env python3
"""mk-initrd.py — génère un initrd cpio newc pour XILI (J3.5+)."""

import argparse
import stat
import sys
import zlib
from pathlib import Path


TRAILER = b"TRAILER!!!"


def _pad(data: bytes) -> bytes:
    rem = len(data) % 4
    return data + (b"\x00" * (4 - rem) if rem else b"")


def _crc32(data: bytes) -> int:
    return zlib.crc32(data) & 0xFFFFFFFF


def encode_entry(name: str, data: bytes, mode: int = 0o644) -> bytes:
    name_bytes = name.encode("utf-8") + b"\x00"
    namesize = len(name_bytes)
    filesize = len(data)
    header = (
        "070701"
        f"{0:08x}"
        f"{(mode | stat.S_IFREG):08x}"
        f"{0:08x}"
        f"{0:08x}"
        f"{1:08x}"
        f"{0:08x}"
        f"{filesize:08x}"
        f"{0:08x}"
        f"{0:08x}"
        f"{0:08x}"
        f"{0:08x}"
        f"{namesize:08x}"
        f"{_crc32(b'070701' + b'\x00' * (8 * 11) + name_bytes + data):08x}"
    ).encode("ascii")
    return _pad(header + name_bytes) + _pad(data)


def build_cpio(entries: list[tuple[str, bytes]]) -> bytes:
    out = b""
    for name, data in entries:
        out += encode_entry(name, data)
        rem = len(out) % 4
        if rem:
            out += b"\x00" * (4 - rem)
    out += encode_entry(TRAILER.decode("utf-8"), b"")
    out += b"\x00" * 4
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Génère un initrd cpio newc pour XILI.")
    ap.add_argument("out", type=Path, help="Chemin de sortie .cpio")
    ap.add_argument("files", nargs="*", type=Path, help="Fichiers à inclure")
    args = ap.parse_args(argv)

    entries: list[tuple[str, bytes]] = []
    for path in args.files:
        if not path.exists():
            print(f"missing: {path}", file=sys.stderr)
            return 1
        entries.append((path.name, path.read_bytes()))

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(build_cpio(entries))
    print(f"initrd écrit : {args.out} ({len(entries)} fichier(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
