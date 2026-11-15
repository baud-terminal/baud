//! A theme file's format, from its content.
//! Each case names the verdict and the line that decided it; the fixtures
//! are hand-made in each format's spelling.

use baud_domain::themefile::Line;
use baud_domain::themefile::detect::*;
use proptest::prelude::*;

fn kind(text: &str) -> Option<Kind> {
    match classify(text.as_bytes()) {
        Verdict::Is(k, _) => Some(k),
        _ => None,
    }
}

fn decided(text: &str) -> (Kind, Line, String) {
    match classify(text.as_bytes()) {
        Verdict::Is(k, m) => (k, m.line, String::from_utf8_lossy(m.key).into_owned()),
        other => panic!("{other:?} for {text:?}"),
    }
}

const BASE16: &str = include_str!("../../tests/themes/day.yaml");
const GHOSTTY: &str = include_str!("../../tests/themes/night");
const PLAIN: &str = include_str!("../../tests/themes/colors");
const KITTY: &str = include_str!("../../tests/themes/kitty.conf");
const FOOT: &str = include_str!("../../tests/themes/foot.ini");
const ALACRITTY: &str = include_str!("../../tests/themes/alacritty.toml");
const XRES: &str = include_str!("../../tests/themes/Xresources");
const TINTED8: &str = include_str!("../../tests/themes/tinted8.yaml");

/// **Each format is named by its own mark** -- a detector that answers
/// one verdict for everything fails here.
#[test]
fn each_format_is_named_by_its_own_mark() {
    assert_eq!(decided(BASE16), (Kind::Tinted, Line(1), "system".into()));
    assert_eq!(decided(GHOSTTY), (Kind::Ghostty, Line(2), "palette".into()));
    assert_eq!(decided(PLAIN), (Kind::PlainList, Line(1), "#1d1f21".into()));
    assert_eq!(decided(KITTY), (Kind::Kitty, Line(3), "foreground".into()));
    assert_eq!(decided(FOOT), (Kind::Foot, Line(2), "colors-dark".into()));
    assert_eq!(
        decided(ALACRITTY),
        (Kind::Alacritty, Line(2), "colors.primary".into())
    );
    assert_eq!(
        decided(XRES),
        (Kind::Xresources, Line(2), "*.foreground".into())
    );
    assert_eq!(decided(TINTED8), (Kind::Tinted8, Line(2), "system".into()));
}

/// The legacy flat form and the current form without `system`: base16's
/// own, `system` "inferred ... depending on which bases are provided"
/// (`[[source: tinted-builder]]` 108).
#[test]
fn tinted_without_system_is_named_by_its_first_slot() {
    let flat = "scheme: \"Flat\"\nauthor: \"baud tests\"\nbase00: \"181818\"\nbase01: \"282828\"\n";
    assert_eq!(decided(flat), (Kind::Tinted, Line(3), "base00".into()));
    let nested = "name: \"N\"\npalette:\n  base00: \"#181818\"\n";
    assert_eq!(decided(nested), (Kind::Tinted, Line(3), "base00".into()));
    // A multi-line description is skipped as the reader skips it, so its
    // lines are neither a slot nor a second grammar.
    let folded = "description: >\n  some words = and more\n  base00 x\nbase00: \"#000000\"\n";
    // A line of a description that looks like a section header is text.
    let literal = "system: \"base16\"\ndescription: |\n  [draft]\nbase00: \"#000000\"\n";
    assert_eq!(decided(literal), (Kind::Tinted, Line(1), "system".into()));
    assert_eq!(decided(folded), (Kind::Tinted, Line(4), "base00".into()));
}

#[test]
fn the_other_yaml_key_sets_are_told_apart() {
    let nn = "name: \"G\"\ncolor_01: \"#000000\"\ncolor_16: \"#ffffff\"\nbackground: \"#000000\"\n";
    assert_eq!(decided(nn), (Kind::ColorNN, Line(2), "color_01".into()));
    let ala = "font:\n  size: 12\ncolors:\n  primary:\n    background: '0x1d1f21'\n";
    assert_eq!(
        decided(ala),
        (Kind::AlacrittyYaml, Line(4), "primary".into())
    );
    let other = "scheme:\n  system: \"tinted9\"\n";
    assert!(matches!(
        classify(other.as_bytes()),
        Verdict::Unknown(Unknown::TintedSystem(m)) if m.line == Line(2)
    ));
    assert!(matches!(
        classify(b"name: \"x\"\nauthor: \"y\"\n"),
        Verdict::Unknown(Unknown::Yaml(m)) if m.line == Line(1)
    ));
}

