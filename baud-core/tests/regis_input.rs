//! **ReGIS graphics input against `[[source: dec-std-070]]` 070-8 § 8.8.5
//! and 070-13 § 5, and the VT330/VT340's manual where they leave room
//! (`[[source: vt3xx-report]]`, `[[source: vt3xx-locator]]`).**
//!
//! A one-shot `R(P(I))` holds the host's output until a key or a button
//! ends it with a report; multiple mode reports at once and on each button.
//! The cursors are drawn into the page shown and read back here pixel for
//! pixel, with no cell size set so the page is the VT300's 800 by 480 and a
//! page pixel is a device pixel.

use baud_core::*;
use baud_domain::{
    Codepoint, ImageBudget, ImagePx, KeyCode, KeyEvent, KeyEventKind, LocatorAt, LocatorButton,
    LocatorButtons, LocatorInput, LocatorKind, Modifiers, Pixel, PointerPx, RegisInput,
};

mod support;

/// The VT340's default map (`[[source: vt3xx-screen-control]]` Table 2-3):
/// entry 0 black, the cursor's stroke; entry 15 80% gray, its border; entry
/// 7 50% gray, the default foreground.
const BLACK: u32 = 0xff00_0000;
const GRAY80: u32 = 0xffcc_cccc;
const GRAY50: u32 = 0xff87_8787;

fn term(f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_regis_hold(RegisHold::On);
    f(&mut t);
}

/// One read: what it answered and the bytes it held.
fn read(t: &mut Term<'_>, bytes: &[u8]) -> (Vec<u8>, Option<Vec<u8>>) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let rest = t
        .take_held()
        .map(|r| bytes[r.at.index()..r.at.index() + r.len.index()].to_vec());
    (answered(t), rest)
}

