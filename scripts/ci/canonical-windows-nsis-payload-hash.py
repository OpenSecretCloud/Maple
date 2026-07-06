#!/usr/bin/env python3
import argparse
import hashlib
import os
import shutil
import struct
import subprocess
import sys
import tempfile


PE_CERTIFICATE_DIRECTORY_INDEX = 4
PE_MACHINE_AMD64 = 0x8664
WINDOWS_RUNTIME_PAYLOAD_FILES = (
    "maple.exe",
    "onnxruntime.dll",
    "MSVCP140.dll",
    "MSVCP140_1.dll",
    "VCRUNTIME140.dll",
    "VCRUNTIME140_1.dll",
)
GENERATED_NSIS_PAYLOAD_FILES = frozenset(
    {
        "uninstall.exe",
    }
)


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def read_u16(data, offset):
    if offset < 0 or offset + 2 > len(data):
        raise ValueError("offset outside file")
    return struct.unpack_from("<H", data, offset)[0]


def read_u32(data, offset):
    if offset < 0 or offset + 4 > len(data):
        raise ValueError("offset outside file")
    return struct.unpack_from("<I", data, offset)[0]


def write_u32(data, offset, value):
    if offset < 0 or offset + 4 > len(data):
        raise ValueError("offset outside file")
    struct.pack_into("<I", data, offset, value)


def is_pe(data):
    if len(data) < 0x40 or data[:2] != b"MZ":
        return False
    try:
        pe_offset = read_u32(data, 0x3C)
        return pe_offset + 4 <= len(data) and data[pe_offset : pe_offset + 4] == b"PE\0\0"
    except ValueError:
        return False


def pe_machine(data, label):
    if not is_pe(data):
        raise ValueError(f"not a PE file: {label}")

    pe_offset = read_u32(data, 0x3C)
    return read_u16(data, pe_offset + 4)


def has_arm64ec_markers(data):
    lowered = data.lower()
    return (
        b"arm64ec" in lowered
        or b"arm64ret" in lowered
        or b".arm64.pdb" in lowered
    )


def canonical_pe_bytes(data, label):
    if not is_pe(data):
        return data

    out = bytearray(data)
    pe_offset = read_u32(out, 0x3C)
    coff_offset = pe_offset + 4
    optional_offset = coff_offset + 20
    optional_size = read_u16(out, coff_offset + 16)

    if optional_offset + optional_size > len(out):
        raise ValueError(f"PE optional header exceeds file size: {label}")

    magic = read_u16(out, optional_offset)
    if magic == 0x10B:
        data_directory_offset = optional_offset + 96
    elif magic == 0x20B:
        data_directory_offset = optional_offset + 112
    else:
        raise ValueError(f"Unsupported PE optional header magic 0x{magic:x}: {label}")

    checksum_offset = optional_offset + 64
    certificate_directory_offset = data_directory_offset + (8 * PE_CERTIFICATE_DIRECTORY_INDEX)
    if certificate_directory_offset + 8 > optional_offset + optional_size:
        raise ValueError(f"PE certificate directory is outside optional header: {label}")

    certificate_offset = read_u32(out, certificate_directory_offset)
    certificate_size = read_u32(out, certificate_directory_offset + 4)

    write_u32(out, checksum_offset, 0)
    write_u32(out, certificate_directory_offset, 0)
    write_u32(out, certificate_directory_offset + 4, 0)

    if certificate_offset == 0 and certificate_size == 0:
        return bytes(out)

    aligned_certificate_size = (certificate_size + 7) & ~7
    certificate_end = certificate_offset + aligned_certificate_size
    if certificate_offset > len(out) or certificate_end > len(out):
        raise ValueError(f"PE certificate table points outside file: {label}")

    if certificate_end != len(out):
        raise ValueError(
            f"PE certificate table is not appended at EOF and cannot be safely stripped: {label}"
        )

    return bytes(out[:certificate_offset])


def require_7z():
    for candidate in [
        shutil.which("7z"),
        shutil.which("7z.exe"),
        os.path.join(os.environ.get("ProgramFiles", ""), "7-Zip", "7z.exe"),
        os.path.join(os.environ.get("ProgramFiles(x86)", ""), "7-Zip", "7z.exe"),
    ]:
        if candidate and os.path.isfile(candidate):
            return candidate
    raise RuntimeError("7z is required to extract NSIS installers")


def extract_installer(installer, destination):
    seven_zip = require_7z()
    subprocess.run(
        [seven_zip, "x", "-y", f"-o{destination}", installer],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )


def canonical_tree_entries(root):
    entries = []
    for current_root, dirs, files in os.walk(root):
        dirs.sort()
        for name in sorted(files):
            path = os.path.join(current_root, name)
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            # makensis generates uninstall.exe from a temporary helper during
            # packaging and signs that helper in signed builds. It is not part
            # of Maple's reproducible app/runtime payload.
            if rel.lower() in GENERATED_NSIS_PAYLOAD_FILES:
                continue
            with open(path, "rb") as f:
                canonical = canonical_pe_bytes(f.read(), rel)
            entries.append((rel, len(canonical), sha256_bytes(canonical)))
    return entries


