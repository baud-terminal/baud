#!/usr/bin/env python3
"""Which crates' feature arms a change can break, for check-features.sh --since.

    python3 -I scripts/check-features-since.py <rev>
    python3 -I scripts/check-features-since.py --explain

With <rev>, the changed paths are those that differ between <rev> and the
working tree, committed or not, plus untracked files git does not ignore:
cargo checks the working tree, so an edit not yet committed is a change. It
prints `all<TAB><reason>` when every crate must be checked, and otherwise one
`<crate directory><TAB><reason>` line per member a change can reach. A <rev>
git cannot read as a commit prints a FAIL line and exits 1; anything else that
goes wrong prints `all`, because a scope it cannot compute is no scope.

A crate is reached when a changed path is in its directory, is a file it
reads at compile time from outside that directory, or is in a crate it
depends on, transitively and of every kind, since the arms build
`--all-targets`. --explain prints what the scan below finds for each member.

**The compile-time reads are scanned from the sources, not listed.** A crate
reads outside its own directory, so it does not depend only on its
dependencies: baud-bench compiles baud-app's `trail.rs` through `#[path]`
without depending on baud-app, and baud-domain, baud-render and baud-app
include files from the repository's tests/, assets/ and docs/. So every Rust
file of every member is read with comments and literals told apart, and:

- an `include!`, `include_bytes!` or `include_str!` names its file, relative
  to the file it is written in or, through `env!("CARGO_MANIFEST_DIR")`, to
  the crate; one through `env!("OUT_DIR")` reads what the build script wrote.
  One inside a `macro_rules!` body is resolved from every file that invokes
  the macro, and a piece given by a metavariable or by another macro is
  taken from the invocations or the macro's own arms, when every literal
  there stays below the known part of the path;
- a `#[path]` module, an `include!`d source and a target whose root is
  outside the crate are sources of the crate: the file, the module directory
  beside it, and their own reads, scanned the same way;
- any other macro or attribute given a literal with a `..` component, the
  form a file-reading procedural macro takes, names that path;
- the build script, and the modules it declares, name every literal with a
  `..` component, against the crate and the script's directory;
- a symbolic link in the crate's directory names what it points at.

What the scan cannot resolve fails closed: an include it cannot spell, and a
build script that climbs with `.parent()` or `.ancestors()`, mark the crate
as reading anything outside the members, so any such change reaches it.
Reads at run time, a test opening docs/ with `std::fs`, cannot break a
`cargo check` and are not counted.

**Some files reach every crate**: the lock file, the workspace manifest,
cargo's configuration, the toolchain's pins and these two scripts.
"""

import json
import os
import re
import subprocess
import sys

EVERYTHING_FILES = {
    "Cargo.lock",
    "Cargo.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".envrc",
    "devenv.nix",
    "devenv.lock",
    "devenv.yaml",
    "flake.nix",
    "flake.lock",
    "scripts/check-features.sh",
    "scripts/check-features-since.py",
}
EVERYTHING_DIRS = (".cargo/", "nix/")

INCLUDES = {"include", "include_bytes", "include_str"}
# Macros whose arguments are evaluated at run time or are text: a literal
# given to one names no file the compiler reads. A nested include is found
# on its own.
RUNTIME_MACROS = {
    "assert", "assert_eq", "assert_ne", "debug_assert", "debug_assert_eq",
    "debug_assert_ne", "dbg", "eprint", "eprintln", "format", "format_args",
    "panic", "print", "println", "todo", "unimplemented", "unreachable",
    "vec", "write", "writeln", "matches", "concat", "stringify", "env",
    "option_env", "cfg", "compile_error", "macro_rules",
}

