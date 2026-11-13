"""Writes the composer-cat PNG frames into ../art/.

Run: python3 -I plugins/examples/composer-cat/art-src/sprite.py
Standard library only. The cat is rasterized from shapes, outlined by
dilation and shaded by rule, then small details are stamped on top. Every
frame is the same drawing with one parameter changed (eyes, tail or ear).
"""

import math
import struct
import zlib
from pathlib import Path

WIDTH = 36
HEIGHT = 28
CX = 14.0  # vertical axis of the cat, between pixel columns 13 and 14

CLEAR = (0, 0, 0, 0)
OUTLINE = (74, 41, 26, 255)
SHADOW = (196, 104, 40, 255)
BASE = (233, 138, 53, 255)
LIGHT = (248, 170, 82, 255)
STRIPE = (160, 80, 32, 255)
CREAM = (255, 232, 196, 255)
CREAM_SHADOW = (230, 190, 150, 255)
PINK = (240, 128, 144, 255)
EYE = (40, 24, 24, 255)
WHITE = (255, 255, 255, 255)

# Light comes from the top left.
LIGHT_DIR = (-0.6, -0.8)


# ---- Shapes: each returns a set of (x, y) pixels ----

def ellipse(cx, cy, rx, ry):
    out = set()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            dx, dy = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
            if dx * dx + dy * dy <= 1.0:
                out.add((x, y))
    return out


def triangle(a, b, c):
    def side(p, q, x, y):
        return (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0])

    out = set()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            px, py = x + 0.5, y + 0.5
            s = [side(a, b, px, py), side(b, c, px, py), side(c, a, px, py)]
            if all(v >= 0 for v in s) or all(v <= 0 for v in s):
                out.add((x, y))
    return out


def thick_path(points, radius, tip_radius):
    """Discs stamped along a polyline. Returns pixels and path parameter."""
    steps = []
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        n = max(1, int(math.hypot(x1 - x0, y1 - y0) * 4))
        steps += [(x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n) for i in range(n)]
    steps.append(points[-1])
    out = {}
    for y in range(HEIGHT):
        for x in range(WIDTH):
            best, where = None, 0.0
            for i, (sx, sy) in enumerate(steps):
                d = math.hypot(x + 0.5 - sx, y + 0.5 - sy)
                if best is None or d < best:
                    best, where = d, i / (len(steps) - 1)
            limit = radius + (tip_radius - radius) * where
            if best is not None and best <= limit:
                out[(x, y)] = where
    return out


def mirror(pixels):
    return {(int(2 * CX) - 1 - x, y) for x, y in pixels}


def neighbours(x, y):
    return [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]


def ring(mask):
    """Pixels touching the mask edge on a side, outside the mask."""
    return {n for p in mask for n in neighbours(*p) if n not in mask}


# ---- Canvas ----

class Canvas:
    def __init__(self):
        self.px = {}

    def set(self, x, y, color):
        if 0 <= x < WIDTH and 0 <= y < HEIGHT:
            self.px[(x, y)] = color

    def paint(self, mask, shader, merge=frozenset()):
        """Outline ring first, then the shaded fill, so later parts overlap.
        Ring pixels inside `merge` are skipped so the part joins that shape."""
        for x, y in ring(mask):
            if (x, y) not in merge:
                self.set(x, y, OUTLINE)
        for x, y in mask:
            self.set(x, y, shader(x, y))

    def rows(self):
        return [[self.px.get((x, y), CLEAR) for x in range(WIDTH)] for y in range(HEIGHT)]


def round_shader(cx, cy, rx, ry, base=BASE, light=LIGHT, shadow=SHADOW):
    """Tone by how much the surface faces the light, strongest near the rim."""
    def shade(x, y):
        nx, ny = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
        facing = nx * LIGHT_DIR[0] + ny * LIGHT_DIR[1]
        s = facing * math.hypot(nx, ny)
        if s > 0.5:
            return light
        if s < -0.45:
            return shadow
        return base
    return shade


# ---- The cat ----

