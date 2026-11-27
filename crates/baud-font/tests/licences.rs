//! **Every font in the tree is either baud's own or has its row and its own
//! licence beside it.**
//!
//! The SIL Open Font License 1.1, condition 5: "The Font Software, modified or
//! unmodified, in part or in whole, must be distributed entirely under this
//! license, and must not be distributed under any other license." So a
//! third-party font keeps that licence, its text in the font's directory, and
//! a `[[tree]]` row of `docs/research/third-party.toml` says so. OFL condition
//! 2 wants the licence with each copy: `var/bake/` held four instances of
//! JetBrains Mono and no licence until this test said so.
//!
//! The fonts baud wrote from scratch are listed by path with the script or
//! test that writes each, and have a row under `own`, so a new font is a
//! decision someone has to make here rather than a file nobody has classified.

use std::fs;
use std::path::{Path, PathBuf};

mod sha256;
use sha256::sha256_hex;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Fonts baud wrote from nothing, each with the script, tool or test that
/// writes it -- or, for fontTools' instances of baud's bake cases, the test
/// that records how they were observed. They are under the repository's
/// licence
/// (`crates/baud-font/tests/fonts/sbix-LICENSE.txt` says so for the one that sits
/// among third-party faces).
const OWN: &[(&str, &str)] = &[
    (
        "crates/baud-font/tests/fonts/sbix-strikes.ttf",
        "crates/baud-font/tests/bitmap.rs",
    ),
    (
        "crates/baud-font/tests/bitmap/cblc-cases.ttf",
        "crates/baud-font/tests/cblc.rs",
    ),
    (
        "crates/baud-font/tests/var/gvar-cases.ttf",
        "tools/baud-oracle/src/cases/gvar.rs",
    ),
    (
        "crates/baud-font/tests/var/ink-cases.ttf",
        "tools/baud-oracle/src/cases/ink.rs",
    ),
    (
        "crates/baud-font/tests/var/component-cases.ttf",
        "tools/baud-oracle/src/cases/component.rs",
    ),
    (
        "crates/baud-font/tests/var/bearing-cases.ttf",
        "tools/baud-oracle/src/cases/bearing.rs",
    ),
    (
        "crates/baud-font/tests/var/bearing-cases-bit1.ttf",
        "tools/baud-oracle/src/cases/bearing.rs",
    ),
    (
        "crates/baud-font/tests/var/vertical-cases.ttf",
        "tools/baud-oracle/src/cases/vertical.rs",
    ),
    (
        "crates/baud-font/tests/var/colour-refused-cases.ttf",
        "tools/baud-oracle/src/cases/colour_refused.rs",
    ),
    (
        "crates/baud-font/tests/var/budget-cases.ttf",
        "tools/baud-oracle/src/cases/budget.rs",
    ),
    (
        "crates/baud-font/tests/var/joining-cases.ttf",
        "tools/baud-oracle/src/cases/joining.rs",
    ),
    (
        "crates/baud-font/tests/var/cff2-cases.otf",
        "tools/baud-oracle/src/cases/cff2.rs",
    ),
    (
        "crates/baud-font/tests/var/bake/cases.ttf",
        "tools/baud-oracle/src/cases/bake.rs",
    ),
    (
        "crates/baud-font/tests/var/bake/cases-250.ttf",
        "crates/baud-font/tests/bake.rs",
    ),
    (
        "crates/baud-font/tests/var/bake/cases-550.ttf",
        "crates/baud-font/tests/bake.rs",
    ),
    (
        "crates/baud-font/tests/var/bake/cases-700.ttf",
        "crates/baud-font/tests/bake.rs",
    ),
    (
        "crates/baud-font/tests/var/bake/cases-875.ttf",
        "crates/baud-font/tests/bake.rs",
    ),
];

/// `crates/baud-font/tests/var/hvar/` is written whole by one case.
const OWN_DIR: (&str, &str) = (
    "crates/baud-font/tests/var/hvar/",
    "tools/baud-oracle/src/cases/hvar.rs",
);

