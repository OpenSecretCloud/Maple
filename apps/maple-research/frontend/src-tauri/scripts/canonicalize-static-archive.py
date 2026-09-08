#!/usr/bin/env python3
"""Rewrite a BSD ar archive into a stable form for reproducible hashing."""

from __future__ import annotations

from pathlib import Path
import sys


MAGIC = b"!<arch>\n"
HEADER_END = b"`\n"
SYMBOL_TABLE_NAMES = {"__.SYMDEF", "__.SYMDEF SORTED"}


def clean_member_name(raw: bytes) -> str:
    return raw.decode("utf-8", "replace").rstrip("\x00 ").rstrip("/")


def archive_members(path: Path):
    data = path.read_bytes()
    if not data.startswith(MAGIC):
        raise SystemExit(f"not an ar archive: {path}")

    offset = len(MAGIC)
    index = 0
    while offset < len(data):
        header = data[offset : offset + 60]
        if len(header) != 60:
            raise SystemExit(f"short ar header at byte {offset}: {path}")
        if header[58:60] != HEADER_END:
            raise SystemExit(f"invalid ar header terminator at byte {offset}: {path}")

        offset += 60
        raw_name = header[:16].decode("utf-8", "replace").strip()
        size = int(header[48:58].decode("ascii").strip() or "0")
        payload = data[offset : offset + size]
        offset += size
        if size % 2:
            offset += 1

        if raw_name.startswith("#1/"):
            name_length = int(raw_name[3:])
            name = clean_member_name(payload[:name_length])
            payload = payload[name_length:]
        else:
            name = clean_member_name(raw_name.encode())

        if name in SYMBOL_TABLE_NAMES or name.startswith("/SYM64"):
            continue

        index += 1
        yield index, payload


def write_member(out: bytearray, name: str, payload: bytes) -> None:
    header = (
        f"{name:<16}"
        f"{0:<12}"
        f"{0:<6}"
        f"{0:<6}"
        f"{'100644':<8}"
        f"{len(payload):<10}"
    ).encode("ascii") + HEADER_END

    out += header
    out += payload
    if len(payload) % 2:
        out += b"\n"


def canonicalize(src: Path, dst: Path) -> None:
    out = bytearray(MAGIC)
    for index, payload in archive_members(src):
        write_member(out, f"m{index:06d}.o", payload)
    dst.write_bytes(out)


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: canonicalize-static-archive.py <input.a> <output.a>", file=sys.stderr)
        return 2

    canonicalize(Path(sys.argv[1]), Path(sys.argv[2]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
