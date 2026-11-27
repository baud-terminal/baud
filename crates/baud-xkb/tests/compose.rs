//! **libxkbcommon is the oracle for Compose too.** Each `oracle-*.txt`
//! under `tests/fixtures/compose/` is `tools/baud-oracle`'s `baud-oracle
//! xkb compose` record of libxkbcommon 1.13.2 reading one Compose file
//! from a buffer, under the environment `baud-oracle xkb` gives it: every
//! entry its iterator yields, then traces fed to its state machine --
//! each sequence and a keysym after it, all but its last keysym and then
//! one that continues nothing, and for every sixteenth a Shift_L fed
//! after the first keysym -- with the status after each feed and, where
//! one composed, the keysym and the text. The two system tables are
//! libX11 1.8.13's `en_US.UTF-8` and `el_GR.UTF-8`, pinned in
//! `docs/research/sources/`; the small files beside the oracles are the
//! grammar's corners, the includes and the ways a file fails.

use baud_xkb::Keysym;
use baud_xkb::compose::{self, Compose, Fed, Files, NoFiles, Status, Table};
use std::fs;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compose")
}

fn source(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/research/sources")
        .join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn syms(s: &str) -> Vec<Keysym> {
    s.split(',')
        .map(|h| Keysym(u32::from_str_radix(h, 16).unwrap()))
        .collect()
}

/// The oracle's record: `None` for "table none".
struct Oracle {
    entries: Option<Vec<String>>,
    traces: Vec<(Vec<Keysym>, String)>,
}

fn oracle(name: &str) -> Oracle {
    let p = dir().join(format!("oracle-{name}.txt"));
    let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut entries = Some(Vec::new());
    let mut traces = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line == "table none" {
            entries = None;
        } else if let Some(e) = line.strip_prefix("e ") {
            entries.as_mut().unwrap().push(e.to_string());
        } else if let Some(t) = line.strip_prefix("t ") {
            let (feeds, results) = t.split_once(" = ").unwrap();
            traces.push((syms(feeds), results.to_string()));
        }
    }
    Oracle { entries, traces }
}

/// An entry as the oracle writes it.
fn entry(seq: &[Keysym], c: &compose::Composed) -> String {
    let seq: Vec<String> = seq.iter().map(|k| format!("{:x}", k.0)).collect();
    let text = c.text.as_deref().unwrap_or("");
    let u8: String = text.bytes().map(|b| format!("{b:02x}")).collect();
    format!("{} {:x} {u8}", seq.join(","), c.keysym.0)
}

/// A trace's results as the oracle writes them.
fn trace(t: &Table, feeds: &[Keysym]) -> String {
    let mut st = Compose::default();
    let mut out = Vec::new();
    for &k in feeds {
        let fed = st.feed(t, k);
        let s = st.status(t);
        let mut r = String::from(match fed {
            Fed::Ignored => "I",
            Fed::Accepted => "A",
        });
        r.push(match s {
            Status::Nothing => 'n',
            Status::Composing => 'g',
            Status::Composed => 'd',
            Status::Cancelled => 'x',
        });
        if s == Status::Composed {
            let c = st.composed(t).unwrap();
            let u8: String = c.utf8().bytes().map(|b| format!("{b:02x}")).collect();
            r.push_str(&format!(":{:x}:{u8}", c.keysym.0));
        }
        out.push(r);
    }
    out.join(" ")
}

