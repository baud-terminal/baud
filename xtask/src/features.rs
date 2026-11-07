//! `cargo xtask features [--check]`: docs/guide/features.md and
//! docs/roadmap.md, written from docs/data/features.toml and
//! docs/data/roadmap.toml.
//!
//! ```text
//! cargo xtask features           write both pages
//! cargo xtask features --check   fail if anything below is wrong,
//!                                both pages included
//! ```
//!
//! Of the twelve terminals surveyed for this, five keep a second feature
//! list by hand -- iTerm2, contour, rio, Windows Terminal and Wave -- and
//! all five have drifted from their programs. These two pages are
//! generated instead, and the data under them is checked against the
//! tree. The checks, each of which has to be able to fail:
//!
//! 1. every feature names its proof, and a feature marked `built` names at
//!    least one test, a coverage row that is done, or a measurement in
//!    MEASUREMENTS.md;
//! 2. every proof path exists, and every measurement a feature cites is a
//!    heading of MEASUREMENTS.md;
//! 3. every proof test is a `fn` in the file it names, so a renamed or
//!    deleted test is caught where the claim points;
//! 4. every coverage row a feature cites is a row of that table, and a
//!    `built` feature's rows are all done;
//! 5. every protocol chip is a protocol the data defines;
//! 6. every roadmap id a feature or gap names exists and is not done yet;
//! 7. every done roadmap item is claimed by a feature, or listed as not a
//!    feature with the reason, so a built feature cannot be missing from
//!    the list;
//! 8. the count README.md links the list with ("See all N features") is
//!    the number of features the data holds;
//! 9. both pages are byte for byte what this module writes.
//!
//! The data files' headers say what each field means. Every file is read
//! as Python's text layer read it for the script this replaced
//! (`text::read_universal`), so a page checked out with `\r\n` endings
//! passes as it did, and each failure line is the line it printed, but
//! for the command the stale line names. Where the script stopped on a
//! `KeyError` or a `TypeError` -- a feature with no `id`, a test that is
//! not a string -- the run fails with a line naming it.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use toml::{Table, Value};

use crate::html::Count;
use crate::third_party::{self, Outcome};
use crate::{py, text};

pub const FEATURES: &str = "docs/data/features.toml";
pub const ROADMAP: &str = "docs/data/roadmap.toml";
pub const FEATURES_MD: &str = "docs/guide/features.md";
pub const ROADMAP_MD: &str = "docs/roadmap.md";
pub const README: &str = "README.md";
pub const MEASUREMENTS: &str = "MEASUREMENTS.md";
/// The coverage tables a `rows` entry names by its prefix.
pub const TABLES: [(&str, &str); 2] = [
    ("vt", "docs/vt-coverage.md"),
    ("input", "docs/input-coverage.md"),
];

/// The command the stale line tells a reader to run.
pub const GENERATOR: &str = "cargo xtask features";

const STATUS: [&str; 3] = ["built", "partial", "interim"];
const PLATFORM: [&str; 3] = ["both", "macos", "linux"];
const ROAD_STATUS: [&str; 3] = ["in_progress", "planned", "done"];
const ROW_WORDS: [&str; 6] = ["done", "partial", "out", "todo", "parsed", "counted"];

/// Write the pages, or check them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Write,
    Check,
}

/// What stopped a run before its checks finished: the exception the
/// script raised, as a line.
type Stop = String;

/// `repr()` of a tuple of strings, as an f-string prints `STATUS`.
fn tuple(xs: &[&str]) -> String {
    let inner: Vec<String> = xs.iter().map(|s| py::repr_str(s)).collect();
    format!("({})", inner.join(", "))
}

/// Python's truth value of what `.get(key)` returned.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Integer(n)) => *n != 0,
        Some(Value::Float(f)) => *f != 0.0,
        Some(Value::Boolean(b)) => *b,
        Some(Value::Datetime(_)) => true,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Table(t)) => !t.is_empty(),
    }
}

/// `str()` of a value.
fn s(v: &Value) -> String {
    py::str_of(Some(v))
}

/// A value as a key of a Python `set` or `dict`: two values are one key
/// when their `repr` is the same.
fn key(v: &Value) -> String {
    py::repr(Some(v))
}

