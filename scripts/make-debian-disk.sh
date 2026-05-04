#!/usr/bin/env bash
# Build a Debian trixie riscv64 disk image + matching kernel + initrd.
#
# riscv64 is an official architecture in Debian 13 (trixie), so the standard
# mirror works.  bookworm (Debian 12) would need the ports mirror instead.
#
# Usage:  bash scripts/make-debian-disk.sh
# Output: images/debian-Image       (kernel to boot — replaces images/Image)
#         images/debian-initrd.img  (initramfs with virtio-blk modules)
#         images/debian-riscv64.img (4 GB ext4 rootfs, mounted as /dev/vda)
#
# Boot:
#   cargo run --release -- --dtb images/fw_jump.elf \
#     --kernel images/debian-Image \
#     --initrd images/debian-initrd.img \
#     --disk   images/debian-riscv64.img
#
# Requires: Ubuntu/Debian host with sudo.  Works on WSL2.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGES="$(dirname "$SCRIPT_DIR")/images"
IMG="$IMAGES/debian-riscv64.img"
INITRD_OUT="$IMAGES/debian-initrd.img"
KERNEL_OUT="$IMAGES/debian-Image"
ROOTFS="$(mktemp -d /tmp/debian-rv64-XXXXXX)"
IMG_SIZE=4G
MIRROR="http://deb.debian.org/debian"
SUITE=trixie

die() { echo "ERROR: $*" >&2; exit 1; }

cleanup() {
    set +e
    mountpoint -q "$ROOTFS" 2>/dev/null && sudo umount "$ROOTFS"
    [[ -d "$ROOTFS" ]] && sudo rm -rf "$ROOTFS"
}
trap cleanup EXIT

# ── 1. Dependencies ────────────────────────────────────────────────────────
echo "==> Installing build dependencies..."
sudo apt-get install -y --no-install-recommends \
    debootstrap qemu-user-static binfmt-support e2fsprogs

# Register RISC-V binfmt so the second-stage chroot can run riscv64 ELFs.
if [[ ! -e /proc/sys/fs/binfmt_misc/qemu-riscv64 ]]; then
    echo "==> Registering RISC-V binfmt..."
    sudo update-binfmts --enable qemu-riscv64 2>/dev/null || {
        # WSL2 fallback: mount binfmt_misc and write the registration manually
        sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc 2>/dev/null || true
        printf ':qemu-riscv64:M::\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\xf3\x00:\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff:/usr/bin/qemu-riscv64-static:F\n' \
            | sudo tee /proc/sys/fs/binfmt_misc/register >/dev/null 2>&1 \
            || echo "WARNING: binfmt registration failed — second stage may fail"
    }
fi

# ── 2. Create ext4 image ───────────────────────────────────────────────────
echo "==> Creating ${IMG_SIZE} ext4 disk image..."
mkdir -p "$IMAGES"
truncate -s "$IMG_SIZE" "$IMG"
mkfs.ext4 -F -L riscv-root "$IMG"

# ── 3. Mount ───────────────────────────────────────────────────────────────
echo "==> Mounting image..."
sudo mount -o loop "$IMG" "$ROOTFS"

# ── 4. Debootstrap first stage ────────────────────────────────────────────
echo "==> debootstrap first stage (downloading $SUITE packages)..."
# --no-check-gpg: Ubuntu may not have Debian's trixie signing key in its keyring.
sudo debootstrap \
    --arch=riscv64 \
    --foreign \
    --no-check-gpg \
    --include=systemd,init,udev,e2fsprogs,iproute2,iputils-ping,\
curl,wget,vim,nano,sudo,less,procps,kmod,initramfs-tools \
    "$SUITE" "$ROOTFS" "$MIRROR"

# ── 5. Inject qemu into chroot ────────────────────────────────────────────
echo "==> Copying qemu-riscv64-static into chroot..."
sudo cp /usr/bin/qemu-riscv64-static "$ROOTFS/usr/bin/"

# ── 6. Second stage ───────────────────────────────────────────────────────
echo "==> debootstrap second stage (configuring packages — takes ~5 minutes)..."
sudo chroot "$ROOTFS" /debootstrap/debootstrap --second-stage

# ── 7. Install kernel → get modules + initrd ──────────────────────────────
echo "==> Installing kernel and generating initrd (takes ~5 minutes)..."

# policy-rc.d: suppress service starts and update-grub in postinst scripts.
printf '#!/bin/sh\nexit 101\n' | sudo tee "$ROOTFS/usr/sbin/policy-rc.d" >/dev/null
sudo chmod +x "$ROOTFS/usr/sbin/policy-rc.d"

