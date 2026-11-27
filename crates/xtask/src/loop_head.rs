//! `cargo xtask loop-head`: one function's machine code compared between
//! two binaries, addresses aside.
//!
//! ```text
//! cargo xtask loop-head [--fn NAME] [--no-pageoff] A [B]
//! cargo xtask loop-head --self-test
//! ```
//!
//! **Why it exists.** `ingest2` once inlined its timed loop into `main`, so
//! the loop shared a register allocation with everything `main` built before
//! it. A setup-only change -- the cluster store's `vec!` sized 1024 instead
//! of 64, or that `vec!` moved into a function of its own, with the core
//! untouched -- put two stack reloads at the head of the dispatch loop and
//! moved instructions per rep +0.69% on `cat`. Nothing in the timing said
//! which; this does. Run it on both arms before attributing a small
//! `ingest2` delta: if the timed function differs, the delta may be the
//! allocator's and not the change's.
//!
//! **What it prints**, for each binary: the function's instruction count,
//! its loads from the stack frame (`ldr*`/`ldp`/`ldur*` off `sp` or `x29`),
//! and a digest of its normalised body. Given two binaries, it says
//! IDENTICAL or prints a unified diff of the normalised bodies, and exits 0
//! or 1. The verdict names the form it held under: page offsets normalised,
//! or kept (`--no-pageoff`). A default-form IDENTICAL is compared again with
//! offsets kept and says whether it holds there too. It exits 2 when the
//! function is not in a binary, or when the name, demangled, names more than
//! one function there, never 0.
//!
//! **Normalised** means: no instruction addresses; a branch inside the
//! function becomes its offset from the function's start; a call keeps its
//! target's name and loses its address (a stub, which objdump names
//! `<_sym+0x...>`, becomes `<stub>`); an `adr`/`adrp` target becomes `PAGE`;
//! every immediate offset from a register that some `adr`/`adrp` in the
//! function writes becomes `#PAGEOFF`, because those offsets move with the
//! data layout; a pc-relative literal load becomes `LIT`. Registers, other
//! immediates and stack offsets are kept, so a different register allocation
//! is a difference.
//!
//! The page-offset rewrite applies to a register function-wide, live or not
//! (13 registers in `timed`, one of them holding a stack address), so an
//! offset-only change through one of them reads IDENTICAL. `--no-pageoff`
//! keeps every offset. Every IDENTICAL arm checked also compares IDENTICAL
//! with the rewrite disabled but one: the G cut's outlined-`vec!` arm, where
//! `ingest2::run` loads one 16-byte constant from 0x80 lower in the same page
//! (`[x26, #0x820]` for `#0x8a0`, an `adrp` base) at three sites.
//!
//! **Default function: `ingest2::timed`**, the harness's out-of-line timed
//! loop. `--fn ingest2::main` reads a harness from before it was outlined,
//! where the whole of `main` differs whenever the setup does, so it cannot
//! isolate the loop.
//!
//! **Controls.** `--self-test` runs the normaliser on synthetic bodies: the
//! same body at two addresses with different call and page targets must
//! compare equal, and one changed register, one added stack reload, or an
//! immediate off a register no `adr` writes must not; the two page offsets
//! that compare equal must differ under `--no-pageoff`; and a name that two
//! functions share must exit 2. On real binaries: two byte-identical builds
//! report IDENTICAL, and so does a setup-only change (the cluster `vec!`
//! outlined, or `NCLUSTER` raised); a change to `Term`'s layout (32,776
//! unread bytes added to `ClusterStore`) reports DIFFER: the function's
//! frame grows from 0x340 to 0x390 bytes and its registers are allocated
//! anew; no instruction in that diff differs only in an offset off a
//! non-stack register.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::process::Command;
use std::sync::LazyLock;

use regex::{NoExpand, Regex};

use crate::features_since::Exit;
use crate::html::Count;
use crate::{difflib, py, sha256, text};

const USAGE: &str = "loop-head [--fn NAME] [--no-pageoff] A [B] | --self-test";

/// What stopped a run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Stop {
    /// `fail()`, exit 2: a verdict could not be reached.
    Fail(String),
    /// An uncaught exception in the script, exit 1: objdump could not be
    /// started, or a line it wrote cannot be read.
    Crash(String),
}

impl Stop {
    fn exit(&self) -> Exit {
        match self {
            Stop::Fail(_) => Exit(2),
            Stop::Crash(_) => Exit(1),
        }
    }

    fn message(&self) -> String {
        match self {
            Stop::Fail(m) => format!("loop-head: {m}"),
            Stop::Crash(m) => m.clone(),
        }
    }
}

