"""The lockup's geometry, read off the renderer's own drawing of it.

    geometry.py <walls.png> <out dir>

`walls.png` is the logo film's canvas of the held lockup on the dark
half, 840 by 580, its block shown (`baud-app/src/logo_film.rs`):
`assets/readme/logo-walls.png`, the drawing the plasma's path was chosen
against. Writes into `out dir`, each 840 by 580 row by row:

  sdf.f32    the signed distance to the glyphs' outline in canvas pixels,
             positive outside the ink and negative inside, zero on the
             contour where the coverage is a half, sub-pixel by the
             coverage within a pixel of it; little-endian float32;
  nx.f32,    its gradient normalised, the outline's outward normal;
  ny.f32
  holes.u8   1 inside a closed counter -- the open pixels no path from
             the canvas's border reaches, the bowls of b, a and d -- and
             0 elsewhere.

Each pixel's coverage is solved along the line from the page to the ink
it is nearest, the word's or the amber's: the renderer draws each glyph
source-over in its one colour on the page. Canvas pixels are lockup units
moved by (60, 75) and doubled, as the film draws it. Every step is exact
arithmetic or a correctly rounded square root (`det.py`), so the files
are the same bits on every platform.
"""

import os
import sys
from collections import deque

import numpy as np

W, H = 840, 580
PAGE = (0x0D, 0x11, 0x17)
INKS = ((0xF3, 0xF4, 0xF6), (0xF4, 0xA3, 0x2C))


def coverage(img):
    """Each pixel's coverage, 0 to 1, along its nearer ink's line."""
    page = np.array(PAGE, float)
    best_c, best_e = None, None
    for ink in INKS:
        d = np.array(ink, float) - page
        dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        q = img - page
        c = np.clip((q[..., 0] * d[0] + q[..., 1] * d[1] + q[..., 2] * d[2]) / dd, 0, 1)
        e = np.abs(img - (page + c[..., None] * d)).max(2)
        if best_c is None:
            best_c, best_e = c, e
        else:
            # The word's ink wins a tie, as it is tried first.
            take = e < best_e
            best_c = np.where(take, c, best_c)
            best_e = np.where(take, e, best_e)
    return best_c


def components(mask):
    """The 4-connected components of a boolean mask: labels and count."""
    lab = np.zeros(mask.shape, np.int32)
    n = 0
    ys, xs = np.nonzero(mask)
    for y0, x0 in zip(ys, xs):
        if lab[y0, x0]:
            continue
        n += 1
        q = deque([(y0, x0)])
        lab[y0, x0] = n
        while q:
            y, x = q.popleft()
            for yy, xx in ((y + 1, x), (y - 1, x), (y, x + 1), (y, x - 1)):
                if 0 <= yy < H and 0 <= xx < W and mask[yy, xx] and not lab[yy, xx]:
                    lab[yy, xx] = n
                    q.append((yy, xx))
    return lab, n


def edt(inside):
    """The exact Euclidean distance from every pixel to the nearest pixel
    not in `inside` (0 outside it): Felzenszwalb and Huttenlocher's
    lower envelope of parabolas, down the columns and then along the rows."""
    inf = 1e12

    def d1(f):
        n = len(f)
        d = np.empty(n)
        v = np.zeros(n, int)
        z = np.empty(n + 1)
        k = 0
        z[0], z[1] = -inf, inf
        for q in range(1, n):
            s = ((f[q] + q * q) - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k])
            while s <= z[k]:
                k -= 1
                s = ((f[q] + q * q) - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k])
            k += 1
            v[k] = q
            z[k], z[k + 1] = s, inf
        k = 0
        for q in range(n):
            while z[k + 1] < q:
                k += 1
            d[q] = (q - v[k]) * (q - v[k]) + f[v[k]]
        return d

    f = np.where(inside, inf, 0.0)
    g = np.empty_like(f)
    for x in range(f.shape[1]):
        g[:, x] = d1(f[:, x])
    out = np.empty_like(f)
    for y in range(f.shape[0]):
        out[y, :] = d1(g[y, :])
    return np.sqrt(out)


def fields(img):
    """The signed distance, the normal's two components and the holes."""
    cov = coverage(img)
    ink = cov >= 0.5
    olab, on = components(~ink)
    border = set(np.unique(np.concatenate([olab[0], olab[-1], olab[:, 0], olab[:, -1]]))) - {0}
    holes = np.isin(olab, [i for i in range(1, on + 1) if i not in border])
    sdf = np.where(ink, -(edt(ink) - 0.5), edt(~ink) - 0.5)
    # Within a pixel of the contour the coverage says where it is.
    sdf = np.where(np.abs(sdf) <= 1.0, 0.5 - cov, sdf)
    gy, gx = np.gradient(sdf)
    gn = np.sqrt(gx * gx + gy * gy) + 1e-9
    return sdf, gx / gn, gy / gn, holes


def main():
    from PIL import Image

    walls, out = sys.argv[1:3]
    pic = Image.open(walls)
    if pic.size != (W, H) or pic.mode != "RGB":
        raise SystemExit(f"geometry: {walls} is {pic.size} {pic.mode}, not an 840 by 580 RGB canvas")
    img = np.asarray(pic).astype(np.float64)
    sdf, nx, ny, holes = fields(img)
    os.makedirs(out, exist_ok=True)
    for name, a in (("sdf", sdf), ("nx", nx), ("ny", ny)):
        a.astype("<f4").tofile(os.path.join(out, f"{name}.f32"))
    holes.astype(np.uint8).tofile(os.path.join(out, "holes.u8"))


if __name__ == "__main__":
    main()