HEAD = (CX, 11.6, 10.8, 8.4)
BODY_TOP = (CX, 19.5, 5.6, 4.5)
BODY_LOW = (CX, 22.6, 7.4, 5.6)
FLOOR = HEIGHT - 2  # last fill row, the outline row sits below it

# Tail path: base on the right of the body, rising and curling at the tip.
TAIL_PATH = [
    (19.5, 24.8), (23.5, 25.4), (27.4, 24.2), (29.6, 20.8), (30.2, 17.2),
    (30.2, 14.0), (30.8, 11.0), (31.6, 8.8), (32.6, 7.4),
]
TAIL_RADIUS = 1.55
TAIL_TIP_RADIUS = 1.25
TAIL_FIRST_MOVING = 4  # path points from here on follow the sway


def swayed_tail(sway):
    """Shift the upper tail sideways, more the further up it is."""
    out = []
    for i, (x, y) in enumerate(TAIL_PATH):
        weight = max(0, i - TAIL_FIRST_MOVING + 1) / (len(TAIL_PATH) - TAIL_FIRST_MOVING)
        out.append((x + sway * weight, y))
    return out


def tail_shader(parts):
    def shade(x, y):
        t = parts[(x, y)]
        if t > 0.88:
            return CREAM
        band = int(t * 14)
        if t > 0.12 and band % 3 == 0 and x >= 0:
            return STRIPE
        return BASE
    return shade


def draw_tail(cv, sway):
    parts = thick_path(swayed_tail(sway), TAIL_RADIUS, TAIL_TIP_RADIUS)
    mask = set(parts)
    cv.paint(mask, tail_shader(parts))
    # Light the left edge of the tail, shade the right edge.
    for (x, y), t in parts.items():
        if cv.px[(x, y)] == BASE:
            if (x - 1, y) not in mask:
                cv.set(x, y, LIGHT)
            elif (x + 1, y) not in mask:
                cv.set(x, y, SHADOW)


def draw_body(cv):
    top = ellipse(*BODY_TOP)
    low = {p for p in ellipse(*BODY_LOW) if p[1] <= FLOOR}
    body = top | low
    cv.paint(body, lambda x, y: round_shader(*BODY_LOW)(x, y))
    # Chest and belly, cream with a shadow edge on the right.
    chest = {p for p in ellipse(CX, 19.8, 3.2, 3.4) if p[1] <= FLOOR}
    for x, y in chest:
        edge = (x + 1, y) not in chest or (x, y + 1) not in chest and y < FLOOR
        cv.set(x, y, CREAM_SHADOW if (x >= CX + 1 and edge) else CREAM)
    # Under-chin shadow: darken the first fur rows below the head.
    head = ellipse(*HEAD)
    for x, y in body:
        if (x, y) in head:
            continue
        if any((x, y - d) in head for d in (1, 2)):
            cv.set(x, y, CREAM_SHADOW if (x, y) in chest else SHADOW)
    # Back stripes on the left and right flanks.
    for x, y in ((6, 21), (7, 21), (6, 24), (7, 24), (21, 21), (20, 21), (21, 24), (20, 24)):
        if cv.px.get((x, y)) in (BASE, SHADOW, LIGHT):
            cv.set(x, y, STRIPE)
    # Front paws: cream, 4 px wide, one outline column between them.
    for x0 in (9, 14):
        for x in range(x0, x0 + 4):
            cv.set(x, FLOOR - 1, CREAM)
            cv.set(x, FLOOR, CREAM_SHADOW if x in (12, 17) else CREAM)
    for y in (FLOOR - 2, FLOOR - 1, FLOOR):
        cv.set(13, y, OUTLINE)
    return body


def body_mask():
    top = ellipse(*BODY_TOP)
    return top | {p for p in ellipse(*BODY_LOW) if p[1] <= FLOOR}


EAR_PIVOT = (9.0, 6.0)
EAR_TIP_ANGLE = math.radians(20)


def rotate(p, pivot, angle):
    """Turn p about pivot so an upward point swings to the left (screen y down)."""
    vx, vy = p[0] - pivot[0], p[1] - pivot[1]
    c, s = math.cos(angle), math.sin(angle)
    return (pivot[0] + vx * c + vy * s, pivot[1] - vx * s + vy * c)