/// The smallest files that decide a verdict: one line, an empty file, a
/// file of comments, and a list one line short or long of sixteen.
#[test]
fn minimal_files() {
    assert_eq!(decided("palette = 0=#000000\n").0, Kind::Ghostty);
    assert_eq!(decided("color0 #000000\n").0, Kind::Kitty);
    assert_eq!(decided("[colors-dark]\n").0, Kind::Foot);
    assert_eq!(
        decided("font-family = Fixture Mono\n"),
        (Kind::Ghostty, Line(1), "font-family".into())
    );
    assert_eq!(classify(b""), Verdict::Empty);
    assert_eq!(
        classify(b"# only a comment\n\n# and another\n"),
        Verdict::Empty
    );
    let fifteen: String = PLAIN.lines().skip(1).map(|l| format!("{l}\n")).collect();
    assert_eq!(
        classify(fifteen.as_bytes()),
        Verdict::Unknown(Unknown::HexLines(Line(15)))
    );
    let seventeen = format!("{PLAIN}#000000\n");
    assert_eq!(
        classify(seventeen.as_bytes()),
        Verdict::Unknown(Unknown::HexLines(Line(17)))
    );
}

/// Two grammars, or two formats' keys, are refused naming the first line
/// of each -- unanimity, never a majority.
#[test]
fn mixed_files_name_both_lines() {
    let yaml_and_eq = "base00: \"#000000\"\nbase01: \"#111111\"\npalette = 0=#000000\n";
    assert_eq!(
        classify(yaml_and_eq.as_bytes()),
        Verdict::Mixed(
            Shape::Grammar(Grammar::Colon),
            Mark {
                line: Line(1),
                key: b"base00"
            },
            Shape::Grammar(Grammar::Equals),
            Mark {
                line: Line(3),
                key: b"palette"
            },
        )
    );
    let tinted_and_nn = "base00: \"#000000\"\ncolor_01: \"#000000\"\n";
    assert!(matches!(
        classify(tinted_and_nn.as_bytes()),
        Verdict::Mixed(Shape::Kind(Kind::Tinted), a, Shape::Kind(Kind::ColorNN), b)
            if a.line == Line(1) && b.line == Line(2)
    ));
    let eq_and_space = "palette = 0=#000000\ncolor0 #000000\n";
    assert!(matches!(
        classify(eq_and_space.as_bytes()),
        Verdict::Mixed(
            Shape::Grammar(Grammar::Equals),
            _,
            Shape::Grammar(Grammar::Space),
            _
        )
    ));
    let ghostty_and_foot = "regular0=000000\npalette = 0=#000000\n";
    assert!(matches!(
        classify(ghostty_and_foot.as_bytes()),
        Verdict::Mixed(Shape::Kind(Kind::Foot), a, Shape::Kind(Kind::Ghostty), b)
            if a.line == Line(1) && b.line == Line(2)
    ));
}

/// tinted's slots in `key = value` syntax are refused; foot's keys outside a
/// section are `[main]`'s (`[[source: foot-2705e36-foot-ini-5]]` 9-11); foot's
/// old `[colors]` is named as removed.
#[test]
fn shapes_no_format_reads_are_named() {
    assert!(matches!(
        classify(b"base00 = #181818\nbase01 = #282828\n"),
        Verdict::Unknown(Unknown::TintedSlotsInEquals(m)) if m.line == Line(1)
    ));
    assert!(matches!(
        classify(b"background=1d1f21\nregular0=000000\n"),
        Verdict::Unknown(Unknown::FootOutsideSection(m)) if m.key == b"regular0"
    ));
    assert!(matches!(
        classify(b"[colors]\nregular0=000000\n"),
        Verdict::Unknown(Unknown::FootRemovedSection(_))
    ));
    assert!(matches!(
        classify(b"[main]\nfont=Mono\n"),
        Verdict::Unknown(Unknown::Sections(m)) if m.key == b"main"
    ));
    assert_eq!(
        decided("[colors]\nansi = [\"#000000\"]\nbrights = [\"#ffffff\"]\n").0,
        Kind::Wezterm
    );
    assert_eq!(
        decided("[colors]\nprimary.background = \"#000000\"\n").0,
        Kind::Alacritty
    );
    assert_eq!(
        decided("colors.primary.background = \"#000000\"\n").0,
        Kind::Alacritty
    );
}

