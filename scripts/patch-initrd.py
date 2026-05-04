#!/usr/bin/env python3
"""patch-initrd.py — patch a CPIO newc initramfs.

For each of the two target .ko.xz kernel modules:
  - decompress with lzma
  - replace the archive entry with a .ko entry (shorter name, updated sizes)

Also appends a conf/modules file (before TRAILER) listing both modules so
the initramfs module loader picks them up automatically.

Also patches the gzip-compressed second CPIO:
  - text module database files (modules.dep, modules.alias, etc.) have
    .ko.xz → .ko so that libkmod can find the pre-decompressed modules
  - binary .bin module index files are removed (libkmod falls back to text)

Usage:
    python3 scripts/patch-initrd.py [INPUT] [OUTPUT]

If OUTPUT is omitted it overwrites INPUT in-place (via a temp file).
"""

import gzip
import io
import lzma
import os
import sys
import tempfile

# --------------------------------------------------------------------------- #
# CPIO newc (SVR4, no-CRC) helpers
# --------------------------------------------------------------------------- #

HEADER_LEN = 110
MAGIC = b"070701"

TARGETS = {
    # virtio bus + block device
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/drivers/virtio/virtio_mmio.ko.xz",
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/drivers/block/virtio_blk.ko.xz",
    # ext4 stack (ext4 → jbd2 + mbcache + crc16 + crc32c)
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/fs/ext4/ext4.ko.xz",
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/fs/jbd2/jbd2.ko.xz",
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/fs/mbcache.ko.xz",
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/lib/crc16.ko.xz",
    "usr/lib/modules/6.12.85+deb13-riscv64/kernel/crypto/crc32c_generic.ko.xz",
}

# Text module database files whose content references module paths with .ko.xz
MODULE_DB_TEXT = {
    "modules.dep",
    "modules.alias",
    "modules.symbols",
    "modules.softdep",
    "modules.builtin.alias",
    "modules.order",
}

# Binary module index files to drop so libkmod falls back to the text files
MODULE_DB_BIN = {
    "modules.dep.bin",
    "modules.alias.bin",
    "modules.symbols.bin",
    "modules.builtin.alias.bin",
}


def _pad4(n: int) -> bytes:
    """Return zero bytes needed to pad *n* up to the next 4-byte boundary."""
    r = (-n) % 4
    return b"\x00" * r


def parse_entries(raw: bytes) -> list[dict]:
    """Walk *raw* and return a list of entry dicts."""
    entries = []
    pos = 0
    while pos < len(raw):
        if raw[pos : pos + 6] != MAGIC:
            raise ValueError(f"Bad CPIO magic at offset {pos:#x}")

        hdr = raw[pos : pos + HEADER_LEN]
        ino       = int(hdr[6:14],   16)
        mode      = int(hdr[14:22],  16)
        uid       = int(hdr[22:30],  16)
        gid       = int(hdr[30:38],  16)
        nlink     = int(hdr[38:46],  16)
        mtime     = int(hdr[46:54],  16)
        filesize  = int(hdr[54:62],  16)
        devmajor  = int(hdr[62:70],  16)
        devminor  = int(hdr[70:78],  16)
        rdevmajor = int(hdr[78:86],  16)
        rdevminor = int(hdr[86:94],  16)
        namesize  = int(hdr[94:102], 16)
        check     = int(hdr[102:110], 16)

        name_raw = raw[pos + HEADER_LEN : pos + HEADER_LEN + namesize]
        name_str = name_raw.rstrip(b"\x00").decode("utf-8", errors="replace")

        name_end  = pos + HEADER_LEN + namesize
        name_pad  = len(_pad4(HEADER_LEN + namesize))
        data_start = name_end + name_pad
        data       = raw[data_start : data_start + filesize]
        data_pad   = len(_pad4(filesize))
        next_pos   = data_start + filesize + data_pad

        entries.append(
            {
                "ino": ino,
                "mode": mode,
                "uid": uid,
                "gid": gid,
                "nlink": nlink,
                "mtime": mtime,
                "devmajor": devmajor,
                "devminor": devminor,
                "rdevmajor": rdevmajor,
                "rdevminor": rdevminor,
                "check": check,
                "name": name_str,
                "data": data,
            }
        )

        if name_str == "TRAILER!!!":
            break

        pos = next_pos

    return entries


