#!/usr/bin/env bash

# Builds the example in the current directory and leaves the firmware there.
# Runs inside the container; see scripts/docker.sh.

set -euo pipefail

cargo build --release

if grep -qs '^\[tasks\.uf2\]' Makefile.toml; then
    cargo make uf2 --release
fi

# The ELFs are in a Docker volume, so copy them to where the host can see them.
built=0
for file in "$CARGO_TARGET_DIR"/*/release/*; do
    if [[ -f "$file" && -x "$file" ]]; then
        cp "$file" "$(basename "$file").elf"
        built=1
    fi
done

# Cargo succeeds without building anything when every bin has required-features.
if [[ $built -eq 0 ]]; then
    echo "error: cargo built no firmware here; build it with \`scripts/docker.sh run\`" >&2
    exit 1
fi

ls -l ./*.elf ./*.hex ./*.uf2 ./*.bin 2>/dev/null || true