fn answered(t: &mut Term<'_>) -> Vec<u8> {
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

fn key(t: &mut Term<'_>, code: KeyCode, mods: Modifiers) -> Vec<u8> {
    t.key(KeyEvent::press(code, mods));
    answered(t)
}

fn letter(c: char) -> KeyCode {
    KeyCode::Char(Codepoint::from_char(c))
}

fn at(x: u16, y: u16) -> Option<LocatorAt> {
    Some(LocatorAt {
        col: Col::ZERO,
        row: Row::ZERO,
        px: Some(PointerPx {
            x: ImagePx::of(x),
            y: ImagePx::of(y),
        }),
    })
}

fn pointer(t: &mut Term<'_>, kind: LocatorKind, x: u16, y: u16) -> Vec<u8> {
    let held = match kind {
        LocatorKind::Press(b) => LocatorButtons::NONE.pressed(b),
        _ => LocatorButtons::NONE,
    };
    t.locator(LocatorInput {
        kind,
        at: at(x, y),
        held,
    });
    answered(t)
}

/// The page's pixels and width: the one ReGIS image placed.
fn page(t: &Term<'_>) -> (Vec<u32>, usize) {
    let imgs = t.images();
    let (_, p) = imgs.placements().next().expect("a page placed");
    let v = imgs.image(p.image).expect("its image");
    (
        v.pixels.iter().map(|px| px.bits()).collect(),
        v.width.index(),
    )
}

fn px(page: &(Vec<u32>, usize), x: usize, y: usize) -> u32 {
    page.0[y * page.1 + x]
}

fn painted(page: &(Vec<u32>, usize)) -> Vec<(usize, usize, u32)> {
    page.0
        .iter()
        .enumerate()
        .filter(|(_, c)| **c != Pixel::CLEAR.bits())
        .map(|(i, c)| (i % page.1, i / page.1, *c))
        .collect()
}

/// **A drawn cursor keeps the page's count of lit pixels a recount's**:
/// the output cursor between reads, the move that draws it again, a
/// one-shot's crosshair, a pointer move and the key that ends the input,
/// and `ST` taking the cursors away. The store frees a page whose count
/// reaches zero, so a cursor's pixels left out of it free a page that
/// still shows them.
#[test]
fn a_drawn_cursor_keeps_the_page_s_lit_count_exact() {
    fn kept(t: &Term<'_>, step: &str) {
        let imgs = t.images();
        let (_, p) = imgs.placements().next().expect("a page placed");
        let v = imgs.image(p.image).expect("its image");
        let n = v
            .pixels
            .iter()
            .filter(|px| px.bits() != Pixel::CLEAR.bits())
            .count();
        assert_eq!(imgs.lit(p.image), Some(u32::try_from(n).unwrap()), "{step}");
    }
    term(|t| {
        read(t, b"\x1bPpF(V[0,0][96,0][96,48][0,48])");
        kept(t, "the output cursor");
        read(t, b"P[200,100]");
        kept(t, "the output cursor moved");
        read(t, b"R(P(I))");
        assert_eq!(t.regis_input(), RegisInput::OneShot);
        kept(t, "the crosshair");
        pointer(t, LocatorKind::Moved, 60, 30);
        kept(t, "the crosshair moved");
        key(t, letter('a'), Modifiers::NONE);
        kept(t, "the input ended");
        read(t, b"\x1b\\");
        kept(t, "the cursors gone");
    });
}

// ---------------------------------------------------------------------------
// One-shot mode.
// ---------------------------------------------------------------------------

/// **A one-shot `R(P(I))` waits for a key and holds the host's output**:
/// "no further data from the application (screen output) is processed
/// until the locator input is complete" (070-13 § 5.1). The key's own byte
/// goes first, then the position and a carriage return -- the VT340's
/// `A[102,200]<CR>` -- and the held output draws once it is fed.
#[test]
fn a_one_shot_report_waits_for_a_key_and_holds_the_read() {
    term(|t| {
        let (reply, rest) = read(t, b"\x1bPpP[100,100]R(P(I))V[110,100]\x1b\\");
        assert_eq!(reply, b"", "nothing answered until the user acts");
        assert_eq!(rest.as_deref(), Some(&b"V[110,100]\x1b\\"[..]));
        assert_eq!(t.regis_input(), RegisInput::OneShot);
        assert_eq!(key(t, letter('A'), Modifiers::NONE), b"A[100,100]\r");
        assert_eq!(t.regis_input(), RegisInput::Off);
        let (reply, rest) = read(t, &rest.unwrap());
        assert_eq!((reply.as_slice(), rest), (&b""[..], None));
        let want: Vec<_> = (100..=110).map(|x| (x, 100, GRAY50)).collect();
        assert_eq!(painted(&page(t)), want, "the vector, and no cursor left");
    });
}

/// **The arrow keys step the cursor a device pixel, Shift ten, and wrap at
/// the screen's edge**: "The cursor moves one pixel ... Shift-arrow key:
/// The cursor moves 10 pixels ... If you move the cursor past a screen
/// boundary, the cursor wraps to the other side of the screen"
/// (`[[source: vt3xx-report]]`). They send nothing.
#[test]
fn arrow_keys_step_the_cursor_and_wrap_at_the_edge() {
    term(|t| {
        read(t, b"\x1bPpP[100,100]R(P(I))\x1b\\");
        assert_eq!(key(t, KeyCode::Right, Modifiers::NONE), b"");
        assert_eq!(key(t, KeyCode::Down, Modifiers::SHIFT), b"");
        assert_eq!(key(t, KeyCode::Right, Modifiers::NONE), b"");
        assert_eq!(key(t, letter('x'), Modifiers::NONE), b"x[102,110]\r");
    });
    term(|t| {
        read(t, b"\x1bPpP[0,475]R(P(I))\x1b\\");
        key(t, KeyCode::Left, Modifiers::NONE);
        key(t, KeyCode::Down, Modifiers::SHIFT);
        assert_eq!(key(t, letter('q'), Modifiers::NONE), b"q[799,5]\r");
    });
}

/// **A key that sends nothing, a release and a repeat DECARM refuses leave
/// the input waiting**: "Pressing any non-arrow key that is not dead"
/// ends it (070-13 § 5.1).
#[test]
fn a_dead_key_or_a_release_leaves_one_shot_waiting() {
    term(|t| {
        read(t, b"\x1bPpR(P(I))\x1b\\");
        let mut release = KeyEvent::press(letter('a'), Modifiers::NONE);
        release.kind = KeyEventKind::Release;
        t.key(release);
        assert_eq!(answered(t), b"");
        assert_eq!(t.regis_input(), RegisInput::OneShot);
        assert_eq!(key(t, KeyCode::Enter, Modifiers::NONE), b"\r[0,0]\r");
        assert_eq!(t.regis_input(), RegisInput::Off);
    });
}

/// **A button ends one-shot with its string**, `CSI 241 ~` for the left
/// (`[[source: vt3xx-locator]]` Table 15-1), at the pointer's position; a
/// release reports nothing, "Up Transitions are ignored by default"
/// (070-13 § 5.4), and does not end it.
#[test]
fn a_button_ends_one_shot_with_its_string() {
    term(|t| {
        read(t, b"\x1bPpR(P(I))\x1b\\");
        assert_eq!(pointer(t, LocatorKind::Moved, 300, 200), b"");
        assert_eq!(
            pointer(t, LocatorKind::Release(LocatorButton::Left), 300, 200),
            b""
        );
        assert_eq!(t.regis_input(), RegisInput::OneShot);
        assert_eq!(
            pointer(t, LocatorKind::Press(LocatorButton::Middle), 310, 205),
            b"\x1b[243~[310,205]\r"
        );
        assert_eq!(t.regis_input(), RegisInput::Off);
        assert_eq!(
            pointer(t, LocatorKind::Press(LocatorButton::Left), 1, 1),
            b"",
            "over: the button is the terminal's again"
        );
    });
}

/// **The report is in user coordinates, to a tenth**: "expressed as an
/// absolute bracketed extent in user coordinates" (070-13 § 5.1), the
/// VT340's `[102.5,200]`. Under `S(A[0,0][399,239])` a device pixel is
/// half a user unit.
#[test]
fn the_report_is_in_user_coordinates_to_a_tenth() {
    term(|t| {
        read(t, b"\x1bPpS(A[0,0][399,239])P[50,60]R(P(I))\x1b\\");
        key(t, KeyCode::Right, Modifiers::NONE);
        assert_eq!(key(t, letter('k'), Modifiers::NONE), b"k[50.5,60]\r");
    });
}

/// **Without a held read the answer is a non-interactive device's**, 070-8
/// § 8.8.5.3's first response, counted.
#[test]
fn without_a_hold_one_shot_answers_at_once_and_is_counted() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    support::feed(&mut t, b"\x1bPpP[7,8]R(P(I))\x1b\\");
    assert_eq!(t.reply(), b"[7,8]\r");
    assert_eq!(t.regis_input(), RegisInput::Off);
    assert_eq!(t.regis_counts().inputs_unheld, 1);
}

