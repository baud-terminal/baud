//! iTerm2's colour presets and Windows Terminal's schemes: the colour
//! spaces an `.itermcolors` names, converted to sRGB and held to macOS's own
//! conversion; the plist and JSON shapes each program reads; and what each
//! refuses.

use baud_domain::Color;
use baud_domain::themefile::gamut::{Fit, Space, to_srgb};
use baud_domain::themefile::*;

fn c(h: &str) -> Color {
    hex(h.as_bytes()).unwrap()
}

/// macOS's results, from `tools/scripts/nscolor-oracle.js` on macOS 27.0.
const ORACLE: &str = include_str!("nscolor-to-srgb.txt");

/// Each space's colours against macOS's: how many 8-bit channels differ,
/// and the largest difference.
fn against_macos(space: &str, which: Space) -> (usize, usize, u8) {
    let (mut n, mut off, mut worst) = (0, 0, 0u8);
    for row in ORACLE.lines().filter(|l| l.starts_with(space)) {
        let v: Vec<f64> = row.split(' ').skip(1).map(|x| x.parse().unwrap()).collect();
        let (got, _) = to_srgb(which, [v[0], v[1], v[2]]);
        for (g, want) in got.iter().zip(&v[3..6]) {
            let want = (want * 255.0).round() as i32;
            let d = (i32::from(*g) - want).unsigned_abs();
            worst = worst.max(u8::try_from(d).unwrap());
            off += usize::from(d != 0);
        }
        n += 1;
    }
    (n, off, worst)
}

/// **Each colour space converts as macOS converts it**, channel for
/// channel on the 1029 of each space: all but one calibrated channel,
/// (0.41, 1, 0.23)'s red, which baud puts at 117.499 of 255 and macOS's
/// single precision at 117.503, either side of the half step.
#[test]
fn each_colour_space_converts_as_macos_does() {
    assert_eq!(against_macos("srgb ", Space::Srgb), (343, 0, 0));
    let cal = against_macos("cal ", Space::Calibrated);
    let p3 = against_macos("p3 ", Space::DisplayP3);
    assert_eq!(
        (cal, p3),
        ((343, 1, 1), (343, 0, 0)),
        "colours, channels off, largest step"
    );
}

/// A P3 grey is the same sRGB grey; a P3 primary outside sRGB is clipped
/// and said so.
#[test]
fn p3_greys_stay_grey_and_its_red_is_clipped() {
    assert_eq!(
        to_srgb(Space::DisplayP3, [0.5, 0.5, 0.5]),
        ([128, 128, 128], Fit::Inside)
    );
    assert_eq!(
        to_srgb(Space::DisplayP3, [1.0, 0.0, 0.0]),
        ([255, 0, 0], Fit::Clipped)
    );
    assert_eq!(
        to_srgb(Space::Srgb, [0.2, 0.4, 0.6]),
        ([51, 102, 153], Fit::Inside)
    );
}

fn plist(entries: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n{entries}</dict>\n</plist>\n"
    )
}

fn colour(key: &str, rgb: [f64; 3], space: Option<&str>) -> String {
    let space = space.map_or(String::new(), |s| {
        format!("<key>Color Space</key><string>{s}</string>")
    });
    format!(
        "<key>{key}</key>\n<dict><key>Red Component</key><real>{}</real>\
         <key>Green Component</key><real>{}</real><key>Blue Component</key><real>{}</real>{space}</dict>\n",
        rgb[0], rgb[1], rgb[2]
    )
}

/// The lines a reader skipped, each with why.
type Skips = Vec<(u32, String, Skipped)>;

fn read_iterm(text: &str) -> (Result<Scheme, SchemeError>, Skips) {
    let mut skipped = Vec::new();
    let r = iterm(text.as_bytes(), |l, k, w| {
        skipped.push((l.0, String::from_utf8_lossy(k).into_owned(), w))
    });
    (r, skipped)
}

