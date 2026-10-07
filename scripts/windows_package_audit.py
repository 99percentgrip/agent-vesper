"""Fail closed if shipped PE executables require an unbundled VC/CRT runtime."""

from pathlib import Path
import argparse
import re
import struct


EXECUTABLES = ("agent-vesper-acp.exe", "agent-vesper-tui.exe",
               "vesper-web-fetch.exe", "sandbox_init.exe")
RUNTIME = re.compile(r"^(?:vcruntime\d.*|msvcp\d.*|msvcr\d.*|ucrtbase|"
                     r"api-ms-win-crt-.*)\.dll$", re.IGNORECASE)


def pe_imports(data):
    """Read normal and delay imports from a bounded PE32+ file, without loading it."""
    def unpack(fmt, offset):
        size = struct.calcsize(fmt)
        if offset < 0 or offset + size > len(data):
            raise ValueError("truncated PE structure")
        return struct.unpack_from(fmt, data, offset)

    if data[:2] != b"MZ":
        raise ValueError("missing DOS signature")
    pe, = unpack("<I", 0x3C)
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("missing PE signature")
    machine, count = unpack("<HH", pe + 4)
    optional_size, = unpack("<H", pe + 20)
    optional = pe + 24
    magic, = unpack("<H", optional)
    if machine != 0x8664 or magic != 0x20B or count > 96 or optional_size < 112:
        raise ValueError("expected bounded x86_64 PE32+ executable")
    directories, = unpack("<I", optional + 108)
    if directories < 14 or optional_size < 112 + 14 * 8:
        raise ValueError("missing PE import directories")
    image_base, = unpack("<Q", optional + 24)
    headers, = unpack("<I", optional + 60)
    sections = []
    for index in range(count):
        address = optional + optional_size + index * 40
        virtual_size, rva, raw_size, raw = unpack("<IIII", address + 8)
        if raw + raw_size > len(data):
            raise ValueError("section exceeds file")
        sections.append((rva, raw_size, raw))

    def offset(rva):
        if 0 <= rva < min(headers, len(data)):
            return rva
        for start, size, raw in sections:
            if start <= rva < start + size:
                return raw + rva - start
        raise ValueError("import RVA outside file-backed sections")

    def name(rva):
        start = offset(rva)
        end = data.find(b"\0", start, min(start + 256, len(data)))
        if end < 0:
            raise ValueError("unterminated import name")
        result = data[start:end].decode("ascii")
        if not re.fullmatch(r"[A-Za-z0-9_.-]+\.dll", result, re.IGNORECASE):
            raise ValueError("invalid import name")
        return result

    imports = set()
    for directory, width in ((1, 20), (13, 32)):
        rva, size = unpack("<II", optional + 112 + directory * 8)
        if rva == 0 and size == 0:
            continue
        if rva == 0 or size < width or size > 65536:
            raise ValueError("invalid import directory")
        terminated = False
        for position in range(0, size - width + 1, width):
            fields = unpack("<" + "I" * (width // 4), offset(rva + position))
            if not any(fields):
                terminated = True
                break
            if directory == 1:
                imports.add(name(fields[3]))
            else:
                if fields[0] not in (0, 1):
                    raise ValueError("unsupported delay import attributes")
                imports.add(name(fields[1] if fields[0] == 1 else fields[1] - image_base))
        if not terminated:
            raise ValueError("unterminated import directory")
    if not imports:
        raise ValueError("executable has no imports")
    return sorted(imports, key=str.lower)


def audit(bundle):
    results = {}
    for executable in EXECUTABLES:
        path = bundle / executable
        imports = pe_imports(path.read_bytes())
        forbidden = [dll for dll in imports if RUNTIME.fullmatch(dll)]
        if forbidden:
            raise ValueError(f"{executable} requires separately installed runtime: {', '.join(forbidden)}")
        results[executable] = imports
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    args = parser.parse_args()
    try:
        results = audit(args.bundle)
    except (ValueError, OSError) as error:
        parser.exit(1, f"error: Windows release package audit: {error}\n")
    for executable, imports in results.items():
        print(f"PASS {executable}: no VC/CRT DLL imports; imports={','.join(imports)}")