/// **A one-shot input given up sends the empty report**, "Only the
/// terminating CR is sent ... indicating that NO INPUT was to be returned"
/// (070-8 § 8.8.5.3), counted.
#[test]
fn a_one_shot_given_up_sends_the_empty_report() {
    term(|t| {
        read(t, b"\x1bPpR(P(I))\x1b\\");
        assert!(t.regis_input_abandon());
        assert_eq!(answered(t), b"\r");
        assert_eq!(t.regis_input(), RegisInput::Off);
        assert_eq!(t.regis_counts().inputs_abandoned, 1);
        assert!(!t.regis_input_abandon(), "nothing waits now");
    });
}

/// **A one-shot `R(P(I))` inside a macrograph stops it there**, and the
/// rest of the macrograph plays once the report has gone.
#[test]
fn a_one_shot_input_in_a_macrograph_stops_it_until_the_report() {
    term(|t| {
        let (reply, rest) = read(t, b"\x1bPp@:AP[10,10]R(P(I))P[100,100]V[110,100]@;@A\x1b\\");
        assert_eq!(reply, b"");
        assert_eq!(t.regis_input(), RegisInput::OneShot);
        assert_ne!(px(&page(t), 105, 100), GRAY50, "the rest waits");
        assert_eq!(key(t, letter('z'), Modifiers::NONE), b"z[10,10]\r");
        read(t, &rest.unwrap());
        let want: Vec<_> = (100..=110).map(|x| (x, 100, GRAY50)).collect();
        assert_eq!(painted(&page(t)), want);
    });
}

// ---------------------------------------------------------------------------
// Multiple mode.
// ---------------------------------------------------------------------------

/// **In multiple mode `R(P(I))` answers at once behind the null button**,
/// `CSI 240 ~` -- "<CSI>240~[100,100]<CR> The terminal received the ReGIS
/// report position interactive command" (`[[source: vt3xx-locator]]`) --
/// after `R(I1)`'s own carriage return, and in 8-bit form under S8C1T.
#[test]
fn multiple_mode_reports_at_once_behind_the_null_button() {
    term(|t| {
        let (reply, rest) = read(t, b"\x1bPpP[50,60]R(I1)R(P(I))\x1b\\");
        assert_eq!(reply, b"\r\x1b[240~[50,60]\r");
        assert_eq!(rest, None, "nothing is held in multiple mode");
    });
    term(|t| {
        let (reply, _) = read(t, b"\x1b G\x1bPpR(I1)R(P(I))\x1b\\");
        assert_eq!(reply, b"\r\x9b240~[0,0]\r");
    });
}

