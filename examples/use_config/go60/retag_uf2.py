#!/usr/bin/env python3
"""Set a UF2 file's family ID. The Go60 bootloader takes only its own per-half IDs."""

import struct
import sys

BLOCK_SIZE = 512
FAMILY_ID_OFFSET = 28
FLAG_FAMILY_ID_PRESENT = 0x2000

path, family = sys.argv[1], int(sys.argv[2], 16)
with open(path, "rb") as f:
    data = bytearray(f.read())
if len(data) % BLOCK_SIZE:
    sys.exit(f"{path}: not a whole number of UF2 blocks")

for offset in range(0, len(data), BLOCK_SIZE):
    magic0, magic1, flags = struct.unpack_from("<III", data, offset)
    if (magic0, magic1) != (0x0A324655, 0x9E5D5157):
        sys.exit(f"{path}: bad UF2 magic at offset {offset}")
    struct.pack_into("<I", data, offset + 8, flags | FLAG_FAMILY_ID_PRESENT)
    struct.pack_into("<I", data, offset + FAMILY_ID_OFFSET, family)

with open(path, "wb") as f:
    f.write(data)
