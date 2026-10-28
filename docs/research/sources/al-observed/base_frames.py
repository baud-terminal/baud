#!/usr/bin/env python3
"""Note AL's cross-check: is every base ("before") frame that more than one
prototype rendered byte-identical across them? Each prototype built its own
base binary from c00dfd1 plus the harness; the O2 and O4 prototypes built into
one shared scratch bin/, whose flash-base was overwritten (its last write,
20:35:32 on 2026-10-09, carries O2's harness: gates-out.txt section 5), so a
base run could have run another prototype's binary. Byte-identical frames
across the prototypes rule that out for the frames compared.

    python3 -I base_frames.py FLASH_FIX_DIR   # the prototypes' working directory
"""
import hashlib, os, sys
from collections import Counter, defaultdict

root = sys.argv[1]
h = defaultdict(dict)
for d, pre, sub in (("proto-O1", "before-", "png"), ("proto-O2", "before-", ""),
                    ("proto-O3", "base-", "png"), ("proto-O4", "before-", "")):
    p = os.path.join(root, d, sub)
    for n in sorted(os.listdir(p)):
        if n.startswith(pre) and n.endswith(".png"):
            h[n[len(pre):]][d] = hashlib.sha256(open(os.path.join(p, n), "rb").read()).hexdigest()
multi = sorted(k for k, v in h.items() if len(v) > 1)
same = [k for k in multi if len(set(h[k].values())) == 1]
print(f"base frames rendered by 2+ prototypes: {len(multi)}; byte-identical across them: {len(same)}")
print(f"by how many prototypes rendered each: {dict(sorted(Counter(len(h[k]) for k in multi).items()))}")
for k in multi:
    if k not in same:
        print("DIFFERS", k, {d: s[:12] for d, s in h[k].items()})