/// **Multiple mode's buttons report inside the envelope, and it resumes on
/// re-entry**: "If this happens, the ReGIS locator will be disabled until
/// ReGIS is re-entered, at which time multiple input mode resumes with the
/// input cursor at its last known position" (070-13 § 5.2).
#[test]
fn multiple_mode_works_inside_the_envelope_and_resumes_on_reentry() {
    term(|t| {
        assert_eq!(read(t, b"\x1bPpR(I1)").0, b"\r");
        assert_eq!(t.regis_input(), RegisInput::Multiple);
        assert_eq!(
            pointer(t, LocatorKind::Press(LocatorButton::Left), 40, 30),
            b"\x1b[241~[40,30]\r"
        );
        assert_eq!(t.regis_input(), RegisInput::Multiple, "it stays");
        read(t, b"\x1b\\");
        assert_eq!(t.regis_input(), RegisInput::Off);
        assert_eq!(
            pointer(t, LocatorKind::Press(LocatorButton::Left), 90, 90),
            b""
        );
        assert_eq!(
            read(t, b"\x1bPpR(P(I))").0,
            b"\x1b[240~[40,30]\r",
            "where it was left"
        );
        assert_eq!(t.regis_input(), RegisInput::Multiple);
    });
}

/// **`R(I0)` and a valid `S(A)` end multiple mode** (070-8 § 8.8.5.2, "The
/// commands R(I0) or S(A) exit multiple input mode").
#[test]
fn r_i0_and_s_a_end_multiple_mode() {
    for end in [&b"R(I0)"[..], b"S(A[0,0][799,479])"] {
        term(|t| {
            read(t, &[&b"\x1bPpR(I1)"[..], end].concat());
            answered(t);
            assert_eq!(t.regis_input(), RegisInput::Off, "{:?}", end);
        });
    }
}

/// **In multiple mode the keys are the terminal's**, arrows included: "If
/// you press an arrow key in multiple mode, the terminal sends that key's
/// escape sequence to the host" (`[[source: vt3xx-locator]]`).
#[test]
fn arrow_keys_in_multiple_mode_are_sent() {
    term(|t| {
        read(t, b"\x1bPpR(I1)");
        answered(t);
        assert_eq!(key(t, KeyCode::Up, Modifiers::NONE), b"\x1b[A");
        assert_eq!(key(t, letter('w'), Modifiers::NONE), b"w");
    });
}

/// **`DECSTR` keeps graphics input**: its list of what it resets names no
/// graphics state (070-4, DECSTR), so a suspended multiple mode resumes at
/// the next envelope.
#[test]
fn decstr_keeps_multiple_mode() {
    term(|t| {
        read(t, b"\x1bPpP[40,30]R(I1)\x1b\\\x1b[!p");
        answered(t);
        let (reply, _) = read(t, b"\x1bPpR(P(I))\x1b\\");
        assert_eq!(reply, b"\x1b[240~[40,30]\r");
    });
}

/// **`RIS` ends graphics input**: the device to its power-up state.
#[test]
fn ris_ends_graphics_input() {
    term(|t| {
        read(t, b"\x1bPpR(I1)");
        answered(t);
        assert_eq!(t.regis_input(), RegisInput::Multiple);
        read(t, b"\x1b\\\x1bc\x1bPp");
        assert_eq!(t.regis_input(), RegisInput::Off);
    });
}

// ---------------------------------------------------------------------------
// The cursors, pixel for pixel.
// ---------------------------------------------------------------------------