/// No `Color Space` is iTerm2's calibrated RGB, as is `Calibrated`; `sRGB`
/// and `P3` are theirs. A dictionary of fewer than three entries is black.
#[test]
fn iterm_reads_each_colour_in_the_space_it_names() {
    let text = plist(&format!(
        "{}{}{}{}<key>Ansi 4 Color</key>\n<dict><key>Red Component</key><real>1</real></dict>\n",
        colour("Ansi 0 Color", [0.5, 0.5, 0.5], None),
        colour("Ansi 1 Color", [0.5, 0.5, 0.5], Some("Calibrated")),
        colour("Ansi 2 Color", [0.5, 0.5, 0.5], Some("sRGB")),
        colour("Ansi 3 Color", [0.25, 0.5, 0.75], Some("P3")),
    ));
    let s = read_iterm(&text).0.unwrap();
    let cal = to_srgb(Space::Calibrated, [0.5, 0.5, 0.5]).0;
    assert_eq!(s.ansi[0], s.ansi[1]);
    assert_eq!(
        s.ansi[0],
        Some(c(&format!("#{:02x}{:02x}{:02x}", cal[0], cal[1], cal[2])))
    );
    assert_eq!(s.ansi[2], Some(c("#808080")));
    let p3 = to_srgb(Space::DisplayP3, [0.25, 0.5, 0.75]).0;
    assert_eq!(
        s.ansi[3],
        Some(c(&format!("#{:02x}{:02x}{:02x}", p3[0], p3[1], p3[2])))
    );
    assert_eq!(s.ansi[4], Some(c("#000000")));
}

/// **A preset with `(Light)` keys holds two schemes** and is refused at
/// iTerm2's own mark; other keys are said, the clipped colour too.
#[test]
fn iterm_refuses_a_pair_and_says_the_rest() {
    let pair = plist(&format!(
        "{}{}",
        colour("Foreground Color", [1.0, 1.0, 1.0], None),
        colour("Foreground Color (Light)", [0.0, 0.0, 0.0], None)
    ));
    assert_eq!(
        read_iterm(&pair).0,
        Err(SchemeError {
            line: Line(6),
            kind: SchemeErrorKind::Pair
        })
    );
    let text = plist(&format!(
        "{}{}<key>Name</key><string>x</string>\n",
        colour("Bold Color", [1.0, 1.0, 1.0], None),
        colour("Ansi 1 Color", [1.0, 0.0, 0.0], Some("P3")),
    ));
    let (s, skipped) = read_iterm(&text);
    assert_eq!(s.unwrap().ansi[1], Some(c("#ff0000")));
    assert_eq!(
        skipped,
        [
            (4, "Bold Color".into(), Skipped::Unheld),
            (6, "Ansi 1 Color".into(), Skipped::Clipped),
            (8, "Name".into(), Skipped::NotAColour),
        ]
    );
    assert!(matches!(
        read_iterm("<plist><dict><key>Ansi 0 Color</key>").0,
        Err(SchemeError {
            kind: SchemeErrorKind::Syntax,
            ..
        })
    ));
}

fn read_wt(text: &str) -> (Result<Scheme, SchemeError>, Skips) {
    let mut skipped = Vec::new();
    let r = windows_terminal(text.as_bytes(), |l, k, w| {
        skipped.push((l.0, String::from_utf8_lossy(k).into_owned(), w))
    });
    (r, skipped)
}

const TABLE: &str = "\"black\": \"#000\", \"red\": \"#100000\", \"green\": \"#001000\", \
    \"yellow\": \"#101000\", \"blue\": \"#000010\", \"cyan\": \"#001010\", \"white\": \"#fff\", \
    \"brightBlack\": \"#808080\", \"brightRed\": \"#ff0000\", \"brightGreen\": \"#00ff00\", \
    \"brightYellow\": \"#ffff00\", \"brightBlue\": \"#0000ff\", \"brightCyan\": \"#00ffff\", \
    \"brightWhite\": \"#ffffff\"";

