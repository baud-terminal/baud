//! `cargo xtask pdf-search <key> ...`: a term in a pinned PDF, with a
//! positive control that cannot be left out.
//!
//! **A miss is evidence only once a term known to be present has hit.**
//! `SGR 60-65` sat refused for months because six probes missed in a text
//! layer that was intact and letter-spaced, and six misses were read as
//! absence. So a document has a row in [`DOCUMENTS`] or it is not searched:
//! the row names how its text comes out and the terms that must be found in
//! it, every search runs them, and a control that does not fire exits
//! non-zero and says the extraction is broken. A key with no row is refused
//! and gets one when someone needs to search it.
//!
//! What a form prints and how it exits is what the script it replaced
//! printed and how it exited, which is why they differ by document:
//!
//! | form | a term misses | a control misses |
//! |---|---|---|
//! | `ecma48`, no arguments | 1 | 1 |
//! | `ecma48 --find N`, `--section sgr` | 1 | 1, said on stderr |
//! | `ecma35 --find N` | 0, printed | 2 |
//! | `<key> <terms>`, every document | 0, printed | 1 |
//!
//! `<key>` is a `MANIFEST.toml` key. An `external` document, read but not
//! committed, is named with `--file PATH`; any `--file` must hash to the
//! manifest's sha256.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::features_since::Exit;
use crate::html::Count;
use crate::pdf::{self, Extract, Flat, Stop};
use crate::py;
use crate::sha256;
use crate::sources;

/// How a document's text comes out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    /// Flate streams in the clear ([`pdf::flate`]).
    Flate,
    /// `LZWDecode` streams ([`pdf::lzw`]).
    Lzw,
    /// The standard security handler's RC4, then Flate ([`pdf::encrypted`]).
    Encrypted,
}

/// A window of a document a form can print by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Section {
    pub name: &'static str,
    /// What the window opens at, folded.
    pub anchor: &'static str,
    pub before: Count,
    pub after: Count,
}

/// A searchable document.
#[derive(Clone, Copy, Debug)]
pub struct Document {
    /// Its `MANIFEST.toml` key.
    pub key: &'static str,
    pub method: Method,
    /// Terms known to be present from outside the search: its own title, or
    /// what `[[source: ctlseqs]]` and xterm say ECMA-48 defines.
    pub controls: &'static [&'static str],
    /// What the no-argument form looks for.
    pub probes: &'static [&'static str],
    pub section: Option<Section>,
}

const fn document(
    key: &'static str,
    method: Method,
    controls: &'static [&'static str],
) -> Document {
    Document {
        key,
        method,
        controls,
        probes: &[],
        section: None,
    }
}

/// `[[source: ecma48]]`'s controls, its probes, which were once reported
/// absent and every one of which is present, and SGR's section, 8.3.117.
const ECMA48: Document = Document {
    key: "ecma48",
    method: Method::Flate,
    controls: &[
        "selectgraphicrendition",
        "blink",
        "negativeimage",
        "underline",
        "italicized",
        "concealed",
    ],
    probes: &[
        "fraktur",
        "gothic",
        "proportional",
        "framed",
        "encircled",
        "overlined",
        "notframed",
        "notoverlined",
        "ideogram",
        "stressmarking",
    ],
    section: Some(Section {
        name: "sgr",
        anchor: "sgrisusedtoestablish",
        before: Count(40),
        after: Count(4000),
    }),
};

/// Every document that can be searched: each tracked PDF, and the two
/// `external` ones whose reads a record cites.
pub const DOCUMENTS: [Document; 11] = [
    ECMA48,
    document("ecma35", Method::Lzw, &["graphic character"]),
    document("ecma-tr53", Method::Lzw, &["graphic character"]),
    document(
        "adobe-5176-cff",
        Method::Encrypted,
        &[
            "compactfontformat",
            "charstrings",
            "fdselect",
            "privatedict",
            "index",
        ],
    ),
    document(
        "adobe-5177-type2",
        Method::Encrypted,
        &[
            "type2charstring",
            "hintmask",
            "callsubr",
            "endchar",
            "transientarray",
        ],
    ),
    document(
        "utf8-lookup",
        Method::Flate,
        &["validatingutf8", "johnkeiser", "daniellemire"],
    ),
    document(
        "simdjson",
        Method::Flate,
        &["parsinggigabytes", "daniellemire", "structuralindex"],
    ),
    document(
        "lemire-transcoding",
        Method::Flate,
        &["transcoding", "daniellemire", "simdutf"],
    ),
    document(
        "simdutf",
        Method::Flate,
        &["transcoding", "daniellemire", "simdutf"],
    ),
    document(
        "parabix",
        Method::Flate,
        &["simonfraseruniversity", "parabix", "bitstream", "grep"],
    ),
    document(
        "pdf-reference-1-4",
        Method::Flate,
        &[
            "computinganencryptionkey",
            "standardsecurityhandler",
            "earlychange",
        ],
    ),
];