def serialise_entry(e: dict) -> bytes:
    """Serialise one entry dict back to bytes."""
    name_bytes = e["name"].encode("utf-8") + b"\x00"
    data = e["data"]

    hdr = (
        "070701"
        f"{e['ino']       & 0xFFFFFFFF:08X}"
        f"{e['mode']      & 0xFFFFFFFF:08X}"
        f"{e['uid']       & 0xFFFFFFFF:08X}"
        f"{e['gid']       & 0xFFFFFFFF:08X}"
        f"{e['nlink']     & 0xFFFFFFFF:08X}"
        f"{e['mtime']     & 0xFFFFFFFF:08X}"
        f"{len(data)      & 0xFFFFFFFF:08X}"
        f"{e['devmajor']  & 0xFFFFFFFF:08X}"
        f"{e['devminor']  & 0xFFFFFFFF:08X}"
        f"{e['rdevmajor'] & 0xFFFFFFFF:08X}"
        f"{e['rdevminor'] & 0xFFFFFFFF:08X}"
        f"{len(name_bytes)& 0xFFFFFFFF:08X}"
        f"{e['check']     & 0xFFFFFFFF:08X}"
    ).encode("ascii")

    out = bytearray()
    out += hdr
    out += name_bytes
    out += _pad4(HEADER_LEN + len(name_bytes))
    out += data
    out += _pad4(len(data))
    return bytes(out)


def serialise_trailer() -> bytes:
    """Write the standard TRAILER!!! entry (110-byte newc header, namesize=11)."""
    name_bytes = b"TRAILER!!!\x00"
    hdr = (
        "070701"
        "00000000"  # ino
        "00000000"  # mode
        "00000000"  # uid
        "00000000"  # gid
        "00000001"  # nlink (1 for TRAILER)
        "00000000"  # mtime
        "00000000"  # filesize
        "00000000"  # devmajor
        "00000000"  # devminor
        "00000000"  # rdevmajor
        "00000000"  # rdevminor
        f"{len(name_bytes):08X}"  # namesize = 0x0B = 11
        "00000000"  # check
    ).encode("ascii")
    assert len(hdr) == HEADER_LEN, f"BUG: trailer hdr is {len(hdr)} bytes, want {HEADER_LEN}"
    out = bytearray()
    out += hdr
    out += name_bytes
    out += _pad4(HEADER_LEN + len(name_bytes))
    return bytes(out)


def serialise_cpio(entries: list[dict]) -> bytes:
    out = bytearray()
    for e in entries:
        out += serialise_entry(e)
    out += serialise_trailer()
    remainder = len(out) % 512
    if remainder:
        out += b"\x00" * (512 - remainder)
    return bytes(out)


# --------------------------------------------------------------------------- #
# Main patching logic
# --------------------------------------------------------------------------- #

def find_first_cpio_end(raw: bytes) -> int:
    """Return the byte offset just after the first CPIO archive (incl. TRAILER padding).
    Any bytes from this offset onward (e.g. a gzip-compressed second CPIO) are preserved."""
    pos = 0
    while pos < len(raw):
        if raw[pos : pos + 6] != MAGIC:
            break
        hdr = raw[pos : pos + HEADER_LEN]
        namesize = int(hdr[94:102], 16)
        filesize  = int(hdr[54:62], 16)
        name_raw  = raw[pos + HEADER_LEN : pos + HEADER_LEN + namesize]
        name_str  = name_raw.rstrip(b"\x00").decode("utf-8", errors="replace")
        name_end  = pos + HEADER_LEN + namesize
        data_start = name_end + len(_pad4(HEADER_LEN + namesize))
        data_end   = data_start + filesize
        next_pos   = data_end + len(_pad4(filesize))
        if name_str == "TRAILER!!!":
            return next_pos
        pos = next_pos
    return len(raw)


