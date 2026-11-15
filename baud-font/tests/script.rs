//! The Script property, its resolution in context (UAX #24), and the
//! OpenType Script table that selects (`[[source: ms-opentype-chapter2]]`).

use baud_domain::Codepoint;
use baud_font::{
    Bracket, Direction, Face, Features, RunText, Script, ScriptTag, Unresolved, resolve,
};

fn cp(c: char) -> Codepoint {
    Codepoint::from_char(c)
}

fn code(s: Script) -> String {
    String::from_utf8(s.code().to_vec()).unwrap()
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
    // The built JetBrains Mono carries a `hebr` record.
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

/// **A script is right to left when every letter of it is Bidi_Class R or
/// AL**, read here from the pinned files rather than from the generator.
/// UAX #9 resolves such a letter to an odd level whatever the paragraph's
/// direction (`[[source: uax9-bidi]]`, W3 and I1-I2), which is what makes a
/// `morx` subtable's "layout order" the reverse of logical order on its
/// runs (`[[source: apple-ttrm-morx]]`). The set is HarfBuzz's
/// `hb_script_get_horizontal_direction` list (hb-common.cc:517-594), all 36,
/// and Old Hungarian, whose letters are R and which HarfBuzz leaves to the
/// caller as written either way.
#[test]
fn a_script_is_right_to_left_when_its_letters_are() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    let read = |name: &str| {
        let mut out = std::collections::HashMap::new();
        for line in std::fs::read_to_string(format!("{dir}/{name}"))
            .unwrap()
            .lines()
        {
            let line = line.split('#').next().unwrap().trim();
            let Some((range, value)) = line.split_once(';') else {
                continue;
            };
            let (lo, hi) = range
                .trim()
                .split_once("..")
                .unwrap_or((range.trim(), range.trim()));
            for v in u32::from_str_radix(lo, 16).unwrap()..=u32::from_str_radix(hi, 16).unwrap() {
                out.insert(v, value.trim().to_string());
            }
        }
        out
    };
    let gc = read("unicode-derivedgeneralcategory.txt");
    let bidi = read("unicode-derivedbidiclass.txt");
    let mut letters: std::collections::HashMap<String, (usize, usize)> = Default::default();
    for (&v, g) in &gc {
        if g.starts_with('L') {
            let s = code(Script::of(Codepoint::new(v).unwrap()));
            let e = letters.entry(s).or_default();
            e.0 += 1;
            e.1 += usize::from(matches!(bidi[&v].as_str(), "R" | "AL"));
        }
    }
    let mut rtl = Vec::new();
    for s in Script::all() {
        let (all, strong) = letters.get(&code(s)).copied().unwrap_or((0, 0));
        let want = if s.is_specific() && all > 0 && all == strong {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };
        assert_eq!(
            s.direction(),
            want,
            "{}: {strong} of {all} letters R or AL",
            code(s)
        );
        if want == Direction::RightToLeft {
            rtl.push(code(s));
        }
    }
    rtl.sort();
    let harfbuzz = [
        "Adlm", "Arab", "Armi", "Avst", "Chrs", "Cprt", "Elym", "Gara", "Hatr", "Hebr", "Hung",
        "Khar", "Lydi", "Mand", "Mani", "Mend", "Merc", "Mero", "Narb", "Nbat", "Nkoo", "Orkh",
        "Ougr", "Palm", "Phli", "Phlp", "Phnx", "Prti", "Rohg", "Samr", "Sarb", "Sidt", "Sogd",
        "Sogo", "Syrc", "Thaa", "Yezi",
    ];
    assert_eq!(rtl, harfbuzz, "HarfBuzz's 36 and Old Hungarian");
}

