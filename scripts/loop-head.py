#!/usr/bin/env python3
"""Compare one function's machine code between two binaries, addresses aside.

    scripts/loop-head.py [--fn NAME] [--no-pageoff] A [B]
    scripts/loop-head.py --self-test

**Why it exists.** `ingest2` once inlined its timed loop into `main`, so the
loop shared a register allocation with everything `main` built before it. A
setup-only change -- the cluster store's `vec!` sized 1024 instead of 64, or
that `vec!` moved into a function of its own, with the core untouched -- put
two stack reloads at the head of the dispatch loop and moved instructions per
rep +0.69% on `cat`. Nothing in the timing said which; this does. Run it on both
arms before attributing a small `ingest2` delta: if the timed function differs,
the delta may be the allocator's and not the change's (docs/record.md,
"ingest2's setup moved its timed loop").

**What it prints**, for each binary: the function's instruction count, its
loads from the stack frame (`ldr*`/`ldp`/`ldur*` off `sp` or `x29`), and a
digest of its normalised body. Given two binaries, it says IDENTICAL or prints
a unified diff of the normalised bodies, and exits 0 or 1. The verdict names
the form it held under: page offsets normalised, or kept (`--no-pageoff`). A
default-form IDENTICAL is compared again with offsets kept and says whether it
holds there too. It exits 2 when the function is not in a binary, or when the
name, demangled, names more than one function there, never 0.

**Normalised** means: no instruction addresses; a branch inside the function
becomes its offset from the function's start; a call keeps its target's name
and loses its address (a stub, which objdump names `<_sym+0x...>`, becomes
`<stub>`); an `adr`/`adrp` target becomes `PAGE`; every immediate offset from
a register that some `adr`/`adrp` in the function writes becomes `#PAGEOFF`,
because those offsets move with the data layout; a pc-relative literal load
becomes `LIT`. Registers, other immediates and stack offsets are kept, so a
different register allocation is a difference.

The page-offset rewrite applies to a register function-wide, live or not (13
registers in `timed`, one of them holding a stack address), so an offset-only
change through one of them reads IDENTICAL. `--no-pageoff` keeps every
offset. Every IDENTICAL arm in the record section also compares IDENTICAL
with the rewrite disabled but one: the G cut's outlined-`vec!` arm, where
`ingest2::run` loads one 16-byte constant from 0x80 lower in the same page
(`[x26, #0x820]` for `#0x8a0`, an `adrp` base) at three sites.

**Default function: `ingest2::timed`**, the harness's out-of-line timed loop.
`--fn ingest2::main` reads a harness from before it was outlined, where the
whole of `main` differs whenever the setup does, so it cannot isolate the loop.

**Controls.** `--self-test` runs the normaliser on synthetic bodies: the same
body at two addresses with different call and page targets must compare equal,
and one changed register, one added stack reload, or an immediate off a
register no `adr` writes must not; the two page offsets that compare equal
must differ under `--no-pageoff`; and a name that two functions share must
exit 2. On real binaries (docs/record.md, the
section named above): two byte-identical builds report IDENTICAL, and so does
a setup-only change (the cluster `vec!` outlined, or `NCLUSTER` raised); a
change to `Term`'s layout (32,776 unread bytes added to `ClusterStore`)
reports DIFFER: the function's frame grows from 0x340 to 0x390 bytes and its
registers are allocated anew; no instruction in that diff differs only in an
offset off a non-stack register.
"""

import contextlib
import difflib
import hashlib
import io
import re
import subprocess
import sys


def fail(msg):
    print(f"loop-head: {msg}", file=sys.stderr)
    sys.exit(2)


def disasm(path):
    p = subprocess.run(
        ["objdump", "-d", "--no-show-raw-insn", "-C", path],
        capture_output=True,
        text=True,
    )
    if p.returncode != 0:
        fail(f"objdump failed on {path}: {p.stderr.strip()}")
    return p.stdout.splitlines()


def functions(lines):
    """({name: [(addr, text)]}, {name: count}) from objdump's `<name>:` headers.

    Demangling can give several functions one name (`finish_grow`); their
    bodies would merge under one key, so each such name is counted."""
    fs, dup, cur = {}, {}, None
    for line in lines:
        m = re.match(r"^([0-9a-f]+) <(.+)>:$", line.strip())
        if m:
            cur = m.group(2)
            if cur in fs:
                dup[cur] = dup.get(cur, 1) + 1
            fs.setdefault(cur, [])
            continue
        m = re.match(r"^\s*([0-9a-f]+):\s+(.*)$", line)
        if cur is not None and m:
            fs[cur].append((int(m.group(1), 16), m.group(2).strip()))
    return fs, dup


def find(fs, dup, name, path):
    if name in dup:
        fail(f"{name!r} names {dup[name]} functions in {path}")
    if name in fs:
        return name
    near = [n for n in fs if n.endswith("::" + name.split("::")[-1])]
    fail(f"no function named {name!r} in {path}; ending alike: {near[:5]}")