/// The usage line, and all `--help` prints.
pub const USAGE: &str = "usage: cargo xtask pdf-search <key> [--file PATH] [<term>...]
       cargo xtask pdf-search <flate key> [--dump | --find NEEDLE | --section sgr]
       cargo xtask pdf-search <lzw key> [--find NEEDLE]
";

/// A command line refused before the document is read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Refused {
    pub exit: Exit,
    pub message: String,
}

fn refused<T>(exit: Exit, message: impl Into<String>) -> Result<T, Refused> {
    Err(Refused {
        exit,
        message: message.into(),
    })
}

/// What a command line asks of a document.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Form {
    /// Everything the document's script did with no search named.
    Survey,
    /// The folded text, or an LZW document's text, with no control: this is
    /// not a search.
    Dump,
    Find(String),
    Section(Section),
    Terms(Vec<String>),
    Help,
}

/// How argparse read one argument of the ECMA-48 script's command line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class<'a> {
    Positional,
    /// `--`: everything after it is positional.
    Dashes,
    Help,
    /// An option that consumes no text, given text.
    Ignored,
    Dump,
    Find(Option<&'a str>),
    Section(Option<&'a str>),
    Ambiguous,
    Unknown,
}

const LONG: [&str; 4] = ["--help", "--dump", "--find", "--section"];

/// `-h` with more after it: 3.13's `consume_optional` keeps taking
/// single-dash flags out of the tail, and runs what it took even when it
/// reaches a letter no flag has. Only `-` and `=` stop it with an error.
fn help_tail(tail: &str) -> Class<'_> {
    let mut rest = tail;
    loop {
        match rest.chars().next() {
            None => return Class::Help,
            Some('-' | '=') => return Class::Ignored,
            Some('h') => rest = &rest[1..],
            Some(_) => return Class::Help,
        }
    }
}

static NEGATIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-\d+\n?\z|^-\d*\.\d+\n?\z").expect("the negative number"));

fn long_option<'a>(name: &str, explicit: Option<&'a str>) -> Class<'a> {
    match name {
        "--help" if explicit.is_none() => Class::Help,
        "--dump" if explicit.is_none() => Class::Dump,
        "--help" | "--dump" => Class::Ignored,
        "--find" => Class::Find(explicit),
        _ => Class::Section(explicit),
    }
}

/// `ArgumentParser._parse_optional` of Python 3.13.12, for the options
/// `-h`, `--help`, `--dump`, `--find` and `--section`: a prefix of a long
/// option names it, `=` gives it its value, and a dash-led word holding a
/// space, or spelling a negative number, is a positional.
fn classify(arg: &str) -> Class<'_> {
    if arg == "--" {
        return Class::Dashes;
    }
    if !arg.starts_with('-') || arg == "-" {
        return Class::Positional;
    }
    if arg == "-h" {
        return Class::Help;
    }
    if LONG.contains(&arg) {
        return long_option(arg, None);
    }
    let (name, explicit) = match arg.split_once('=') {
        Some((n, e)) => (n, Some(e)),
        None => (arg, None),
    };
    if explicit.is_some() {
        if name == "-h" {
            return Class::Ignored;
        }
        if LONG.contains(&name) {
            return long_option(name, explicit);
        }
    }
    if arg.starts_with("--") {
        let found: Vec<&&str> = LONG.iter().filter(|o| o.starts_with(name)).collect();
        match found[..] {
            [] => {}
            [one] => return long_option(one, explicit),
            _ => return Class::Ambiguous,
        }
    } else {
        if let Some(tail) = arg.strip_prefix("-h") {
            return help_tail(tail);
        }
        // `-=x`: a prefix of every option.
        if name == "-" {
            return Class::Ambiguous;
        }
    }
    if NEGATIVE.is_match(arg) || arg.contains(' ') {
        return Class::Positional;
    }
    Class::Unknown
}

/// Whether `--find` or `--section` can take this as its value: argparse
/// strips `-*` from an option's nargs pattern, so `--` is no value and does
/// not reach past itself to one.
fn value_of<'a>(args: &[&'a str]) -> Option<&'a str> {
    let next = args.first()?;
    matches!(classify(next), Class::Positional).then_some(*next)
}