/// **The input cursor is a crosshair by default**, its lines across the
/// screen through the position in map entry 0 with a one-pixel border in
/// entry 15, drawn into the page shown, and taken away when the input
/// ends. The crosshair is the VT340's default, an open departure from
/// 070-8's numbering, whose default is the diamond.
#[test]
fn the_default_input_cursor_is_a_two_tone_crosshair() {
    term(|t| {
        read(t, b"\x1bPpP[100,200]R(P(I))\x1b\\");
        let p = page(t);
        assert!((0..800).all(|x| x == 100 || px(&p, x, 200) == BLACK));
        assert!((0..480).all(|y| y == 200 || px(&p, 100, y) == BLACK));
        assert_eq!(px(&p, 300, 199), GRAY80, "the border above");
        assert_eq!(px(&p, 300, 201), GRAY80, "the border below");
        assert_eq!(px(&p, 99, 50), GRAY80, "left of the vertical");
        assert_eq!(px(&p, 300, 198), Pixel::CLEAR.bits());
        key(t, letter('a'), Modifiers::NONE);
        // The output cursor shows now, ReGIS waiting on the held `ST`; of
        // the crosshair nothing is left.
        let p = page(t);
        let clear = Pixel::CLEAR.bits();
        let off = |v: usize| !(188..=212).contains(&v) && !(88..=112).contains(&v);
        assert!(
            (0..800)
                .filter(|&x| off(x))
                .all(|x| px(&p, x, 200) == clear)
        );
        assert!(
            (0..480)
                .filter(|&y| off(y))
                .all(|y| px(&p, 100, y) == clear)
        );
    });
}

/// **The cursor moves with the keys and the pointer, leaving no trail.**
#[test]
fn a_moved_cursor_leaves_no_trail() {
    term(|t| {
        read(t, b"\x1bPpP[100,200]R(P(I))\x1b\\");
        key(t, KeyCode::Down, Modifiers::SHIFT);
        pointer(t, LocatorKind::Moved, 500, 50);
        let p = page(t);
        assert_eq!(px(&p, 300, 200), Pixel::CLEAR.bits(), "the old line gone");
        assert_eq!(px(&p, 300, 210), Pixel::CLEAR.bits());
        assert_eq!(px(&p, 300, 50), BLACK, "the new one there");
        assert_eq!(px(&p, 500, 300), BLACK);
    });
}

/// **The cursor is never in the plane**: a complement drawn over it after
/// the input inverts what the program drew, and nothing of the cursor.
#[test]
fn the_cursor_is_not_drawn_into_the_plane() {
    term(|t| {
        let (_, rest) = read(t, b"\x1bPpP[0,100]R(P(I))W(C)P[50,90]V[50,110]\x1b\\");
        key(t, letter('a'), Modifiers::NONE);
        read(t, &rest.unwrap());
        let p = page(t);
        // Complement over never-written pixels gives index 15, and the
        // crosshair's row 100 is one of them.
        assert_eq!(px(&p, 50, 100), GRAY80);
        assert_eq!(painted(&p).len(), 21, "the 21 pixels the vector drew");
    });
}

/// **The input cursor's numbers are the VT340's**: `S(C(I0))`, `S(C(I2))`
/// and an omitted number the crosshair, `S(C(I1))` the diamond, 21 by 21
/// with its diagonals, its vertices ten pixels from the position
/// (`[[source: vt3xx-screen-control]]`; an open departure from 070-8
/// § 8.8.1.4.2 b, where 0 is the diamond too).
#[test]
fn s_c_i1_selects_the_diamond_and_i0_i2_and_omitted_the_crosshair() {
    for c in [
        &b"S(C(I0))"[..],
        b"S(C(I2))",
        b"S(C(I))",
        b"S(C(I1))S(C(I))",
    ] {
        term(|t| {
            let mut bytes = b"\x1bPp".to_vec();
            bytes.extend_from_slice(c);
            bytes.extend_from_slice(b"P[100,100]R(P(I))\x1b\\");
            read(t, &bytes);
            let p = page(t);
            let what = String::from_utf8_lossy(c);
            assert_eq!(px(&p, 300, 100), BLACK, "{what}: the crosshair");
            assert_eq!(px(&p, 100, 300), BLACK, "{what}: its vertical");
            assert_eq!(px(&p, 95, 95), Pixel::CLEAR.bits(), "{what}: no diamond");
        });
    }
    term(|t| {
        read(t, b"\x1bPpS(C(I1))P[200,200]R(P(I))\x1b\\");
        let p = page(t);
        for (x, y) in [
            (190, 200),
            (210, 200),
            (200, 190),
            (200, 210),
            (200, 200),
            (195, 195),
        ] {
            assert_eq!(px(&p, x, y), BLACK, "({x},{y})");
        }
        assert_eq!(px(&p, 300, 200), Pixel::CLEAR.bits(), "no crosshair");
    });
}

