#!/usr/bin/env bash
# Builds rv64ui/um/ua/mi-p-* test ELFs from source and copies them to tests/riscv-tests/.
# Requires: riscv64-unknown-elf-gcc (or riscv64-linux-gnu-gcc), autoconf, make.
# On Ubuntu/Debian: sudo apt install gcc-riscv64-unknown-elf autoconf
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="$SCRIPT_DIR/../tests/riscv-tests"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Cloning riscv-tests..."
git clone --depth 1 https://github.com/riscv-software-src/riscv-tests "$WORK/riscv-tests"
cd "$WORK/riscv-tests"
git submodule update --init --recursive

echo "Configuring..."
autoconf
./configure --prefix="$WORK/install"

echo "Building isa tests (p-environment only; v-environment failures are ignored)..."
# Use -k || true so that failures in rv64*-v-* tests (missing libc headers) don't abort us.
make isa -k -j"$(nproc)" 2>/dev/null || true

echo "Copying rv64ui-p-* to $DEST..."
mkdir -p "$DEST"
find "$WORK/riscv-tests/isa" -name 'rv64ui-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64um-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64um-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64ua-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64ua-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64mi-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64mi-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64si-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64si-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Done."
echo "  rv64ui: $(ls "$DEST" | grep -c 'rv64ui-p-') ELFs"
echo "  rv64um: $(ls "$DEST" | grep -c 'rv64um-p-') ELFs"
echo "  rv64ua: $(ls "$DEST" | grep -c 'rv64ua-p-') ELFs"
echo "  rv64mi: $(ls "$DEST" | grep -c 'rv64mi-p-') ELFs"
echo "  rv64si: $(ls "$DEST" | grep -c 'rv64si-p-') ELFs"
