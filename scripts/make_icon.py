"""Portward app icon: a neon wireframe globe drawn in an "inline" retro style
(each line is a thick stroke split by a thin dark groove), red outer ring,
white grid, soft glow, on a black rounded square. Original artwork.
Pure Python, no dependencies.
Usage: python3 make_icon.py out.png [size] [--tray]"""
import math, struct, sys, zlib

args = [a for a in sys.argv[1:] if not a.startswith("--")]
TRAY = "--tray" in sys.argv
N = int(args[1]) if len(args) > 1 else (44 if TRAY else 1024)
OUT = args[0]

R = 0.80 if TRAY else 0.60          # globe radius (icon space -1..1)
T = 0.15 if TRAY else 0.074         # stroke width
G = 0.0 if TRAY else 0.020          # inline groove width
RED, WHITE = (234, 32, 39), (245, 245, 245)

def seg(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)

def ellipse(x, y, a, b):
    g = math.sqrt((x / a) ** 2 + (y / b) ** 2) + 1e-9
    gx, gy = x / (a * a * g), y / (b * b * g)
    return abs(g - 1) / (math.hypot(gx, gy) + 1e-9)

def elements(x, y):
    """(distance, color) to each stroke centerline."""
    out = [(abs(math.hypot(x, y) - R), RED)]
    inner = R - (0 if TRAY else T * 1.25)   # white grid stops short of the red ring
    for k in ((0.62,) if TRAY else (0.55,)):
        out.append((ellipse(x, y, inner * k, inner), WHITE))
    out.append((seg(x, y, 0, -inner, 0, inner), WHITE))
    for f in ((0.0,) if TRAY else (-0.5, 0.0, 0.5)):
        yy = f * inner
        w = math.sqrt(inner * inner - yy * yy)
        out.append((seg(x, y, -w, yy, w, yy), WHITE))
    if TRAY:
        out.append((seg(x, y, -R * .87, -R * .5, R * .87, -R * .5), WHITE))
        out.append((seg(x, y, -R * .87, R * .5, R * .87, R * .5), WHITE))
    return out

def rounded_rect(x, y, r=0.38, h=0.94):
    qx, qy = abs(x) - (h - r), abs(y) - (h - r)
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r

px = 2.0 / N
def cover(d, half):  # anti-aliased coverage of a band |d| < half
    return max(0.0, min(1.0, (half - d) / px + 0.5))

rows = []
for j in range(N):
    row = bytearray([0])
    y = (j + 0.5) / N * 2 - 1
    for i in range(N):
        x = (i + 0.5) / N * 2 - 1
        if TRAY:
            a = 0.0
            for d, _ in elements(x, y):
                a = max(a, cover(d, T / 2))
            row += bytes([0, 0, 0, int(255 * a)])
            continue
        bg = cover(rounded_rect(x, y), 0) if False else max(0.0, min(1.0, -rounded_rect(x, y) / px + 0.5))
        if bg <= 0:
            row += b"\0\0\0\0"
            continue
        # background: near-black with a faint red vignette glow behind the globe
        rr = math.hypot(x, y)
        base = [10 + 26 * math.exp(-((rr - R) / 0.22) ** 2), 10, 12]
        col = list(base)
        # glow
        glow = [0.0, 0.0, 0.0]
        for d, c in elements(x, y):
            gl = 0.55 * math.exp(-max(0.0, d - T / 2) / 0.035)
            for k in range(3):
                glow[k] = max(glow[k], gl * c[k])
        for k in range(3):
            col[k] = min(255, col[k] + glow[k] * 0.55)
        # strokes (inline: solid band minus a thin groove)
        best = None
        for d, c in elements(x, y):
            a = cover(d, T / 2) - cover(d, G / 2)
            if best is None or a > best[0]:
                best = (a, c)
        a, c = best
        col = [col[k] * (1 - a) + c[k] * a for k in range(3)]
        row += bytes([int(col[0]), int(col[1]), int(col[2]), int(255 * bg)])
    rows.append(bytes(row))

def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

open(OUT, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b""))
