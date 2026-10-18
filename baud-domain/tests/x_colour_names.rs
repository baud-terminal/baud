//! The X server's colour names, as `OSC 4`, `10`, `11` and `12` read them.
//!
//! XParseColor sends a name to the server (`[[source: libx11-parsecol-b8d5a70]]`
//! 114-139), which finds it in X.Org's rgb.txt (`[[source: xorg-rgb-0d2caec]]`)
//! by `strncasecmp` and an exact length
//! (`[[source: xserver-oscolor-98aed9d]]` 852-884): case is ignored and
//! nothing else. The table baud builds from rgb.txt is held to every line of
//! it here.
use baud_domain::{Chan, XColor};

fn rgb(r: u8, g: u8, b: u8) -> XColor<'static> {
    XColor::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

/// **Every line of rgb.txt is a colour, as written, upper-cased and
/// lower-cased**, with the line's value. The 782 count is the positive
/// control: a reader that matched nothing would count none.
#[test]
fn every_name_in_rgb_txt_resolves_in_any_case() {
    let src = include_str!("../../docs/research/sources/xorg-rgb-0d2caec.txt");
    let mut n = 0;
    for line in src.lines().filter(|l| !l.is_empty() && !l.starts_with('!')) {
        let mut f = line.split_whitespace();
        let v: Vec<u8> = f.by_ref().take(3).map(|x| x.parse().unwrap()).collect();
        let name = f.collect::<Vec<_>>().join(" ");
        let want = rgb(v[0], v[1], v[2]);
        for spelling in [
            name.clone(),
            name.to_ascii_uppercase(),
            name.to_ascii_lowercase(),
        ] {
            assert_eq!(XColor::parse(spelling.as_bytes()), want, "{spelling}");
        }
        n += 1;
    }
    assert_eq!(n, 782);
}

/// **Only case is forgiven.** A space the table does not spell, a name the
/// 2008 rgb.txt lacked but the server holds, and Debian's `DebianRed`,
/// which kitty's table carries and X.Org's does not.
#[test]
fn a_name_is_the_servers_spelling_in_any_case() {
    assert_eq!(XColor::parse(b"GhostWhite"), rgb(248, 248, 255));
    assert_eq!(XColor::parse(b"ghost white"), rgb(248, 248, 255));
    assert_eq!(XColor::parse(b"Ghost White"), rgb(248, 248, 255));
    // CSS's green beside X11's: the server keeps both.
    assert_eq!(XColor::parse(b"green"), rgb(0, 255, 0));
    assert_eq!(XColor::parse(b"WebGreen"), rgb(0, 128, 0));
    assert_eq!(XColor::parse(b"rebeccapurple"), rgb(102, 51, 153));
    for miss in [
        &b"ghost  white"[..],
        b"ghostwhite ",
        b" red",
        b"gh ostwhite",
        b"DebianRed",
        b"notacolour",
        b"",
    ] {
        assert_eq!(XColor::parse(miss), XColor::Unresolved(miss), "{miss:?}");
    }
    // A malformed `#` form is XParseColor's own refusal and never a name.
    assert_eq!(XColor::parse(b"#red"), XColor::Unresolved(b"#red"));
}