/// **A run of a right-to-left script that holds a number and no letter is
/// laid out left to right**: a European or Arabic digit (EN, AN) resolves
/// to an even level under UAX #9 (`[[source: uax9-bidi]]`, I1-I2) and a
/// regional indicator is Bidi_Class L. The rule takes every decimal digit
/// (gc Nd), as HarfBuzz's does, so the 20 of Bidi_Class R -- NKo's
/// U+07C0..07C9 and Adlam's U+1E950..1E959 -- are laid out left to right
/// too, where UAX #9 would level them odd; HarfBuzz applies the same rule when its caller asks for
/// left to right (`[[source: harfbuzz-hb-ot-shape]]`, hb-ot-shape.cc:613),
/// and CoreText lays a flag pair beside Hebrew out left to right. One letter
/// anywhere keeps the run right to left, and a left-to-right script's run
/// stays left to right whatever it holds.
#[test]
fn a_right_to_left_run_of_numbers_alone_is_left_to_right() {
    let run = |text: &str| {
        let mut t = RunText::EMPTY;
        for c in text.chars() {
            t.push(cp(c));
        }
        t
    };
    let hebr = Script::of(cp('\u{05D0}'));
    let arab = Script::of(cp('\u{0627}'));
    let latn = Script::of(cp('a'));
    assert_eq!(
        run("\u{05D1}\u{05BC}").direction(hebr),
        Direction::RightToLeft
    );
    assert_eq!(run("\u{05D0}12").direction(hebr), Direction::RightToLeft);
    assert_eq!(run("12").direction(hebr), Direction::LeftToRight);
    assert_eq!(
        run("\u{0661}\u{0662}").direction(arab),
        Direction::LeftToRight
    );
    assert_eq!(
        run("\u{0627}\u{0661}").direction(arab),
        Direction::RightToLeft
    );
    assert_eq!(
        run("\u{1F1EE}\u{1F1F1}").direction(arab),
        Direction::LeftToRight
    );
    // Neither a letter nor a number: the script decides.
    assert_eq!(run("\u{05BE} ").direction(hebr), Direction::RightToLeft);
    assert_eq!(RunText::EMPTY.direction(hebr), Direction::RightToLeft);
    assert_eq!(run("ab1").direction(latn), Direction::LeftToRight);
}

/// Settles `text` and groups it into runs of one script, as
/// `(text, ISO code)`.
fn runs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (c, s) in text.chars().zip(settled(text)) {
        match out.last_mut() {
            Some((t, last)) if *last == s => t.push(c),
            _ => out.push((c.to_string(), s)),
        }
    }
    out
}

fn run_list(r: &[(&str, &str)]) -> Vec<(String, String)> {
    r.iter()
        .map(|(t, s)| ((*t).to_string(), (*s).to_string()))
        .collect()
}

/// **A bracket pair takes the script of the text that encloses it**
/// (`[[source: uax24]]` 5.1): "paired punctuation, such as brackets or
/// quotation marks, belongs to the enclosing or outer level of the text
/// and should therefore match the script of the enclosing text. In
/// addition, opening and closing elements of a pair resolve to the same
/// Script property values, where possible". The preceding-character
/// heuristic alone made the closer take the script inside the pair --
/// the text's own example, "... gamma (γ) is ...", where it "would cause
/// matching parentheses to be in different scripts". A pair is UAX #9's
/// (`[[source: uax9-bidi]]`, BD16) over `[[source: unicode-bidibrackets]]`.
#[test]
fn a_bracket_pair_takes_the_enclosing_script() {
    // UAX #24's own example, and the character after the closer is back in
    // the enclosing text.
    assert_eq!(
        runs("gamma (γ) is"),
        run_list(&[("gamma (", "Latn"), ("γ", "Grek"), (") is", "Latn")])
    );
    // Inside Hebrew, Arabic and Greek.
    assert_eq!(
        runs("א(a)ב"),
        run_list(&[("א(", "Hebr"), ("a", "Latn"), (")ב", "Hebr")])
    );
    assert_eq!(
        runs("ب[a]ت"),
        run_list(&[("ب[", "Arab"), ("a", "Latn"), ("]ت", "Arab")])
    );
    assert_eq!(
        runs("α{a}β"),
        run_list(&[("α{", "Grek"), ("a", "Latn"), ("}β", "Grek")])
    );
    // A pair closing the line takes the enclosing script too.
    assert_eq!(
        runs("שלום (abc)"),
        run_list(&[("שלום (", "Hebr"), ("abc", "Latn"), (")", "Hebr")])
    );
    // Nested: the inner pair is enclosed by the Greek.
    assert_eq!(
        runs("a[α(β)γ]b"),
        run_list(&[("a[", "Latn"), ("α(β)γ", "Grek"), ("]b", "Latn")])
    );
    // A closer that matches deeper pops through the unclosed opener
    // (BD16 step 4), and so is the outer pair's.
    assert_eq!(
        runs("a(α[β)b"),
        run_list(&[("a(", "Latn"), ("α[β", "Grek"), (")b", "Latn")])
    );
    // U+2329 and U+3008 are canonically equivalent, as are U+232A and
    // U+3009, and BD16 pairs across them. U+3008 and U+3009 join only
    // the CJK scripts and Yi, so the enclosing text is Han.
    assert_eq!(
        runs("漢\u{2329}a\u{3009}"),
        run_list(&[("漢\u{2329}", "Hani"), ("a", "Latn"), ("\u{3009}", "Hani")])
    );
    assert_eq!(
        runs("漢\u{3008}a\u{232A}"),
        run_list(&[("漢\u{3008}", "Hani"), ("a", "Latn"), ("\u{232A}", "Hani")])
    );
    // An opener that leads the line takes what follows it, and its closer
    // takes the opener's script, not the one after the closer.
    assert_eq!(
        runs("(α a)b"),
        run_list(&[("(α ", "Grek"), ("a", "Latn"), (")", "Grek"), ("b", "Latn")])
    );
    // Where its Script_Extensions admit the opener's script: the corner
    // brackets join Han, Hiragana and Katakana and not Latin.
    assert_eq!(
        runs("漢「a」"),
        run_list(&[("漢「", "Hani"), ("a", "Latn"), ("」", "Hani")])
    );
    // A pair that leads the line, with other text inside before its
    // closer: what follows the closer continues the opener's script, as
    // the preceding-character heuristic has it.
    assert_eq!(
        runs("(שלום world)."),
        run_list(&[("(שלום ", "Hebr"), ("world", "Latn"), (").", "Hebr")])
    );
    assert_eq!(
        runs("[α a] b."),
        run_list(&[
            ("[α ", "Grek"),
            ("a", "Latn"),
            ("] ", "Grek"),
            ("b.", "Latn")
        ])
    );
    assert_eq!(
        runs("(α a) b"),
        run_list(&[
            ("(α ", "Grek"),
            ("a", "Latn"),
            (") ", "Grek"),
            ("b", "Latn")
        ])
    );
    assert_eq!(
        runs("(α a)(b)"),
        run_list(&[
            ("(α ", "Grek"),
            ("a", "Latn"),
            (")(", "Grek"),
            ("b", "Latn"),
            (")", "Grek")
        ])
    );
}

