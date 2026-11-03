"""Merge Noto Sans Devanagari into one built face.

  merge.py BUILT.ttf NOTO.ttf STYLE OUT.ttf REPORT.json

BUILT is a face scripts/build-fonts.sh has built (JetBrains Mono with baud's
Hebrew), NOTO the pinned Noto Sans Devanagari variable font instanced at the
style's weight, its width axis kept (hebrew/build/instance.py), and STYLE one
of Regular, Bold, Italic, BoldItalic. OUT gets every glyph, lookup and table
of BUILT unchanged, and after them:

- Noto's glyphs: each one a code point BUILT does not map reaches, each one a
  Noto lookup names, and their components, in Noto's order and at Noto's
  scale (both faces are 1000 units to the em), with their advances, their
  classes in GDEF and their outline and advance variations;
- the code points BUILT does not map, in every cmap subtable that can hold
  them;
- Noto's dev2 and deva scripts in GSUB and GPOS, with Noto's lookups, its
  features for them and its mark filtering sets, appended after BUILT's, and
  its GPOS variation store as GDEF's;
- one single substitution more, under dev2's and deva's nukt: BUILT's dotted
  circle to Noto's. The Indic shapers put U+25CC before a mark that has no
  base after locl and ccmp, from the face's cmap, which gives BUILT's dotted
  circle; Noto's mark positioning names its own (lookups 20-27), so the
  substitution, in the first stage after that, hands it Noto's, and a mark
  sits on the circle it was placed for. BUILT's U+25CC is unchanged
  everywhere else;
- Noto's width axis: fvar (one axis, wdth 62.5-100, default 100, hidden from
  user interfaces: it is there for a program to fit a word, and only the
  Devanagari glyphs vary along it), avar, gvar and HVAR (BUILT's glyphs vary
  by nothing), and the STAT table OpenType requires of a variable font;
- the font-wide values the new glyphs change: OS/2's Devanagari range bit,
  usMaxContext and xAvgCharWidth, post's isFixedPitch (0: the Devanagari is
  proportionally spaced), and head's and hhea's extents, which fontTools
  computes on save. The line metrics, the em and every existing advance are
  BUILT's.

The italics carry the upright Devanagari at their weight: Noto has no
italic, and nothing is synthesised.

Noto's glyph for U+0020 is not copied: Noto's one lookup naming it (GPOS
lookup 3, a chained context of `dist` for Vedic signs after a space or a
dotted circle) names BUILT's space instead, the glyph a space in the run
already is. Noto's other glyphs for code points BUILT maps are not copied
either: no lookup names them and BUILT's cmap keeps its own.

GSUB is written by hand around BUILT's own bytes (spliced()): BUILT's
lookups and every subtable they name are copied as one block, byte for
byte, so stock JetBrains Mono's lookups stay its own bytes, with the
appended lookups after them, under a ScriptList, FeatureList and LookupList
written again: the features sorted by tag, as OpenType's common table
formats ask, which moves BUILT's feature indices, so its language systems
are written again too. BUILT's old list tables that lie among its lookups
(772 bytes in each style: the ten LangSys tables of DFLT, which hebr shares,
and latn, and the four stylistic sets' FeatureParams) stay where they were,
named by nothing. GPOS is
compiled by fontTools, as the Hebrew build compiles it.

The report holds what was kept and the map from Noto's glyph IDs to OUT's,
and digests of BUILT's objects before and after: every glyph's outline bytes
and metrics, every cmap entry, every lookup's bytes compiled alone, the
features each existing language system names, GDEF's classes and mark sets,
and every name record but nameIDs 0 and 7. The merge refuses to write OUT
when one of them differs.
"""
import copy
import hashlib
import json
import os
import struct
import sys

from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables import otTables as ot
from fontTools.ttLib.tables.DefaultTable import DefaultTable
from fontTools.ttLib.tables.otBase import OTTableWriter, USE_HARFBUZZ_REPACKER
from fontTools.otlLib.builder import buildStatTable
from fontTools.otlLib.maxContextCalc import maxCtxFont

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'hebrew', 'build'))
from pin import save_pinned  # noqa: E402

