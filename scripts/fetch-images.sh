#!/usr/bin/env bash
set -euo pipefail
mkdir -p images

# 1. OpenSBI
bash scripts/fetch-opensbi.sh

# 2. Linux kernel (Debian bookworm riscv64)
if [ ! -f images/Image ]; then
    BASE="https://ftp.debian.org/debian/pool/main/l/linux"
    DEB=$(curl -s "$BASE/" \
        | grep -o 'linux-image-[0-9][^"]*-riscv64_[^"]*_riscv64\.deb' \
        | sort -V | tail -1)
    [ -n "$DEB" ] || { echo "Error: No matching DEB found at $BASE"; exit 1; }
    curl -L -o /tmp/linux-image-riscv64.deb "$BASE/$DEB"
    rm -rf /tmp/linux-riscv64
    dpkg-deb -x /tmp/linux-image-riscv64.deb /tmp/linux-riscv64/
    VMLINUZ=$(find /tmp/linux-riscv64 -name "vmlinuz-*-riscv64" | sort | tail -1)
    cp "$VMLINUZ" images/Image
    rm -rf /tmp/linux-image-riscv64.deb /tmp/linux-riscv64
    echo "Done: images/Image"
fi

# 3. Minimal BusyBox initramfs
if [ ! -f images/rootfs.img ]; then
    BASE="https://ftp.debian.org/debian/pool/main/b/busybox"
    DEB=$(curl -s "$BASE/" \
        | grep -o 'busybox-static_[^"]*_riscv64\.deb' \
        | sort -V | tail -1)
    [ -n "$DEB" ] || { echo "Error: No matching DEB found at $BASE"; exit 1; }
    curl -L -o /tmp/busybox-static-riscv64.deb "$BASE/$DEB"
    rm -rf /tmp/busybox-riscv64
    dpkg-deb -x /tmp/busybox-static-riscv64.deb /tmp/busybox-riscv64/
    [ -f /tmp/busybox-riscv64/bin/busybox ] || { echo "Error: busybox not found in DEB at /tmp/busybox-riscv64/bin/busybox"; exit 1; }

    D=$(mktemp -d) || { echo "Error: mktemp failed"; exit 1; }
    mkdir -p "$D"/{bin,proc,sys,dev,etc}
    cp /tmp/busybox-riscv64/bin/busybox "$D/bin/busybox"
    [ -f "$D/bin/busybox" ] || { echo "Error: Failed to copy busybox binary"; exit 1; }
    chmod +x "$D/bin/busybox"
    for cmd in sh ls uname mount; do ln -sf busybox "$D/bin/$cmd"; done
    cat > "$D/init" <<'EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
exec /bin/sh
EOF
    chmod +x "$D/init"
    (cd "$D" && find . | cpio -o -H newc --quiet) | gzip > images/rootfs.img

    rm -rf /tmp/busybox-static-riscv64.deb /tmp/busybox-riscv64 "$D"
    echo "Done: images/rootfs.img"
fi