fn fonts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            fonts(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x == "ttf" || x == "otf" || x == "ttc")
        {
            out.push(p);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root().canonicalize().unwrap())
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_own(path: &str) -> bool {
    OWN.iter().any(|(p, _)| *p == path) || path.starts_with(OWN_DIR.0)
}

/// One `[[tree]]` row of `docs/research/third-party.toml`: what it says about
/// the paths it lists.
struct Row {
    status: String,
    paths: Vec<String>,
}

/// The sidecar's `[[tree]]` rows, read line by line: `paths` is an array of
/// quoted strings on one line or across several.
fn rows() -> Vec<Row> {
    let text = fs::read_to_string(root().join("docs/research/third-party.toml"))
        .expect("third-party.toml");
    let quoted = |s: &str| -> Vec<String> {
        s.split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    };
    let mut out: Vec<Row> = Vec::new();
    let mut in_paths = false;
    let mut in_tree = false;
    for line in text.lines() {
        if line.starts_with('[') && !in_paths {
            in_tree = line == "[[tree]]";
            if in_tree {
                out.push(Row {
                    status: String::new(),
                    paths: Vec::new(),
                });
            }
            continue;
        }
        let Some(row) = out.last_mut().filter(|_| in_tree) else {
            continue;
        };
        if in_paths {
            row.paths.extend(quoted(line));
            in_paths = !line.trim_end().ends_with(']');
        } else if line.starts_with("paths") {
            let value = line.split_once('=').map_or("", |(_, v)| v);
            row.paths.extend(quoted(value));
            in_paths = !line.trim_end().ends_with(']');
        } else if line.starts_with("status") {
            row.status = quoted(line).into_iter().next().unwrap_or_default();
        }
    }
    // The control: a reader that stopped matching would find no row, and
    // every font would be wrong or, under a loosened test, none.
    assert!(out.len() >= 70, "only {} rows read", out.len());
    assert!(
        out.iter()
            .all(|r| !r.status.is_empty() && !r.paths.is_empty()),
        "a row read with no status or no paths"
    );
    out
}

/// The status of each row that lists `path`.
fn statuses<'a>(rows: &'a [Row], path: &str) -> Vec<&'a str> {
    rows.iter()
        .filter(|r| r.paths.iter().any(|p| p == path))
        .map(|r| r.status.as_str())
        .collect()
}

/// The licence a third-party font carries beside it: in `crates/baud-font/tests`,
/// `<Family>-LICENSE.txt`, the family being the file name up to its first
/// `-` (the convention `oracle.rs` already holds the top directory to); in
/// `crates/baud-app/fonts`, the directory's `OFL.txt` or `LICENSE`.
fn licence_beside(p: &Path) -> Option<PathBuf> {
    let dir = p.parent()?;
    let path = rel(p);
    if path.starts_with("crates/baud-font/tests/") {
        let stem = p.file_stem()?.to_string_lossy().into_owned();
        let family = stem.split('-').next()?.to_string();
        let lic = dir.join(format!("{family}-LICENSE.txt"));
        return lic.exists().then_some(lic);
    }
    ["OFL.txt", "LICENSE"]
        .iter()
        .map(|n| dir.join(n))
        .find(|l| l.exists())
}