/// The ECMA-48 script's command line, as its `argparse` parser read it, left
/// to right: an error or `--help` takes effect where it stands, and unknown
/// words are reported at the end.
///
/// **One departure, by design.** A positional was exit 2, "unrecognized
/// arguments"; it is now a term of the `<key> <terms>` form the three
/// scripts' replacement exists to have. With an option beside it, it stays
/// exit 2.
pub fn parse_flate(args: &[&str], doc: &Document) -> Result<Form, Refused> {
    let bad = |m: String| Refused {
        exit: Exit(2),
        message: format!("{USAGE}pdf-search: error: {m}\n"),
    };
    let (mut dump, mut find, mut section) = (false, None, None);
    let (mut terms, mut unknown, mut dashes) = (Vec::new(), Vec::new(), false);
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        i += 1;
        match classify(arg) {
            Class::Positional => terms.push(arg.to_string()),
            Class::Dashes => {
                dashes = true;
                terms.extend(args[i..].iter().map(ToString::to_string));
                i = args.len();
            }
            Class::Help => return Ok(Form::Help),
            Class::Ignored => {
                return Err(bad(format!("{arg}: ignored explicit argument")));
            }
            Class::Ambiguous => return Err(bad(format!("ambiguous option: {arg}"))),
            Class::Unknown => unknown.push(arg),
            Class::Dump => dump = true,
            Class::Find(explicit) => {
                let Some(value) = explicit.or_else(|| value_of(&args[i..])) else {
                    return Err(bad("argument --find: expected one argument".to_string()));
                };
                i += usize::from(explicit.is_none());
                find = Some(value);
            }
            Class::Section(explicit) => {
                let Some(value) = explicit.or_else(|| value_of(&args[i..])) else {
                    return Err(bad("argument --section: expected one argument".to_string()));
                };
                i += usize::from(explicit.is_none());
                let Some(s) = doc.section.filter(|s| s.name == value) else {
                    return Err(bad(format!("argument --section: invalid choice: {value}")));
                };
                section = Some(s);
            }
        }
    }
    if !unknown.is_empty() {
        return Err(bad(format!(
            "unrecognized arguments: {}",
            unknown.join(" ")
        )));
    }
    let optioned = dump || find.is_some() || section.is_some();
    if (optioned && (dashes || !terms.is_empty())) || (dashes && terms.is_empty()) {
        return Err(bad(format!("unrecognized arguments: {}", terms.join(" "))));
    }
    if !terms.is_empty() {
        return Ok(Form::Terms(terms));
    }
    Ok(if dump {
        Form::Dump
    } else if let Some(s) = section {
        Form::Section(s)
    } else {
        // `if args.find:` -- an empty needle is no search.
        match find {
            Some(n) if !n.is_empty() => Form::Find(n.to_string()),
            _ => Form::Survey,
        }
    })
}

/// The ECMA-35 script's command line: `sys.argv.index('--find')`, the word
/// after it, and nothing else read. A word with no `--find` was ignored and
/// is now a term; `--find` as the last word was an `IndexError`.
pub fn parse_lzw(args: &[&str]) -> Result<Form, Refused> {
    match args.iter().position(|a| *a == "--find") {
        Some(at) => match args.get(at + 1) {
            Some(needle) => Ok(Form::Find((*needle).to_string())),
            None => refused(Exit(1), "--find: a needle is needed\n"),
        },
        None if args.is_empty() => Ok(Form::Dump),
        None => Ok(Form::Terms(args.iter().map(ToString::to_string).collect())),
    }
}

/// The form a document's arguments ask for.
pub fn parse(doc: &Document, args: &[&str]) -> Result<Form, Refused> {
    match doc.method {
        Method::Flate => parse_flate(args, doc),
        Method::Lzw => parse_lzw(args),
        // The Adobe script took every word as a probe.
        Method::Encrypted => Ok(Form::Terms(args.iter().map(ToString::to_string).collect())),
    }
}

/// The text of a document.
pub fn extract(method: Method, data: &[u8]) -> Result<Extract, Stop> {
    match method {
        Method::Flate => Ok(pdf::flate(data)),
        Method::Lzw => Ok(pdf::lzw(data)),
        Method::Encrypted => pdf::encrypted(data),
    }
}

fn count(flat: &str, term: &str) -> Count {
    Count(flat.matches(pdf::fold(term).as_str()).count())
}

fn misses(flat: &str, doc: &Document) -> Count {
    Count(
        doc.controls
            .iter()
            .filter(|c| count(flat, c).0 == 0)
            .count(),
    )
}

