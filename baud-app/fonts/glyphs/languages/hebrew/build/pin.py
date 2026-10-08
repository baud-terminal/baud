"""The one fontTools setting the gated bytes depend on, shared by build.py and instance.py.

fontTools packs GSUB and GPOS with HarfBuzz's repacker whenever uharfbuzz is
importable (USE_HARFBUZZ_REPACKER unset), and with its own pure-Python
serializer otherwise. The two give the same tables in a different subtable
order. Measured on 2026-10-08 with the pinned interpreter, on the final3
recipes before the hebr script record and the second trademark notice were
added: with the repacker off, or with uharfbuzz absent from the env,
Regular-A-L2 and Bold-A-L2 came out c12ef998... and 6499bbbf... instead of the
then-gated ea07610d... and c508ee5a...; GPOS was the only table that differed
(same length, identical XML) and head differed only through its checksum. The
gated files were built with uharfbuzz importable and the option unset.

So the option is pinned to True, not left unset: unset falls back to the
pure-Python serializer, silently, if uharfbuzz is missing or the repack fails,
and writes different bytes; True raises instead. uharfbuzz is a build-time
dependency only; nothing at run time links HarfBuzz.
"""
from fontTools.ttLib.tables.otBase import USE_HARFBUZZ_REPACKER


def table_pinned(font, tag):
    """The bytes save_pinned writes for table `tag`: compiled under the same pin
    when fontTools has decompiled it, the file's own bytes when it has not."""
    font.cfg[USE_HARFBUZZ_REPACKER] = True
    return font.getTableData(tag)


def save_pinned(font, path):
    font.cfg[USE_HARFBUZZ_REPACKER] = True
    font.save(path)
