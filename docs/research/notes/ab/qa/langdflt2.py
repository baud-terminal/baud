import sys, struct
# Raw sfnt walk: every ScriptRecord tag and LangSysRecord tag in GSUB/GPOS of every face.
def faces(d):
    if d[:4] == b'ttcf':
        n = struct.unpack('>I', d[8:12])[0]
        return [struct.unpack('>I', d[12+4*i:16+4*i])[0] for i in range(n)]
    return [0]
import collections; C = collections.Counter()
for p in sys.argv[1:]:
    d = open(p, 'rb').read()
    if d[:4] not in (b'ttcf', b'\0\1\0\0', b'OTTO', b'true'): continue
    for fi, base in enumerate(faces(d)):
        n = struct.unpack('>H', d[base+4:base+6])[0]
        for i in range(n):
            r = base + 12 + 16*i; tag = d[r:r+4]
            if tag not in (b'GSUB', b'GPOS'): continue
            off = struct.unpack('>I', d[r+8:r+12])[0]
            sl = off + struct.unpack('>H', d[off+4:off+6])[0]
            if sl == off: continue
            cnt = struct.unpack('>H', d[sl:sl+2])[0]
            for k in range(cnt):
                st = d[sl+2+6*k:sl+6+6*k]; so = sl + struct.unpack('>H', d[sl+6+6*k:sl+8+6*k])[0]
                lc = struct.unpack('>H', d[so+2:so+4])[0]
                langs = [d[so+4+6*j:so+8+6*j] for j in range(lc)]
                C["script:" + st.decode("latin1")] += 1
                for L in langs: C["lang:" + L.decode("latin1")] += 1
                if st.lower() == b'dflt' and st != b'DFLT': print("SCRIPT-dflt", p, fi, tag.decode())
                if b'dflt' in langs or b'DFLT' in langs: print("LANG", (b'dflt' in langs and 'dflt' or 'DFLT'), st.decode('latin1'), p, fi, tag.decode())
print("COUNTS", C["script:DFLT"], C["script:latn"], C["lang:ROM "], C["lang:TRK "], sum(v for k, v in C.items() if k.startswith("lang:")))