/// **A pair that leads the line settles what follows it as one that does
/// not.** After the closer the opener's script is the one before, under
/// the same Script_Extensions limit: the Devanagari danda cannot continue
/// Hebrew (`[[source: uax24]]` 5.3, example 2), and settles as it does
/// where a Latin letter leads the line. And where nothing settled is
/// inside the pair, the closer takes its opener's script and the text
/// after the closer follows it: the prolonged sound mark joins only
/// Hiragana and Katakana, so the opener does not take the Latin after.
#[test]
fn a_pair_that_leads_the_line_settles_what_follows_it_alike() {
    let led = settled("(שלום क)। x");
    let unled = settled("a (שלום क)। x");
    assert_eq!(&led[..6], ["Hebr"; 6], "{led:?}");
    assert_eq!(&led[6..8], ["Deva", "Hebr"], "{led:?}");
    assert_eq!(led[8], unled[10], "{led:?} {unled:?}");

    let s = settled("(ー). x");
    assert_ne!(s[0], "Latn", "{s:?}");
    assert_eq!(&s[..5], [s[0].as_str(); 5], "{s:?}");
    assert_eq!(s[5], "Latn");
}

/// **What is not a pair keeps the preceding script.** A closer with no
/// opener, a closer of another pair, a quotation mark -- "it is not
/// possible to tell from the character code alone whether a particular
/// quotation mark is used as an opening or closing punctuation"
/// (`[[source: uax24]]` 5.1), and BidiBrackets.txt lists none -- and a
/// bracket that names a script of its own.
#[test]
fn what_is_not_a_pair_keeps_the_preceding_script() {
    assert_eq!(runs("α)a"), run_list(&[("α)", "Grek"), ("a", "Latn")]));
    assert_eq!(
        runs("a(α]b"),
        run_list(&[("a(", "Latn"), ("α]", "Grek"), ("b", "Latn")])
    );
    assert_eq!(
        runs("a«α»b"),
        run_list(&[("a«", "Latn"), ("α»", "Grek"), ("b", "Latn")])
    );
    // Tibetan's brackets are Tibetan.
    assert_eq!(
        runs("a\u{0F3A}α\u{0F3B}b"),
        run_list(&[
            ("a", "Latn"),
            ("\u{0F3A}", "Tibt"),
            ("α", "Grek"),
            ("\u{0F3B}", "Tibt"),
            ("b", "Latn")
        ])
    );
}

/// **The stack is BD16's, 63 deep** (`[[source: uax9-bidi]]`): "If an
/// opening paired bracket is found and there is no room in the stack, stop
/// processing BD16 for the remainder of the isolating run sequence and
/// return an empty list". At 63 openers every pair holds; at 64 none
/// does, and every closer keeps the preceding script.
#[test]
fn the_pair_stack_is_63_deep() {
    for (depth, closers) in [(63, "Latn"), (64, "Grek")] {
        let text = format!("a{}α{}", "(".repeat(depth), ")".repeat(depth));
        let s = settled(&text);
        assert!(s[..=depth].iter().all(|c| c == "Latn"), "{depth}");
        assert_eq!(s[depth + 1], "Grek");
        assert!(
            s[depth + 2..].iter().all(|c| c == closers),
            "{depth}: {:?}",
            &s[depth + 2..]
        );
    }
}