/// **A valid `S(A)` selects the crosshair again**, the VT340's default,
/// after `S(C(I1))` chose the diamond; 070-8 § 8.8.1.4.1 g's written
/// equivalent, `S(C(H1)(I1)1)`, selects the diamond, `I1` being the
/// diamond in the VT340's numbers (the open departure above).
#[test]
fn a_valid_s_a_selects_the_crosshair_and_s_c_i1_the_diamond() {
    term(|t| {
        read(
            t,
            b"\x1bPpS(C(I1))S(A[0,0][799,479])P[100,200]R(P(I))\x1b\\",
        );
        assert_eq!(px(&page(t), 300, 200), BLACK, "S(A): the crosshair");
    });
    term(|t| {
        read(t, b"\x1bPpS(C(I3))S(C(H1)(I1)1)P[100,200]R(P(I))\x1b\\");
        let p = page(t);
        assert_eq!(px(&p, 300, 200), Pixel::CLEAR.bits(), "I1: no crosshair");
        assert_eq!(px(&p, 90, 200), BLACK, "I1: the diamond's vertex");
    });
}

/// **A rubber band line runs from the drawing position at the start of the
/// input to the input position** (070-8 § 8.8.1.4.2 b).
#[test]
fn a_rubber_band_line_is_anchored_at_the_drawing_position() {
    term(|t| {
        read(t, b"\x1bPpS(C(I3))P[100,100]R(P(I))\x1b\\");
        pointer(t, LocatorKind::Moved, 140, 100);
        let p = page(t);
        assert!((100..=140).all(|x| px(&p, x, 100) == BLACK));
        assert_eq!(px(&p, 141, 100), GRAY80, "its border");
        assert_eq!(px(&p, 300, 100), Pixel::CLEAR.bits());
    });
}

/// **The output cursor shows where a read ends inside an envelope**, the
/// diamond at the drawing position, "only ... when the terminal is in ReGIS
/// mode and waiting for ReGIS input from the host" (`[[source:
/// vt3xx-regis-intro]]`), and goes at `ST`; `S(C0)` turns it off. It is
/// drawn on a page already placed, and places none of its own, so where
/// the reads fell never decides whether a page is there.
#[test]
fn the_output_cursor_shows_while_the_envelope_waits() {
    term(|t| {
        read(t, b"\x1bPpP[0,0]V[1,0]P[300,200]");
        let p = page(t);
        assert_eq!(px(&p, 290, 200), BLACK);
        assert_eq!(px(&p, 300, 210), BLACK);
        read(t, b"\x1b\\");
        assert_eq!(painted(&page(t)).len(), 2, "gone at ST, the vector left");
    });
    term(|t| {
        read(t, b"\x1bPpP[300,200]");
        assert_eq!(t.images().placements().count(), 0, "no page for it alone");
    });
    term(|t| {
        read(t, b"\x1bPpS(C0)V[300,200]");
        assert_eq!(painted(&page(t)).len(), 301, "the vector alone");
    });
}

/// **A user-defined cursor is two characters of the drawing alphabet**:
/// "One character is displayed in the foreground color, the other in the
/// background color" -- the VT340's entries 14 and 1 -- from the cell's
/// upper-left, the offset back from the position (070-8 § 8.8.1.4.2 c).
#[test]
fn a_user_defined_cursor_draws_its_two_characters() {
    term(|t| {
        // Alphabet 1, 8 by 2 cells: 'a' a full top row, 'b' a full bottom
        // row.
        read(
            t,
            b"\x1bPpL(A1)[2,8]\"a\"FF,00\"b\"00,FFT(A1)S(C(I[+1,+0]\"ab\"))P[100,100]R(P(I))\x1b\\",
        );
        let p = page(t);
        let yellow_gray = 0xff99_9954;
        let blue = 0xff33_33cc;
        assert!((99..107).all(|x| px(&p, x, 100) == yellow_gray), "fg");
        assert!((99..107).all(|x| px(&p, x, 101) == blue), "bg");
        assert_eq!(px(&p, 107, 100), Pixel::CLEAR.bits());
    });
}