def normalise(body, name, pageoff=True):
    if not body:
        return []
    base = body[0][0]
    # Every register an `adr`/`adrp` writes anywhere in the function: their
    # offsets are layout, and the allocator hoists a page base far from its uses.
    paged = set()
    for _, text in body:
        m = re.match(r"^adrp?\s+([wx]\d+),", text)
        if m:
            paged.add("x" + m.group(1)[1:])
    if not pageoff:
        paged = set()
    out = []
    for addr, text in body:
        t = re.sub(r"\s+;.*$", "", text).strip()
        parts = t.split(None, 1)
        mn, ops = parts[0], (parts[1] if len(parts) > 1 else "")

        def local(m):
            return f"<+{int(m.group(1), 16) - base:#x}>"

        ops = re.sub(r"0x([0-9a-f]+) <" + re.escape(name) + r"(?:\+0x[0-9a-f]+)?>", local, ops)
        ops = re.sub(r"0x[0-9a-f]+ <_[^>]*\+0x[0-9a-f]+>", "<stub>", ops)
        ops = re.sub(r"0x[0-9a-f]+ (<.*>)$", r"\1", ops)
        dst = ops.split(",")[0].strip()
        if mn in ("adrp", "adr"):
            ops = f"{dst}, PAGE"
            out.append(f"{mn}\t{ops}")
            continue
        if mn.startswith("ldr") and re.match(r"^[wxqdsb]\d+, 0x[0-9a-f]+", ops):
            ops = re.sub(r"0x[0-9a-f]+.*$", "LIT", ops)
        for r in paged:
            ops = re.sub(r"(\[" + r + r", |, " + r + r", )#-?0x[0-9a-f]+", r"\1#PAGEOFF", ops)
        out.append(f"{mn}\t{ops}")
    return out


def reloads(norm):
    return sum(
        1 for l in norm if re.match(r"^ld(r|p|ur)\w*\t", l) and re.search(r"\[(sp|x29)(, #-?0x[0-9a-f]+)?\]", l)
    )


def describe(path, name, norm):
    digest = hashlib.sha256("\n".join(norm).encode()).hexdigest()[:16]
    print(f"{path}: {name}: {len(norm)} instructions, {reloads(norm)} stack loads, body {digest}")


def self_test():
    name = "ingest2::timed"
    a = [
        (0x1000, "stp\tx29, x30, [sp, #-0x10]!"),
        (0x1004, "adrp\tx8, 0x100067000 <GCC_except_table431+0x4>"),
        (0x1008, "add\tx8, x8, #0xa00"),
        (0x100C, "bl\t0x100065b80 <_writev+0x100065b80>"),
        (0x1010, "b.hs\t0x1000 <ingest2::timed>"),
        (0x1014, "bl\t0x100026d40 <<u64 as core::fmt::Display>::fmt>"),
        (0x1018, "ldr\tx26, [x8, #0x2e8]"),
    ]
    b = [
        (0x2000, "stp\tx29, x30, [sp, #-0x10]!"),
        (0x2004, "adrp\tx8, 0x100099000 <GCC_except_table12+0x8>"),
        (0x2008, "add\tx8, x8, #0x140"),
        (0x200C, "bl\t0x100077000 <_writev+0x100077000>"),
        (0x2010, "b.hs\t0x2000 <ingest2::timed>"),
        (0x2014, "bl\t0x100031000 <<u64 as core::fmt::Display>::fmt>"),
        (0x2018, "ldr\tx26, [x8, #0x2e8]"),
    ]
    na, nb = normalise(a, name), normalise(b, name)
    assert na == nb, list(difflib.unified_diff(na, nb, lineterm=""))
    c = list(b)
    c[6] = (0x2018, "ldr\tx27, [x8, #0x2e8]")
    assert normalise(c, name) != na, "a changed register must differ"
    d = b[:4] + [(0x200C, "ldr\tx26, [sp, #0x2e8]")] + b[4:]
    nd = normalise(d, name)
    assert nd != na and reloads(nd) == reloads(na) + 1, "an added reload must differ and count"
    e = list(b)
    e[2] = (0x2008, "add\tx8, x9, #0x140")
    assert normalise(e, name) != na, "an add off another register keeps its immediate"
    assert normalise(a, name, False) != normalise(b, name, False), "--no-pageoff keeps page offsets"
    lines = [
        "0000000100001000 <alloc::raw_vec::finish_grow>:",
        "100001000: ret",
        "0000000100002000 <alloc::raw_vec::finish_grow>:",
        "100002000: ret",
    ]
    fs, dup = functions(lines)
    try:
        with contextlib.redirect_stderr(io.StringIO()):
            find(fs, dup, "alloc::raw_vec::finish_grow", "synthetic")
        raise AssertionError("a name two functions share must exit 2")
    except SystemExit as x:
        assert x.code == 2, "a name two functions share must exit 2"
    print("self-test: 6 of 6 hold")


def main(argv):
    if argv == ["--self-test"]:
        self_test()
        return 0
    name, pageoff = "ingest2::timed", True
    while argv[:1] in (["--fn"], ["--no-pageoff"]):
        if argv[0] == "--no-pageoff":
            pageoff, argv = False, argv[1:]
        elif len(argv) > 1:
            name, argv = argv[1], argv[2:]
        else:
            fail(__doc__)
    if len(argv) not in (1, 2):
        fail(__doc__)
    form = "page offsets normalised" if pageoff else "page offsets kept"
    norms, kept = [], []
    for path in argv:
        fs, dup = functions(disasm(path))
        n = find(fs, dup, name, path)
        norm = normalise(fs[n], n, pageoff)
        describe(path, n, norm)
        norms.append(norm)
        kept.append(normalise(fs[n], n, False))
    if len(norms) == 1:
        return 0
    if norms[0] == norms[1]:
        if pageoff:
            also = "IDENTICAL" if kept[0] == kept[1] else "DIFFER"
            print(f"IDENTICAL ({form}; with page offsets kept: {also})")
        else:
            print(f"IDENTICAL ({form})")
        return 0
    print(f"DIFFER ({form})")
    for l in difflib.unified_diff(norms[0], norms[1], argv[0], argv[1], lineterm="", n=2):
        print(l)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
