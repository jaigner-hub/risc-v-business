#!/usr/bin/env bash
set -euo pipefail

VERSION="1.8.1"
TARBALL="opensbi-${VERSION}-rv-bin.tar.xz"
URL="https://github.com/riscv-software-src/opensbi/releases/download/v${VERSION}/${TARBALL}"
ELF_PATH="opensbi-${VERSION}-rv-bin/share/opensbi/lp64/generic/firmware/fw_jump.elf"

mkdir -p images

if [ -f "images/fw_jump.elf" ]; then
    echo "images/fw_jump.elf already exists — skipping download"
    exit 0
fi

echo "Downloading OpenSBI v${VERSION}..."
curl -L -o "/tmp/${TARBALL}" "${URL}"

echo "Extracting fw_jump.elf..."
tar -xf "/tmp/${TARBALL}" -C /tmp "${ELF_PATH}"
cp "/tmp/${ELF_PATH}" images/fw_jump.elf
rm -f "/tmp/${TARBALL}"
rm -rf "/tmp/opensbi-${VERSION}-rv-bin"

echo "Done: images/fw_jump.elf"