fn hit(n: Count) -> &'static str {
    if n.0 == 0 { "MISS" } else { "HIT " }
}

fn broken(n: Count) -> String {
    format!(
        "{} control term(s) missed: the extraction is broken, not the PDF.",
        n.0
    )
}

/// Controls, then probes: the no-argument form.
fn survey(flat: &str, doc: &Document, out: &mut impl Write) -> io::Result<Exit> {
    writeln!(out, "positive control -- these are known present:")?;
    let mut bad = 0;
    for t in doc.controls {
        let n = count(flat, t);
        writeln!(out, "  {} {t:<24} x{}", hit(n), n.0)?;
        bad += usize::from(n.0 == 0);
    }
    if bad != 0 {
        writeln!(out, "\n{}", broken(Count(bad)))?;
        return Ok(Exit(1));
    }
    if doc.probes.is_empty() {
        return Ok(Exit(0));
    }
    writeln!(out, "\nprobes -- these were once reported absent:")?;
    for t in doc.probes {
        let n = count(flat, t);
        writeln!(out, "  {} {t:<24} x{}", hit(n), n.0)?;
        bad += usize::from(n.0 == 0);
    }
    writeln!(out)?;
    if bad == 0 {
        writeln!(
            out,
            "controls passed, so a MISS above would be real absence."
        )?;
        Ok(Exit(0))
    } else {
        writeln!(out, "{bad} probe(s) missed.")?;
        Ok(Exit(1))
    }
}

fn window(text: &str, flat: &Flat, needle: &str, before: Count, after: Count) -> Option<String> {
    let at = flat.flat.find(pdf::fold(needle).as_str())?;
    let first = *flat.index.get(at.saturating_sub(before.0))?;
    let last = *flat.index.get(
        at.saturating_add(after.0)
            .min(flat.index.len().checked_sub(1)?),
    )?;
    Some(pdf::unspace(&text[first..last]))
}

/// Where the Adobe script's form was the only one: every control and term
/// a line, and what it read. A term's miss is printed and is not a failure.
fn terms(
    name: &str,
    extract: &Extract,
    flat: &str,
    doc: &Document,
    words: &[String],
    out: &mut impl Write,
) -> io::Result<Exit> {
    let streams = match doc.method {
        Method::Encrypted => "decrypted streams",
        _ => "streams",
    };
    writeln!(
        out,
        "{name}: {} chars of text from {} {streams}",
        extract.text.chars().count(),
        extract.streams.0
    )?;
    let mut bad = false;
    for c in doc.controls {
        let found = flat.contains(pdf::fold(c).as_str());
        bad |= !found;
        writeln!(
            out,
            "  control {c:<22} {}",
            if found { "HIT" } else { "MISS" }
        )?;
    }
    for p in words {
        let found = flat.contains(pdf::fold(p).as_str());
        writeln!(
            out,
            "  probe   {p:<22} {}",
            if found { "HIT" } else { "MISS" }
        )?;
    }
    if bad {
        writeln!(
            out,
            "POSITIVE CONTROL FAILED -- the extraction is broken, the document is not exonerated"
        )?;
        return Ok(Exit(1));
    }
    Ok(Exit(0))
}