# The notices nameID 0 and 7 gain, read from the Noto file and required to be
# this text byte for byte ("Any trademark notices must remain in any
# derivative fonts", OFL-FAQ 1.1-update7 Q3.7).
NOTO_COPYRIGHT = 'Copyright 2022 The Noto Project Authors (https://github.com/notofonts/devanagari)'
NOTO_TRADEMARK = 'Noto is a trademark of Google Inc.'
# OS/2 ulUnicodeRange1 bit 15: Devanagari (OpenType OS/2, Unicode ranges).
DEVANAGARI_RANGE_BIT = 15
# fvar's axis flag 0x0001 (OpenType fvar): "The axis should not be exposed
# directly in user interfaces."
HIDDEN_AXIS = 0x0001
STYLES = {'Regular': (400, 0), 'Bold': (700, 0), 'Italic': (400, 1), 'BoldItalic': (700, 1)}
SUFFIX = '.deva'
NO_VARIATION = 0xFFFFFFFF


def fail(msg):
    raise SystemExit('merge.py: ' + msg)


def digest(b):
    return hashlib.sha256(b).hexdigest()


def walk(obj, visit, seen=None):
    """Call visit on every otTables object reachable from obj, once each."""
    if seen is None:
        seen = set()
    if id(obj) in seen:
        return
    seen.add(id(obj))
    if isinstance(obj, (list, tuple)):
        for x in obj:
            walk(x, visit, seen)
    elif isinstance(obj, dict):
        for x in obj.values():
            walk(x, visit, seen)
    elif isinstance(obj, ot.BaseTable):
        visit(obj)
        for x in vars(obj).values():
            if isinstance(x, (list, tuple, dict, ot.BaseTable)):
                walk(x, visit, seen)


def names_in(obj):
    """Every string reachable from an otTables object: its glyph names."""
    out = set()

    def rec(o, seen):
        if isinstance(o, str):
            out.add(o)
            return
        if id(o) in seen:
            return
        seen.add(id(o))
        if isinstance(o, (list, tuple)):
            for x in o:
                rec(x, seen)
        elif isinstance(o, dict):
            for k, v in o.items():
                rec(k, seen)
                rec(v, seen)
        elif isinstance(o, ot.BaseTable):
            for v in vars(o).values():
                rec(v, seen)
    rec(obj, set())
    return out


def lookup_bytes(font, tag, lookup):
    """A lookup's bytes compiled alone, against `font`'s glyph order."""
    w = OTTableWriter(tableTag=tag)
    lookup.compile(w, font)
    return w.getAllData()


def snapshot(path, names=None):
    """Digests of what the merge must not change in the face at `path`, read
    from the file: each glyph's bytes as stored and its metrics, each cmap
    entry, each lookup compiled alone, the features each language system
    names, each glyph's GDEF class, each mark set, each name record but 0 and
    7. `names` limits the glyphs to the first face's."""
    font = TTFont(path)
    order = font.getGlyphOrder() if names is None else names
    glyf, hmtx = font['glyf'], font['hmtx']
    snap = {'glyphs': {}, 'cmap': {}, 'lookups': {}, 'langsys': {}, 'gdef': {}, 'names': {}}
    for g in order:
        raw = glyf.glyphs[g]
        data = getattr(raw, 'data', None)
        if data is None:
            # an empty glyph has no bytes and nothing else (fontTools' Glyph(b''))
            if vars(raw) != {'numberOfContours': 0}:
                fail('glyph %s was decompiled before its bytes were read' % g)
            data = b''
        snap['glyphs'][g] = digest(data) + ' %d %d' % hmtx[g]
    keep = set(order)
    for t in font['cmap'].tables:
        key = '%d/%d/%d' % (t.platformID, t.platEncID, t.format)
        for c, g in t.cmap.items():
            if g in keep:
                snap['cmap']['%s U+%04X' % (key, c)] = g
    for tag in ('GSUB', 'GPOS'):
        table = font[tag].table
        for i, lk in enumerate(table.LookupList.Lookup):
            snap['lookups']['%s %d' % (tag, i)] = digest(lookup_bytes(font, tag, lk))
        feats = table.FeatureList.FeatureRecord
        for sr in table.ScriptList.ScriptRecord:
            systems = [('dflt', sr.Script.DefaultLangSys)] + [(r.LangSysTag, r.LangSys) for r in sr.Script.LangSysRecord]
            for ltag, ls in systems:
                if ls is None:
                    continue
                named = sorted((feats[i].FeatureTag, tuple(feats[i].Feature.LookupListIndex)) for i in ls.FeatureIndex)
                req = ls.ReqFeatureIndex
                required = None if req == 0xFFFF else (feats[req].FeatureTag, tuple(feats[req].Feature.LookupListIndex))
                snap['langsys']['%s %s %s' % (tag, sr.ScriptTag, ltag)] = digest(json.dumps([named, required]).encode())
    gdef = font['GDEF'].table
    for g in order:
        snap['gdef']['class ' + g] = str(gdef.GlyphClassDef.classDefs.get(g, 0))
    if gdef.MarkGlyphSetsDef:
        for i, cov in enumerate(gdef.MarkGlyphSetsDef.Coverage):
            if names is None or i < NSETS[0]:
                snap['gdef']['set %d' % i] = digest(json.dumps(sorted(g for g in cov.glyphs if g in keep)).encode())
    for r in font['name'].names:
        if r.nameID not in (0, 7):
            snap['names']['%d %d %d %d' % (r.nameID, r.platformID, r.platEncID, r.langID)] = digest(r.toUnicode().encode())
    return snap