/// `t[k]`, or the `KeyError` Python stopped on.
fn at<'a>(t: &'a Table, k: &str) -> Result<&'a Value, Stop> {
    t.get(k)
        .ok_or_else(|| format!("KeyError: {}", py::repr_str(k)))
}

/// `t[k]` as a string, for a string method or a dictionary lookup.
fn at_str<'a>(t: &'a Table, k: &str) -> Result<&'a str, Stop> {
    match at(t, k)? {
        Value::String(v) => Ok(v),
        other => Err(format!(
            "TypeError: {k} is {}, not a string",
            py::repr(Some(other))
        )),
    }
}

/// What `for x in v` walks: an array's items, a string's characters, a
/// table's keys.
fn items(v: &Value) -> Result<Vec<Value>, Stop> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        Value::String(st) => Ok(st.chars().map(|c| Value::String(c.to_string())).collect()),
        Value::Table(t) => Ok(t.keys().map(|k| Value::String(k.clone())).collect()),
        other => Err(format!(
            "TypeError: {} is not iterable",
            py::repr(Some(other))
        )),
    }
}

/// `for x in t.get(k, [])`.
fn get_items(t: &Table, k: &str) -> Result<Vec<Value>, Stop> {
    t.get(k).map_or_else(|| Ok(Vec::new()), items)
}

/// Each item of `t[k]` (or of `t.get(k, [])` with `missing`) as a table.
fn tables<'a>(t: &'a Table, k: &str, missing: Missing) -> Result<Vec<&'a Table>, Stop> {
    let v = match (t.get(k), missing) {
        (Some(v), _) => v,
        (None, Missing::Empty) => return Ok(Vec::new()),
        (None, Missing::KeyError) => return Err(format!("KeyError: {}", py::repr_str(k))),
    };
    match v {
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::Table(t) => Ok(t),
                other => Err(format!(
                    "TypeError: an item of {k} is {}, not a table",
                    py::repr(Some(other))
                )),
            })
            .collect(),
        other => Err(format!(
            "TypeError: {k} is {}, not a list of tables",
            py::repr(Some(other))
        )),
    }
}

/// What a lookup of an absent key did: `t[k]` or `t.get(k, [])`.
#[derive(Clone, Copy)]
enum Missing {
    KeyError,
    Empty,
}

/// A string from a value Python called a string method on.
fn string(v: &Value) -> Result<&str, Stop> {
    match v {
        Value::String(st) => Ok(st),
        other => Err(format!(
            "AttributeError: {} is not a string",
            py::repr(Some(other))
        )),
    }
}

/// `x.partition(sep)`: before, whether `sep` was there, after.
fn partition<'a>(x: &'a str, sep: &str) -> (&'a str, bool, &'a str) {
    match x.split_once(sep) {
        Some((a, b)) => (a, true, b),
        None => (x, false, ""),
    }
}

static NOT_SLUG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}_\- ]").expect("NOT_SLUG"));
/// `See all (\S+) features`, with Python's `\S`: `str.isspace()` also
/// holds for the four information separators.
static SEE_ALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"See all ([^\s\x1c-\x1f]+) features").expect("SEE_ALL"));

/// GitHub's anchor for a heading: lower case, punctuation dropped, each
/// space a hyphen.
pub fn slug(heading: &str) -> String {
    let h = text::strip(heading).to_lowercase().replace('`', "");
    NOT_SLUG.replace_all(&h, "").replace(' ', "-")
}

/// The text of a heading line that `#{2,4} (.*)` matches at its start.
fn table_heading(line: &str) -> Option<&str> {
    let n = line.len() - line.trim_start_matches('#').len();
    let rest = &line[n..];
    ((2..=4).contains(&n) && rest.starts_with(' ')).then(|| &rest[1..])
}

/// The text of a heading line that `^#+ (.*)$` matches.
fn any_heading(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    (rest.len() < line.len() && rest.starts_with(' ')).then(|| &rest[1..])
}

/// The status word `\**([a-z]+)` reads at the start of a cell, if it is
/// one of `ROW_WORDS`.
fn row_word(cell: &str) -> Option<&'static str> {
    let rest = cell.trim_start_matches('*');
    let end = rest
        .find(|c: char| !c.is_ascii_lowercase())
        .unwrap_or(rest.len());
    ROW_WORDS.iter().copied().find(|w| *w == &rest[..end])
}

