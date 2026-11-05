//! **Every font in the tree is either baud's own or carved out of the
//! repository's licence, with its own licence beside it.**
//!
//! The SIL Open Font License 1.1, condition 5: "The Font Software, modified or
//! unmodified, in part or in whole, must be distributed entirely under this
//! license, and must not be distributed under any other license." The
//! repository's `LICENSE` is CC BY-NC-SA 4.0, so a third-party font it does
//! not name as an exception is one it claims to license otherwise -- which
//! `baud-app/fonts/built/FONTLOG.txt` recorded as a push blocker. And OFL
//! condition 2 wants the licence with each copy: `var/bake/` held four
//! instances of JetBrains Mono and no licence until this test said so.
//!
//! The fonts baud wrote from scratch are listed by path with the script or
//! test that writes each, so a new font is a decision someone has to make
//! here rather than a file that slips under the blanket licence.

use std::fs;
use std::path::{Path, PathBuf};

mod sha256;
use sha256::sha256_hex;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// Fonts baud wrote from nothing, each with the script, tool or test that
/// writes it -- or, for fontTools' instances of baud's bake cases, the test
/// that records how they were observed. They are under the repository's
/// licence
/// (`baud-font/tests/fonts/sbix-LICENSE.txt` says so for the one that sits
/// among third-party faces).
const OWN: &[(&str, &str)] = &[
    (
        "baud-font/tests/fonts/sbix-strikes.ttf",
        "baud-font/tests/bitmap.rs",
    ),
    (
        "baud-font/tests/bitmap/cblc-cases.ttf",
        "baud-font/tests/cblc.rs",
    ),
    (
        "baud-font/tests/var/gvar-cases.ttf",
        "scripts/gvar-cases-font.py",
    ),
    (
        "baud-font/tests/var/ink-cases.ttf",
        "scripts/ink-cases-font.py",
    ),
    (
        "baud-font/tests/var/component-cases.ttf",
        "tools/baud-oracle/src/cases/component.rs",
    ),
    (
        "baud-font/tests/var/bearing-cases.ttf",
        "tools/baud-oracle/src/cases/bearing.rs",
    ),
    (
        "baud-font/tests/var/bearing-cases-bit1.ttf",
        "tools/baud-oracle/src/cases/bearing.rs",
    ),
    (
        "baud-font/tests/var/vertical-cases.ttf",
        "scripts/vertical-cases-font.py",
    ),
    (
        "baud-font/tests/var/colour-refused-cases.ttf",
        "tools/baud-oracle/src/cases/colour_refused.rs",
    ),
    (
        "baud-font/tests/var/budget-cases.ttf",
        "tools/baud-oracle/src/cases/budget.rs",
    ),
    (
        "baud-font/tests/var/joining-cases.ttf",
        "scripts/joining-cases-font.py",
    ),
    (
        "baud-font/tests/var/cff2-cases.otf",
        "tools/baud-oracle/src/cases/cff2.rs",
    ),
    (
        "baud-font/tests/var/bake/cases.ttf",
        "tools/baud-oracle/src/cases/bake.rs",
    ),
    (
        "baud-font/tests/var/bake/cases-250.ttf",
        "baud-font/tests/bake.rs",
    ),
    (
        "baud-font/tests/var/bake/cases-550.ttf",
        "baud-font/tests/bake.rs",
    ),
    (
        "baud-font/tests/var/bake/cases-700.ttf",
        "baud-font/tests/bake.rs",
    ),
    (
        "baud-font/tests/var/bake/cases-875.ttf",
        "baud-font/tests/bake.rs",
    ),
];

/// `baud-font/tests/var/hvar/` is written whole by one script.
const OWN_DIR: (&str, &str) = ("baud-font/tests/var/hvar/", "scripts/hvar-cases-font.py");

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

/// The licence a third-party font carries beside it: in `baud-font/tests`,
/// `<Family>-LICENSE.txt`, the family being the file name up to its first
/// `-` (the convention `oracle.rs` already holds the top directory to); in
/// `baud-app/fonts`, the directory's `OFL.txt` or `LICENSE`.
fn licence_beside(p: &Path) -> Option<PathBuf> {
    let dir = p.parent()?;
    let path = rel(p);
    if path.starts_with("baud-font/tests/") {
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
fn every_third_party_font_is_carved_out_with_its_licence_beside_it() {
    let root = root().canonicalize().unwrap();
    let licence = fs::read_to_string(root.join("LICENSE")).expect("LICENSE");
    // A path or a directory LICENSE names on a line of its own. A substring
    // would not do: `baud-font/tests/fonts/` is the prefix of every test
    // font's path, baud's own among them.
    let lines: Vec<&str> = licence.lines().map(str::trim).collect();
    let mut all = Vec::new();
    for d in ["baud-font/tests", "baud-app/fonts"] {
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
        let dir = format!("{}/", rel(p.parent().unwrap()));
        let named = lines.iter().any(|l| *l == path || *l == dir);
        if is_own(&path) {
            if named {
                wrong.push(format!(
                    "{path} is baud's own and LICENSE carves it out of baud's licence"
                ));
            }
            continue;
        }
        if !named {
            wrong.push(format!(
                "{path} is a third-party font and LICENSE does not name it or {dir}"
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
const ICON_LICENCES: &str = "baud-app/fonts/symbols-nerd-font/icon-licences";

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
    let bundle = fs::read_to_string(root.join("scripts/bundle-macos.sh")).expect("bundle-macos.sh");
    if !bundle.contains("symbols-nerd-font/icon-licences") {
        wrong.push("scripts/bundle-macos.sh does not copy icon-licences/ into the bundle".into());
    }
    let licence = fs::read_to_string(root.join("LICENSE")).expect("LICENSE");
    if !licence.contains("icon-licences/") {
        wrong.push("LICENSE does not name icon-licences/".into());
    }
    assert!(
        wrong.is_empty(),
        "{} icon licence problem(s):\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// **A readout of a third-party font has a licence row.** The oracle scripts
/// (`scripts/*-oracle.py`) write what HarfBuzz reads out of the test fonts
/// into `var/*.txt`, one font per line in the first column. A readout of a
/// third-party font is named by a `[[tree]]` row of
/// `docs/research/third-party.toml`, or by LICENSE; one that is neither is
/// third-party data nobody has classified. Only the top-level `var/*.txt`
/// readouts are scanned; `var/bake/`, `var/hvar/` and `tests/shape/*.txt`
/// are not.
#[test]
fn every_readout_of_a_third_party_font_has_a_licence_row() {
    let root = root().canonicalize().unwrap();
    let licence = fs::read_to_string(root.join("LICENSE")).expect("LICENSE");
    let rows =
        fs::read_to_string(root.join("docs/research/third-party.toml")).expect("third-party.toml");
    let mut all = Vec::new();
    fonts(&root.join("baud-font/tests"), &mut all);
    let third: Vec<String> = all
        .iter()
        .filter(|p| !is_own(&rel(p)))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // The control: a walk that found no third-party font would pass.
    assert!(third.len() >= 8, "only {} third-party fonts", third.len());

    let var = root.join("baud-font/tests/var");
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

    let quoted = |path: &str| format!("\"{path}\"");
    let mut wrong = Vec::new();
    for (path, font) in &readouts {
        let row = rows
            .lines()
            .filter(|l| l.starts_with("paths"))
            .any(|l| l.contains(&quoted(path)));
        if !row && !licence.contains(path.as_str()) {
            wrong.push(format!(
                "{path} reads {font} and neither a third-party.toml row nor LICENSE names it"
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