def payload_file(root, name):
    matches = []
    for current_root, _, files in os.walk(root):
        for filename in files:
            if filename.lower() == name.lower():
                matches.append(os.path.join(current_root, filename))

    if len(matches) != 1:
        raise ValueError(
            f"expected exactly one Windows payload file named {name}, found {len(matches)}"
        )

    return matches[0]


def verify_windows_runtime_payload(installer, label):
    with tempfile.TemporaryDirectory() as tmp:
        extract_installer(installer, tmp)

        for name in WINDOWS_RUNTIME_PAYLOAD_FILES:
            path = payload_file(tmp, name)
            with open(path, "rb") as f:
                data = f.read()

            machine = pe_machine(data, name)
            if machine != PE_MACHINE_AMD64:
                raise ValueError(
                    f"Windows payload file is not native AMD64: {name} machine=0x{machine:04x}"
                )

            if has_arm64ec_markers(data):
                raise ValueError(
                    f"Windows payload file is ARM64EC, not native AMD64: {name}"
                )

            digest = sha256_bytes(data)
            print(f"verified-windows-runtime-native-amd64  {digest}  {label}::{name}")


def canonical_tree_hash(installer):
    with tempfile.TemporaryDirectory() as tmp:
        extract_installer(installer, tmp)
        entries = canonical_tree_entries(tmp)

    digest_input = "".join(
        f"{digest}  {size}  {rel}\n" for rel, size, digest in entries
    ).encode("utf-8")
    return sha256_bytes(digest_input), entries


def print_manifest(installer, label):
    digest, _ = canonical_tree_hash(installer)
    print(f"sha256-windows-nsis-payload-canonical  {digest}  {label}")
    return digest


def compare_installers(signed_installer, unsigned_installer, signed_label, unsigned_label):
    signed_digest, signed_entries = canonical_tree_hash(signed_installer)
    unsigned_digest, unsigned_entries = canonical_tree_hash(unsigned_installer)

    if signed_entries != unsigned_entries:
        print("Windows signed-vs-unsigned NSIS canonical payload mismatch.", file=sys.stderr)
        signed_by_path = {rel: (size, digest) for rel, size, digest in signed_entries}
        unsigned_by_path = {rel: (size, digest) for rel, size, digest in unsigned_entries}
        for rel in sorted(set(signed_by_path) | set(unsigned_by_path)):
            signed_value = signed_by_path.get(rel)
            unsigned_value = unsigned_by_path.get(rel)
            if signed_value != unsigned_value:
                print(
                    f"mismatch  {rel}  signed={signed_value}  unsigned={unsigned_value}",
                    file=sys.stderr,
                )
        return 1

    if signed_digest != unsigned_digest:
        print("Windows signed-vs-unsigned NSIS canonical tree digest mismatch.", file=sys.stderr)
        print(f"signed={signed_digest}", file=sys.stderr)
        print(f"unsigned={unsigned_digest}", file=sys.stderr)
        return 1

    print(f"sha256-windows-nsis-payload-canonical  {signed_digest}  {signed_label}")
    print(
        "verified-windows-signed-vs-unsigned-payload-canonical  "
        f"{signed_digest}  {signed_label}  {unsigned_label}"
    )
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Compute or compare canonical NSIS payload hashes for Windows installers."
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--compare", action="store_true", help="compare signed and unsigned installers")
    mode.add_argument(
        "--verify-runtime-dlls",
        action="store_true",
        help="verify installed Windows payload files are native AMD64, not ARM64EC",
    )
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()

    try:
        if args.verify_runtime_dlls:
            if len(args.paths) not in {1, 2}:
                parser.error("--verify-runtime-dlls requires installer [label]")
            installer = args.paths[0]
            label = args.paths[1] if len(args.paths) == 2 else installer
            verify_windows_runtime_payload(installer, label)
            return 0

        if args.compare:
            if len(args.paths) not in {2, 4}:
                parser.error("--compare requires signed unsigned [signed-label unsigned-label]")
            signed_installer = args.paths[0]
            unsigned_installer = args.paths[1]
            signed_label = args.paths[2] if len(args.paths) == 4 else signed_installer
            unsigned_label = args.paths[3] if len(args.paths) == 4 else unsigned_installer
            return compare_installers(
                signed_installer,
                unsigned_installer,
                signed_label,
                unsigned_label,
            )

        if len(args.paths) not in {1, 2}:
            parser.error("hash mode requires installer [label]")
        installer = args.paths[0]
        label = args.paths[1] if len(args.paths) == 2 else installer
        print_manifest(installer, label)
        return 0
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as exc:
        print(f"canonical-windows-nsis-payload-hash: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