/// One coverage table's row: its status word and the anchor of the
/// heading above it.
#[derive(Clone, Debug)]
struct Row {
    word: String,
    anchor: String,
}

/// Each coverage table's rows by first cell, and the `table:cell`s two rows
/// of one table share, which a citation could not tell apart.
struct Coverage {
    rows: HashMap<String, HashMap<String, Row>>,
    dupes: HashSet<String>,
}

impl Coverage {
    fn row(&self, t: &str, cell: &str) -> Option<&Row> {
        self.rows.get(t).and_then(|r| r.get(cell))
    }

    fn read(root: &Path) -> Result<Coverage, Stop> {
        let mut out = HashMap::new();
        let mut dupes = HashSet::new();
        for (k, path) in TABLES {
            let mut rows: HashMap<String, Row> = HashMap::new();
            let mut anchor = String::new();
            // How many headings before this one slugged the same.
            let mut seen: HashMap<String, Count> = HashMap::new();
            for line in read(root, path)?.split('\n') {
                if let Some(h) = table_heading(line) {
                    let a = slug(h);
                    let n = seen.entry(a.clone()).or_insert(Count(0));
                    anchor = if n.0 == 0 { a } else { format!("{a}-{}", n.0) };
                    n.0 += 1;
                    continue;
                }
                if !line.starts_with("| ") || line.starts_with("|---") {
                    continue;
                }
                let inner = text::strip(line).trim_matches('|');
                let cells: Vec<&str> = inner.split(" | ").map(text::strip).collect();
                if cells.len() < 2 {
                    continue;
                }
                let word = cells[1..]
                    .iter()
                    .find_map(|c| row_word(c))
                    .unwrap_or("")
                    .to_string();
                if rows.contains_key(cells[0]) {
                    dupes.insert(format!("{k}:{}", cells[0]));
                }
                rows.insert(
                    cells[0].to_string(),
                    Row {
                        word,
                        anchor: anchor.clone(),
                    },
                );
            }
            out.insert(k.to_string(), rows);
        }
        Ok(Coverage { rows: out, dupes })
    }
}

/// A file under `root` as the script read it, or the error it stopped on.
fn read(root: &Path, path: &str) -> Result<String, Stop> {
    text::read_universal(&root.join(path)).map_err(|e| format!("{path}: {e}"))
}

/// The names `\bfn ([A-Za-z0-9_]+)` finds, with Python's word boundary.
pub fn fns(src: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("fn ").map(|i| i + from) {
        let boundary = src[..i].chars().next_back().is_none_or(|c| !py::word(c));
        let rest = &src[i + 3..];
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if boundary && len > 0 {
            out.insert(rest[..len].to_string());
            from = i + 3 + len;
        } else {
            from = i + 1;
        }
    }
    out
}

/// Everything the checks read besides the two data files.
struct Tree<'a> {
    root: &'a Path,
    coverage: Coverage,
    headings: HashSet<String>,
    fns: HashMap<String, HashSet<String>>,
}

impl Tree<'_> {
    fn has_fn(&mut self, path: &str, name: &str) -> Result<bool, Stop> {
        if !self.fns.contains_key(path) {
            let src = read(self.root, path)?;
            self.fns.insert(path.to_string(), fns(&src));
        }
        Ok(self.fns[path].contains(name))
    }

    fn is_file(&self, p: &str) -> bool {
        self.root.join(p).is_file()
    }
}