/// Every entry and every trace, against the oracle's.
fn check(name: &str, table: Result<Table, compose::Error>, at_least: usize) {
    let o = oracle(name);
    let Some(want) = o.entries else {
        assert!(table.is_err(), "{name}: the library made no table; got one");
        return;
    };
    let t = table.unwrap_or_else(|e| panic!("{name}: the library made a table; got {e:?}"));
    let got: Vec<String> = t.entries().iter().map(|(s, c)| entry(s, c)).collect();
    assert!(want.len() >= at_least, "{name}: the oracle is whole");
    let missing: Vec<&String> = want.iter().filter(|w| !got.contains(w)).collect();
    let extra: Vec<&String> = got.iter().filter(|g| !want.contains(g)).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{name}: {} entries the library has and this lacks, {} it has that the library lacks:\n{:?}\n{:?}",
        missing.len(),
        extra.len(),
        &missing[..missing.len().min(20)],
        &extra[..extra.len().min(20)],
    );
    assert_eq!(got, want, "{name}: the entries, in the iterator's order");
    let wrong: Vec<String> = o
        .traces
        .iter()
        .filter_map(|(feeds, want)| {
            let got = trace(&t, feeds);
            (got != *want).then(|| format!("{feeds:x?}: want {want}, got {got}"))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{name}: {} of {} traces disagree:\n{}",
        wrong.len(),
        o.traces.len(),
        wrong[..wrong.len().min(20)].join("\n")
    );
}

#[test]
fn the_en_us_table_agrees_with_the_library() {
    let t = Table::parse(
        &source("libx11-1.8.13-compose-en_US.UTF-8.txt"),
        &mut NoFiles,
    );
    check("en_US.UTF-8", t, 5000);
}

#[test]
fn the_greek_table_agrees_with_the_library() {
    let t = Table::parse(
        &source("libx11-1.8.13-compose-el_GR.UTF-8.txt"),
        &mut NoFiles,
    );
    check("el_GR.UTF-8", t, 1900);
}

/// The fixtures' X locale directory and home, as the oracle ran with
/// `XLOCALEDIR` and `HOME` set to them and the locale `xx`.
struct Fixtures {
    home: Option<PathBuf>,
}

impl Files for Fixtures {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        fs::read(path).ok()
    }
    fn home(&self) -> Option<String> {
        self.home.as_ref().map(|h| h.display().to_string())
    }
    fn locale_file(&mut self) -> Option<String> {
        let x = dir().join("xlocale");
        let alias = fs::read(x.join("locale.alias")).ok()?;
        let locale = compose::paths::resolve(&alias, compose::paths::Match::Left, "xx")
            .unwrap_or_else(|| "xx".into());
        let reg = fs::read(x.join("compose.dir")).ok()?;
        let f = compose::paths::locale_file(&reg, &locale, &|_| false)?;
        Some(match Path::new(&f).is_absolute() {
            true => f,
            false => x.join(f).display().to_string(),
        })
    }
    fn system_dir(&self) -> String {
        dir().join("xlocale").display().to_string()
    }
}

fn fixture(name: &str, home: Option<PathBuf>) -> Result<Table, compose::Error> {
    let text = fs::read(dir().join(format!("{name}.Compose"))).unwrap();
    let mut f = Fixtures { home };
    Table::parse(&text, &mut f)
}

/// **The grammar's corners**: duplicates and redefinitions, a sequence
/// that extends or is cut short by another, modifiers read and unused,
/// every escape, `U` and `0x` names either side of their bounds, the
/// `XF86_` retry, a modifier keysym inside a sequence, nine errors and a
/// tenth on the last line, which has no newline -- and three includes,
/// `%S`, `%H` and `%L`, the last redefining what came before it.
#[test]
fn the_grammars_corners_agree_with_the_library() {
    check("edge", fixture("edge", Some(dir().join("home"))), 30);
    check("edge2", fixture("edge2", Some(dir().join("home"))), 4);
}

/// **And the ways a file makes no table**: an include nested past five,
/// an include that cannot be read, eleven errors where ten still make one,
/// a first byte that is not ASCII, a NUL in the first two; a byte-order
/// mark is skipped, and `%H` without a home is one error, not a failure.
#[test]
fn the_ways_a_file_fails_agree_with_the_library() {
    for name in ["depth", "missing", "eleven", "ten", "latin1", "nul", "bom"] {
        check(name, fixture(name, Some(dir().join("home"))), 0);
    }
    check("home-unset", fixture("home-unset", None), 1);
}

