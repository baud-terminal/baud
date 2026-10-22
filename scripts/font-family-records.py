"""Give a stock JetBrains Mono italic the embedded family's name records.

    font-family-records.py STOCK.ttf BUILT-REGULAR.ttf OUT.ttf

baud's embedded font is four faces used as one family: the built Regular and
Bold, and JetBrains Mono's stock Italic and Bold Italic. The OpenType name table
says a family name (nameID 1) "should be shared among at most four fonts that
differ only in weight or style (italic/oblique) ... a font style-linking group:
regular, italic (or oblique), bold, and bold italic" (ms-opentype-name). The
built pair carry the embedded family's name; the stock italics carried
"JetBrains Mono". This writes a copy of STOCK whose nameIDs 1, 4 and 6 are the
family BUILT-REGULAR's nameID 1 names, with STOCK's own subfamily (nameID 2):

    1  the family                        Proto Hebrew Mono C
    4  the family and the subfamily      Proto Hebrew Mono C Italic
    6  both without spaces, by a hyphen  ProtoHebrewMonoC-Italic

Nothing else changes. The script refuses to write a file in which any table but
`name` differs from STOCK's, or `head` differs anywhere but checkSumAdjustment,
which a changed table moves. head.modified is STOCK's (no timestamp is
recalculated), so the bytes depend only on the inputs and fontTools.
"""

import sys

from fontTools.ttLib import TTFont


def windows_english(font, name_id):
    """`font`'s one nameID `name_id` record at Windows, Unicode BMP, en-US."""
    got = [
        r
        for r in font["name"].names
        if r.nameID == name_id and (r.platformID, r.platEncID, r.langID) == (3, 1, 0x409)
    ]
    if len(got) != 1:
        raise SystemExit("nameID %d: want one 3/1/0x409 record, found %d" % (name_id, len(got)))
    return got[0]


def main(stock_path, built_path, out_path):
    family = windows_english(TTFont(built_path, lazy=True), 1).toUnicode()
    font = TTFont(stock_path, recalcTimestamp=False, recalcBBoxes=False)
    sub = windows_english(font, 2).toUnicode()
    want = {
        1: family,
        4: family + " " + sub,
        6: family.replace(" ", "") + "-" + sub.replace(" ", ""),
    }
    for name_id, value in want.items():
        rec = windows_english(font, name_id)
        rec.string = value
    for rec in font["name"].names:
        if rec.nameID in want and (rec.platformID, rec.platEncID, rec.langID) != (3, 1, 0x409):
            raise SystemExit("nameID %d has a record this script does not rewrite" % rec.nameID)
    font.save(out_path)

    stock = TTFont(stock_path, lazy=True)
    out = TTFont(out_path, lazy=True)
    if sorted(stock.reader.keys()) != sorted(out.reader.keys()):
        raise SystemExit("the table set changed")
    for tag in stock.reader.keys():
        a, b = stock.reader[tag], out.reader[tag]
        if tag == "head":
            # checkSumAdjustment is bytes 8 to 11.
            a, b = a[:8] + a[12:], b[:8] + b[12:]
        if tag != "name" and a != b:
            raise SystemExit("table %r changed" % tag)
    for name_id, value in want.items():
        if windows_english(out, name_id).toUnicode() != value:
            raise SystemExit("nameID %d did not land" % name_id)
    print("wrote", out_path, "as", want[4])


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    main(*sys.argv[1:])