/// The data, read and checked: the failure lines, or what stopped it.
fn check_data(tree: &mut Tree, feat: &Table, road: &Table) -> Result<Vec<String>, Stop> {
    let mut errs = Vec::new();
    let areas: Vec<String> = tables(feat, "area", Missing::KeyError)?
        .iter()
        .map(|a| at(a, "id").map(key))
        .collect::<Result<_, _>>()?;
    let protocol = tables(feat, "protocol", Missing::Empty)?;
    let protocols: HashSet<String> = protocol
        .iter()
        .map(|p| at(p, "name").map(key))
        .collect::<Result<_, _>>()?;
    let mut road_items: HashMap<String, &Table> = HashMap::new();
    let items = tables(road, "item", Missing::Empty)?;
    for i in &items {
        road_items.insert(key(at(i, "id")?), i);
    }
    let mut ids = HashSet::new();
    for p in &protocol {
        let (t, _, cell) = partition(string(at(p, "row")?)?, ":");
        if tree.coverage.row(t, cell).is_none() {
            errs.push(format!("protocol {}: no {t} row {cell}", s(at(p, "name")?)));
        }
    }
    let roadmap_refs = |errs: &mut Vec<String>, at_: &str, refs: &[Value]| -> Result<(), Stop> {
        for r in refs {
            match road_items.get(&key(r)) {
                None => errs.push(format!("{at_}: roadmap id {} is not in {ROADMAP}", s(r))),
                Some(i) if at(i, "status")? == &Value::String("done".into()) => errs.push(format!(
                    "{at_}: roadmap id {} is done, so it closes no gap",
                    s(r)
                )),
                Some(_) => {}
            }
        }
        Ok(())
    };
    let built = Value::String("built".into());
    // `claimed` keeps the order its keys were first claimed in.
    let mut claimed: Vec<(String, Value)> = Vec::new();
    for f in tables(feat, "feature", Missing::KeyError)? {
        let wh = format!("feature {}", f.get("id").map_or_else(|| "?".to_string(), s));
        for k in ["id", "area", "name", "does", "status", "platform"] {
            if !truthy(f.get(k)) {
                errs.push(format!("{wh}: no {k}"));
            }
        }
        let id = at(f, "id")?;
        if !ids.insert(key(id)) {
            errs.push(format!("{wh}: id used twice"));
        }
        let area = at(f, "area")?;
        if !areas.contains(&key(area)) {
            errs.push(format!("{wh}: area {} is not defined", s(area)));
        }
        let status = at(f, "status")?;
        if !STATUS.iter().any(|x| status == &Value::String((*x).into())) {
            errs.push(format!(
                "{wh}: status {} is not one of {}",
                s(status),
                tuple(&STATUS)
            ));
        }
        let platform = at(f, "platform")?;
        if !PLATFORM
            .iter()
            .any(|x| platform == &Value::String((*x).into()))
        {
            errs.push(format!(
                "{wh}: platform {} is not one of {}",
                s(platform),
                tuple(&PLATFORM)
            ));
        }
        let is_built = status == &built;
        if !is_built && !truthy(f.get("gap")) {
            errs.push(format!(
                "{wh}: a {} feature says what is missing in `gap`",
                s(status)
            ));
        }
        if is_built && truthy(f.get("gap")) {
            errs.push(format!("{wh}: a built feature has no gap"));
        }
        for p in get_items(f, "paths")? {
            let p = string(&p)?;
            if !tree.is_file(p) {
                errs.push(format!("{wh}: proof path {p} does not exist"));
            }
        }
        let tests = f.get("tests");
        for t in get_items(f, "tests")? {
            let t = string(&t)?;
            let (path, sep, name) = partition(t, "::");
            if !sep {
                errs.push(format!("{wh}: test {t} is not written <path>::<fn>"));
            } else if !tree.is_file(path) {
                errs.push(format!("{wh}: test file {path} does not exist"));
            } else if !tree.has_fn(path, name)? {
                errs.push(format!("{wh}: {path} has no fn {name}"));
            }
        }
        let mut done_rows = false;
        for r in get_items(f, "rows")? {
            let r = string(&r)?;
            let (t, _, cell) = partition(r, ":");
            if tree.coverage.dupes.contains(r) {
                errs.push(format!(
                    "{wh}: two {t} rows begin {cell}, so the citation is ambiguous"
                ));
            }
            let Some(row) = tree.coverage.row(t, cell) else {
                errs.push(format!("{wh}: no {t} coverage row {cell}"));
                continue;
            };
            if row.word == "done" {
                done_rows = true;
            } else if is_built {
                let w = if row.word.is_empty() {
                    "unmarked"
                } else {
                    &row.word
                };
                errs.push(format!("{wh}: built, but its row {cell} is {w}"));
            }
        }
        let measured = f.get("measured");
        for m in get_items(f, "measured")? {
            let found = matches!(&m, Value::String(h) if tree.headings.contains(h));
            if !found {
                errs.push(format!(
                    "{wh}: {MEASUREMENTS} has no heading {}",
                    py::repr(Some(&m))
                ));
            }
        }
        if !truthy(tests) && !truthy(f.get("rows")) && !truthy(f.get("paths")) && !truthy(measured)
        {
            errs.push(format!("{wh}: names no proof"));
        }
        if is_built && !truthy(tests) && !done_rows && !truthy(measured) {
            errs.push(format!(
                "{wh}: built, and names no test, no done row and no measurement"
            ));
        }
        for c in get_items(f, "chips")? {
            if !protocols.contains(&key(&c)) {
                errs.push(format!("{wh}: chip {} is not a defined protocol", s(&c)));
            }
        }
        roadmap_refs(&mut errs, &wh, &get_items(f, "roadmap")?)?;
        for sh in get_items(f, "shipped")? {
            let k = key(&sh);
            if !claimed.iter().any(|(c, _)| *c == k) {
                claimed.push((k, sh));
            }
        }
    }
    for g in tables(feat, "missing", Missing::Empty)? {
        let wh = format!(
            "missing {}",
            g.get("name").map_or_else(|| "?".to_string(), s)
        );
        let area = g.get("area");
        if !area.is_some_and(|a| areas.contains(&key(a))) {
            errs.push(format!("{wh}: area {} is not defined", py::str_of(area)));
        }
        if !truthy(g.get("roadmap")) {
            errs.push(format!("{wh}: names no roadmap item"));
        }
        roadmap_refs(&mut errs, &wh, &get_items(g, "roadmap")?)?;
    }
    // `{u["id"]: u}`: a key at its first place, holding its last entry.
    let mut unlisted: Vec<(String, &Table)> = Vec::new();
    for u in tables(feat, "unlisted", Missing::Empty)? {
        let k = key(at(u, "id")?);
        match unlisted.iter_mut().find(|(c, _)| *c == k) {
            Some(slot) => slot.1 = u,
            None => unlisted.push((k, u)),
        }
    }
    for (_, u) in &unlisted {
        if !truthy(u.get("why")) {
            errs.push(format!("unlisted {}: no reason", s(at(u, "id")?)));
        }
    }
    let done = Value::String("done".into());
    for i in &items {
        let wh = format!("roadmap {}", i.get("id").map_or_else(|| "?".to_string(), s));
        let status = i.get("status");
        if !status.is_some_and(|st| {
            ROAD_STATUS
                .iter()
                .any(|x| st == &Value::String((*x).into()))
        }) {
            errs.push(format!(
                "{wh}: status {} is not one of {}",
                py::str_of(status),
                tuple(&ROAD_STATUS)
            ));
        }
        let area = i.get("area");
        if !area.is_some_and(|a| areas.contains(&key(a))) {
            errs.push(format!("{wh}: area {} is not defined", py::str_of(area)));
        }
        if status == Some(&done) {
            let k = key(at(i, "id")?);
            let c = claimed.iter().any(|(x, _)| *x == k);
            let u = unlisted.iter().any(|(x, _)| *x == k);
            if c && u {
                errs.push(format!(
                    "{wh}: both claimed by a feature and listed as not a feature"
                ));
            } else if !c && !u {
                errs.push(format!(
                    "{wh}: done, and no feature claims it in `shipped` -- a built \
                     feature missing from the list, or an `unlisted` entry with its reason"
                ));
            }
        }
    }
    let named: Vec<(&String, String)> = claimed
        .iter()
        .map(|(k, v)| (k, s(v)))
        .chain(
            unlisted
                .iter()
                .map(|(k, u)| (k, u.get("id").map(s).unwrap_or_default())),
        )
        .collect();
    for (k, shown) in named {
        match road_items.get(k) {
            None => errs.push(format!(
                "shipped or unlisted id {shown} is not in {ROADMAP}"
            )),
            Some(i) if at(i, "status")? != &done => {
                errs.push(format!("shipped or unlisted id {shown} is not done"));
            }
            Some(_) => {}
        }
    }
    Ok(errs)
}