/// **The indicator line shows the input position while ReGIS input runs**:
/// "Graphics Input Position Indicator if some graphic input mode is
/// enabled through ReGIS ... Indicates the pixel row and column of the
/// input position" (070-14 p. 14-6), line then column, one-based as the
/// field shows DEC's locator in pixels, and it follows the arrow keys.
#[test]
fn the_indicator_shows_the_input_position() {
    let ws = support::CORPUS;
    support::with_term(&ws, |t| {
        t.set_regis_hold(RegisHold::On);
        let field = |t: &Term<'_>| -> String {
            let line = t.status_line().expect("an indicator");
            line.cells[23..40]
                .iter()
                .filter_map(|c| c.content.as_codepoint())
                .filter_map(|c| char::from_u32(c.value()))
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        support::feed(t, b"\x1b[1$~");
        assert_eq!(field(t), "");
        read(t, b"\x1bPpP[9,4]R(P(I))\x1b\\");
        assert_eq!(field(t), "[5,10]");
        key(t, KeyCode::Right, Modifiers::NONE);
        assert_eq!(field(t), "[5,11]");
        key(t, letter('e'), Modifiers::NONE);
        assert_eq!(field(t), "", "gone with the input");
    });
}

/// **A cancelled envelope takes its cursors with it** in the read that
/// cancels it: `CAN`, `SUB` and a stray `ESC` close the envelope as `ST`
/// closes one, and the output cursor "only appears when the terminal is in
/// ReGIS mode" (`[[source: vt3xx-regis-intro]]`), multiple mode's input
/// cursor with it. The diamond a read ending inside the envelope drew, and
/// the crosshair of multiple mode, are gone with no pointer move, and so
/// when another string follows the cancel in the same read.
#[test]
fn a_cancelled_envelope_takes_its_cursors_away() {
    for cancel in [&b"\x18"[..], b"\x1a", b"\x1b[0m", b"\x18\x1bP$qm\x1b\\"] {
        term(|t| {
            read(t, b"\x1bPpP[0,0]V[1,0]P[100,200]R(I1)");
            let what = String::from_utf8_lossy(cancel);
            assert_eq!(px(&page(t), 95, 195), BLACK, "{what}: the diamond drawn");
            assert_eq!(px(&page(t), 300, 200), BLACK, "{what}: the crosshair drawn");
            read(t, cancel);
            assert_eq!(t.regis_input(), RegisInput::Off);
            let p = page(t);
            assert_eq!(
                px(&p, 95, 195),
                Pixel::CLEAR.bits(),
                "{what}: the diamond gone"
            );
            assert_eq!(
                px(&p, 300, 200),
                Pixel::CLEAR.bits(),
                "{what}: the crosshair gone"
            );
            assert_eq!(painted(&p).len(), 2, "{what}: the vector alone");
        });
    }
}

/// **A stray `ESC` that a read ends on keeps the cursors** until the next
/// byte says whether it is `ST` or a cancel, the envelope still open.
#[test]
fn an_esc_a_read_ends_on_keeps_the_cursor_until_the_next_byte() {
    term(|t| {
        read(t, b"\x1bPpP[0,0]V[1,0]P[100,200]");
        read(t, b"\x1b");
        assert_eq!(px(&page(t), 95, 195), BLACK, "still open");
        read(t, b"[0m");
        assert_eq!(px(&page(t), 95, 195), Pixel::CLEAR.bits(), "cancelled");
        assert_eq!(painted(&page(t)).len(), 2, "the vector alone");
    });
}

// ---------------------------------------------------------------------------
// DECLKD, the locator key definitions (070-13 § 5.4).
// ---------------------------------------------------------------------------

fn press(t: &mut Term<'_>, b: LocatorButton) -> Vec<u8> {
    pointer(t, LocatorKind::Press(b), 7, 9)
}

fn release(t: &mut Term<'_>, b: LocatorButton) -> Vec<u8> {
    pointer(t, LocatorKind::Release(b), 7, 9)
}

/// **A DECLKD programs a key's press and release**: "Kyn / Stnd / Stnu
/// ... string parameter n for down and up key transitions ... a series of
/// ASCII hex pairs" (070-13 § 5.4), the press first as the standard and a
/// real VT340's test script send it (`[[source: vt340test-locatorprog]]`
/// loads `241~` then `242~`); a release with a string reports, in multiple
/// mode too. The bytes go as loaded, 8-bit ones included.
#[test]
fn a_declkd_programs_a_keys_press_and_release() {
    term(|t| {
        read(t, b"\x1bP1$w1/41/42;3/8F50\x1b\\\x1bPpR(I1)");
        answered(t);
        assert_eq!(press(t, LocatorButton::Left), b"A[7,9]\r");
        assert_eq!(release(t, LocatorButton::Left), b"B[7,9]\r");
        assert_eq!(press(t, LocatorButton::Right), b"\x8fP[7,9]\r");
        assert_eq!(release(t, LocatorButton::Right), b"", "no up string");
        assert_eq!(press(t, LocatorButton::Middle), b"\x1b[243~[7,9]\r");
    });
}

