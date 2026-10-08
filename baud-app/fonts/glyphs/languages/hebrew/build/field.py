"""Distance fields for anchor placement: rasterise a glyph's polygons at
RES units, take the Euclidean distance from ink, and score a mark's ink
placed at an offset by the smallest field value along its outline."""
import numpy as np
from scipy import ndimage

RES = 2.0
X0, X1, Y0, Y1 = -700, 1300, -500, 1300   # field extent (units)


def raster(polys):
    """Even-odd fill of flattened polygons into a bool grid (rows = y)."""
    nx = int((X1 - X0) / RES); ny = int((Y1 - Y0) / RES)
    grid = np.zeros((ny, nx), bool)
    edges = []
    for poly in polys:
        n = len(poly)
        for i in range(n):
            (xa, ya), (xb, yb) = poly[i], poly[(i + 1) % n]
            if ya != yb:
                edges.append((xa, ya, xb, yb))
    if not edges:
        return grid
    E = np.array(edges)
    xa, ya, xb, yb = E.T
    for j in range(ny):
        y = Y0 + (j + 0.5) * RES
        m = ((ya <= y) & (y < yb)) | ((yb <= y) & (y < ya))
        if not m.any():
            continue
        xs = np.sort(xa[m] + (y - ya[m]) * (xb[m] - xa[m]) / (yb[m] - ya[m]))
        for k in range(0, len(xs) - 1, 2):
            i0 = int(np.ceil((xs[k] - X0) / RES - 0.5)); i1 = int(np.floor((xs[k + 1] - X0) / RES - 0.5))
            if i1 >= i0:
                grid[j, max(i0, 0):min(i1 + 1, nx)] = True
    return grid


def distance(polys):
    """Distance (units) from every cell to the nearest ink; 0 inside ink."""
    g = raster(polys)
    return ndimage.distance_transform_edt(~g) * RES, g


def sample(field, pts):
    """Field value at points (N,2) in units, nearest cell."""
    nx = field.shape[1]; ny = field.shape[0]
    ix = np.clip(((pts[:, 0] - X0) / RES).astype(int), 0, nx - 1)
    iy = np.clip(((pts[:, 1] - Y0) / RES).astype(int), 0, ny - 1)
    return field[iy, ix]


def clearance(field, ink, outline, off):
    """Gap from a mark (outline points, ink grid) placed at off to the glyph.
    Negative (depth proxy) when the mark covers glyph ink."""
    P = outline + off
    d = sample(field, P)
    return float(d.min()) - RES / 2


def best_in_box(field, outline, box, step=4, prefer=None, wpref=0.0):
    """Offset in box (x0, y0, x1, y1) maximising clearance; optional pull
    toward `prefer` (x, y) by wpref units of clearance per 100 units."""
    x0, y0, x1, y1 = box
    best = (-1e9, None, None)
    for y in np.arange(y0, y1 + 0.1, step):
        for x in np.arange(x0, x1 + 0.1, step):
            c = sample(field, outline + (x, y)).min() - RES / 2
            # a mark entirely inside ink would also read high: require its centre clear
            if sample(field, np.array([[x, y]]))[0] <= 0:
                continue
            s = c
            if prefer is not None:
                s -= wpref * np.hypot(x - prefer[0], y - prefer[1]) / 100
            if s > best[0]:
                best = (s, (float(x), float(y)), float(c))
    return best[1], best[2]