#[test]
fn every_third_party_font_has_its_row_and_its_licence_beside_it() {
    let root = root().canonicalize().unwrap();
    let rows = rows();
    let mut all = Vec::new();
    for d in ["crates/baud-font/tests", "crates/baud-app/fonts"] {
        let dir = root.join(d);
        assert!(
            dir.is_dir(),
            "{d} does not exist -- the walk would see nothing"
        );
        fonts(&dir, &mut all);
    }
    all.sort();
    // The control: a walk that stopped matching would find nothing and pass.
    assert!(all.len() > 30, "only {} fonts found", all.len());

    let mut wrong = Vec::new();
    for p in &all {
        let path = rel(p);
        let statuses = statuses(&rows, &path);
        if is_own(&path) {
            if statuses.as_slice() != ["own"] {
                wrong.push(format!(
                    "{path} is baud's own: it needs one row, under `own`, and has {statuses:?}"
                ));
            }
            continue;
        }
        if statuses.is_empty() {
            wrong.push(format!(
                "{path} is a third-party font and no row of third-party.toml names it"
            ));
        } else if statuses.contains(&"own") {
            wrong.push(format!(
                "{path} is not listed as baud's own, and a row calls it `own`"
            ));
        }
        match licence_beside(p) {
            Some(l) => {
                let text = fs::read_to_string(&l).unwrap_or_default();
                if !(text.contains("SIL OPEN FONT LICENSE Version 1.1")
                    || text.contains("MIT License"))
                {
                    wrong.push(format!(
                        "{path}: {} is neither the OFL 1.1 nor MIT",
                        rel(&l)
                    ));
                }
            }
            None => wrong.push(format!("{path} has no licence beside it")),
        }
    }
    assert!(
        wrong.is_empty(),
        "{} font(s) wrong:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// The list of baud's own fonts names files and scripts that exist, so it
/// cannot go stale and quietly exempt nothing -- or a font that is gone.
#[test]
fn every_font_listed_as_bauds_own_exists_with_its_generator() {
    let root = root();
    for (font, script) in OWN.iter().chain([&OWN_DIR]) {
        assert!(
            root.join(font).exists(),
            "{font} is listed as own and is gone"
        );
        assert!(
            root.join(script).exists(),
            "{font}'s generator {script} is gone"
        );
    }
}

/// Where the icon sets' licence texts sit, beside Symbols Nerd Font Mono. The
/// font carries icon sets under their own licences, and the OFL (condition
/// 2), Apache 2.0 (section 4(a)), CC BY 4.0 and MIT each want their text with
/// every copy -- which the font's own MIT `LICENSE` is not.
const ICON_LICENCES: &str = "crates/baud-app/fonts/symbols-nerd-font/icon-licences";

/// The texts `icon-licences/` must hold: the eight upstream ships under
/// `src/glyphs/` at v3.5.1, the Apache 2.0 text the Material Design
/// notice names only by URL, and the six notices of the MIT sets that
/// nerd-fonts ships none for, each from its own project.
const ICON_TEXTS: &[&str] = &[
    "pomicons/LICENSE",
    "weather-icons/OFL.txt",
    "font-awesome/LICENSE.txt",
    "materialdesign/LICENSE",
    "codicons/LICENSE.txt",
    "octicons/LICENSE",
    "powerline-extra/LICENSE",
    "powerline-symbols/LICENSE.txt",
    "Apache-2.0.txt",
    "devicons/LICENSE",
    "devicons/vorillaz/README.txt",
    "extraglyphs/LICENSE.md",
    "font-awesome-extension/LICENCE",
    "seti-ui/LICENSE.md",
    "power-symbols-iec/LICENSE.txt",
];

fn files_under(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, base, out);
        } else {
            out.push(
                p.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

/// Each icon-set licence text is in `icon-licences/`, byte for byte what its
/// `SOURCE` records, nothing there is unrecorded, and the bundle copies the
/// directory beside the font.
#[test]
fn the_icon_sets_licences_travel_with_the_nerd_font() {
    let root = root();
    let dir = root.join(ICON_LICENCES);
    let source = fs::read_to_string(dir.join("SOURCE")).expect("icon-licences/SOURCE");
    // An entry is a path on a line of its own followed by indented `url` and
    // `sha256` lines.
    let lines: Vec<&str> = source.lines().collect();
    let mut pinned: Vec<(String, String)> = Vec::new();
    let mut current: Option<&str> = None;
    for (i, l) in lines.iter().enumerate() {
        let next_is_url = lines
            .get(i + 1)
            .is_some_and(|n| n.trim_start().starts_with("url "));
        if !l.starts_with(' ') && next_is_url {
            current = Some(l.trim());
        } else if let Some(hex) = l.trim_start().strip_prefix("sha256 ") {
            let path = current.take().expect("a sha256 line with no path above it");
            pinned.push((path.to_string(), hex.trim().to_string()));
        }
    }
    // The control: a parser that stopped matching would pin nothing and pass.
    assert_eq!(
        pinned.len(),
        ICON_TEXTS.len(),
        "SOURCE pins {} texts: {pinned:?}",
        pinned.len()
    );

    let mut wrong = Vec::new();
    for want in ICON_TEXTS {
        if !pinned.iter().any(|(p, _)| p == want) {
            wrong.push(format!("SOURCE records no {want}"));
        }
    }
    for (path, hex) in &pinned {
        match fs::read(dir.join(path)) {
            Ok(bytes) => {
                let got = sha256_hex(&bytes);
                if got != *hex {
                    wrong.push(format!("{path} hashes {got}, SOURCE says {hex}"));
                }
            }
            Err(_) => wrong.push(format!("{path} is recorded in SOURCE and missing")),
        }
    }
    let mut present = Vec::new();
    files_under(&dir, &dir, &mut present);
    for f in present.iter().filter(|f| *f != "SOURCE") {
        if !pinned.iter().any(|(p, _)| p == f) {
            wrong.push(format!("{f} is in icon-licences/ and not in its SOURCE"));
        }
    }
    let bundle =
        fs::read_to_string(root.join("tools/scripts/bundle-macos.sh")).expect("bundle-macos.sh");
    if !bundle.contains("symbols-nerd-font/icon-licences") {
        wrong.push(
            "tools/scripts/bundle-macos.sh does not copy icon-licences/ into the bundle".into(),
        );
    }
    let rows = rows();
    for f in &present {
        if statuses(&rows, &format!("{ICON_LICENCES}/{f}")).is_empty() {
            wrong.push(format!("icon-licences/{f} has no row in third-party.toml"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} icon licence problem(s):\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// **A readout of a third-party font has a licence row.** The oracle bins
/// (`tools/baud-oracle`) write what HarfBuzz reads out of the test fonts
/// into `var/*.txt`, one font per line in the first column. A readout of a
/// third-party font is named by a `[[tree]]` row of
/// `docs/research/third-party.toml`; one that is not is third-party data
/// nobody has classified. Only the top-level `var/*.txt`
/// readouts are scanned; `var/bake/`, `var/hvar/` and `tests/shape/*.txt`
/// are not.
#[test]
fn every_readout_of_a_third_party_font_has_a_licence_row() {
    let root = root().canonicalize().unwrap();
    let rows = rows();
    let mut all = Vec::new();
    fonts(&root.join("crates/baud-font/tests"), &mut all);
    let third: Vec<String> = all
        .iter()
        .filter(|p| !is_own(&rel(p)))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // The control: a walk that found no third-party font would pass.
    assert!(third.len() >= 8, "only {} third-party fonts", third.len());

    let var = root.join("crates/baud-font/tests/var");
    let mut readouts = Vec::new();
    for e in fs::read_dir(&var).expect("var/").flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "txt") {
            continue;
        }
        let text = fs::read_to_string(&p).unwrap_or_default();
        let reads = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_whitespace().next())
            .filter_map(|f| f.rsplit('/').next())
            .find(|f| third.iter().any(|t| t == f))
            .map(str::to_string);
        if let Some(font) = reads {
            readouts.push((rel(&p), font));
        }
    }
    readouts.sort();
    // The control: advances, marks, normalize and the two outline files read
    // third-party fonts; a parser that stopped matching would find none.
    assert!(
        readouts.len() >= 5,
        "only {} readouts of third-party fonts: {readouts:?}",
        readouts.len()
    );

    let mut wrong = Vec::new();
    for (path, font) in &readouts {
        if statuses(&rows, path).is_empty() {
            wrong.push(format!(
                "{path} reads {font} and no row of third-party.toml names it"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} readout(s) unclassified:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}