/// **Both elements of every pair settle alike**, over generated lines of
/// letters of four scripts, spaces and ASCII brackets: the pairs are found
/// here by BD16 as written, independently of the resolver. And every
/// neutral but a paired closer settles as the cell before it, a paired
/// closer included, since every neutral here admits every script.
#[test]
fn both_elements_of_every_pair_settle_alike() {
    let alphabet: Vec<char> = "aαאب ()[]{}".chars().collect();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut paired = 0usize;
    let mut after = 0usize;
    for _ in 0..20_000 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let len = 2 + (seed >> 59);
        let mut text = String::new();
        for _ in 0..len {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            text.push(alphabet[usize::try_from(seed >> 33).unwrap() % alphabet.len()]);
        }
        let chars: Vec<char> = text.chars().collect();
        let s = settled(&text);
        let mut closer = vec![false; chars.len()];
        // BD16: push openers, and pop a closer's match with all above it.
        let mut stack: Vec<(char, usize)> = Vec::new();
        for (i, &c) in chars.iter().enumerate() {
            let close = match c {
                '(' => Some(')'),
                '[' => Some(']'),
                '{' => Some('}'),
                _ => None,
            };
            if let Some(want) = close {
                stack.push((want, i));
            } else if let Some(k) = stack.iter().rposition(|&(w, _)| w == c) {
                let (_, open) = stack[k];
                stack.truncate(k);
                assert_eq!(s[open], s[i], "{text:?} {open} {i}: {s:?}");
                closer[i] = true;
                paired += 1;
            }
        }
        // Every neutral here admits every script, so each that is not a
        // paired closer takes the script of the cell before it.
        for i in 1..chars.len() {
            if " ()[]{}".contains(chars[i]) && !closer[i] {
                assert_eq!(s[i - 1], s[i], "{text:?} {i}: {s:?}");
                after += usize::from(closer[i - 1]);
            }
        }
    }
    // Positive control: the generator reached pairs, and cells after them.
    assert!(paired > 5_000, "{paired}");
    assert!(after > 1_000, "{after}");
}

/// **Every code point's paired-bracket type is BidiBrackets.txt's**, read
/// here from the file itself, and each opener shares its pair with the
/// closer the file names and with no other pair -- but for the two
/// equivalents BD16 names, "U+3009 and U+232A are treated as equivalent"
/// (`[[source: uax9-bidi]]`), which share one.
#[test]
fn every_code_point_has_the_bracket_type_bidibrackets_txt_gives_it() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    let file = std::fs::read_to_string(format!("{dir}/unicode-bidibrackets.txt")).unwrap();
    let mut listed = std::collections::HashMap::new();
    for line in file.lines() {
        let f: Vec<&str> = line
            .split('#')
            .next()
            .unwrap()
            .split(';')
            .map(str::trim)
            .collect();
        if f.len() == 3 && f[2] != "n" {
            let v = |h: &str| u32::from_str_radix(h, 16).unwrap();
            listed.insert(v(f[0]), (v(f[1]), f[2] == "o"));
        }
    }
    // Positive control: the file was read, 64 pairs.
    assert_eq!(listed.len(), 128);
    let c = |v: u32| Codepoint::new(v).unwrap();
    let mut pairs = std::collections::HashMap::new();
    let mut checked = 0usize;
    for v in 0..0x11_0000u32 {
        let Some(cp) = Codepoint::new(v) else {
            continue;
        };
        match (Bracket::of(cp), listed.get(&v)) {
            (Bracket::None, None) => {}
            (Bracket::Open(p), Some(&(closer, true))) => {
                assert_eq!(Bracket::of(c(closer)), Bracket::Close(p), "U+{v:04X}");
                pairs.entry(p).or_insert_with(Vec::new).push(v);
            }
            (Bracket::Close(_), Some(&(_, false))) => {}
            (got, want) => panic!("U+{v:04X}: {got:?} against {want:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 1_112_064, "every scalar value");
    let mut shared: Vec<Vec<u32>> = pairs.into_values().filter(|o| o.len() > 1).collect();
    shared.iter_mut().for_each(|o| o.sort_unstable());
    assert_eq!(shared, [vec![0x2329, 0x3008]], "one pair shared, BD16's");
    assert_eq!(Bracket::of(c(0x232A)), Bracket::of(c(0x3009)));
}