# how many mark sets BUILT has: OUT's after them are Noto's
NSETS = [0]


def u16(b, at):
    return struct.unpack_from('>H', b, at)[0]


RANGES = set()


def lookup_region(t):
    """Where a GSUB or GPOS table's lookups start and each lookup's offset,
    and the bytes of the Script, LangSys, Feature and FeatureParams tables
    that lie past that start, among the lookups. Every subtable lies after
    the table that names it (an offset is unsigned), so the lookups and
    their subtables are all in the block from that start to the table's end;
    the list tables found in it are left there, named by nothing, when the
    block is copied under lists written again."""
    major, minor, sl, fl, ll = struct.unpack_from('>5H', t)
    if (major, minor) != (1, 0):
        fail('table version %d.%d; want 1.0' % (major, minor))
    lookups = [ll + u16(t, ll + 2 + 2 * i) for i in range(u16(t, ll))]
    start = min(lookups)
    ends = [sl + 2 + 6 * u16(t, sl), fl + 2 + 6 * u16(t, fl), ll + 2 + 2 * len(lookups)]

    def langsys(at):
        ends.append(at + 6 + 2 * u16(t, at + 4))
        RANGES.add((at, ends[-1]))
    for i in range(u16(t, sl)):
        sc = sl + u16(t, sl + 6 + 6 * i)
        ends.append(sc + 4 + 6 * u16(t, sc + 2))
        RANGES.add((sc, ends[-1]))
        if u16(t, sc):
            langsys(sc + u16(t, sc))
        for k in range(u16(t, sc + 2)):
            langsys(sc + u16(t, sc + 4 + 6 * k + 4))
    for i in range(u16(t, fl)):
        ft = fl + u16(t, fl + 6 + 6 * i)
        ends.append(ft + 4 + 2 * u16(t, ft + 2))
        RANGES.add((ft, ends[-1]))
        if u16(t, ft):
            ends.append(ft + u16(t, ft) + 4)
            RANGES.add((ft + u16(t, ft), ends[-1]))
    inside = sorted((b, e) for b, e in RANGES if e > start)
    RANGES.clear()
    return start, lookups, inside