_VIRTIO_INSMOD_SCRIPT = b"""\
#!/bin/sh
PREREQ=""
prereqs() { echo "$PREREQ"; }
case "$1" in
prereqs) prereqs; exit 0;;
esac
KVER=6.12.85+deb13-riscv64
M=/usr/lib/modules/${KVER}/kernel
echo "Inserting virtio_mmio..."
/usr/bin/insmod ${M}/drivers/virtio/virtio_mmio.ko && echo "virtio_mmio: OK" || echo "virtio_mmio: FAILED"
echo "Inserting virtio_blk..."
/usr/bin/insmod ${M}/drivers/block/virtio_blk.ko && echo "virtio_blk: OK" || echo "virtio_blk: FAILED"
echo "Inserting ext4 stack..."
/usr/bin/insmod ${M}/crypto/crc32c_generic.ko && echo "crc32c_generic: OK" || echo "crc32c_generic: FAILED"
/usr/bin/insmod ${M}/lib/crc16.ko 2>/dev/null; true
/usr/bin/insmod ${M}/fs/mbcache.ko 2>/dev/null; true
/usr/bin/insmod ${M}/fs/jbd2/jbd2.ko && echo "jbd2: OK" || echo "jbd2: FAILED"
/usr/bin/insmod ${M}/fs/ext4/ext4.ko && echo "ext4: OK" || echo "ext4: FAILED"
"""

_ORDER_VIRTIO_LINE = b"/scripts/init-top/virtio_insmod \"$@\"\n"


def patch_gzip_cpio_modules(gz_bytes: bytes) -> bytes:
    """Unpack the gzip CPIO, update module DB text files (.ko.xz→.ko),
    drop binary .bin index files, inject direct insmod script, and return new gzip bytes."""
    print("  Patching gzip second CPIO for modules database …")
    # The gzip stream may be preceded by null-byte padding from the first CPIO's
    # 512-byte block alignment.  Find the actual gzip magic (0x1f 0x8b).
    gz_start = gz_bytes.find(b"\x1f\x8b")
    if gz_start < 0:
        raise ValueError("No gzip magic found in tail")
    prefix_nulls = gz_bytes[:gz_start]  # padding to preserve
    raw = gzip.decompress(gz_bytes[gz_start:])
    entries = parse_entries(raw)
    print(f"    {len(entries)} entries in second CPIO")

    # Find representative mtime from first real entry
    mtime = 0
    for e in entries:
        if e["mtime"] != 0 and e["name"] != "TRAILER!!!":
            mtime = e["mtime"]
            break

    patched: list[dict] = []
    dropped = 0
    updated = 0
    for e in entries:
        basename = e["name"].rsplit("/", 1)[-1]

        if basename in MODULE_DB_BIN:
            # Drop binary index files; libkmod falls back to text
            dropped += 1
            continue

        if basename in MODULE_DB_TEXT and b".ko.xz" in e["data"]:
            new_data = e["data"].replace(b".ko.xz", b".ko")
            new_e = dict(e)
            new_e["data"] = new_data
            patched.append(new_e)
            updated += 1
            print(f"    Updated {e['name']}")
            continue

        # Inject virtio_insmod line into ORDER before the udev line
        if e["name"] == "scripts/init-top/ORDER":
            order = e["data"]
            udev_line = b"/scripts/init-top/udev"
            if _ORDER_VIRTIO_LINE not in order and udev_line in order:
                order = order.replace(udev_line, _ORDER_VIRTIO_LINE + udev_line)
                new_e = dict(e)
                new_e["data"] = order
                patched.append(new_e)
                print("    Injected virtio_insmod into init-top ORDER")
                continue

        patched.append(e)

    print(f"    Dropped {dropped} binary index files, updated {updated} text files")

    # Inject the virtio_insmod script entry (before TRAILER, which serialise_cpio adds itself)
    print("    Adding scripts/init-top/virtio_insmod")
    script_entry: dict = {
        "ino": 0xFFFD,
        "mode": 0o100755,
        "uid": 0,
        "gid": 0,
        "nlink": 1,
        "mtime": mtime,
        "devmajor": 0,
        "devminor": 0,
        "rdevmajor": 0,
        "rdevminor": 0,
        "check": 0,
        "name": "scripts/init-top/virtio_insmod",
        "data": _VIRTIO_INSMOD_SCRIPT,
    }
    # Remove the TRAILER entry — serialise_cpio() appends its own
    patched = [e for e in patched if e["name"] != "TRAILER!!!"]
    patched.append(script_entry)

    # Re-serialise and re-gzip (mtime=0 for reproducibility)
    cpio_bytes = serialise_cpio(patched)
    buf = io.BytesIO()
    with gzip.GzipFile(fileobj=buf, mode="wb", mtime=0) as gz:
        gz.write(cpio_bytes)
    return prefix_nulls + buf.getvalue()