/// `objdump -d --no-show-raw-insn -C path` from a program name, so a test
/// can give it one that fails.
pub fn disassemble(program: &str, path: &str) -> Result<String, Stop> {
    let output = Command::new(program)
        .args(["-d", "--no-show-raw-insn", "-C", path])
        .output()
        .map_err(|e| Stop::Crash(format!("{program}: {e}")))?;
    let decode = |b: Vec<u8>| {
        String::from_utf8(b)
            .map(|s| text::universal(&s))
            .map_err(|e| Stop::Crash(format!("{program} on {path}: {e}")))
    };
    let (out, err) = (decode(output.stdout)?, decode(output.stderr)?);
    if output.status.success() {
        Ok(out)
    } else {
        Err(Stop::Fail(format!(
            "objdump failed on {path}: {}",
            text::strip(&err)
        )))
    }
}

/// An instruction's address and text.
pub type Instruction = (i128, String);

/// Every function of a listing, from objdump's `<name>:` headers.
#[derive(Clone, Default, Debug)]
pub struct Listing {
    /// Names in the order they first appear.
    pub names: Vec<String>,
    pub bodies: HashMap<String, Vec<Instruction>>,
    /// How many functions a name stands for, where it is more than one.
    /// Demangling can give several functions one name (`finish_grow`), and
    /// their bodies merge under one key, so each such name is counted.
    pub shared: HashMap<String, Count>,
}

static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9a-f]+) <(.+)>:$").expect("the header"));
static LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([0-9a-f]+):\s+(.*)$").expect("the instruction line"));

fn address(hex: &str) -> Result<i128, Stop> {
    i128::from_str_radix(hex, 16).map_err(|e| Stop::Crash(format!("{hex}: {e}")))
}

/// `functions()`.
pub fn functions(lines: &[&str]) -> Result<Listing, Stop> {
    let mut listing = Listing::default();
    let mut current: Option<String> = None;
    for line in lines {
        if let Some(m) = HEADER.captures(text::strip(line)) {
            let name = m[2].to_string();
            if listing.bodies.contains_key(&name) {
                listing.shared.entry(name.clone()).or_insert(Count(1)).0 += 1;
            } else {
                listing.names.push(name.clone());
                listing.bodies.insert(name.clone(), Vec::new());
            }
            current = Some(name);
            continue;
        }
        let Some(name) = &current else {
            continue;
        };
        if let Some(m) = LINE.captures(line) {
            let body = listing.bodies.get_mut(name).expect("its header came first");
            body.push((address(&m[1])?, text::strip(&m[2]).to_string()));
        }
    }
    Ok(listing)
}

/// `find()`: the name, or why it cannot be read.
pub fn find<'a>(listing: &'a Listing, name: &str, path: &str) -> Result<&'a [Instruction], Stop> {
    if let Some(n) = listing.shared.get(name) {
        return Err(Stop::Fail(format!(
            "{} names {} functions in {path}",
            py::repr_str(name),
            n.0
        )));
    }
    if let Some(body) = listing.bodies.get(name) {
        return Ok(body);
    }
    let last = name.rsplit("::").next().unwrap_or(name);
    let ending = format!("::{last}");
    let near: Vec<String> = listing
        .names
        .iter()
        .filter(|n| n.ends_with(&ending))
        .take(5)
        .map(|n| py::repr_str(n))
        .collect();
    Err(Stop::Fail(format!(
        "no function named {} in {path}; ending alike: [{}]",
        py::repr_str(name),
        near.join(", ")
    )))
}

static ADR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^adrp?\s+([wx]\d+),").expect("the adr pattern"));
static COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+;.*$").expect("the comment pattern"));
static STUB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"0x[0-9a-f]+ <_[^>]*\+0x[0-9a-f]+>").expect("the stub pattern"));
static TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"0x[0-9a-f]+ (<.*>)$").expect("the target pattern"));
static LITERAL_FORM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[wxqdsb]\d+, 0x[0-9a-f]+").expect("the literal load"));
static LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"0x[0-9a-f]+.*$").expect("the literal operand"));

/// Whether a body's page offsets are rewritten.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageOff {
    Normalised,
    Kept,
}

impl PageOff {
    fn form(self) -> &'static str {
        match self {
            PageOff::Normalised => "page offsets normalised",
            PageOff::Kept => "page offsets kept",
        }
    }
}

/// `f"{n:#x}"`.
fn hex(n: i128) -> String {
    if n < 0 {
        format!("-{:#x}", -n)
    } else {
        format!("{n:#x}")
    }
}

