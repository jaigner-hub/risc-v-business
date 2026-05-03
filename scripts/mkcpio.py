#!/usr/bin/env python3
"""Create a newc (SVR4 without CRC) cpio archive from the current directory.

Usage: (cd $DIR && python3 mkcpio.py) | gzip > rootfs.img
"""
import os, stat, sys

out = sys.stdout.buffer


def pad4(n: int) -> bytes:
    r = (-n) % 4
    return b"\x00" * r


def write_entry(arcname: str, path: str) -> None:
    st = os.lstat(path)
    if stat.S_ISLNK(st.st_mode):
        data = os.readlink(path).encode()
    elif stat.S_ISREG(st.st_mode):
        with open(path, "rb") as f:
            data = f.read()
    else:
        data = b""

    nb = arcname.encode("utf-8") + b"\x00"
    # newc header: magic + 13 x 8-char hex fields = 110 bytes
    hdr = (
        "070701"
        f"{st.st_ino & 0xFFFFFFFF:08X}"
        f"{st.st_mode:08X}"
        f"{st.st_uid:08X}"
        f"{st.st_gid:08X}"
        f"{st.st_nlink:08X}"
        f"{int(st.st_mtime):08X}"
        f"{len(data):08X}"
        f"{os.major(st.st_dev):08X}"
        f"{os.minor(st.st_dev):08X}"
        f"{'00000000'}"   # rdevmajor
        f"{'00000000'}"   # rdevminor
        f"{len(nb):08X}"
        f"{'00000000'}"   # check
    ).encode("ascii")
    out.write(hdr)
    out.write(nb)
    out.write(pad4(len(hdr) + len(nb)))
    out.write(data)
    out.write(pad4(len(data)))


def write_trailer() -> None:
    nb = b"TRAILER!!!\x00"
    hdr = ("070701" + "00000000" * 12 + f"{len(nb):08X}" + "00000000").encode("ascii")
    out.write(hdr)
    out.write(nb)
    out.write(pad4(len(hdr) + len(nb)))


entries: list[tuple[str, str]] = []

for root, dirs, files in os.walk("."):
    dirs.sort()
    arcroot = os.path.normpath(root)
    if arcroot != ".":
        entries.append((arcroot.lstrip("./"), root))
    for name in sorted(files):
        p = os.path.join(root, name)
        arc = os.path.normpath(p).lstrip("./")
        entries.append((arc, p))
    # symlinks appear in dirs if they point to dirs; handle via scandir
    for entry in sorted(os.scandir(root), key=lambda e: e.name):
        if entry.is_symlink():
            arc = os.path.normpath(entry.path).lstrip("./")
            if arc not in {a for a, _ in entries}:
                entries.append((arc, entry.path))

for arcname, path in entries:
    write_entry(arcname, path)

write_trailer()