def spliced(tag, old, table, nold):
    """`tag`'s bytes: BUILT's lookups and their subtables as BUILT stores
    them, byte for byte and at the same distances from each other, and
    after them the appended lookups, under a ScriptList, FeatureList and
    LookupList written again for the merged table. fontTools would lay the
    whole table out again (the same content in another order), and stock
    JetBrains Mono's GSUB is its own, byte for byte, in every lookup."""
    start, lookups, stranded = lookup_region(old)
    region = old[start:]
    # the appended lookups, laid out by fontTools alone in a table of their own
    tmp = newTable(tag)
    tmp.table = getattr(ot, tag)()
    tmp.table.Version = 0x00010000
    tmp.table.ScriptList = ot.ScriptList()
    tmp.table.ScriptList.ScriptRecord = []
    tmp.table.FeatureList = ot.FeatureList()
    tmp.table.FeatureList.FeatureRecord = []
    tmp.table.LookupList = ot.LookupList()
    tmp.table.LookupList.Lookup = table.LookupList.Lookup[nold:]
    view = TTFont()
    view.setGlyphOrder(BUILT_ORDER[0])
    view.cfg[USE_HARFBUZZ_REPACKER] = True
    fresh = tmp.compile(view)
    fstart, flookups, inside = lookup_region(fresh)
    if inside:
        fail('%s: the appended lookups hold list tables' % tag)
    tail = fresh[fstart:]

    def langsys_bytes(ls):
        return struct.pack('>HHH', 0, ls.ReqFeatureIndex, len(ls.FeatureIndex)) + b''.join(
            struct.pack('>H', i) for i in ls.FeatureIndex)

    def script_bytes(sc):
        systems = [(r.LangSysTag, langsys_bytes(r.LangSys)) for r in sc.LangSysRecord]
        head = 4 + 6 * len(systems)
        out, body = [], b''
        dflt = 0
        if sc.DefaultLangSys is not None:
            dflt = head
            body += langsys_bytes(sc.DefaultLangSys)
        recs = b''
        for ltag, b in systems:
            recs += ltag.encode('ascii') + struct.pack('>H', head + len(body))
            body += b
        return struct.pack('>HH', dflt, len(systems)) + recs + body

    def feature_bytes(f):
        idx = f.LookupListIndex
        params = b''
        if f.FeatureParams is not None:
            if not isinstance(f.FeatureParams, ot.FeatureParamsStylisticSet):
                fail('%s feature params %s' % (tag, type(f.FeatureParams).__name__))
            params = struct.pack('>HH', f.FeatureParams.Version, f.FeatureParams.UINameID)
        off = 4 + 2 * len(idx) if params else 0
        return struct.pack('>HH', off, len(idx)) + b''.join(struct.pack('>H', i) for i in idx) + params

    scripts = table.ScriptList.ScriptRecord
    feats = table.FeatureList.FeatureRecord
    nlook = len(table.LookupList.Lookup)
    sl = 10
    fl = sl + 2 + 6 * len(scripts)
    ll = fl + 2 + 6 * len(feats)
    at = ll + 2 + 2 * nlook
    blobs, script_at = [], []
    seen = {}
    for sr in scripts:
        b = script_bytes(sr.Script)
        if b not in seen:      # one table for identical scripts: DFLT and hebr share one
            seen[b] = at
            blobs.append(b)
            at += len(b)
        script_at.append(seen[b])
    feature_at = []
    for fr in feats:
        b = feature_bytes(fr.Feature)
        feature_at.append(at)
        blobs.append(b)
        at += len(b)
    region_at = at
    tail_at = region_at + len(region)
    look_at = [region_at + (o - start) for o in lookups] + [tail_at + (o - fstart) for o in flookups]
    if len(look_at) != nlook:
        fail('%s: %d lookups placed, %d in the table' % (tag, len(look_at), nlook))
    for name, base, offs in (('script', sl, script_at), ('feature', fl, feature_at), ('lookup', ll, look_at)):
        if max(o - base for o in offs) > 0xFFFF:
            fail('%s: a %s offset overflows' % (tag, name))
    out = struct.pack('>5H', 1, 0, sl, fl, ll)
    out += struct.pack('>H', len(scripts)) + b''.join(
        sr.ScriptTag.encode('ascii') + struct.pack('>H', o - sl) for sr, o in zip(scripts, script_at))
    out += struct.pack('>H', len(feats)) + b''.join(
        fr.FeatureTag.encode('ascii') + struct.pack('>H', o - fl) for fr, o in zip(feats, feature_at))
    out += struct.pack('>H', nlook) + b''.join(struct.pack('>H', o - ll) for o in look_at)
    out += b''.join(blobs)
    if len(out) != region_at:
        fail('%s: the lists are %d bytes, placed for %d' % (tag, len(out), region_at))
    out += region + tail
    if out[region_at:region_at + len(region)] != old[start:]:
        fail('%s: the old lookups moved' % tag)
    STRANDED[tag] = sum(e - b for b, e in stranded)
    return out


# the bytes of BUILT's list tables left among its lookups, named by nothing
STRANDED = {}


# BUILT's glyph order once the Noto glyphs are appended, for compiling the
# appended lookups alone
BUILT_ORDER = [None]


