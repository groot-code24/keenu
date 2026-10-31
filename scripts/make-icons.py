#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Draws Keenu's application icon and writes it in each format the packages need.

    python3 scripts/make-icons.py

Standard library only, so the result is reproducible. The icon is the island: a black notch on a dark
square with a white bar for what is playing. To change the look, change `pixel` below and run it again.
"""
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "packaging", "icons")
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]


def rounded(x, y, left, top, right, bottom, radius):
    """Coverage of a pixel centre by a rounded rectangle, antialiased over a pixel's width."""
    cx = min(max(x, left + radius), right - radius)
    cy = min(max(y, top + radius), bottom - radius)
    distance = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
    return min(max(radius - distance + 0.5, 0.0), 1.0)


def pixel(size, px, py):
    """RGBA of one pixel, composing the shapes back to front."""
    u = size / 256.0
    x, y = px + 0.5, py + 0.5
    r, g, b, a = 0.0, 0.0, 0.0, 0.0

    def over(cr, cg, cb, coverage):
        nonlocal r, g, b, a
        out = coverage + a * (1 - coverage)
        if out > 0:
            r = (cr * coverage + r * a * (1 - coverage)) / out
            g = (cg * coverage + g * a * (1 - coverage)) / out
            b = (cb * coverage + b * a * (1 - coverage)) / out
        a = out

    over(28, 28, 46, rounded(x, y, 8 * u, 8 * u, 248 * u, 248 * u, 52 * u))
    # The island hangs from the top of the square.
    over(0, 0, 0, rounded(x, y, 40 * u, -40 * u, 216 * u, 112 * u, 40 * u) * rounded(x, y, 8 * u, 8 * u, 248 * u, 248 * u, 52 * u))
    over(255, 255, 255, rounded(x, y, 72 * u, 66 * u, 148 * u, 84 * u, 9 * u))
    over(142, 142, 147, rounded(x, y, 162 * u, 66 * u, 186 * u, 84 * u, 9 * u))
    # The K, so it can be told apart in a taskbar.
    over(255, 255, 255, rounded(x, y, 84 * u, 134 * u, 108 * u, 224 * u, 4 * u))
    for step in range(0, 44):
        t = step / 43.0
        over(255, 255, 255, rounded(x, y, (108 + 58 * t) * u - 7 * u, (176 - 40 * t) * u - 7 * u, (108 + 58 * t) * u + 7 * u, (176 - 40 * t) * u + 7 * u, 7 * u))
        over(255, 255, 255, rounded(x, y, (108 + 62 * t) * u - 7 * u, (176 + 44 * t) * u - 7 * u, (108 + 62 * t) * u + 7 * u, (176 + 44 * t) * u + 7 * u, 7 * u))
    return (int(r + 0.5), int(g + 0.5), int(b + 0.5), int(a * 255 + 0.5))


def png(size):
    rows = bytearray()
    for py in range(size):
        rows.append(0)
        for px in range(size):
            rows.extend(pixel(size, px, py))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b"")


def ico(images):
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    body = b""
    for size, data in images:
        side = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        body += data
    return head + entries + body


def icns(images):
    kinds = {16: b"icp4", 32: b"icp5", 64: b"icp6", 128: b"ic07", 256: b"ic08", 512: b"ic09", 1024: b"ic10"}
    body = b""
    for size, data in images:
        if size in kinds:
            body += kinds[size] + struct.pack(">I", len(data) + 8) + data
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def main():
    os.makedirs(OUT, exist_ok=True)
    images = [(size, png(size)) for size in SIZES]
    by_size = dict(images)
    with open(os.path.join(OUT, "keenu.ico"), "wb") as out:
        out.write(ico([(size, by_size[size]) for size in (16, 24, 32, 48, 64, 128, 256)]))
    with open(os.path.join(OUT, "keenu.icns"), "wb") as out:
        out.write(icns(images))
    for size in (256, 512):
        with open(os.path.join(OUT, f"keenu-{size}.png"), "wb") as out:
            out.write(by_size[size])


if __name__ == "__main__":
    main()
