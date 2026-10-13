import json, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
errs = [r for r in rows if 'error' in r]
print("faces", len(rows), "errors", len(errs))
for r in errs: print(" ERR", r['path'], r.get('index'), r['error'][:100])
ok = [r for r in rows if 'error' not in r]
stats = dict(no_layout=0, has_DFLT_both=0)
dflt_faces=[]; nodflt=[]; diff12=[]; diff13=[]
for r in ok:
    for t in ('GSUB','GPOS'):
        d = r.get(t)
        if not d or 'error' in d: continue
        tags = d['tags']
        if 'dflt' in tags: dflt_faces.append((r['path'], r['index'], t, tags))
        if tags and 'DFLT' not in tags:
            latn = d.get('latn')
            lk = latn and latn['default'] and latn['default']['baud_lookups']
            dl = d.get('dflt'); dlk = dl and dl['default'] and dl['default']['baud_lookups']
            nodflt.append((r['path'], r['index'], t, tags, bool(latn), lk or [], d['cmap_scripts_without_table']))
            if latn and lk: diff12.append((r['path'], r['index'], r['name'], t, tags, latn['default']['features'], len(lk), d['cmap_scripts_without_table']))
            if dl and (dlk or []) != (lk or []): diff13.append((r['path'], r['index'], t, tags, dlk, lk))
print("faces with script 'dflt':", len(dflt_faces))
for x in dflt_faces: print(" ", x)
print("table instances with ScriptList but no DFLT:", len(nodflt))
print("of those with latn default naming baud-applied lookups (A1!=A2 candidates):", len(diff12))
for x in diff12: print(" ", json.dumps(x))
print("A3!=A1 candidates:", len(diff13))
for x in diff13: print(" ", x)
