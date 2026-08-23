#!/usr/bin/env python3
"""Generate a deterministic, dependency-free PNG that is expensive enough for interruption tests."""

from __future__ import annotations

import argparse
import binascii
import random
import struct
import zlib
from pathlib import Path


def chunk(kind: bytes, payload: bytes) -> bytes:
    body = kind + payload
    return struct.pack(">I", len(payload)) + body + struct.pack(">I", binascii.crc32(body) & 0xFFFFFFFF)


def generate(path: Path, width: int, height: int, seed: int) -> None:
    rng = random.Random(seed)
    compressor = zlib.compressobj(level=6)
    compressed = bytearray()
    row_bytes = width * 3
    for _ in range(height):
        # Filter type 0 plus deterministic high-entropy RGB pixels.  The resulting file remains
        # reasonably large, while decode/encode work depends on dimensions rather than disk speed.
        compressed.extend(compressor.compress(b"\x00" + rng.randbytes(row_bytes)))
    compressed.extend(compressor.flush())

    png = bytearray(b"\x89PNG\r\n\x1a\n")
    png.extend(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
    png.extend(chunk(b"IDAT", bytes(compressed)))
    png.extend(chunk(b"IEND", b""))
    path.write_bytes(png)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--width", type=int, default=1536)
    parser.add_argument("--height", type=int, default=1024)
    parser.add_argument("--seed", type=int, default=20260823)
    args = parser.parse_args()
    if not (256 <= args.width <= 4096 and 256 <= args.height <= 4096):
        parser.error("width and height must both be between 256 and 4096")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    generate(args.output, args.width, args.height, args.seed)
    print(f"{args.output} {args.output.stat().st_size} bytes {args.width}x{args.height}")


if __name__ == "__main__":
    main()
