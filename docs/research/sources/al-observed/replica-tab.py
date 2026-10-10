import sys
rows=[]
for line in open(sys.argv[1]):
    f=line.rstrip('\n').split('\t')
    d=dict(effect=f[0],pt=f[1],scale=f[2],rate=f[3],ground=f[4],ink=f[5],motion=f[6],arm=f[7])
    for kv in f[8:]:
        k,v=kv.split('=');d[k]=float(v)
    rows.append(d)
rows.sort(key=lambda d:(d['effect'].split('/')[0],int(d['pt']),d['ground'],d['ink'],d['motion'],int(d['rate']),d['effect'],d['arm']))
last=None
for d in rows:
    key=(d['effect'].split('/')[0],d['pt'],d['scale'],d['rate'],d['ground'],d['ink'],d['motion'])
    if key!=last:
        print(f"\n{key[0]} {d['pt']}pt {d['scale']}x {d['rate']}Hz {d['ink']} on {d['ground']} {d['motion']}")
        print(f"  {'driver/arm':<18}{'area':>8}{'apart':>8}{'piece':>8}{'strict':>8}{'gen_ever':>9}{'lit':>7}{'gfl':>4}{'rfl':>4}{'ign':>4}{'vfx':>5}")
        last=key
    fail = 'FAIL' if d['area']>21824 else ''
    name=(d['effect'].split('/')[1] if '/' in d['effect'] else 'shipped')+' '+d['arm']
    print(f"  {name:<18}{d['area']:>8.0f}{d['apart']:>8.0f}{d['contiguous']:>8.0f}{d['strict']:>8.0f}{d['gen_ever']:>9.0f}{d['lit']:>7.0f}{d['gen_flashes']:>4.0f}{d['red_flashes']:>4.0f}{d['ignitions']:>4.0f}{d['vfx']:>5.0f} {fail}")