#[test]
fn documents_are_told_by_their_first_byte() {
    let plist =
        "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n<key>Ansi 0 Color</key>\n";
    assert_eq!(
        decided(plist),
        (Kind::ITermPlist, Line(4), "Ansi 0 Color".into())
    );
    assert_eq!(classify(b"<html></html>\n"), Verdict::Unknown(Unknown::Xml));
    let wt =
        "{\n  \"name\": \"x\",\n  \"brightBlack\": \"#000000\",\n  \"magenta\": \"#880088\"\n}\n";
    assert_eq!(
        decided(wt),
        (Kind::WindowsTerminal, Line(3), "brightBlack".into())
    );
    assert!(matches!(
        classify(b"{\n  \"schemes\": [\n"),
        Verdict::Unknown(Unknown::JsonSchemes(_))
    ));
    assert_eq!(classify(b"[1, 2]\n"), Verdict::Unknown(Unknown::Json));
    assert_eq!(
        classify(b"bplist00\x00\x01"),
        Verdict::NotText(NotText::BinaryPlist)
    );
}

/// **The detector reads bytes**: a byte that is not UTF-8, in a comment or
/// in a value, leaves the verdict what the rest of the file makes it, and
/// a format whose text requires UTF-8 refuses the file in its reader.
#[test]
fn bytes_that_are_not_utf_8_do_not_decide_the_format() {
    for (text, kind, line) in [
        (&b"# Th\xe8me\nbackground = #1d1f21\n"[..], Kind::Ghostty, 2),
        (b"palette = 0=#\xff\n", Kind::Ghostty, 1),
        (
            b"# Th\xe8me\nsystem: \"base16\"\nbase00: \"181818\"\n",
            Kind::Tinted,
            2,
        ),
        (b"# Th\xe8me\nbackground #1d1f21\n", Kind::Kitty, 2),
        (
            b"# Th\xe8me\n[colors-dark]\nbackground=1d1f21\n",
            Kind::Foot,
            2,
        ),
        (
            b"# Th\xe8me\n[colors.primary]\nbackground = '#1d1f21'\n",
            Kind::Alacritty,
            2,
        ),
    ] {
        match classify(text) {
            Verdict::Is(k, m) => assert_eq!((k, m.line), (kind, Line(line)), "{text:?}"),
            other => panic!("{text:?}: {other:?}"),
        }
    }
}

/// Xresources' keys hold `*` or `.` before the `:`, so they are not YAML,
/// and a ghostty value's `:` does not make a line YAML.
#[test]
fn colons_in_resources_and_in_values() {
    assert_eq!(decided("URxvt*background: #000000\n").0, Kind::Xresources);
    assert_eq!(
        decided("XTerm.vt100.foreground: #000000\n").0,
        Kind::Xresources
    );
    assert_eq!(
        decided("palette = 1=rgb:cc/66/66\nfont-family = \"A: B\"\n").0,
        Kind::Ghostty
    );
    assert!(matches!(
        classify(b"*.font: Mono\n"),
        Verdict::Unknown(Unknown::Lines(Grammar::Resource, _))
    ));
}

/// CRLF, a BOM and a blank line give each control the same verdict.
#[test]
fn line_endings_and_a_bom_change_nothing() {
    for text in [
        BASE16, GHOSTTY, PLAIN, KITTY, FOOT, ALACRITTY, XRES, TINTED8,
    ] {
        let want = classify(text.as_bytes());
        let crlf = text.replace('\n', "\r\n");
        let bom = format!("\u{feff}{text}");
        assert_eq!(classify(crlf.as_bytes()), want, "{text:?}");
        assert_eq!(classify(bom.as_bytes()), want, "{text:?}");
        assert_eq!(body(bom.as_bytes()), text.as_bytes());
    }
}

/// The formats' line-ish texts, for the properties.
const CONTROLS: [&str; 7] = [BASE16, GHOSTTY, KITTY, FOOT, ALACRITTY, XRES, TINTED8];

proptest! {
    /// **The verdict is total**: any bytes give one, without a panic.
    #[test]
    fn any_bytes_give_a_verdict(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = classify(&bytes);
    }

    /// A blank line or a comment inserted between two lines of a control
    /// changes nothing but the lines after it. The plain list has no
    /// comment form, so it is not among the controls.
    #[test]
    fn a_comment_or_blank_line_changes_no_verdict(which in 0..CONTROLS.len(), at in 0..32usize, blank in any::<bool>()) {
        let text = CONTROLS[which];
        let want = kind(text);
        let lines: Vec<&str> = text.lines().collect();
        let at = at % (lines.len() + 1);
        let insert = if blank { "" } else { "# a comment" };
        let mut out: Vec<&str> = lines[..at].to_vec();
        out.push(insert);
        out.extend_from_slice(&lines[at..]);
        let text = out.join("\n");
        prop_assert_eq!(kind(&text), want);
    }
}
