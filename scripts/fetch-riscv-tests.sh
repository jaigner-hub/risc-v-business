#!/usr/bin/env bash
# Builds rv64ui-p-* test ELFs from source and copies them to tests/riscv-tests/.
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

echo "Building isa tests..."
make isa -j"$(nproc)"

echo "Copying rv64ui-p-* to $DEST..."
mkdir -p "$DEST"
# Exclude .dump files (disassembly listings) and .o files
find "$WORK/riscv-tests/isa" -name 'rv64ui-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Done. $(ls "$DEST" | grep -c rv64ui-p-) test ELFs copied."
echo "Now run: git add tests/riscv-tests && git commit -m 'test: vendor rv64ui-p-* ELFs'"
