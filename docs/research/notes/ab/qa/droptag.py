# Write a copy of a face with one script tag's ScriptRecord removed from GSUB and GPOS
# (tag "none" = round-trip control with nothing removed).
import sys
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
from fontTools.ttLib import TTFont
src, idx, tag, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
f = TTFont(src, fontNumber=idx)
for t in ('GSUB', 'GPOS'):
    if t not in f: continue
    sl = f[t].table.ScriptList
    before = [r.ScriptTag for r in sl.ScriptRecord]
    sl.ScriptRecord = [r for r in sl.ScriptRecord if r.ScriptTag != tag]
    sl.ScriptCount = len(sl.ScriptRecord)
    print(t, before, "->", [r.ScriptTag for r in sl.ScriptRecord])
f.save(out)