/// README.md's count of the list, which no generator writes.
fn check_readme(root: &Path, feat: &Table) -> Result<Vec<String>, Stop> {
    let n = match feat.get("feature") {
        None => 0,
        Some(v) => items(v)?.len(),
    };
    let readme = read(root, README)?;
    let said: Vec<&str> = SEE_ALL
        .captures_iter(&readme)
        .map(|c| c.get(1).map_or("", |m| m.as_str()))
        .collect();
    if said.is_empty() {
        return Ok(vec![format!(
            "{README}: no 'See all N features' link, so its count is unchecked"
        )]);
    }
    Ok(said
        .iter()
        .filter(|x| **x != n.to_string())
        .map(|x| format!("{README}: says 'See all {x} features', the list has {n}"))
        .collect())
}

/// `str(t[k]).strip()`.
fn stripped(t: &Table, k: &str) -> Result<String, Stop> {
    Ok(text::strip(string(at(t, k)?)?).to_string())
}

fn platform_mark(f: &Table) -> Result<&'static str, Stop> {
    Ok(match at_str(f, "platform")? {
        "macos" => " *macOS.*",
        "linux" => " *Linux.*",
        _ => "",
    })
}

/// The link to a coverage row, `../<table>#<anchor>`.
fn row_link(cov: &Coverage, k: &str) -> Result<String, Stop> {
    let (t, _, cell) = partition(k, ":");
    let file = TABLES
        .iter()
        .find(|(n, _)| *n == t)
        .map(|(_, p)| p.rsplit('/').next().unwrap_or(p))
        .ok_or_else(|| format!("KeyError: {}", py::repr_str(t)))?;
    let row = cov
        .row(t, cell)
        .ok_or_else(|| format!("KeyError: {}", py::repr_str(cell)))?;
    Ok(format!("../{file}#{}", row.anchor))
}

