"""Generates the IntraDesk icons from one geometry, with no dependencies.

Run from the repository root: python res/gen_icons.py

Writes res/logo.svg, flutter/assets/icon.svg, res/icon.png, res/icon.ico,
res/tray-icon.ico and flutter/windows/runner/resources/app_icon.ico. The SVG and
the rasterized images share the shapes below, so edit them here, not in the outputs.
"""
import os
import struct
import zlib

# Brand colors, kept in sync with MyTheme in flutter/lib/common.dart.
TOP = (0xEA, 0x58, 0x0C)  # accent (dark theme)
BOTTOM = (0xC2, 0x41, 0x0C)  # accent (light theme)
WHITE = (0xFF, 0xFF, 0xFF)

# Shapes in a 256x256 box.
BACKGROUND = (8, 8, 248, 248, 48)
SCREEN_OUTER = (44, 58, 212, 170, 18)
SCREEN_INNER = (60, 74, 196, 154, 2)
NECK = (116, 170, 140, 190, 0)
BASE = (84, 190, 172, 204, 7)
ARROW_BAR = (96, 108, 160, 120, 0)
ARROW_LEFT = ((76, 114), (102, 94), (102, 134))
ARROW_RIGHT = ((180, 114), (154, 94), (154, 134))

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def in_rounded_rect(x, y, rect):
    x0, y0, x1, y1, r = rect
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def in_triangle(x, y, tri):
    (ax, ay), (bx, by), (cx, cy) = tri
    d1 = (x - bx) * (ay - by) - (ax - bx) * (y - by)
    d2 = (x - cx) * (by - cy) - (bx - cx) * (y - cy)
    d3 = (x - ax) * (cy - ay) - (cx - ax) * (y - ay)
    neg = d1 < 0 or d2 < 0 or d3 < 0
    pos = d1 > 0 or d2 > 0 or d3 > 0
    return not (neg and pos)


def is_white(x, y):
    if in_rounded_rect(x, y, SCREEN_OUTER) and not in_rounded_rect(x, y, SCREEN_INNER):
        return True
    return (
        in_rounded_rect(x, y, NECK)
        or in_rounded_rect(x, y, BASE)
        or in_rounded_rect(x, y, ARROW_BAR)
        or in_triangle(x, y, ARROW_LEFT)
        or in_triangle(x, y, ARROW_RIGHT)
    )


def render(size):
    """Returns RGBA rows (top-down) of the icon at `size` pixels, supersampled."""
    ss = 4 if size >= 128 else 8
    scale = 256.0 / size
    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            cover = 0
            white = 0
            for sy in range(ss):
                for sx in range(ss):
                    x = (px + (sx + 0.5) / ss) * scale
                    y = (py + (sy + 0.5) / ss) * scale
                    if in_rounded_rect(x, y, BACKGROUND):
                        cover += 1
                        if is_white(x, y):
                            white += 1
            n = ss * ss
            if cover == 0:
                row += b"\0\0\0\0"
                continue
            t = (py + 0.5) / size
            bg = [TOP[i] + (BOTTOM[i] - TOP[i]) * t for i in range(3)]
            w = white / cover
            rgb = [round(bg[i] * (1 - w) + WHITE[i] * w) for i in range(3)]
            row += bytes(rgb + [round(255 * cover / n)])
        rows.append(bytes(row))
    return rows


def png(rows, size):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\0" + r for r in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def dib(rows, size):
    # 32-bit BGRA, bottom-up, followed by an all-zero AND mask (alpha does the masking).
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytearray()
    for r in reversed(rows):
        for i in range(0, len(r), 4):
            pixels += bytes((r[i + 2], r[i + 1], r[i], r[i + 3]))
    mask_row = ((size + 31) // 32) * 4
    return header + bytes(pixels) + b"\0" * (mask_row * size)


def ico(sizes):
    # PNG for 256 (smaller file), classic DIB for the rest, which every loader reads.
    images = []
    for s in sizes:
        rows = render(s)
        images.append((s, png(rows, s) if s >= 256 else dib(rows, s)))
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for s, data in images:
        d = 0 if s >= 256 else s
        out += struct.pack("<BBBBHHII", d, d, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b"".join(d for _, d in images)


def rect_svg(rect, extra=""):
    x0, y0, x1, y1, r = rect
    rx = f' rx="{r}"' if r else ""
    return f'<rect x="{x0}" y="{y0}" width="{x1 - x0}" height="{y1 - y0}"{rx}{extra}/>'


def svg():
    sx0, sy0, sx1, sy1, sr = SCREEN_OUTER
    ix0, iy0, ix1, iy1, ir = SCREEN_INNER
    stroke = ix0 - sx0
    mid = (sx0 + stroke / 2, sy0 + stroke / 2, sx1 - stroke / 2, sy1 - stroke / 2, sr - stroke / 2)
    poly = lambda t: '<polygon points="' + " ".join(f"{x},{y}" for x, y in t) + '"/>'
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">'
        '<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="#{bytes(TOP).hex().upper()}"/>'
        f'<stop offset="1" stop-color="#{bytes(BOTTOM).hex().upper()}"/>'
        "</linearGradient></defs>"
        + rect_svg(BACKGROUND, ' fill="url(#g)"')
        + rect_svg(mid, f' fill="none" stroke="#FFFFFF" stroke-width="{stroke}"')
        + '<g fill="#FFFFFF">'
        + rect_svg(NECK)
        + rect_svg(BASE)
        + rect_svg(ARROW_BAR)
        + poly(ARROW_LEFT)
        + poly(ARROW_RIGHT)
        + "</g></svg>\n"
    )


def write(rel, data):
    path = os.path.join(ROOT, rel)
    mode = "w" if isinstance(data, str) else "wb"
    with open(path, mode, **({"encoding": "utf-8", "newline": "\n"} if mode == "w" else {})) as f:
        f.write(data)
    print("wrote", rel)


def main():
    write("res/logo.svg", svg())
    write("flutter/assets/icon.svg", svg())
    write("res/icon.png", png(render(512), 512))
    app = ico([16, 20, 24, 32, 40, 48, 64, 128, 256])
    write("res/icon.ico", app)
    write("flutter/windows/runner/resources/app_icon.ico", app)
    write("res/tray-icon.ico", ico([16, 20, 24, 32, 40, 48, 64]))


if __name__ == "__main__":
    main()