sudo chroot "$ROOTFS" /usr/bin/qemu-riscv64-static /bin/bash <<INNER
export DEBIAN_FRONTEND=noninteractive
cat > /etc/apt/sources.list <<SOURCES
deb $MIRROR $SUITE main
deb $MIRROR $SUITE-updates main
deb http://deb.debian.org/debian-security $SUITE-security main
SOURCES
apt-get update -qq

# linux-image-riscv64 is the metapackage that installs the current kernel.
apt-get install -y --no-install-recommends linux-image-riscv64 initramfs-tools kmod

# Detect the installed kernel version and generate initrd explicitly,
# in case policy-rc.d suppressed the postinst update-initramfs call.
KVER=\$(ls /lib/modules/ | sort -V | tail -1)
echo "  kernel version: \$KVER"
mkinitramfs -o /boot/initrd.img-\${KVER} \${KVER}
echo "  initrd: \$(du -sh /boot/initrd.img-\${KVER} | cut -f1)"
INNER

sudo rm -f "$ROOTFS/usr/sbin/policy-rc.d"

# ── 8. Configure rootfs ───────────────────────────────────────────────────
echo "==> Configuring rootfs..."

echo "riscv-emu" | sudo tee "$ROOTFS/etc/hostname" >/dev/null
printf '/dev/vda  /  ext4  errors=remount-ro  0  1\n' \
    | sudo tee "$ROOTFS/etc/fstab" >/dev/null
echo "nameserver 8.8.8.8" | sudo tee "$ROOTFS/etc/resolv.conf" >/dev/null

# Passwordless root
sudo chroot "$ROOTFS" /usr/bin/qemu-riscv64-static /bin/bash -c \
    "passwd --delete root"

# Auto-login on ttyS0 (the emulator's UART console)
sudo mkdir -p "$ROOTFS/etc/systemd/system/getty@ttyS0.service.d"
sudo tee "$ROOTFS/etc/systemd/system/getty@ttyS0.service.d/autologin.conf" \
    >/dev/null <<'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I $TERM
EOF

sudo chroot "$ROOTFS" /usr/bin/qemu-riscv64-static /bin/bash -c \
    "systemctl enable getty@ttyS0.service" 2>/dev/null || true

# ── 9. Extract kernel + initrd ────────────────────────────────────────────
echo "==> Extracting kernel and initrd to images/..."

KVER=$(sudo ls "$ROOTFS/lib/modules/" | sort -V | tail -1)
[[ -n "$KVER" ]] || die "No kernel version found in chroot /lib/modules/"

# Kernel image (PE/EFI format on riscv64 — same format as images/Image)
KIMG=$(sudo ls "$ROOTFS/boot/vmlinuz-${KVER}" 2>/dev/null \
    || sudo ls "$ROOTFS/boot/vmlinux-${KVER}" 2>/dev/null \
    || true)
if [[ -n "$KIMG" ]]; then
    sudo cp "$KIMG" "$KERNEL_OUT"
    sudo chown "$(id -u):$(id -g)" "$KERNEL_OUT"
    echo "  kernel: $(du -sh "$KERNEL_OUT" | cut -f1)  $KERNEL_OUT"
else
    echo "  WARNING: kernel image not found in /boot — using existing images/Image"
    KERNEL_OUT="images/Image"
fi

INITRD_SRC=$(sudo ls "$ROOTFS/boot/initrd.img-${KVER}" 2>/dev/null || true)
[[ -n "$INITRD_SRC" ]] || die "initrd not found in chroot /boot/"
sudo cp "$INITRD_SRC" "$INITRD_OUT"
sudo chown "$(id -u):$(id -g)" "$INITRD_OUT"
echo "  initrd: $(du -sh "$INITRD_OUT" | cut -f1)  $INITRD_OUT"

# ── 10. Finalise ──────────────────────────────────────────────────────────
echo "==> Unmounting..."
sudo umount "$ROOTFS"
sudo chown "$(id -u):$(id -g)" "$IMG"

echo ""
echo "Done!  $(du -sh "$IMG" | cut -f1)  $IMG"
echo ""
echo "Boot command:"
echo "  cargo run --release -- --dtb images/fw_jump.elf \\"
echo "    --kernel $KERNEL_OUT \\"
echo "    --initrd $INITRD_OUT \\"
echo "    --disk   $IMG"
