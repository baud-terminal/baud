//! The Script property, its resolution in context (UAX #24), and the
//! OpenType Script table that selects (`[[source: ms-opentype-chapter2]]`).

use baud_domain::Codepoint;
use baud_font::{Face, Features, Script, ScriptTag, Unresolved, resolve};

fn cp(c: char) -> Codepoint {
    Codepoint::from_char(c)
}

fn code(s: Script) -> String {
    String::from_utf8(s.code().to_vec()).unwrap()
}

/// The committed tables match the pinned Unicode data.
#[test]
fn script_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-script-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "script_data.rs is stale -- rerun scripts/gen-script-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **Every code point's Script is the pinned file's**, read here from the
/// file itself rather than from the generator, so a trie that indexes
/// wrongly cannot agree with the generator that built it.
#[test]
fn every_code_point_has_the_script_scripts_txt_gives_it() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    let aliases =
        std::fs::read_to_string(format!("{dir}/unicode-propertyvaluealiases.txt")).unwrap();
    let mut short = std::collections::HashMap::new();
    for line in aliases.lines() {
        let f: Vec<&str> = line
            .split('#')
            .next()
            .unwrap()
            .split(';')
            .map(str::trim)
            .collect();
        if f.first() == Some(&"sc") && f.len() >= 3 {
            short.insert(f[2].to_string(), f[1].to_string());
        }
    }
    let mut want = vec!["Zzzz".to_string(); 0x11_0000];
    let scripts = std::fs::read_to_string(format!("{dir}/unicode-scripts.txt")).unwrap();
    let mut listed = 0usize;
    for line in scripts.lines() {
        let body = line.split('#').next().unwrap().trim();
        if body.is_empty() {
            continue;
        }
        let (range, name) = body.split_once(';').unwrap();
        let (lo, hi) = match range.trim().split_once("..") {
            Some((a, b)) => (a, b),
            None => (range.trim(), range.trim()),
        };
        let (lo, hi) = (
            usize::from_str_radix(lo, 16).unwrap(),
            usize::from_str_radix(hi, 16).unwrap(),
        );
        for w in &mut want[lo..=hi] {
            *w = short[name.trim()].clone();
            listed += 1;
        }
    }
    let mut checked = 0usize;
    for (v, w) in want.iter().enumerate() {
        let Some(c) = Codepoint::new(u32::try_from(v).unwrap()) else {
            continue;
        };
        assert_eq!(&code(Script::of(c)), w, "U+{v:04X}");
        checked += 1;
    }
    // Positive control: the file was read, and named most of the space it
    // assigns; surrogates are not code points `Codepoint` accepts.
    assert!(listed > 150_000, "{listed}");
    assert_eq!(checked, 1_112_064, "every scalar value");
}

#[test]
fn known_characters_have_their_scripts_and_tags() {
    let tags = |s: Script| s.tags().map(|t| t.0).collect::<Vec<_>>();
    assert_eq!(code(Script::of(cp('a'))), "Latn");
    assert_eq!(code(Script::of(cp('א'))), "Hebr");
    // UAX #24 5.2: "the Hebrew marks and accents are used only with Hebrew
    // characters and are therefore assigned the Hebrew Script property".
    assert_eq!(code(Script::of(cp('\u{5B8}'))), "Hebr");
    assert_eq!(Script::of(cp('\u{301}')), Script::INHERITED);
    assert_eq!(Script::of(cp('1')), Script::COMMON);
    assert_eq!(Script::of(Codepoint::new(0x0378).unwrap()), Script::UNKNOWN);
    assert_eq!(tags(Script::of(cp('א'))), [*b"hebr"]);
    assert_eq!(tags(Script::of(cp('a'))), [*b"latn"]);
    // The registry's two Devanagari tags, the newer model's first; the
    // shared kana tag; Yi's padded one.
    assert_eq!(tags(Script::of(cp('क'))), [*b"dev2", *b"deva"]);
    assert_eq!(tags(Script::of(cp('あ'))), [*b"kana"]);
    assert_eq!(tags(Script::of(cp('ア'))), [*b"kana"]);
    assert_eq!(tags(Script::of(cp('ꀀ'))), [*b"yi  "]);
    assert!(tags(Script::COMMON).is_empty());
    assert!(tags(Script::INHERITED).is_empty());
    assert_eq!(Script::all().count(), Script::COUNT);
}