TOKEN = re.compile(r'//|/\*|"|\'|(?<![A-Za-z0-9_])[bc]?r#*"')
RAW = re.compile(r'[bc]?r(#*)"')
LIT = re.compile(r"\x01(\d+)\x02")
MACRO = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*!\s*([(\[{])")
RULES = re.compile(r"\bmacro_rules\s*!\s*([A-Za-z_][A-Za-z0-9_]*)\s*([(\[{])")
ATTR = re.compile(r"#\s*!?\s*\[")
CLIMB = re.compile(r"\.\s*(parent|ancestors)\s*\(")
MOD = re.compile(r"(?:^|[;{}\s])mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
ONLY_LITERALS = re.compile(r"^[\s,()\[\]{}]*(?:\x01\d+\x02[\s,()\[\]{}]*)*$")


def lex(text):
    """The code with comments removed and each string literal replaced by
    \\x01<n>\\x02, and the literals' values in order."""
    out, lits, i, n = [], [], 0, len(text)
    while True:
        m = TOKEN.search(text, i)
        if not m:
            out.append(text[i:])
            break
        out.append(text[i : m.start()])
        i, tok = m.start(), m.group()
        if tok == "//":
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif tok == "/*":
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            out.append(" ")
        elif tok == "'":
            # A char literal, which may hold a quote, or a lifetime.
            if text.startswith("\\", i + 1):
                j = text.find("'", i + 3)
                i = n if j < 0 else j + 1
                out.append("' '")
            elif i + 2 < n and text[i + 2] == "'":
                i += 3
                out.append("' '")
            else:
                i += 1
                out.append("'")
        elif tok == '"':
            j, value = i + 1, []
            while j < n and text[j] != '"':
                if text[j] == "\\" and j + 1 < n:
                    value.append(text[j + 1] if text[j + 1] in '\\"' else text[j : j + 2])
                    j += 2
                else:
                    value.append(text[j])
                    j += 1
            out.append(f"\x01{len(lits)}\x02")
            lits.append("".join(value))
            i = j + 1
        else:
            hashes = RAW.match(text, i).group(1)
            start = i + len(tok)
            j = text.find('"' + hashes, start)
            j = n if j < 0 else j
            out.append(f"\x01{len(lits)}\x02")
            lits.append(text[start:j])
            i = j + 1 + len(hashes)
    return "".join(out), lits


def closing(code, open_at):
    """The index of the bracket closing the one at open_at."""
    depth = 0
    for j in range(open_at, len(code)):
        if code[j] in "([{":
            depth += 1
        elif code[j] in ")]}":
            depth -= 1
            if depth == 0:
                return j
    return len(code)


def split_top(args):
    """`args` split at its top-level commas."""
    parts, depth, start = [], 0, 0
    for j, c in enumerate(args):
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(args[start:j])
            start = j + 1
    parts.append(args[start:])
    return [p.strip() for p in parts if p.strip()]


def climbs(lit):
    return ".." in lit.replace("\\", "/").split("/")


def under(path, d):
    return d == "" or path == d or path.startswith(d + "/")


def resolve(base, lit):
    """A repository path, "" for its root, or None when it leaves it."""
    p = os.path.normpath(os.path.join(base, lit))
    if os.path.isabs(p) or p == ".." or p.startswith("../"):
        return None
    return "" if p == "." else p


def stem_of(path):
    stem = path[:-3] if path.endswith(".rs") else path
    return os.path.dirname(stem) if os.path.basename(stem) == "mod" else stem


def walk(d):
    """Every file under d, symbolic links to directories followed once."""
    seen = set()
    for top, dirs, files in os.walk(d or ".", followlinks=True):
        real = os.path.realpath(top)
        if real in seen:
            dirs[:] = []
            continue
        seen.add(real)
        dirs[:] = sorted(x for x in dirs if x not in ("target", ".git"))
        for f in sorted(files) + [x for x in dirs if os.path.islink(os.path.join(top, x))]:
            yield os.path.normpath(os.path.join(top, f))


class Member:
    def __init__(self, d, name):
        self.dir, self.name = d, name
        self.deps = set()  # member directories
        self.reads = {}  # repository path prefix -> the file that names it
        self.anything = []  # why the scan could not resolve a read
        self.files = {}  # path -> (code, literals)
        self.rules = {}  # macro name -> [(path, start, end, exported)]
        self.calls = {}  # macro name -> [(path, position, arguments)]
        self.queue = []  # (path, build)
        self.done = set()
        self.later = []  # includes resolved once every file is read

    def read(self, target, why):
        if target is None or under(target, self.dir) and target != "":
            return
        if target == "":
            self.anything.append(f"{why} names the repository root")
        else:
            self.reads.setdefault(target, why)

    def source(self, target, why):
        """A Rust file compiled into this crate, wherever it is."""
        if target is None:
            return
        if target == "":
            self.anything.append(f"{why} names the repository root")
            return
        if not under(target, self.dir):
            self.reads.setdefault(stem_of(target), why)
            stem = stem_of(target)
            if os.path.isdir(stem):
                self.queue.extend((f, False) for f in walk(stem) if f.endswith(".rs"))
        self.queue.append((target, False))

    def lexed(self, path):
        if path not in self.files:
            with open(path, encoding="utf-8", errors="replace") as f:
                self.files[path] = lex(f.read())
        return self.files[path]

    def scan(self, path, build):
        if (path, build) in self.done or not os.path.isfile(path):
            return
        self.done.add((path, build))
        code, lits = self.lexed(path)
        here = os.path.dirname(path)
        for r in RULES.finditer(code):
            end = closing(code, r.end() - 1)
            exported = re.search(r"#\s*\[\s*macro_export\b[^\]]*\]\s*$", code[: r.start()]) is not None
            self.rules.setdefault(r.group(1), []).append((path, r.start(), end, exported))
        for mac in MACRO.finditer(code):
            name, open_at = mac.group(1), mac.end() - 1
            args = code[open_at + 1 : closing(code, open_at)]
            self.calls.setdefault(name, []).append((path, mac.start(), args))
            if name in INCLUDES:
                self.later.append((path, mac.start(), name, args))
            elif name not in RUNTIME_MACROS:
                for k in map(int, LIT.findall(args)):
                    if climbs(lits[k]):
                        for base in (here, self.dir):
                            self.read(resolve(base, lits[k]), f"{path} ({name}!)")
        for att in ATTR.finditer(code):
            open_at = att.end() - 1
            body = code[open_at + 1 : closing(code, open_at)]
            is_path = re.match(r"\s*(?:cfg_attr\s*\(.*,\s*)?path\s*=", body, re.S)
            for k in map(int, LIT.findall(body)):
                if is_path:
                    stem = os.path.splitext(os.path.basename(path))[0]
                    for base in (here, os.path.join(here, stem)):
                        target = resolve(base, lits[k])
                        if target is not None and os.path.isfile(target):
                            self.source(target, f"{path} (#[path])")
                        elif target is not None and not under(target, self.dir):
                            self.read(target, f"{path} (#[path])")
                elif climbs(lits[k]):
                    for base in (here, self.dir):
                        self.read(resolve(base, lits[k]), f"{path} (attribute)")
        if build:
            if CLIMB.search(code):
                self.anything.append(f"{path} climbs with .parent() or .ancestors()")
            for lit in lits:
                if climbs(lit):
                    for base in (self.dir, here):
                        self.read(resolve(base, lit), f"{path} (build script)")
            for mod in MOD.findall(code):
                for f in (os.path.join(here, mod + ".rs"), os.path.join(here, mod, "mod.rs")):
                    self.queue.append((os.path.normpath(f), True))

    def enclosing(self, path, pos):
        """The macro_rules! definitions whose bodies hold `pos` in `path`."""
        return [
            (name, exported)
            for name, defs in self.rules.items()
            for p, start, end, exported in defs
            if p == path and start < pos < end
        ]

    def callers(self, name, depth=0):
        """The directories of the files a macro is expanded from."""
        dirs = set()
        for path, pos, _ in self.calls.get(name, []):
            outer = self.enclosing(path, pos)
            if outer and depth < 8:
                for o, _ in outer:
                    dirs |= self.callers(o, depth + 1)
            else:
                dirs.add(os.path.dirname(path))
        return dirs

    def pieces_stay(self, piece, path, pos):
        """Whether what a metavariable or macro call spells has no `..`."""
        _, lits = self.files[path]
        if piece.startswith("$"):
            for name, exported in self.enclosing(path, pos):
                if exported:
                    return False
                for p, _, args in self.calls.get(name, []):
                    plits = self.files[p][1]
                    if not ONLY_LITERALS.match(args) or any(climbs(plits[int(k)]) for k in LIT.findall(args)):
                        return False
            return bool(self.enclosing(path, pos))
        call = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)\s*!\s*[(\[{]\s*[)\]}]", piece)
        if not call or call.group(1) not in self.rules:
            return False
        for p, start, end, _ in self.rules[call.group(1)]:
            code, plits = self.files[p]
            for arm in re.finditer(r"=>\s*([(\[{])", code[start:end]):
                open_at = start + arm.end() - 1
                body = code[open_at + 1 : closing(code, open_at)]
                if not ONLY_LITERALS.match(body) or any(climbs(plits[int(k)]) for k in LIT.findall(body)):
                    return False
        return True

    def include(self, path, pos, name, args):
        code, lits = self.files[path]
        why = f"{path} ({name}!)"
        parts = split_top(args)
        while len(parts) == 1 and re.match(r"concat\s*!\s*[(\[{]", parts[0]):
            inner = parts[0][parts[0].index("!") + 1 :].strip()
            parts = split_top(inner[1:-1])
        flat = []
        for part in parts:
            if re.match(r"concat\s*!\s*[(\[{]", part):
                inner = part[part.index("!") + 1 :].strip()
                flat.extend(split_top(inner[1:-1]))
            else:
                flat.append(part)
        known, unknown, tail_climbs, env_base = "", [], False, False
        for i, part in enumerate(flat):
            lit = re.fullmatch(r"\x01(\d+)\x02", part)
            env = re.fullmatch(r"env\s*!\s*[(\[{]\s*\x01(\d+)\x02\s*,?\s*[)\]}]", part)
            if env:
                var = lits[int(env.group(1))]
                if var == "OUT_DIR" and i == 0:
                    return
                if var != "CARGO_MANIFEST_DIR" or i != 0:
                    self.anything.append(f"{why} spells its path with env!({var})")
                    return
                env_base = True
            elif lit and not unknown:
                known += lits[int(lit.group(1))]
            elif lit:
                tail_climbs |= climbs(lits[int(lit.group(1))])
            elif part.startswith("$") or re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*\s*!\s*[(\[{].*", part, re.S):
                unknown.append(part)
            else:
                self.anything.append(f"{why} spells its path with {' '.join(part.split())[:40]}")
                return
        if unknown and (tail_climbs or not all(self.pieces_stay(u, path, pos) for u in unknown)):
            self.anything.append(f"{why} spells its path with {' '.join(unknown)}")
            return
        if env_base:
            bases = {self.dir}
            known = known.lstrip("/")
        else:
            outer = self.enclosing(path, pos)
            bases = {os.path.dirname(path)}
            for o, _ in outer:
                bases |= self.callers(o)
        for base in sorted(bases):
            target = resolve(base, known)
            if name == "include" and not unknown:
                self.source(target, why)
            else:
                self.read(target, why)

    def run(self):
        # Until no include brings in a file not yet read, since a file read
        # late can hold an invocation an earlier include is resolved from.
        while True:
            while self.queue:
                self.scan(*self.queue.pop())
            before = len(self.done)
            for item in self.later:
                self.include(*item)
            if not self.queue and len(self.done) == before:
                break
        for f in walk(self.dir):
            if os.path.islink(f):
                self.read(resolve(os.path.dirname(f), os.readlink(f)), f"{f} (symbolic link)")


def load():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            capture_output=True,
            check=True,
        ).stdout
    )
    root = os.path.realpath(meta["workspace_root"])
    if root != os.path.realpath("."):
        raise RuntimeError(f"cargo's workspace root {root} is not the checkout")

    def rel(p):
        return os.path.relpath(os.path.realpath(p), root)

    members = {}
    for p in meta["packages"]:
        d = rel(os.path.dirname(p["manifest_path"]))
        members[d] = Member(d, p["name"])
    for p in meta["packages"]:
        m = members[rel(os.path.dirname(p["manifest_path"]))]
        for dep in p["dependencies"]:
            if dep.get("path"):
                d = rel(dep["path"])
                if d in members:
                    m.deps.add(d)
                else:
                    m.read(d, f"{m.dir}/Cargo.toml (path dependency)")
        for t in p["targets"]:
            src = rel(t["src_path"])
            if "custom-build" in t["kind"]:
                m.queue.append((src, True))
            elif not under(src, m.dir):
                m.source(src, f"{m.dir}/Cargo.toml (target {t['name']})")
        m.queue.extend((f, False) for f in walk(m.dir) if f.endswith(".rs"))
        m.run()
    return members


def changed(rev):
    def git(*args):
        return subprocess.run(["git", *args], capture_output=True, check=True).stdout

    paths = git("diff", "--name-only", "--no-renames", "-z", rev, "--")
    paths += git("ls-files", "--others", "--exclude-standard", "-z")
    return sorted({os.fsdecode(p) for p in paths.split(b"\0") if p})


def scope(rev):
    paths = changed(rev)
    for p in paths:
        if p in EVERYTHING_FILES or p.startswith(EVERYTHING_DIRS):
            return [("all", f"{p} changed, so every crate is checked")]
    members = load()
    outside = [p for p in paths if not any(under(p, d) for d in members)]
    why = {}
    for d, m in members.items():
        for p in paths:
            if under(p, d):
                why[d] = f"changed: {p}"
                break
            hit = next((r for r in m.reads if p.startswith(r)), None)
            if hit is not None:
                why[d] = f"reads {p} at compile time, in {m.reads[hit]}"
                break
        if d not in why and m.anything and outside:
            why[d] = f"{outside[0]} changed, and {m.anything[0]}, which the scan cannot follow"
    grew = True
    while grew:
        grew = False
        for d, m in members.items():
            dep = next((x for x in sorted(m.deps) if x in why), None)
            if d not in why and dep is not None:
                why[d] = f"builds from {dep}"
                grew = True
    return sorted(why.items())


def main():
    if sys.argv[1:] == ["--explain"]:
        for d, m in sorted(load().items()):
            print(f"{d} ({m.name})")
            print(f"  depends on: {' '.join(sorted(m.deps)) or '-'}")
            for r, src in sorted(m.reads.items()):
                print(f"  reads {r}  <- {src}")
            for a in m.anything:
                print(f"  reads anything: {a}")
        return 0
    if len(sys.argv) != 2:
        print("usage: check-features-since.py <rev> | --explain")
        return 2
    rev = sys.argv[1]
    ok = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", rev + "^{commit}"],
        capture_output=True,
    )
    if ok.returncode != 0:
        print(f"FAIL: --since {rev}: git knows no such commit")
        return 1
    try:
        lines = scope(rev)
    except Exception as e:  # noqa: BLE001 -- any failure checks everything
        lines = [("all", f"the scope could not be computed ({type(e).__name__}: {e}), so every crate is checked")]
    for d, why in lines:
        print(f"{d}\t{why}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