def patch(input_path: str, output_path: str) -> None:
    print(f"Reading {input_path} …")
    with open(input_path, "rb") as f:
        raw = f.read()
    print(f"  {len(raw):,} bytes read")

    # Locate the gzip second CPIO (if any) so we can preserve it verbatim
    first_end = find_first_cpio_end(raw)
    tail = raw[first_end:]
    if tail.lstrip(b"\x00"):
        print(f"  Found {len(tail):,}-byte tail (second CPIO) at offset {first_end:#x}")
    else:
        tail = b""

    print("Parsing CPIO entries …")
    entries = parse_entries(raw[:first_end] if tail else raw)
    print(f"  {len(entries)} entries (including TRAILER)")

    # Pick a representative mtime from the first real file entry for conf/modules
    representative_mtime = 0
    for e in entries:
        if e["mtime"] != 0 and e["name"] != "TRAILER!!!":
            representative_mtime = e["mtime"]
            break

    patched_entries: list[dict] = []
    patched_names: set[str] = set()

    for e in entries:
        name = e["name"]

        if name == "TRAILER!!!":
            # Inject conf/modules before the trailer
            modules_content = b"virtio_mmio\nvirtio_blk\n"
            if "conf" not in patched_names:
                print("  Appending synthetic 'conf' directory entry")
                conf_dir: dict = {
                    "ino": 0xFFFE,
                    "mode": 0o040755,
                    "uid": 0,
                    "gid": 0,
                    "nlink": 2,
                    "mtime": representative_mtime,
                    "devmajor": 0,
                    "devminor": 0,
                    "rdevmajor": 0,
                    "rdevminor": 0,
                    "check": 0,
                    "name": "conf",
                    "data": b"",
                }
                patched_entries.append(conf_dir)
                patched_names.add("conf")

            print("  Appending conf/modules entry")
            conf_modules: dict = {
                "ino": 0xFFFF,
                "mode": 0o100644,
                "uid": 0,
                "gid": 0,
                "nlink": 1,
                "mtime": representative_mtime,
                "devmajor": 0,
                "devminor": 0,
                "rdevmajor": 0,
                "rdevminor": 0,
                "check": 0,
                "name": "conf/modules",
                "data": modules_content,
            }
            patched_entries.append(conf_modules)
            # TRAILER itself is written separately; don't add it as a regular entry
            continue

        if name in TARGETS:
            new_name = name[:-3]  # strip ".xz"
            compressed = e["data"]
            print(f"  Decompressing {name}")
            print(f"    compressed size : {len(compressed):,} bytes")
            decompressed = lzma.decompress(compressed)
            print(f"    decompressed size: {len(decompressed):,} bytes")
            print(f"    → renaming to {new_name}")
            new_entry = dict(e)
            new_entry["name"] = new_name
            new_entry["data"] = decompressed
            patched_entries.append(new_entry)
            patched_names.add(new_name)
            # Original .ko.xz entry is intentionally dropped
            continue

        patched_entries.append(e)
        patched_names.add(name)

    print(f"Serialising {len(patched_entries)} patched entries …")
    out_buf = bytearray(serialise_cpio(patched_entries))

    # Patch the gzip second CPIO to update the module database, then append
    if tail:
        patched_tail = patch_gzip_cpio_modules(tail)
        print(f"  Appending {len(patched_tail):,}-byte patched gzip tail")
        out_buf += patched_tail

    print(f"Writing {output_path} …")
    # Write to a sibling temp file, then atomically rename
    dir_name = os.path.dirname(os.path.abspath(output_path))
    fd, tmp_path = tempfile.mkstemp(dir=dir_name, prefix=".patch-initrd-tmp.")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(out_buf)
        os.replace(tmp_path, output_path)
    except BaseException:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise

    print(f"Done. Output: {output_path} ({len(out_buf):,} bytes)")


# --------------------------------------------------------------------------- #

def main() -> None:
    args = sys.argv[1:]
    if len(args) == 0:
        input_path = os.path.join(
            os.path.dirname(os.path.abspath(__file__)),
            "..",
            "images",
            "debian-initrd.img",
        )
        output_path = input_path
    elif len(args) == 1:
        input_path = args[0]
        output_path = args[0]
    elif len(args) == 2:
        input_path = args[0]
        output_path = args[1]
    else:
        print(f"Usage: {sys.argv[0]} [INPUT [OUTPUT]]", file=sys.stderr)
        sys.exit(1)

    patch(os.path.abspath(input_path), os.path.abspath(output_path))


if __name__ == "__main__":
    main()
