import sys, os, re, json, glob, bisect, hashlib
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
from fontTools.ttLib import TTFont, TTCollection

REPO = "."
SD = open(f"{REPO}/baud-font/src/script_data.rs").read()
def lit(name):
    m = re.search(r'pub static %s: &\[u8; \d+\] = b"\\\n(.*?)";' % name, SD, re.S)
    return bytes(int(h, 16) for h in re.findall(r'\\x([0-9a-f]{2})', m.group(1)))
CODES = lit("CODES"); TAGS = lit("TAGS")
code2tags = {}
for i in range(len(CODES) // 4):
    c = CODES[4*i:4*i+4].decode()
    ts = [TAGS[8*i:8*i+4], TAGS[8*i+4:8*i+8]]
    code2tags[c] = [t.decode('latin1') for t in ts if t != b'\0\0\0\0']
# Scripts.txt -> long name -> short code
short = {}
for line in open(f"{REPO}/docs/research/sources/unicode-propertyvaluealiases.txt"):
    f = [x.strip() for x in line.split('#')[0].split(';')]
    if f[0] == 'sc' and len(f) >= 3: short[f[2]] = f[1]
ranges = []
for line in open(f"{REPO}/docs/research/sources/unicode-scripts.txt"):
    b = line.split('#')[0].strip()
    if not b: continue
    r, n = b.split(';'); r = r.strip()
    lo, hi = (r.split('..') + [r])[:2] if '..' in r else (r, r)
    ranges.append((int(lo, 16), int(hi, 16), short[n.strip()]))
ranges.sort(); starts = [r[0] for r in ranges]
def script_of(cp):
    i = bisect.bisect_right(starts, cp) - 1
    if i >= 0 and ranges[i][0] <= cp <= ranges[i][1]: return ranges[i][2]
    return 'Zzzz'

BAUD_FEATS = {'ccmp', 'calt', 'mark', 'mkmk'}  # Features::GRID + mark/mkmk (layout.rs Features::wants)

def langsys_info(table, srec):
    s = srec.Script
    out = {}
    def feats(ls):
        if ls is None: return None
        idx = list(ls.FeatureIndex)
        fl = table.FeatureList.FeatureRecord
        names = []; baud_lk = set()
        for i in idx:
            if i >= len(fl): continue
            fr = fl[i]; names.append(fr.FeatureTag)
            if fr.FeatureTag in BAUD_FEATS: baud_lk |= set(fr.Feature.LookupListIndex)
        if ls.ReqFeatureIndex != 0xFFFF and ls.ReqFeatureIndex < len(fl):
            names.append('REQ:' + fl[ls.ReqFeatureIndex].FeatureTag)
        return {'features': sorted(set(names)), 'baud_lookups': sorted(baud_lk)}
    out['default'] = feats(s.DefaultLangSys)
    out['langs'] = [r.LangSysTag for r in s.LangSysRecord]
    return out

def face_info(f):
    info = {}
    cmap = f.getBestCmap() or {}
    scripts_in_cmap = {}
    for cp in cmap:
        sc = script_of(cp)
        scripts_in_cmap[sc] = scripts_in_cmap.get(sc, 0) + 1
    info['cmap_scripts'] = scripts_in_cmap
    try:
        info['name'] = f['name'].getDebugName(4)
    except Exception: info['name'] = None
    for tname in ('GSUB', 'GPOS'):
        if tname not in f: info[tname] = None; continue
        try:
            t = f[tname].table
        except Exception as e:
            info[tname] = {'error': repr(e)}; continue
        if t.ScriptList is None: info[tname] = {'tags': []}; continue
        recs = {r.ScriptTag: r for r in t.ScriptList.ScriptRecord}
        d = {'tags': [r.ScriptTag for r in t.ScriptList.ScriptRecord]}
        for k in ('DFLT', 'dflt', 'latn'):
            if k in recs: d[k] = langsys_info(t, recs[k])
        # specific scripts in cmap with no table
        missing = []
        for sc, n in sorted(scripts_in_cmap.items()):
            if sc in ('Zyyy', 'Zinh', 'Zzzz'): continue
            tags = code2tags.get(sc, [])
            if not any(tg in recs for tg in tags): missing.append((sc, n))
        d['cmap_scripts_without_table'] = missing
        info[tname] = d
    return info

def faces(path):
    out = []
    try:
        if path.lower().endswith(('.ttc', '.otc')):
            c = TTCollection(path, lazy=True)
            for i, f in enumerate(c.fonts): out.append((i, f))
        else:
            out.append((0, TTFont(path, lazy=True)))
    except Exception as e:
        out.append((-1, repr(e)))
    return out

paths = sys.argv[1:]
for p in paths:
    for i, f in faces(p):
        if i < 0:
            print(json.dumps({'path': p, 'index': i, 'error': f})); continue
        try:
            inf = face_info(f)
        except Exception as e:
            inf = {'error': repr(e)}
        inf['path'] = p; inf['index'] = i
        print(json.dumps(inf), flush=True)