/// The file a feature's proof link opens: its first test's, else its
/// first path.
fn first_test_file(f: &Table) -> Result<Option<String>, Stop> {
    if let Some(t) = get_items(f, "tests")?.first() {
        return Ok(Some(partition(string(t)?, "::").0.to_string()));
    }
    if let Some(p) = get_items(f, "paths")?.first() {
        return Ok(Some(s(p)));
    }
    Ok(None)
}

fn render_features(feat: &Table, cov: &Coverage) -> Result<String, Stop> {
    let area = tables(feat, "area", Missing::KeyError)?;
    let features = tables(feat, "feature", Missing::KeyError)?;
    let protocol = tables(feat, "protocol", Missing::Empty)?;
    let protocols: HashMap<String, &Table> = protocol
        .iter()
        .map(|p| Ok((key(at(p, "name")?), *p)))
        .collect::<Result<_, Stop>>()?;
    let mut road_anchor: HashMap<String, String> = HashMap::new();
    for a in &area {
        road_anchor.insert(key(at(a, "id")?), slug(at_str(a, "title")?));
    }
    let mut out: Vec<String> = vec![
        "# Features".into(),
        String::new(),
        stripped(feat, "intro")?,
        String::new(),
        stripped(feat, "legend")?,
        String::new(),
    ];
    let nav: Vec<String> = area
        .iter()
        .map(|a| {
            let t = at_str(a, "title")?;
            Ok(format!("[{t}](#{})", slug(t)))
        })
        .collect::<Result<_, Stop>>()?;
    out.push(nav.join(" · ") + " · [Where baud is behind](#where-baud-is-behind)");
    out.push(String::new());
    for a in &area {
        let id = at(a, "id")?;
        out.push(format!("## {}", s(at(a, "title")?)));
        out.push(String::new());
        if truthy(a.get("intro")) {
            out.push(stripped(a, "intro")?);
            out.push(String::new());
        }
        for f in features.iter().filter(|f| f.get("area") == Some(id)) {
            let mut line = format!(
                "- **{}** — {}{}",
                s(at(f, "name")?),
                stripped(f, "does")?,
                platform_mark(f)?
            );
            match at_str(f, "status")? {
                "partial" => line += &format!(" **Partly:** {}", stripped(f, "gap")?),
                "interim" => line += &format!(" **For now:** {}", stripped(f, "gap")?),
                _ => {}
            }
            let mut small = Vec::new();
            for c in get_items(f, "chips")? {
                let p = protocols
                    .get(&key(&c))
                    .ok_or_else(|| format!("KeyError: {}", py::repr(Some(&c))))?;
                small.push(format!(
                    "[{}]({})",
                    s(&c),
                    row_link(cov, at_str(p, "row")?)?
                ));
            }
            if let Some(p) = first_test_file(f)?.filter(|p| !p.is_empty()) {
                small.push(format!("[proof](../../{p})"));
            }
            if let Some(m) = get_items(f, "measured")?.first() {
                small.push(format!(
                    "[measured](../../{MEASUREMENTS}#{})",
                    slug(string(m)?)
                ));
            }
            if truthy(f.get("rows")) {
                let first = get_items(f, "rows")?;
                small.push(format!(
                    "[reference]({})",
                    row_link(cov, string(&first[0])?)?
                ));
            }
            line += &format!(" <sub>{}</sub>", small.join(" · "));
            out.push(line);
        }
        out.push(String::new());
    }
    out.push("## Protocols".into());
    out.push(String::new());
    out.push(stripped(feat, "protocols_intro")?);
    out.push(String::new());
    out.push("| protocol | from | reference |".into());
    out.push("|---|---|---|".into());
    for p in &protocol {
        out.push(format!(
            "| {} | {} | [in the coverage table]({}) |",
            s(at(p, "name")?),
            s(at(p, "origin")?),
            row_link(cov, at_str(p, "row")?)?
        ));
    }
    out.push(String::new());
    out.push("## Where baud is behind".into());
    out.push(String::new());
    out.push(stripped(feat, "behind_intro")?);
    out.push(String::new());
    let anchor = |t: &Table| -> Result<String, Stop> {
        let a = at(t, "area")?;
        road_anchor
            .get(&key(a))
            .cloned()
            .ok_or_else(|| format!("KeyError: {}", py::repr(Some(a))))
    };
    for f in &features {
        if truthy(f.get("behind")) {
            let tail = if truthy(f.get("roadmap")) {
                format!(" [Roadmap](../roadmap.md#{})", anchor(f)?)
            } else {
                String::new()
            };
            out.push(format!(
                "- **{}** — {}{tail}",
                s(at(f, "name")?),
                stripped(f, "behind")?
            ));
        }
    }
    for g in tables(feat, "missing", Missing::Empty)? {
        out.push(format!(
            "- **{}** — {} [Roadmap](../roadmap.md#{})",
            s(at(g, "name")?),
            stripped(g, "does")?,
            anchor(g)?
        ));
    }
    out.push(String::new());
    out.push("## How this page stays true".into());
    out.push(String::new());
    out.push(stripped(feat, "footer")?);
    Ok(out.join("\n") + "\n")
}