/// `normalise()`.
pub fn normalise(body: &[Instruction], name: &str, pageoff: PageOff) -> Result<Vec<String>, Stop> {
    let Some(base) = body.first().map(|i| i.0) else {
        return Ok(Vec::new());
    };
    // Every register an `adr`/`adrp` writes anywhere in the function: their
    // offsets are layout, and the allocator hoists a page base far from its
    // uses.
    let mut paged: BTreeSet<String> = BTreeSet::new();
    if pageoff == PageOff::Normalised {
        for (_, t) in body {
            if let Some(m) = ADR.captures(t) {
                paged.insert(format!("x{}", &m[1][1..]));
            }
        }
    }
    let offsets: Vec<Regex> = paged
        .iter()
        .map(|r| {
            Regex::new(&format!(r"(\[{r}, |, {r}, )#-?0x[0-9a-f]+")).expect("a register's pattern")
        })
        .collect();
    let local = Regex::new(&format!(
        r"0x([0-9a-f]+) <{}(?:\+0x[0-9a-f]+)?>",
        regex::escape(name)
    ))
    .expect("the local branch");
    let mut out = Vec::with_capacity(body.len());
    for (_, t) in body {
        let cleaned = COMMENT.replace_all(t, NoExpand(""));
        let stripped = text::strip(&cleaned);
        let mut parts = stripped.splitn(2, text::space);
        let Some(mn) = parts.next().filter(|m| !m.is_empty()) else {
            return Err(Stop::Crash(format!("an empty instruction in {name}")));
        };
        let rest = parts.next().unwrap_or("").trim_start_matches(text::space);
        let mut failure = None;
        let ops = local.replace_all(rest, |c: &regex::Captures<'_>| match address(&c[1]) {
            Ok(a) => format!("<+{}>", hex(a - base)),
            Err(e) => {
                failure = Some(e);
                String::new()
            }
        });
        if let Some(e) = failure {
            return Err(e);
        }
        let ops = STUB.replace_all(&ops, NoExpand("<stub>"));
        let ops = TARGET.replace_all(&ops, "$1");
        if mn == "adrp" || mn == "adr" {
            let dst = text::strip(ops.split(',').next().unwrap_or(""));
            out.push(format!("{mn}\t{dst}, PAGE"));
            continue;
        }
        let mut ops = ops.into_owned();
        if mn.starts_with("ldr") && LITERAL_FORM.is_match(&ops) {
            ops = LITERAL.replace_all(&ops, NoExpand("LIT")).into_owned();
        }
        for re in &offsets {
            ops = re.replace_all(&ops, "${1}#PAGEOFF").into_owned();
        }
        out.push(format!("{mn}\t{ops}"));
    }
    Ok(out)
}

static LOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ld(r|p|ur)\w*\t").expect("the load pattern"));
static FRAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(sp|x29)(, #-?0x[0-9a-f]+)?\]").expect("the frame pattern"));

/// `reloads()`: how many instructions load from the stack frame.
pub fn reloads(norm: &[String]) -> Count {
    Count(
        norm.iter()
            .filter(|l| LOAD.is_match(l) && FRAME.is_match(l))
            .count(),
    )
}

fn describe(path: &str, name: &str, norm: &[String], out: &mut impl Write) -> io::Result<()> {
    let digest = sha256::hex(norm.join("\n").as_bytes());
    writeln!(
        out,
        "{path}: {name}: {} instructions, {} stack loads, body {}",
        norm.len(),
        reloads(norm).0,
        &digest[..16]
    )
}

fn checked(hold: impl FnOnce() -> Result<bool, Stop>, claim: &str) -> Result<(), Stop> {
    if hold()? {
        Ok(())
    } else {
        Err(Stop::Crash(format!("self-test: {claim}")))
    }
}

fn synthetic(lines: [(i128, &str); 7]) -> Vec<Instruction> {
    lines.iter().map(|(a, t)| (*a, (*t).to_string())).collect()
}

/// `self_test()`: the six controls, each of which must fail if the rule it
/// holds is gone.
pub fn self_test() -> Result<&'static str, Stop> {
    let name = "ingest2::timed";
    let a = synthetic([
        (0x1000, "stp\tx29, x30, [sp, #-0x10]!"),
        (0x1004, "adrp\tx8, 0x100067000 <GCC_except_table431+0x4>"),
        (0x1008, "add\tx8, x8, #0xa00"),
        (0x100C, "bl\t0x100065b80 <_writev+0x100065b80>"),
        (0x1010, "b.hs\t0x1000 <ingest2::timed>"),
        (0x1014, "bl\t0x100026d40 <<u64 as core::fmt::Display>::fmt>"),
        (0x1018, "ldr\tx26, [x8, #0x2e8]"),
    ]);
    let b = synthetic([
        (0x2000, "stp\tx29, x30, [sp, #-0x10]!"),
        (0x2004, "adrp\tx8, 0x100099000 <GCC_except_table12+0x8>"),
        (0x2008, "add\tx8, x8, #0x140"),
        (0x200C, "bl\t0x100077000 <_writev+0x100077000>"),
        (0x2010, "b.hs\t0x2000 <ingest2::timed>"),
        (0x2014, "bl\t0x100031000 <<u64 as core::fmt::Display>::fmt>"),
        (0x2018, "ldr\tx26, [x8, #0x2e8]"),
    ]);
    let n = PageOff::Normalised;
    let (na, nb) = (normalise(&a, name, n)?, normalise(&b, name, n)?);
    checked(
        || Ok(na == nb),
        "the same body at two addresses must compare equal",
    )?;
    let mut c = b.clone();
    c[6] = (0x2018, "ldr\tx27, [x8, #0x2e8]".to_string());
    checked(
        || Ok(normalise(&c, name, n)? != na),
        "a changed register must differ",
    )?;
    let mut d = b.clone();
    d.insert(4, (0x200C, "ldr\tx26, [sp, #0x2e8]".to_string()));
    let nd = normalise(&d, name, n)?;
    checked(
        || Ok(nd != na && reloads(&nd).0 == reloads(&na).0 + 1),
        "an added reload must differ and count",
    )?;
    let mut e = b.clone();
    e[2] = (0x2008, "add\tx8, x9, #0x140".to_string());
    checked(
        || Ok(normalise(&e, name, n)? != na),
        "an add off another register keeps its immediate",
    )?;
    checked(
        || Ok(normalise(&a, name, PageOff::Kept)? != normalise(&b, name, PageOff::Kept)?),
        "--no-pageoff keeps page offsets",
    )?;
    let shared = functions(&[
        "0000000100001000 <alloc::raw_vec::finish_grow>:",
        "100001000: ret",
        "0000000100002000 <alloc::raw_vec::finish_grow>:",
        "100002000: ret",
    ])?;
    let two = find(&shared, "alloc::raw_vec::finish_grow", "synthetic");
    checked(
        || Ok(matches!(two, Err(Stop::Fail(_)))),
        "a name two functions share must exit 2",
    )?;
    Ok("self-test: 6 of 6 hold")
}

