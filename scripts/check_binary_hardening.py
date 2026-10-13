#!/usr/bin/env python3
"""Check release-binary hardening without executing the binary.

Linux ELF release binaries must be dynamically linked PIE executables with
full RELRO, immediate binding, and a non-executable stack. Their dynamic
dependencies must be the platform C library, not OpenSSL or SQLite.

macOS release binaries must be PIE executables without an executable stack
and without a bundled OpenSSL or SQLite. RELRO is a Linux property and is
not required here.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

ELF_MAGIC = b"\x7fELF"
ET_DYN = 3
PT_LOAD = 1
PT_DYNAMIC = 2
PT_INTERP = 3
PT_GNU_STACK = 0x6474E551
PT_GNU_RELRO = 0x6474E552
PF_X = 1
DT_NULL = 0
DT_NEEDED = 1
DT_STRTAB = 5
DT_RPATH = 15
DT_RUNPATH = 29
DT_BIND_NOW = 24
DT_FLAGS = 30
DT_FLAGS_1 = 0x6FFFFFFB
DF_BIND_NOW = 0x8
DF_1_NOW = 0x1
DF_1_PIE = 0x08000000

MH_MAGIC_64 = 0xFEEDFACF
MH_CIGAM_64 = 0xCFFAEDFE
FAT_MAGIC = 0xCAFEBABE
MH_EXECUTE = 2
MH_PIE = 0x00200000
MH_ALLOW_STACK_EXECUTION = 0x00020000
LC_LOAD_DYLIB = 0xC
LC_LOAD_WEAK_DYLIB = 0x80000018
LC_REEXPORT_DYLIB = 0x8000001F
LC_LAZY_LOAD_DYLIB = 0x80000020
LC_RPATH = 0x8000001C
DYLIB_CMDS = {
    LC_LOAD_DYLIB,
    LC_LOAD_WEAK_DYLIB,
    LC_REEXPORT_DYLIB,
    LC_LAZY_LOAD_DYLIB,
}

LINUX_NEEDED = ("libc.so", "libm.so", "libgcc_s.so", "libpthread.so", "libdl.so", "ld-linux")


def check_path(path: Path) -> list[str]:
    data = path.read_bytes()
    return check_bytes(data, label=str(path))


def check_bytes(data: bytes, label: str = "binary") -> list[str]:
    if data.startswith(ELF_MAGIC):
        return check_elf(data, label)
    if len(data) >= 4:
        magic = struct.unpack_from("<I", data, 0)[0]
        if magic in (MH_MAGIC_64, MH_CIGAM_64) or magic == FAT_MAGIC:
            return check_macho(data, label)
    return [f"{label}: not an ELF or Mach-O release binary"]


def check_elf(data: bytes, label: str) -> list[str]:
    problems: list[str] = []
    if len(data) < 64 or data[4] != 2 or data[5] != 1:
        return [f"{label}: only 64-bit little-endian ELF release binaries are supported"]
    e_type = struct.unpack_from("<H", data, 16)[0]
    phoff = struct.unpack_from("<Q", data, 32)[0]
    phentsize = struct.unpack_from("<H", data, 54)[0]
    phnum = struct.unpack_from("<H", data, 56)[0]
    if e_type != ET_DYN:
        problems.append(f"{label}: ELF type is {e_type}, expected ET_DYN (PIE)")
    if phoff <= 0 or phentsize < 56 or phnum <= 0:
        return problems + [f"{label}: ELF program headers are missing"]

    loads: list[tuple[int, int, int]] = []
    has_interp = False
    has_relro = False
    has_stack = False
    stack_exec = False
    dynamic = b""
    for index in range(phnum):
        start = phoff + index * phentsize
        if start + 56 > len(data):
            return problems + [f"{label}: ELF program header {index} is truncated"]
        p_type, p_flags, p_offset, p_vaddr, _paddr, p_filesz = struct.unpack_from(
            "<IIQQQQ", data, start
        )
        if p_type == PT_LOAD:
            loads.append((p_offset, p_vaddr, p_filesz))
        elif p_type == PT_INTERP:
            has_interp = True
        elif p_type == PT_GNU_RELRO:
            has_relro = True
        elif p_type == PT_GNU_STACK:
            has_stack = True
            stack_exec = bool(p_flags & PF_X)
        elif p_type == PT_DYNAMIC:
            end = p_offset + p_filesz
            if p_offset < 0 or end > len(data):
                problems.append(f"{label}: PT_DYNAMIC is outside the file")
            else:
                dynamic = data[p_offset:end]

    if not has_interp:
        problems.append(f"{label}: missing PT_INTERP; the release binary is expected to be dynamically linked")
    if not has_relro:
        problems.append(f"{label}: missing GNU_RELRO")
    if not has_stack:
        problems.append(f"{label}: missing GNU_STACK")
    elif stack_exec:
        problems.append(f"{label}: GNU_STACK is executable")
    if not dynamic:
        return problems + [f"{label}: missing PT_DYNAMIC"]

    strtab_vaddr = None
    flags = 0
    flags_1 = 0
    bind_now = False
    needed_offsets: list[int] = []
    has_rpath = False
    for cursor in range(0, len(dynamic) - 15, 16):
        tag, value = struct.unpack_from("<qQ", dynamic, cursor)
        if tag == DT_NULL:
            break
        if tag == DT_NEEDED:
            needed_offsets.append(value)
        elif tag == DT_STRTAB:
            strtab_vaddr = value
        elif tag == DT_FLAGS:
            flags = value
        elif tag == DT_FLAGS_1:
            flags_1 = value
        elif tag == DT_BIND_NOW:
            bind_now = True
        elif tag in (DT_RPATH, DT_RUNPATH):
            has_rpath = True
    if has_rpath:
        problems.append(f"{label}: DT_RPATH or DT_RUNPATH is set")
    if not (bind_now or (flags & DF_BIND_NOW) or (flags_1 & DF_1_NOW)):
        problems.append(f"{label}: full RELRO requires BIND_NOW")
    if not (flags_1 & DF_1_PIE):
        problems.append(f"{label}: DF_1_PIE is not set")
    if strtab_vaddr is None:
        return problems + [f"{label}: DT_STRTAB is missing"]
    strtab_off = vaddr_to_offset(loads, strtab_vaddr)
    if strtab_off is None:
        return problems + [f"{label}: DT_STRTAB does not fall inside a PT_LOAD"]
    for needed_off in needed_offsets:
        name = read_c_string(data, strtab_off + needed_off)
        if name is None:
            problems.append(f"{label}: DT_NEEDED string is truncated")
            continue
        if not any(name.startswith(prefix) for prefix in LINUX_NEEDED):
            problems.append(f"{label}: unexpected dynamic dependency {name}")
    return problems


def vaddr_to_offset(loads: list[tuple[int, int, int]], vaddr: int) -> int | None:
    for offset, base, size in loads:
        if base <= vaddr < base + size:
            return offset + (vaddr - base)
    return None


def read_c_string(data: bytes, offset: int) -> str | None:
    if offset < 0 or offset >= len(data):
        return None
    end = data.find(b"\0", offset)
    if end < 0:
        return None
    return data[offset:end].decode("utf-8", "replace")


def check_macho(data: bytes, label: str) -> list[str]:
    problems: list[str] = []
    magic = struct.unpack_from("<I", data, 0)[0]
    if magic == FAT_MAGIC or magic == MH_CIGAM_64:
        return [f"{label}: only thin little-endian Mach-O 64-bit binaries are supported"]
    if len(data) < 32:
        return [f"{label}: Mach-O header is truncated"]
    _magic, _cpu, _sub, filetype, ncmds, sizeofcmds, flags, _reserved = struct.unpack_from(
        "<IiiIIIII", data, 0
    )
    if filetype != MH_EXECUTE:
        problems.append(f"{label}: Mach-O type is {filetype}, expected MH_EXECUTE")
    if not flags & MH_PIE:
        problems.append(f"{label}: MH_PIE is not set")
    if flags & MH_ALLOW_STACK_EXECUTION:
        problems.append(f"{label}: MH_ALLOW_STACK_EXECUTION is set")
    offset = 32
    end = 32 + sizeofcmds
    if end > len(data):
        return problems + [f"{label}: Mach-O load commands are truncated"]
    dylibs: list[str] = []
    for _ in range(ncmds):
        if offset + 8 > end:
            return problems + [f"{label}: Mach-O load command is truncated"]
        cmd, cmdsize = struct.unpack_from("<II", data, offset)
        if cmdsize < 8 or offset + cmdsize > len(data):
            return problems + [f"{label}: Mach-O load command size is invalid"]
        if cmd == LC_RPATH:
            problems.append(f"{label}: LC_RPATH is set")
        if cmd in DYLIB_CMDS:
            name_off = struct.unpack_from("<I", data, offset + 8)[0]
            name = read_c_string(data, offset + name_off)
            if name is None:
                problems.append(f"{label}: Mach-O dylib name is truncated")
            else:
                dylibs.append(name)
        offset += cmdsize
    if not dylibs:
        problems.append(f"{label}: Mach-O binary has no dynamic libraries to inspect")
    for name in dylibs:
        lowered = name.lower()
        if any(token in lowered for token in ("ssl", "crypto", "sqlite")):
            problems.append(f"{label}: unexpected dynamic dependency {name}")
            continue
        if name.startswith("@rpath") or name.startswith("@loader_path") or name.startswith("@executable_path"):
            problems.append(f"{label}: unexpected relative dynamic dependency {name}")
            continue
        if not (name.startswith("/usr/lib/") or name.startswith("/System/Library/") or name.startswith("/usr/lib/system/")):
            problems.append(f"{label}: unexpected dynamic dependency {name}")
    return problems


def synthetic_elf(*, stack_exec: bool = False, needed: bytes = b"libc.so.6") -> bytes:
    strtab = b"\0" + needed + b"\0"
    dyn_len = 16 * 4
    ph_count = 5
    content_off = 64 + ph_count * 56
    strtab_off = content_off
    dyn_off = strtab_off + len(strtab)
    interp = b"/lib64/ld-linux-x86-64.so.2\0"
    interp_off = dyn_off + dyn_len
    file_size = interp_off + len(interp)
    flags_1 = DF_1_NOW | DF_1_PIE
    dynamic = b"".join(
        [
            struct.pack("<qQ", DT_NEEDED, 1),
            struct.pack("<qQ", DT_STRTAB, strtab_off),
            struct.pack("<qQ", DT_FLAGS_1, flags_1),
            struct.pack("<qQ", DT_NULL, 0),
        ]
    )
    header = bytearray(64)
    header[0:4] = ELF_MAGIC
    header[4] = 2
    header[5] = 1
    header[6] = 1
    struct.pack_into("<HHIQQQIHHHHHH", header, 16, ET_DYN, 0x3E, 1, 0, 64, 0, 0, 64, 56, ph_count, 0, 0, 0)

    def program(p_type: int, p_flags: int, p_offset: int, p_filesz: int) -> bytes:
        return struct.pack(
            "<IIQQQQQQ",
            p_type,
            p_flags,
            p_offset,
            p_offset,
            p_offset,
            p_filesz,
            p_filesz,
            8,
        )

    stack_flags = 7 if stack_exec else 6
    headers = b"".join(
        [
            program(PT_LOAD, 5, 0, file_size),
            program(PT_INTERP, 4, interp_off, len(interp)),
            program(PT_DYNAMIC, 6, dyn_off, dyn_len),
            program(PT_GNU_STACK, stack_flags, 0, 0),
            program(PT_GNU_RELRO, 4, 0, 1),
        ]
    )
    blob = bytearray(file_size)
    blob[0:64] = header
    blob[64:content_off] = headers
    blob[strtab_off : strtab_off + len(strtab)] = strtab
    blob[dyn_off : dyn_off + len(dynamic)] = dynamic
    blob[interp_off : interp_off + len(interp)] = interp
    return bytes(blob)


def synthetic_macho(*, pie: bool = True, dylib: bytes = b"/usr/lib/libSystem.B.dylib") -> bytes:
    name = dylib + b"\0"
    raw = 24 + len(name)
    cmdsize = (raw + 7) & ~7
    command = struct.pack("<II", LC_LOAD_DYLIB, cmdsize)
    command += struct.pack("<IIII", 24, 0, 0x10000, 0x10000)
    command += name + b"\0" * (cmdsize - 24 - len(name))
    flags = MH_PIE if pie else 0
    header = struct.pack("<IiiIIIII", MH_MAGIC_64, 0x0100000C, 0, MH_EXECUTE, 1, cmdsize, flags, 0)
    return header + command


def self_test() -> None:
    good = check_bytes(synthetic_elf(), label="good-elf")
    if good:
        raise SystemExit("self-test good ELF failed: " + "; ".join(good))
    bad_stack = check_bytes(synthetic_elf(stack_exec=True), label="bad-stack")
    if not any("executable" in item for item in bad_stack):
        raise SystemExit("self-test did not reject an executable GNU_STACK")
    bad_ssl = check_bytes(synthetic_elf(needed=b"libssl.so.3"), label="bad-ssl")
    if not any("libssl" in item for item in bad_ssl):
        raise SystemExit("self-test did not reject libssl")
    good_mach = check_bytes(synthetic_macho(), label="good-macho")
    if good_mach:
        raise SystemExit("self-test good Mach-O failed: " + "; ".join(good_mach))
    bad_pie = check_bytes(synthetic_macho(pie=False), label="bad-pie")
    if not any("MH_PIE" in item for item in bad_pie):
        raise SystemExit("self-test did not reject a non-PIE Mach-O")
    bad_sqlite = check_bytes(synthetic_macho(dylib=b"/usr/lib/libsqlite3.dylib"), label="bad-sqlite")
    if not any("sqlite" in item for item in bad_sqlite):
        raise SystemExit("self-test did not reject libsqlite")


def main(argv: list[str]) -> int:
    if len(argv) == 2 and argv[1] == "--self-test":
        self_test()
        print("binary hardening self-test passed")
        return 0
    if len(argv) != 2:
        print("usage: check_binary_hardening.py [--self-test] <binary>", file=sys.stderr)
        return 2
    self_test()
    problems = check_path(Path(argv[1]))
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1
    print(f"{argv[1]}: hardening checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