def kept_glyphs(built, noto):
    """Noto's glyphs to copy, in Noto's order."""
    bcm, ncm = built.getBestCmap(), noto.getBestCmap()
    keep = {g for c, g in ncm.items() if c not in bcm}
    refs = set()
    for tag in ('GSUB', 'GPOS'):
        for lk in noto[tag].table.LookupList.Lookup:
            refs |= names_in(lk) & set(noto.getGlyphOrder())
    keep |= refs
    keep.discard('space')
    keep.discard('.notdef')
    glyf = noto['glyf']
    stack = sorted(keep)
    while stack:
        g = glyf[stack.pop()]
        if g.isComposite():
            for c in g.components:
                if c.glyphName not in keep:
                    keep.add(c.glyphName)
                    stack.append(c.glyphName)
    if 'space' in keep:
        fail("Noto's space is a component; it cannot be BUILT's space too")
    return [g for g in noto.getGlyphOrder() if g in keep]


def remapped(noto, tag, names):
    """Noto's table `tag` decompiled again from its bytes against a glyph
    order that names each of Noto's glyph IDs by its name in OUT, so every
    glyph it names, in any subtable, is OUT's glyph."""
    view = TTFont()
    view.setGlyphOrder(names)
    t = newTable(tag)
    t.decompile(noto.getTableData(tag), view)
    t.ensureDecompiled(recurse=True)
    return t


def merged_features(bfeats, nfeats):
    """BUILT's and Noto's feature records in one list sorted by tag
    ("The FeatureRecord array should be sorted alphabetically by feature
    tag", OpenType's common table formats), BUILT's before Noto's under one
    tag, each list's own order kept; and where each input index went."""
    tagged = [(r.FeatureTag, 0, i, r) for i, r in enumerate(bfeats)] + [(r.FeatureTag, 1, i, r) for i, r in enumerate(nfeats)]
    tagged.sort(key=lambda x: (x[0], x[1], x[2]))
    where = ({}, {})
    out = []
    for k, (tag, side, i, r) in enumerate(tagged):
        where[side][i] = k
        out.append(r)
    return out, where


