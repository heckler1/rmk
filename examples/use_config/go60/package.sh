#!/usr/bin/env bash

# Builds the Go60 firmware in Docker and writes bootloader-ready UF2 files here.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
../../../scripts/docker.sh run . cargo make uf2 --release

# CONFIG_BUILD_OUTPUT_UF2_FAMILY_ID in MoErgo's go60_lh_defconfig and go60_rh_defconfig
python3 retag_uf2.py go60_lh.uf2 0x9809B007
python3 retag_uf2.py go60_rh.uf2 0x980AB007
ls -l go60_lh.uf2 go60_rh.uf2