/// The order an item sorts in: by `(order[status], title)` for an open
/// one, and by title, then by date, newest first, for a done one.
fn sort_key(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        other => s(other),
    }
}

fn render_roadmap(feat: &Table, road: &Table) -> Result<String, Stop> {
    let area = tables(feat, "area", Missing::KeyError)?;
    let all = tables(road, "item", Missing::Empty)?;
    let mut out: Vec<String> = vec![
        "# Roadmap".into(),
        String::new(),
        stripped(road, "intro")?,
        String::new(),
    ];
    // Only the areas the page has a heading for: an area with no roadmap
    // item gets none, and a link to it would go nowhere.
    let mut listed = Vec::new();
    for a in &area {
        let id = at(a, "id")?;
        let mut any = false;
        for i in &all {
            if at(i, "area")? == id {
                any = true;
                break;
            }
        }
        if any {
            let t = at_str(a, "title")?;
            listed.push(format!("[{t}](#{})", slug(t)));
        }
    }
    out.push(listed.join(" · "));
    out.push(String::new());
    let order = |st: &str| -> Result<Order, Stop> {
        match st {
            "in_progress" => Ok(Order::InProgress),
            "planned" => Ok(Order::Planned),
            "done" => Ok(Order::Done),
            other => Err(format!("KeyError: {}", py::repr_str(other))),
        }
    };
    for a in &area {
        let id = at(a, "id")?;
        let mut items = Vec::new();
        for i in &all {
            if at(i, "area")? == id {
                items.push(*i);
            }
        }
        if items.is_empty() {
            continue;
        }
        let mut open = Vec::new();
        let mut done = Vec::new();
        for i in items {
            let st = at_str(i, "status")?;
            let title = sort_key(at(i, "title")?);
            if st == "done" {
                done.push((title, sort_key(at(i, "date")?), i));
            } else {
                open.push(((order(st)?, title), i));
            }
        }
        open.sort_by(|x, y| x.0.cmp(&y.0));
        done.sort_by(|x, y| x.0.cmp(&y.0));
        // `reverse=True` keeps equal dates in the order the title sort left.
        done.sort_by(|x, y| y.1.cmp(&x.1));
        out.push(format!("## {}", s(at(a, "title")?)));
        out.push(String::new());
        for i in open.iter().map(|o| o.1).chain(done.iter().map(|d| d.2)) {
            let txt = stripped(i, "summary")?;
            let title = s(at(i, "title")?);
            match at_str(i, "status")? {
                "done" => out.push(format!("- [x] **{title}** — {txt} ({})", s(at(i, "date")?))),
                "in_progress" => out.push(format!("- [ ] **{title}** — {txt} *In progress.*")),
                _ => out.push(format!("- [ ] **{title}** — {txt}")),
            }
        }
        out.push(String::new());
    }
    out.push("## How this page stays true".into());
    out.push(String::new());
    out.push(stripped(road, "footer")?);
    Ok(out.join("\n") + "\n")
}