/// `main()`, with what it spawns named.
pub fn run(
    args: &[&str],
    objdump: &str,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<Exit> {
    match compare(args, objdump, out) {
        Ok(exit) => Ok(exit),
        Err(Failure::Io(e)) => Err(e),
        Err(Failure::Stop(stop)) => {
            writeln!(err, "{}", stop.message())?;
            Ok(stop.exit())
        }
    }
}

enum Failure {
    Io(io::Error),
    Stop(Stop),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Io(e)
    }
}

impl From<Stop> for Failure {
    fn from(s: Stop) -> Self {
        Failure::Stop(s)
    }
}

fn compare(args: &[&str], objdump: &str, out: &mut impl Write) -> Result<Exit, Failure> {
    if args == ["--self-test"] {
        writeln!(out, "{}", self_test()?)?;
        return Ok(Exit(0));
    }
    let mut name = "ingest2::timed";
    let mut pageoff = PageOff::Normalised;
    let mut rest = args;
    loop {
        match rest {
            ["--no-pageoff", tail @ ..] => {
                pageoff = PageOff::Kept;
                rest = tail;
            }
            ["--fn", value, tail @ ..] => {
                name = *value;
                rest = tail;
            }
            ["--fn"] => return Err(Stop::Fail(USAGE.to_string()).into()),
            _ => break,
        }
    }
    if !matches!(rest.len(), 1 | 2) {
        return Err(Stop::Fail(USAGE.to_string()).into());
    }
    let mut norms = Vec::new();
    let mut kept = Vec::new();
    for path in rest {
        let listing = disassemble(objdump, path)?;
        let lines = text::splitlines(&listing);
        let parsed = functions(&lines)?;
        let body = find(&parsed, name, path)?;
        let norm = normalise(body, name, pageoff)?;
        describe(path, name, &norm, out)?;
        norms.push(norm);
        kept.push(normalise(body, name, PageOff::Kept)?);
    }
    let [first, second] = &norms[..] else {
        return Ok(Exit(0));
    };
    let form = pageoff.form();
    if first == second {
        if pageoff == PageOff::Normalised {
            let also = if kept[0] == kept[1] {
                "IDENTICAL"
            } else {
                "DIFFER"
            };
            writeln!(out, "IDENTICAL ({form}; with page offsets kept: {also})")?;
        } else {
            writeln!(out, "IDENTICAL ({form})")?;
        }
        return Ok(Exit(0));
    }
    writeln!(out, "DIFFER ({form})")?;
    for line in difflib::unified_diff(first, second, rest[0], rest[1]) {
        writeln!(out, "{line}")?;
    }
    Ok(Exit(1))
}
