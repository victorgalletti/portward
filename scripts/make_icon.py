"""Draws the Portward app icon (an original shield mark) as a 1024px PNG.
Pure Python, no dependencies. Usage: python3 make_icon.py out.png"""
import math, struct, sys, zlib

N = 1024
SS = 2  # supersampling per axis

def shield_w(y):
    # Half width of the shield at height y (y down, -1..1). None outside.
    top, mid, bot = -0.70, 0.05, 0.80
    if y < top or y > bot:
        return None
    if y <= mid:
        # slight concave top edge handled by caller; straight sides
        return 0.60
    t = (y - mid) / (bot - mid)
    return 0.60 * (1 - t ** 1.7) ** 0.75

def in_shield(x, y, scale=1.0, dy=0.0):
    x, y = x / scale, (y - dy) / scale
    w = shield_w(y)
    if w is None or abs(x) > w:
        return False
    # top edge dips in the middle like a crest
    return y >= -0.70 + 0.08 * (1 - (x / 0.60) ** 2)

def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)

def in_rounded_rect(x, y, r=0.36, h=0.92):
    qx, qy = abs(x) - (h - r), abs(y) - (h - r)
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r <= 0

BG1, BG2 = (34, 34, 38), (18, 18, 19)
BLUE, BLUE2 = (78, 151, 250), (45, 105, 214)
DARK = (22, 24, 30)
WHITE = (255, 255, 255)

def color(x, y):
    if not in_rounded_rect(x, y):
        return None
    t = (y + 1) / 2
    c = tuple(int(BG1[i] * (1 - t) + BG2[i] * t) for i in range(3))
    if in_shield(x, y):
        t = (y + 0.7) / 1.5
        c = tuple(int(BLUE[i] * (1 - t) + BLUE2[i] * t) for i in range(3))
        if in_shield(x, y, 0.74, 0.02):
            c = DARK
            # check mark
            d = min(seg_dist(x, y, -0.24, 0.02, -0.06, 0.20), seg_dist(x, y, -0.06, 0.20, 0.27, -0.18))
            if d < 0.075:
                c = WHITE
    return c

rows = []
for j in range(N):
    row = bytearray([0])
    for i in range(N):
        acc = [0, 0, 0, 0]
        for sj in range(SS):
            for si in range(SS):
                x = ((i + (si + 0.5) / SS) / N) * 2 - 1
                y = ((j + (sj + 0.5) / SS) / N) * 2 - 1
                c = color(x, y)
                if c:
                    acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; acc[3] += 255
        n = SS * SS
        a = acc[3] // n
        if a:
            k = acc[3] / 255
            row += bytes([int(acc[0] / k), int(acc[1] / k), int(acc[2] / k), a])
        else:
            row += b"\0\0\0\0"
    rows.append(bytes(row))

def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
