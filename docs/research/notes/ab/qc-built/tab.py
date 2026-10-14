import sys, re, statistics, collections
d = collections.defaultdict(list); sinks = collections.defaultdict(set)
for l in open(sys.argv[1]):
    if l.startswith("#"): continue
    f = l.split()
    r, arm, mode, face = int(f[0]), f[1], f[2], f[3]
    v = float(re.search(r"ns_per_cluster=([0-9.]+)", l).group(1))
    s = re.search(r"sink=(\d+)", l).group(1)
    sinks[(arm, mode, face)].add(s)
    if r == 1: continue
    d[(mode, face, arm)].append(v)
for mode, face in [("norm","Courier_New"),("norm","Regular"),("bench","Courier_New"),("bench","Regular")]:
    row = [f"{mode} {face}"]
    for arm in ("c1","c1b","c2","c2b"):
        v = d[(mode, face, arm)]
        row.append(f"{statistics.median(v):.2f} [{min(v):.2f}-{max(v):.2f}]")
    print(" | ".join(row))
for k, v in sorted(sinks.items()): print(k, v)