def merge(built_path, noto_path, style, out_path, report_path):
    if style not in STYLES:
        fail('style %r' % style)
    before = snapshot(built_path)
    built = TTFont(built_path)
    noto = TTFont(noto_path)
    old_gsub = built.reader['GSUB']

    for f, want in ((built, 1000), (noto, 1000)):
        if f['head'].unitsPerEm != want:
            fail('unitsPerEm %d' % f['head'].unitsPerEm)
    axes = [(a.axisTag, a.minValue, a.defaultValue, a.maxValue) for a in noto['fvar'].axes]
    if axes != [('wdth', 62.5, 100.0, 100.0)]:
        fail('Noto axes %r; want wdth 62.5-100 alone' % axes)
    for tag in ('fvar', 'gvar', 'HVAR', 'avar', 'STAT', 'MVAR'):
        if tag in built:
            fail('BUILT already has %s' % tag)
    for f in (built, noto):
        for tag in ('GSUB', 'GPOS'):
            if getattr(f[tag].table, 'FeatureVariations', None) is not None:
                fail('%s has FeatureVariations' % tag)
    nrec = [r.toUnicode() for r in noto['name'].names if r.nameID == 0 and r.platformID == 3]
    trec = [r.toUnicode() for r in noto['name'].names if r.nameID == 7 and r.platformID == 3]
    if nrec != [NOTO_COPYRIGHT] or trec != [NOTO_TRADEMARK]:
        fail("Noto's nameID 0 / 7 are %r / %r" % (nrec, trec))

    border = built.getGlyphOrder()
    bnames = set(border)
    norder = noto.getGlyphOrder()
    kept = kept_glyphs(built, noto)
    new_name = {g: (g + SUFFIX if g in bnames else g) for g in kept}
    new_name['space'] = 'space'
    if 'space' not in bnames:
        fail('BUILT has no glyph named space')
    if len(set(new_name.values())) != len(new_name) or (set(new_name.values()) - {'space'}) & bnames:
        fail('a glyph name would repeat')
    view_names = [new_name.get(g, '.dropped.%d' % i) for i, g in enumerate(norder)]

    # ---- glyphs, metrics, variations
    glyf, hmtx = built['glyf'], built['hmtx']
    nglyf, nhmtx = noto['glyf'], noto['hmtx']
    order = border + [new_name[g] for g in kept]
    for g in kept:
        gl = copy.deepcopy(nglyf[g])
        if gl.isComposite():
            for c in gl.components:
                c.glyphName = new_name[c.glyphName]
        glyf.glyphs[new_name[g]] = gl
        hmtx.metrics[new_name[g]] = nhmtx[g]
    built.setGlyphOrder(order)
    glyf.setGlyphOrder(order)
    BUILT_ORDER[0] = order

    # ---- cmap: the code points BUILT does not map, in every subtable that holds them
    bcm, ncm = built.getBestCmap(), noto.getBestCmap()
    added = {c: new_name[g] for c, g in ncm.items() if c not in bcm}
    for t in built['cmap'].tables:
        for c, g in sorted(added.items()):
            if c > 0xFFFF and t.format == 4:
                continue
            t.cmap[c] = g

    # ---- GDEF
    gdef = built['GDEF'].table
    ngdef = remapped(noto, 'GDEF', view_names).table
    nsets = len(gdef.MarkGlyphSetsDef.Coverage) if gdef.MarkGlyphSetsDef else 0
    keptnames = set(new_name[g] for g in kept)
    for g, cls in ngdef.GlyphClassDef.classDefs.items():
        if g in keptnames:
            gdef.GlyphClassDef.classDefs[g] = cls
    for cov in ngdef.MarkGlyphSetsDef.Coverage:
        c = ot.Coverage()
        c.glyphs = [g for g in cov.glyphs if g in keptnames]
        gdef.MarkGlyphSetsDef.Coverage.append(c)
    gdef.MarkGlyphSetsDef.MarkSetCount = len(gdef.MarkGlyphSetsDef.Coverage)
    for field in ('AttachList', 'LigCaretList', 'MarkAttachClassDef'):
        if getattr(ngdef, field, None) is not None:
            fail('Noto GDEF has a %s' % field)
    gdef.Version = 0x00010003
    gdef.VarStore = ngdef.VarStore

    # ---- names: the axis, Noto's stylistic set, STAT's values
    name = built['name']

    def new_id(text):
        return name.addMultilingualName({'en': text}, built, minNameID=256)

    # ---- GSUB and GPOS
    nold_gsub = len(built['GSUB'].table.LookupList.Lookup)
    for tag in ('GSUB', 'GPOS'):
        table = built[tag].table
        ntable = remapped(noto, tag, view_names).table
        nl = len(table.LookupList.Lookup)
        lookups = ntable.LookupList.Lookup

        def shift(o):
            if isinstance(o, (ot.SubstLookupRecord, ot.PosLookupRecord)):
                o.LookupListIndex += nl
        for lk in lookups:
            walk(lk, shift)
            if lk.LookupFlag & 0x0010:
                lk.MarkFilteringSet += nsets
            stray = sorted(n for n in names_in(lk) if n.startswith('.dropped.'))
            if stray:
                fail('a Noto %s lookup names a glyph not copied: %s' % (tag, stray[:5]))
        table.LookupList.Lookup.extend(lookups)
        if tag == 'GSUB':
            swap = ot.Lookup()
            swap.LookupType = 1
            swap.LookupFlag = 0
            sub = ot.SingleSubst()
            sub.Format = 2
            sub.mapping = {'uni25CC': new_name['uni25CC']}
            swap.SubTable = [sub]
            swap.SubTableCount = 1
            table.LookupList.Lookup.append(swap)
            swap_index = len(table.LookupList.Lookup) - 1
        table.LookupList.LookupCount = len(table.LookupList.Lookup)

        # the features dev2 and deva name, and only those
        used = []
        for sr in ntable.ScriptList.ScriptRecord:
            if sr.ScriptTag in ('dev2', 'deva'):
                for ls in [sr.Script.DefaultLangSys] + [r.LangSys for r in sr.Script.LangSysRecord]:
                    if ls.ReqFeatureIndex != 0xFFFF:
                        fail('Noto %s %s has a required feature' % (tag, sr.ScriptTag))
                    used += [i for i in ls.FeatureIndex if i not in used]
        used.sort()
        nfeats = []
        for i in used:
            fr = ntable.FeatureList.FeatureRecord[i]
            fr.Feature.LookupListIndex = [j + nl for j in fr.Feature.LookupListIndex]
            if tag == 'GSUB' and fr.FeatureTag == 'nukt':
                fr.Feature.LookupListIndex.append(swap_index)
            params = fr.Feature.FeatureParams
            if params is not None:
                if not isinstance(params, ot.FeatureParamsStylisticSet):
                    fail('Noto %s feature %s has %s' % (tag, fr.FeatureTag, type(params).__name__))
                text = noto['name'].getDebugName(params.UINameID)
                params.UINameID = new_id(text)
            fr.Feature.LookupCount = len(fr.Feature.LookupListIndex)
            nfeats.append(fr)
        if tag == 'GSUB' and not any(fr.FeatureTag == 'nukt' for fr in nfeats):
            fail('Noto names no nukt for dev2 or deva')
        bfeats = table.FeatureList.FeatureRecord
        feats, where = merged_features(bfeats, nfeats)
        nwhere = {i: where[1][k] for k, i in enumerate(used)}
        table.FeatureList.FeatureRecord = feats
        table.FeatureList.FeatureCount = len(feats)
        moved = set()
        for sr in table.ScriptList.ScriptRecord:
            for ls in [sr.Script.DefaultLangSys] + [r.LangSys for r in sr.Script.LangSysRecord]:
                if ls is None or id(ls) in moved:
                    continue
                moved.add(id(ls))
                ls.FeatureIndex = [where[0][i] for i in ls.FeatureIndex]
                if ls.ReqFeatureIndex != 0xFFFF:
                    ls.ReqFeatureIndex = where[0][ls.ReqFeatureIndex]
        for sr in ntable.ScriptList.ScriptRecord:
            if sr.ScriptTag not in ('dev2', 'deva'):
                continue
            for ls in [sr.Script.DefaultLangSys] + [r.LangSys for r in sr.Script.LangSysRecord]:
                ls.FeatureIndex = [nwhere[i] for i in ls.FeatureIndex]
            if sr.ScriptTag in [r.ScriptTag for r in table.ScriptList.ScriptRecord]:
                fail('BUILT already has a %s script' % sr.ScriptTag)
            table.ScriptList.ScriptRecord.append(sr)
        table.ScriptList.ScriptRecord.sort(key=lambda r: r.ScriptTag)
        table.ScriptList.ScriptCount = len(table.ScriptList.ScriptRecord)

    # ---- variations
    fvar = noto['fvar']
    axis = fvar.axes[0]
    axis.flags = HIDDEN_AXIS
    axis.axisNameID = new_id(noto['name'].getDebugName(axis.axisNameID))
    inst = fvar.instances[0]
    if inst.coordinates != {'wdth': 100.0}:
        fail('Noto instance %r' % inst.coordinates)
    inst.subfamilyNameID = 2      # the default instance: the face's own subfamily
    inst.postscriptNameID = 6
    fvar.instances = [inst]
    built['fvar'] = fvar
    built['avar'] = noto['avar']

    gvar = newTable('gvar')
    gvar.version, gvar.reserved = 1, 0
    gvar.variations = {g: [] for g in border}
    for g in kept:
        gvar.variations[new_name[g]] = copy.deepcopy(noto['gvar'].variations.get(g, []))
    built['gvar'] = gvar

    nh = remapped(noto, 'HVAR', view_names).table
    if nh.LsbMap is not None or nh.RsbMap is not None:
        fail('Noto HVAR has side-bearing maps')
    hvar = newTable('HVAR')
    hvar.table = ot.HVAR()
    hvar.table.Version = 0x00010000
    hvar.table.VarStore = nh.VarStore
    # BUILT's glyphs vary by nothing: each is mapped to a row of the store
    # whose deltas are all 0, which OpenType's common formats call
    # functionally the same as NO_VARIATION_INDEX (0xFFFF/0xFFFF) and which
    # takes one byte an entry where 0xFFFFFFFF takes four.
    zero = NO_VARIATION
    for outer, vd in enumerate(nh.VarStore.VarData):
        rows = [i for i, row in enumerate(vd.Item) if not any(row)]
        if rows:
            zero = (outer << 16) | rows[0]
            break
    amap = nh.AdvWidthMap
    mapping = {g: zero for g in border}
    for g in kept:
        mapping[new_name[g]] = amap.mapping[new_name[g]]
    amap.mapping = mapping
    hvar.table.AdvWidthMap = amap
    hvar.table.LsbMap = hvar.table.RsbMap = None
    built['HVAR'] = hvar

    # STAT: each fvar axis and named instance has its axis record and value
    # (OpenType fvar), and the family's weight and slope beside them.
    weight, ital = STYLES[style]
    elide = 0x0002   # ELIDABLE_AXIS_VALUE_NAME
    nstat = noto['STAT'].table
    widths = []
    for av in nstat.AxisValueArray.AxisValue:
        if av.Format == 1 and nstat.DesignAxisRecord.Axis[av.AxisIndex].AxisTag == 'wdth':
            widths.append({'value': av.Value, 'name': noto['name'].getDebugName(av.ValueNameID), 'flags': av.Flags & elide})
    if [w['value'] for w in widths] != [62.5, 75.0, 87.5, 100.0]:
        fail("Noto's width values %r" % widths)
    buildStatTable(built, [
        {'tag': 'wght', 'name': 'Weight', 'ordering': 0,
         'values': [{'value': weight, 'name': 'Regular' if weight == 400 else 'Bold', 'flags': elide if weight == 400 else 0}]},
        {'tag': 'wdth', 'name': name.getDebugName(axis.axisNameID), 'ordering': 1, 'values': widths},
        {'tag': 'ital', 'name': 'Italic', 'ordering': 2,
         'values': [{'value': ital, 'name': 'Italic' if ital else 'Upright', 'flags': 0 if ital else elide}]},
    ], elidedFallbackName=2)

    # ---- the notices and the summaries the new glyphs change
    for r in name.names:
        if r.nameID == 0:
            r.string = r.toUnicode() + ' Devanagari glyphs from Noto Sans Devanagari 2.007: ' + NOTO_COPYRIGHT + '.'
        elif r.nameID == 7:
            r.string = r.toUnicode() + ' ' + NOTO_TRADEMARK
    os2 = built['OS/2']
    os2.ulUnicodeRange1 |= 1 << DEVANAGARI_RANGE_BIT
    os2.usMaxContext = maxCtxFont(built)
    # "the arithmetic average of the escapement (width) of all non-zero
    # width glyphs in the font" (OpenType OS/2, xAvgCharWidth)
    widths = [w for w, _ in hmtx.metrics.values() if w]
    os2.xAvgCharWidth = round(sum(widths) / len(widths))
    # "Set to 0 if the font is proportionally spaced" (OpenType post,
    # isFixedPitch): the Devanagari is; PANOSE, which classifies the Latin
    # design, keeps Monospaced.
    built['post'].isFixedPitch = 0

    gsub = DefaultTable('GSUB')
    gsub.data = spliced('GSUB', old_gsub, built['GSUB'].table, nold_gsub)
    built['GSUB'] = gsub
    save_pinned(built, out_path)
    NSETS[0] = nsets
    after = snapshot(out_path, border)
    changed = []
    for part in before:
        for k, v in before[part].items():
            if after[part].get(k) != v:
                changed.append('%s %s' % (part, k))
    if changed:
        os.remove(out_path)
        fail('%d objects of BUILT changed, nothing written: %s' % (len(changed), changed[:10]))

    n2o = {i: order.index(new_name[g]) for i, g in enumerate(norder) if g in new_name}
    report = {
        'style': style,
        'kept': len(kept),
        'first_appended_glyph': len(border),
        'code_points_added': len(added),
        'noto_to_out': n2o,
        'unchanged': {part: {'count': len(v), 'sha256': digest(json.dumps(sorted(v.items())).encode())} for part, v in before.items()},
        'dotted_circle_swap_lookup': swap_index,
        'gsub_list_bytes_left_among_the_lookups': STRANDED['GSUB'],
    }
    with open(report_path, 'w') as f:
        json.dump(report, f, indent=0, sort_keys=True)
    print('merged', style, len(kept), 'glyphs,', len(added), 'code points; unchanged:',
          ', '.join('%d %s' % (len(v), k) for k, v in before.items()) +
          '; GSUB list bytes left among the lookups: %d' % STRANDED['GSUB'])


if __name__ == '__main__':
    if len(sys.argv) != 6:
        fail('usage: merge.py BUILT.ttf NOTO.ttf STYLE OUT.ttf REPORT.json')
    merge(*sys.argv[1:])