/// **A dead key and the letter after it compose**, and the machine walks
/// back to the top: the user's view of what the oracle checks in bulk.
/// `dead_acute` then `a` is `á`; `dead_acute` then F35 cancels, and the
/// `a` after the cancel is a plain `a` the table does not know; Shift held
/// between `dead_acute` and `A` is ignored, so `Á`.
#[test]
fn a_dead_key_then_a_letter_composes() {
    let t = Table::parse(
        &source("libx11-1.8.13-compose-en_US.UTF-8.txt"),
        &mut NoFiles,
    )
    .unwrap();
    let mut st = Compose::default();
    let k = |n: &str| Keysym::from_xkb_name(n).unwrap();
    assert_eq!(st.feed(&t, k("dead_acute")), Fed::Accepted);
    assert_eq!(st.status(&t), Status::Composing);
    st.feed(&t, k("a"));
    assert_eq!(st.status(&t), Status::Composed);
    assert_eq!(st.composed(&t).unwrap().utf8(), "á");
    st.feed(&t, k("dead_acute"));
    st.feed(&t, k("F35"));
    assert_eq!(st.status(&t), Status::Cancelled);
    st.feed(&t, k("a"));
    assert_eq!(st.status(&t), Status::Nothing);
    st.feed(&t, k("dead_acute"));
    assert_eq!(st.feed(&t, k("Shift_L")), Fed::Ignored);
    st.feed(&t, k("A"));
    assert_eq!(st.composed(&t).unwrap().utf8(), "Á");
    st.feed(&t, k("Multi_key"));
    st.feed(&t, k("o"));
    st.feed(&t, k("c"));
    assert_eq!(st.composed(&t).unwrap().utf8(), "©");
}

/// `compose.dir` and `locale.alias` read as `resolve_name` reads them:
/// the first matching line, an optional colon, comments skipped; `C` is
/// `en_US.UTF-8`, and a locale the registry lacks falls back to it when
/// the system has the locale and makes no table when it does not, as
/// libxkbcommon's `paths.c` asks `newlocale`.
#[test]
fn the_registry_names_a_locales_file() {
    let reg = b"# comment\nen_US.UTF-8/Compose:\t\ten_US.UTF-8\nel_GR.UTF-8/Compose:\tel_GR.UTF-8\n/abs/Compose\t\txx_YY.UTF-8\nfirst/Compose: el_GR.UTF-8\n";
    let installed = |l: &str| l == "fr_FR.UTF-8";
    let f = |l: &str| compose::paths::locale_file(reg, l, &installed);
    assert_eq!(f("el_GR.UTF-8").as_deref(), Some("el_GR.UTF-8/Compose"));
    assert_eq!(f("C").as_deref(), Some("en_US.UTF-8/Compose"));
    assert_eq!(f("fr_FR.UTF-8").as_deref(), Some("en_US.UTF-8/Compose"));
    assert_eq!(f("tlh_KX.UTF-8"), None, "no such locale: no table");
    assert_eq!(f("xx_YY.UTF-8").as_deref(), Some("/abs/Compose"));
    let none = compose::paths::locale_file(
        b"el_GR.UTF-8/Compose: el_GR.UTF-8\n",
        "fr_FR.UTF-8",
        &installed,
    );
    assert_eq!(none, None, "a fallback the registry lacks is no table");
    let alias = b"# x\nru_RU.utf8\t\tru_RU.UTF-8\nen_US: en_US.ISO8859-1\n";
    let a = |l: &str| compose::paths::resolve(alias, compose::paths::Match::Left, l);
    assert_eq!(a("ru_RU.utf8").as_deref(), Some("ru_RU.UTF-8"));
    assert_eq!(a("en_US").as_deref(), Some("en_US.ISO8859-1"));
    assert_eq!(a("de_DE"), None);
}