/// **`Pc` 0 empties every key, and an empty string does not end
/// one-shot**: "All button definition strings are empty (not the
/// default)", and "A locator transition for which the key definition is
/// empty will not terminate ReGIS One-Shot Graphics Input Mode". The null
/// button's `CSI 240 ~` is not one of the four keys and stays.
#[test]
fn pc_0_empties_every_key_and_an_empty_key_does_not_end_one_shot() {
    term(|t| {
        read(t, b"\x1bP0$w\x1b\\\x1bPpR(P(I))\x1b\\");
        assert_eq!(press(t, LocatorButton::Left), b"");
        assert_eq!(t.regis_input(), RegisInput::OneShot, "still waiting");
        assert_eq!(key(t, letter('k'), Modifiers::NONE), b"k[7,9]\r");
        let (reply, _) = read(t, b"\x1bPpR(I1)R(P(I))\x1b\\");
        assert_eq!(reply, b"\r\x1b[240~[0,0]\r", "the null button stays");
    });
}

/// **`Pc` 1 clears only the keys redefined**, both transitions of each.
#[test]
fn pc_1_clears_only_the_keys_redefined() {
    term(|t| {
        read(t, b"\x1bP1$w2/43\x1b\\\x1bPpR(I1)");
        answered(t);
        assert_eq!(press(t, LocatorButton::Middle), b"C[7,9]\r");
        assert_eq!(press(t, LocatorButton::Left), b"\x1b[241~[7,9]\r");
    });
}

/// **`RIS` restores the defaults and `DECSTR` keeps what was loaded**:
/// "A power-up restart or Reset to Initial State (RIS) will cause the LKDs
/// to be set to their default value. LKDs are not affected by Soft
/// Terminal Reset (DECSTR)."
#[test]
fn ris_restores_the_keys_and_decstr_keeps_them() {
    term(|t| {
        read(t, b"\x1bP1$w1/41\x1b\\\x1b[!p\x1bPpR(I1)");
        answered(t);
        assert_eq!(press(t, LocatorButton::Left), b"A[7,9]\r", "DECSTR kept it");
        read(t, b"\x1b\\\x1bc\x1bPpR(I1)");
        answered(t);
        assert_eq!(press(t, LocatorButton::Left), b"\x1b[241~[7,9]\r");
    });
}

/// **A DECLKD is read as DECUDK's loading reads one** (070-11's
/// `FILL_KEYS`): a key past 4 skips its definition, a broken hex pair
/// stops the rest, a string past six bytes is not stored and stops the
/// rest, a `Pc` past 1 refuses the string -- each counted.
#[test]
fn a_bad_declkd_is_refused_as_decudk_refuses_one() {
    let case = |load: &[u8], left: &[u8], middle: &[u8]| {
        term(|t| {
            read(t, &[load, b"\x1bPpR(I1)"].concat());
            answered(t);
            assert_eq!(press(t, LocatorButton::Left), left, "{:?}", load);
            assert_eq!(press(t, LocatorButton::Middle), middle, "{:?}", load);
            assert_eq!(t.locator_keys_refused().index(), 1, "{:?}", load);
        });
    };
    case(b"\x1bP1$w5/58;1/41\x1b\\", b"A[7,9]\r", b"\x1b[243~[7,9]\r");
    case(
        b"\x1bP1$w1/4;2/43\x1b\\",
        b"\x1b[241~[7,9]\r",
        b"\x1b[243~[7,9]\r",
    );
    case(b"\x1bP1$w2/43;1/4G\x1b\\", b"\x1b[241~[7,9]\r", b"C[7,9]\r");
    case(
        b"\x1bP1$w1/41424344454647;2/43\x1b\\",
        b"\x1b[241~[7,9]\r",
        b"\x1b[243~[7,9]\r",
    );
    case(b"\x1bP0$w1/41424344454647;2/43\x1b\\", b"", b"");
    case(
        b"\x1bP2$w1/41\x1b\\",
        b"\x1b[241~[7,9]\r",
        b"\x1b[243~[7,9]\r",
    );
}