/// One form over a document's bytes. `name` is the file's name, as the
/// Adobe script's first line printed it.
pub fn search(
    doc: &Document,
    name: &str,
    data: &[u8],
    form: &Form,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<Exit> {
    if *form == Form::Help {
        out.write_all(USAGE.as_bytes())?;
        return Ok(Exit(0));
    }
    let extract = match extract(doc.method, data) {
        Ok(e) => e,
        Err(Stop(message)) => {
            writeln!(err, "{message}")?;
            return Ok(Exit(1));
        }
    };
    let text = &extract.text;
    let flat = pdf::flatten(text);
    let folded = flat.flat.as_str();
    match form {
        Form::Help => unreachable!("answered above"),
        Form::Dump => {
            let dump = if doc.method == Method::Flate {
                folded
            } else {
                text
            };
            out.write_all(dump.as_bytes())?;
            Ok(Exit(0))
        }
        Form::Survey if doc.method == Method::Flate => survey(folded, doc, out),
        Form::Survey => terms(name, &extract, folded, doc, &[], out),
        Form::Terms(words) => terms(name, &extract, folded, doc, words, out),
        Form::Find(needle) if doc.method == Method::Lzw => {
            let mut missed = 0;
            for c in doc.controls {
                let n = count(folded, c);
                missed += usize::from(n.0 == 0);
                let word = if n.0 == 0 { "MISS" } else { "HIT" };
                writeln!(out, "control {}: {word} x{}", py::repr_str(c), n.0)?;
            }
            let k = count(folded, needle);
            let word = if k.0 == 0 { "MISS" } else { "HIT" };
            writeln!(out, "{}: {word} x{}", py::repr_str(needle), k.0)?;
            if missed != 0 {
                writeln!(err, "{}", broken(Count(missed)))?;
                return Ok(Exit(2));
            }
            Ok(Exit(0))
        }
        Form::Find(needle) => {
            let bad = misses(folded, doc);
            if bad.0 != 0 {
                writeln!(err, "{}", broken(bad))?;
                return Ok(Exit(1));
            }
            let n = count(folded, needle);
            writeln!(out, "{} {needle} x{}", hit(n), n.0)?;
            if n.0 == 0 {
                return Ok(Exit(1));
            }
            let shown = window(text, &flat, needle, Count(60), Count(400));
            writeln!(out, "\n{}", shown.unwrap_or_default())?;
            Ok(Exit(0))
        }
        Form::Section(s) => {
            let bad = misses(folded, doc);
            if bad.0 != 0 {
                writeln!(err, "{}", broken(bad))?;
                return Ok(Exit(1));
            }
            let Some(w) = window(text, &flat, s.anchor, s.before, s.after) else {
                writeln!(err, "the section is not found -- extraction is broken")?;
                return Ok(Exit(1));
            };
            writeln!(out, "{w}")?;
            Ok(Exit(0))
        }
    }
}

/// `--file PATH`, which only the word after the key can be.
fn file_argument<'a>(rest: &'a [&'a str]) -> Result<(Option<&'a str>, &'a [&'a str]), Refused> {
    match rest {
        ["--file", path, tail @ ..] => Ok((Some(path), tail)),
        ["--file"] => refused(Exit(2), "--file: a path is needed\n"),
        _ => Ok((None, rest)),
    }
}

fn located(root: &Path, doc: &Document, given: Option<&str>) -> Result<PathBuf, Refused> {
    let manifest = fs::read_to_string(root.join(sources::MANIFEST)).map_err(|e| Refused {
        exit: Exit(2),
        message: format!("{}: {e}\n", sources::MANIFEST),
    })?;
    let entries = sources::line_reader(&manifest);
    let Some(entry) = entries.iter().find(|e| e.key == doc.key) else {
        return refused(
            Exit(2),
            format!("{} is not in {}\n", doc.key, sources::MANIFEST),
        );
    };
    let path = match given {
        Some(p) => PathBuf::from(p),
        None if entry.file.is_empty() => {
            return refused(
                Exit(2),
                format!(
                    "{} is {}: not in the repository; name a copy with --file PATH\n",
                    doc.key, entry.status
                ),
            );
        }
        None => root.join(sources::SRC).join(&entry.file),
    };
    if !path.exists() {
        return refused(Exit(2), format!("missing {}\n", path.display()));
    }
    let sha = sha256::file(&path).map_err(|e| Refused {
        exit: Exit(2),
        message: format!("{}: {e}\n", path.display()),
    })?;
    if sha != entry.sha {
        return refused(
            Exit(2),
            format!(
                "{} is not the document {} pins: sha256 {sha}, pinned {}\n",
                path.display(),
                doc.key,
                entry.sha
            ),
        );
    }
    Ok(path)
}

/// The command. Every refusal says why on stderr.
pub fn run(
    root: &Path,
    args: &[&str],
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<Exit> {
    let Some((key, rest)) = args.split_first() else {
        err.write_all(USAGE.as_bytes())?;
        return Ok(Exit(2));
    };
    let Some(doc) = DOCUMENTS.iter().find(|d| d.key == *key) else {
        writeln!(
            err,
            "{key}: no extraction method or positive control is recorded for it; \
             a search without a control proves nothing, so add a row to DOCUMENTS"
        )?;
        return Ok(Exit(2));
    };
    let attempt = file_argument(rest).and_then(|(given, tail)| {
        let form = parse(doc, tail)?;
        if form == Form::Help {
            return Ok((form, None));
        }
        located(root, doc, given).map(|p| (form, Some(p)))
    });
    let (form, path) = match attempt {
        Ok(found) => found,
        Err(r) => {
            err.write_all(r.message.as_bytes())?;
            return Ok(r.exit);
        }
    };
    let Some(path) = path else {
        return search(doc, "", &[], &form, out, err);
    };
    let data = fs::read(&path)?;
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    search(doc, &name, &data, &form, out, err)
}