/// Settles `text`, one cluster per character, to ISO codes.
fn settled(text: &str) -> Vec<String> {
    let cells: Vec<Unresolved> = text.chars().map(|c| Unresolved::of(&[cp(c)])).collect();
    let mut out = vec![Script::COMMON; cells.len()];
    resolve(&cells, &mut out);
    out.into_iter().map(code).collect()
}

#[test]
fn common_characters_take_their_context_script() {
    // The preceding script (UAX #24 5.1), and the following one for what
    // leads a line.
    assert_eq!(settled("a-b"), ["Latn"; 3]);
    assert_eq!(settled("-> a"), ["Latn"; 4]);
    assert_eq!(settled("שלום, 123"), ["Hebr"; 9]);
    assert_eq!(settled("ab אב"), ["Latn", "Latn", "Latn", "Hebr", "Hebr"]);
    // A line of nothing but Common stays Common, and selects DFLT.
    assert_eq!(settled("123 ->"), ["Zyyy"; 6]);
    // Script_Extensions limit it (5.3, Example 2): the prolonged sound mark
    // "should not continue a Latin script run".
    assert_eq!(settled("aー"), ["Latn", "Hira"]);
    assert_eq!(settled("aーア"), ["Latn", "Kana", "Kana"]);
    assert_eq!(settled("アー"), ["Kana", "Kana"]);
    // The Arabic comma joins Arabic and not Latin.
    assert_eq!(settled("a\u{60C}"), ["Latn", "Arab"]);
}

#[test]
fn a_cluster_is_in_the_script_of_its_first_character_that_names_one() {
    // UAX #24 5.2: a dotted circle carrying a Hebrew point is Hebrew, and a
    // Latin letter carrying one stays Latin.
    let of = |s: &str| code(Script::of_cluster(&s.chars().map(cp).collect::<Vec<_>>()));
    assert_eq!(of("\u{25CC}\u{5B8}"), "Hebr");
    assert_eq!(of("a\u{5B8}"), "Latn");
    assert_eq!(of("e\u{301}"), "Latn");
    assert_eq!(of("1\u{301}"), "Zyyy");
}

fn face_file(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// **The Script table is the text's own, else `DFLT`, else `latn`.**
#[test]
fn text_selects_its_own_script_table_else_dflt() {
    let chosen = |data: &[u8], c: Script| {
        let f = Face::parse(data).unwrap();
        (
            f.gsub().and_then(|g| g.chosen(c)),
            f.gpos().and_then(|g| g.chosen(c)),
        )
    };
    let hebrew = Script::of(cp('א'));
    let latin = Script::of(cp('a'));
    let both = |t: &[u8; 4]| (Some(ScriptTag(*t)), Some(ScriptTag(*t)));
    // The built JetBrains Mono carries a `hebr` record (note AC).
    let built = face_file("baud-app/fonts/built/Regular.ttf");
    assert_eq!(chosen(&built, hebrew), both(b"hebr"));
    assert_eq!(chosen(&built, latin), both(b"latn"));
    assert_eq!(chosen(&built, Script::COMMON), both(b"DFLT"));
    // Stock JetBrains Mono has no `hebr`: Hebrew text takes `DFLT`, not
    // `latn` -- the order baud used before it read a script.
    let stock = face_file("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    assert_eq!(chosen(&stock, hebrew), both(b"DFLT"));
    // Fira Code has Greek and Cyrillic tables.
    let fira = face_file("baud-font/tests/fonts/FiraCode-Regular.ttf");
    assert_eq!(chosen(&fira, Script::of(cp('α'))), both(b"grek"));
    assert_eq!(chosen(&fira, Script::of(cp('ж'))), both(b"cyrl"));
}

/// **A face with neither the script's table nor `DFLT` falls to `latn`**:
/// Courier New lists `arab`, `hebr` and `latn` only, so digits take `latn`,
/// which is also HarfBuzz's last resort. A macOS system face.
#[test]
fn a_face_with_no_dflt_falls_back_to_latn() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Courier New is a macOS system face");
        return;
    }
    let data = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf").unwrap();
    let f = Face::parse(&data).unwrap();
    let gsub = f.gsub().unwrap();
    assert_eq!(gsub.chosen(Script::COMMON), Some(ScriptTag::LATN));
    assert_eq!(gsub.chosen(Script::of(cp('א'))), Some(ScriptTag(*b"hebr")));
    // And the two select different lookups, which is what cuts a run.
    assert_ne!(
        gsub.lookups(Script::COMMON, Features::GRID),
        gsub.lookups(Script::of(cp('א')), Features::GRID)
    );
}