/// Windows Terminal's colours: `#rgb` doubled or `#rrggbb`; `magenta` is
/// `purple`'s alternate; comments and a trailing comma are jsoncpp's
/// defaults; the page and the text are optional.
#[test]
fn windows_terminal_reads_its_scheme() {
    let text = format!(
        "{{ /* a scheme */ \"name\": \"x\", // its name\n \"background\": \"#123\", {TABLE}, \
         \"magenta\": \"#100010\", \"brightMagenta\": \"#ff00ff\", }}\n"
    );
    let (s, skipped) = read_wt(&text);
    let s = s.unwrap();
    assert_eq!((s.page, s.text), (Some(c("#112233")), None));
    assert_eq!(
        (s.ansi[0], s.ansi[5], s.ansi[13]),
        (Some(c("#000000")), Some(c("#100010")), Some(c("#ff00ff")))
    );
    assert!(skipped.is_empty(), "{skipped:?}");
}

/// What Windows Terminal refuses is refused: no `name`, fewer than
/// sixteen table colours, a colour it cannot convert, broken JSON.
#[test]
fn windows_terminal_refuses_what_it_refuses() {
    let kind = |t: &str| read_wt(t).0.map(|_| ()).map_err(|e| e.kind);
    assert_eq!(
        kind(&format!(
            "{{ {TABLE}, \"purple\": \"#100010\", \"brightPurple\": \"#ff00ff\" }}"
        )),
        Err(SchemeErrorKind::NoName)
    );
    assert_eq!(
        kind(&format!("{{ \"name\": \"x\", {TABLE} }}")),
        Err(SchemeErrorKind::Count)
    );
    assert_eq!(
        kind(&format!(
            "{{ \"name\": \"x\", {TABLE}, \"purple\": \"#1000100\", \"brightPurple\": \"#f0f\" }}"
        )),
        Err(SchemeErrorKind::Hex(HexError::NotHex))
    );
    assert_eq!(
        kind("{ \"name\": \"x\", \"black\": }"),
        Err(SchemeErrorKind::Syntax)
    );
}

/// **JSON exchanged as a file must be UTF-8** (`[[source: rfc8259-json]]`
/// 8.1), and so must a plist that declares no encoding or UTF-8
/// (`[[source: xml-1-0-5e]]` 4.3.3); a plist that declares another is read
/// as bytes.
#[test]
fn json_and_a_utf_8_plist_that_are_not_utf_8_are_refused() {
    let wt = b"{\n  \"name\": \"Th\xe8me\",\n  \"brightBlack\": \"#000000\"\n}\n";
    assert_eq!(
        windows_terminal(wt, |_, _, _| {}).map(|_| ()),
        Err(SchemeError {
            line: Line(2),
            kind: SchemeErrorKind::NotUtf8
        })
    );
    let plist = |decl: &str| {
        let mut t = decl.as_bytes().to_vec();
        t.extend_from_slice(
            b"\n<plist version=\"1.0\">\n<dict>\n<!-- Th\xe8me -->\n<key>Ansi 1 Color</key>\n<dict>\n\
              <key>Color Space</key><string>sRGB</string>\n\
              <key>Red Component</key><real>1</real>\n\
              <key>Green Component</key><real>0</real>\n\
              <key>Blue Component</key><real>0</real>\n</dict>\n</dict>\n</plist>\n",
        );
        iterm(&t, |_, _, _| {}).map(|s| s.ansi[1])
    };
    let refused = Err(SchemeError {
        line: Line(4),
        kind: SchemeErrorKind::NotUtf8,
    });
    assert_eq!(plist("<?xml version=\"1.0\"?>"), refused);
    assert_eq!(plist("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"), refused);
    assert_eq!(
        plist("<?xml version=\"1.0\" encoding='ISO-8859-1'?>"),
        Ok(Some(c("#ff0000")))
    );
}