/// An open item's place: in progress, then planned.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Order {
    InProgress,
    Planned,
    Done,
}

/// `cargo xtask features [--check]`, run against `root`; what the script
/// printed to stderr goes to `err`.
pub fn run(root: &Path, mode: Mode, err: &mut impl Write) -> io::Result<Outcome> {
    let stop = |err: &mut dyn Write, e: Stop| -> io::Result<Outcome> {
        writeln!(err, "{e}")?;
        Ok(Outcome::Failed)
    };
    let feat = match third_party::load(root, FEATURES) {
        Ok(t) => t,
        Err(e) => return stop(err, e),
    };
    let road = match third_party::load(root, ROADMAP) {
        Ok(t) => t,
        Err(e) => return stop(err, e),
    };
    let cov = match Coverage::read(root) {
        Ok(c) => c,
        Err(e) => return stop(err, e),
    };
    let headings = match read(root, MEASUREMENTS) {
        Ok(m) => m
            .split('\n')
            .filter_map(any_heading)
            .map(|h| text::strip(h).to_string())
            .collect(),
        Err(e) => return stop(err, e),
    };
    let mut tree = Tree {
        root,
        coverage: cov,
        headings,
        fns: HashMap::new(),
    };
    let errs = match check_data(&mut tree, &feat, &road).and_then(|mut e| {
        check_readme(root, &feat).map(|r| {
            e.extend(r);
            e
        })
    }) {
        Ok(e) => e,
        Err(e) => return stop(err, e),
    };
    if !errs.is_empty() {
        writeln!(err, "{} problem(s) in {FEATURES} / {ROADMAP}:", errs.len())?;
        for e in &errs {
            writeln!(err, "  {e}")?;
        }
        return Ok(Outcome::Failed);
    }
    // Both rendered before either is written, as the script built its
    // dictionary of pages first.
    let rendered = match render_features(&feat, &tree.coverage)
        .and_then(|f| render_roadmap(&feat, &road).map(|r| [(FEATURES_MD, f), (ROADMAP_MD, r)]))
    {
        Ok(r) => r,
        Err(e) => return stop(err, e),
    };
    let mut stale = Vec::new();
    for (path, page) in rendered {
        let full = root.join(path);
        match mode {
            Mode::Check => {
                let have = if full.exists() {
                    text::read_universal(&full)?
                } else {
                    String::new()
                };
                if have != page {
                    stale.push(path);
                }
            }
            Mode::Write => fs::write(&full, page)?,
        }
    }
    if !stale.is_empty() {
        writeln!(
            err,
            "stale: {} -- edit the data under docs/data/ and rerun {GENERATOR}; \
             a generated page is never edited by hand",
            stale.join(", ")
        )?;
        return Ok(Outcome::Failed);
    }
    Ok(Outcome::Passed)
}
