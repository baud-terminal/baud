#!/usr/bin/env python3
"""FreeType as a rasterisation oracle for baud-font/tests/raster.rs.

Renders every glyph of one face at the requested pixel sizes with FreeType's
unhinted smooth rasteriser (`FT_LOAD_NO_HINTING | FT_LOAD_TARGET_NORMAL`,
`[[source: freetype-ftgrays]]`) and prints each bitmap with the origin
FreeType gave it, so the test can rasterise the same glyph into the same
frame and compare pixel by pixel.

Usage: raster-oracle.py <font file> <face index> <px> [<px> ...]

Output, per glyph and size, two lines:

    g <gid> <px> <bitmap_left> <bitmap_top> <width> <rows>
    <width*rows bytes as hex, row-major, top row first>

Needs `freetype-py` (`nix-shell -p python3Packages.freetype-py`).
"""

import sys

import freetype


def main():
    path, index = sys.argv[1], int(sys.argv[2])
    sizes = [float(s) for s in sys.argv[3:]]
    face = freetype.Face(path, index)
    if not (face.face_flags & freetype.FT_FACE_FLAG_SCALABLE):
        return  # a bitmap-only face has no outlines to rasterise
    # NO_BITMAP: a face with embedded strikes (Monaco has them at small
    # sizes) would otherwise answer with the strike rather than the outline,
    # and its rows do not even match the declared pitch through this binding.
    flags = (
        freetype.FT_LOAD_NO_HINTING
        | freetype.FT_LOAD_NO_BITMAP
        | freetype.FT_LOAD_RENDER
        | freetype.FT_LOAD_TARGET_NORMAL
    )
    out = []
    for px in sizes:
        # 26.6 fixed point at 72 dpi: one point is one pixel.
        face.set_char_size(int(round(px * 64)), 0, 72, 72)
        for gid in range(face.num_glyphs):
            face.load_glyph(gid, flags)
            bmp = face.glyph.bitmap
            w, h, pitch = bmp.width, bmp.rows, bmp.pitch
            buf = bmp.buffer
            assert pitch >= 0 and len(buf) == h * pitch, (gid, w, h, pitch, len(buf))
            rows = []
            for r in range(h):
                rows.append(bytes(buf[r * pitch : r * pitch + w]))
            out.append(
                "g %d %g %d %d %d %d\n%s\n"
                % (gid, px, face.glyph.bitmap_left, face.glyph.bitmap_top, w, h, b"".join(rows).hex())
            )
    sys.stdout.write("".join(out))


if __name__ == "__main__":
    main()