def ear_points(side, tipped):
    """Left ear triangle (a, b, apex). The tipped ear folds down and out."""
    a, b = (4.2, 9.2), (11.4, 3.4)
    apex = (4.0, 0.6)
    if tipped:
        a, b, apex = (rotate(p, EAR_PIVOT, EAR_TIP_ANGLE) for p in (a, b, apex))
    if side == "left":
        return a, b, apex
    mx = lambda p: (2 * CX - p[0], p[1])
    return mx(a), mx(b), mx(apex)


def ear_mask(ear_tipped):
    out = set()
    for side in ("left", "right"):
        out |= triangle(*ear_points(side, ear_tipped and side == "left"))
    return out


def draw_ears(cv, ear_tipped):
    for side in ("left", "right"):
        mask = triangle(*ear_points(side, ear_tipped and side == "left"))
        cv.paint(mask, round_shader(CX + (-8 if side == "left" else 8), 5, 6, 6))


def draw_inner_ears(cv, ear_tipped):
    """Pink inner ear: the ear triangle pulled towards its centre."""
    for side in ("left", "right"):
        a, b, apex = ear_points(side, ear_tipped and side == "left")
        cx, cy = (a[0] + b[0] + apex[0]) / 3, (a[1] + b[1] + apex[1]) / 3
        pull = lambda p: (cx + (p[0] - cx) * 0.5, cy + (p[1] - cy) * 0.5)
        outer = triangle(a, b, apex)
        for x, y in triangle(pull(a), pull(b), pull(apex)) & outer:
            if y <= apex[1] + 5.6:
                cv.set(x, y, PINK)


def draw_head(cv, ear_tipped):
    head = ellipse(*HEAD)
    cv.paint(head, round_shader(*HEAD), merge=ear_mask(ear_tipped) | body_mask())
    # Muzzle: cream with a shadow tone along its bottom.
    muzzle = ellipse(CX, 16.6, 4.4, 2.8)
    for x, y in muzzle:
        low = (x, y + 1) not in muzzle
        cv.set(x, y, CREAM_SHADOW if low else CREAM)


def draw_face(cv, blink):
    # Forehead stripes.
    for x, y in ((13, 3), (14, 3), (13, 4), (14, 4), (13, 5), (14, 5)):
        cv.set(x, y, STRIPE)
    for x, y in ((10, 4), (10, 5), (17, 4), (17, 5)):
        cv.set(x, y, STRIPE)
    # Eyes: 2 by 3, wide and low, one white highlight at the top left.
    for x0 in (8, 18):
        if blink:
            for x, y in ((x0 - 1, 13), (x0, 14), (x0 + 1, 14), (x0 + 2, 13)):
                cv.set(x, y, EYE)
        else:
            for x in (x0, x0 + 1):
                for y in (12, 13, 14):
                    cv.set(x, y, EYE)
            cv.set(x0, 12, WHITE)
    # Nose and mouth.
    cv.set(13, 16, PINK)
    cv.set(14, 16, PINK)
    for x, y in ((13, 17), (14, 17), (12, 16), (15, 16)):
        cv.set(x, y, OUTLINE)


def render(blink=False, sway=0.0, ear_tipped=False):
    cv = Canvas()
    draw_tail(cv, sway)
    draw_ears(cv, ear_tipped)
    draw_body(cv)
    draw_head(cv, ear_tipped)
    draw_inner_ears(cv, ear_tipped)
    draw_face(cv, blink)
    return cv.rows()


# ---- PNG output ----

def png(grid):
    raw = b"".join(b"\x00" + b"".join(bytes(c) for c in row) for row in grid)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def frames():
    return {
        "cat-idle": render(),
        "cat-blink": render(blink=True),
        "cat-tail-a": render(sway=-1.5),
        "cat-tail-b": render(sway=1.0),
        "cat-ear": render(ear_tipped=True),
    }


def main():
    out = Path(__file__).resolve().parent.parent / "art"
    out.mkdir(exist_ok=True)
    for name, grid in frames().items():
        (out / f"{name}.png").write_bytes(png(grid))


if __name__ == "__main__":
    main()
