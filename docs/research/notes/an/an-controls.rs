//! Controls for note AN (DCS bodies and Sixel), committed in docs/research/notes/an/ and not
//! built there. Run in a git-archive copy of baud at dd4dbe5 as baud-core/tests/an_controls.rs:
//!   cargo test -p baud-core --test an_controls -- --nocapture
//!
//! Every line prints what baud does today. The lines marked "control" are the positive
//! controls the others are read against: a case whose answer is already known, run through
//! the same path, so that a probe that cannot fire shows up as a failed control.
//!
//! D1-D4: a DECRQSS whose Pt holds one byte (part 1 of the note).
//! S1-S20: a Sixel body holding one byte or one command shape (part 2).
mod support;
use baud_core::*;
use baud_domain::{ColorWord, ImageBudget, Row, Sixel};

const RED: &[u8] = b"#1;2;100;0;0#1";
const GREEN_THEN: &[u8] = b"#1;2;0;100;0#1";

fn show(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x1b => "ESC ".to_string(),
            0x20..=0x7e => (c as char).to_string(),
            c => format!("<{c:02X}>"),
        })
        .collect()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn row_text(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y))
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn colour(w: ColorWord) -> String {
    match (w.is_default(), w.as_rgb()) {
        (true, _) => "DEFAULT (transparent)".to_string(),
        (false, Some((r, g, b))) => format!("rgb({r},{g},{b})"),
        (false, None) => format!("{:#010x}", w.bits()),
    }
}

/// What a program left: placements, the first image's size and its top-left pixel, the
/// refusal count, bells, the reply, the cursor and row 0's text.
fn run(bytes: &[u8]) -> String {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(64 * 48 + 64, 8192));
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, bytes);
    let imgs = t.images();
    let image = imgs.placements().next().and_then(|(_, p)| {
        imgs.image(p.image).map(|v| {
            format!(
                "{}x{} px0 {}",
                v.width.raw(),
                v.height.raw(),
                colour(v.pixels[0])
            )
        })
    });
    format!(
        "placements {} image {} refused {} bells {} reply {:?} cursor {:?} row0 {:?}",
        imgs.placements().count(),
        image.unwrap_or_else(|| "-".to_string()),
        imgs.images_refused,
        t.bells,
        show(t.reply()),
        t.cursor(),
        row_text(&t, 0),
    )
}

fn sixel(body: &[&[u8]]) -> Vec<u8> {
    let mut v = b"\x1bPq".to_vec();
    for b in body {
        v.extend_from_slice(b);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

fn sixel_latin1(body: &[&[u8]]) -> Vec<u8> {
    let mut v = b"\x1b%@\x90q".to_vec();
    for b in body {
        v.extend_from_slice(b);
    }
    v.push(0x9c);
    v
}

fn line(tag: &str, what: &str, bytes: &[u8]) {
    println!("{tag} {what}: {}  <= {}", run(bytes), show(bytes));
}

#[test]
fn controls() {
    // D1-D3: DCS $ q m <b> ST, UTF-8.
    line("D1", "DECRQSS m (control)", b"\x1bP$qm\x1b\\");
    for (name, b) in [
        ("NUL", 0x00u8),
        ("SOH", 0x01),
        ("ENQ", 0x05),
        ("BEL", 0x07),
        ("BS", 0x08),
        ("CR", 0x0d),
        ("SO", 0x0e),
        ("US", 0x1f),
        ("DEL", 0x7f),
    ] {
        let mut q = b"\x1bP$qm".to_vec();
        q.push(b);
        q.extend_from_slice(b"\x1b\\");
        line("D2", &format!("DECRQSS m {name}"), &q);
    }
    // D3: under ISO 8859-1, 'm' with its eighth bit set (ED) in the body.
    line(
        "D3",
        "DECRQSS m, ISO 8859-1 (control)",
        b"\x1b%@\x1bP$qm\x1b\\",
    );
    line(
        "D3",
        "DECRQSS ED (m | 0x80), ISO 8859-1",
        b"\x1b%@\x1bP$q\xed\x1b\\",
    );
    // D4: GR in the introducer under ISO 8859-1: A4 = '$' | 0x80, F1 = 'q' | 0x80.
    line(
        "D4",
        "DCS A4 F1 m ST, ISO 8859-1",
        b"\x1b%@\x1bP\xa4\xf1m\x1b\\",
    );

    // S1: the control image.
    line("S1", "red, one data byte (control)", &sixel(&[RED, b"~"]));
    // S2-S4: SPACE.
    line("S2", "!10~ (control)", &sixel(&[RED, b"!10~"]));
    line("S2", "!1 SPACE 0~", &sixel(&[RED, b"!1 0~"]));
    line("S3", "~ SPACE ~ (between data)", &sixel(&[RED, b"~ ~"]));
    line(
        "S4",
        "#1 SPACE ;2;100;0;0#1~",
        &sixel(&[b"#1 ;2;100;0;0#1~"]),
    );
    // S5: a format effector and DEL inside a number (built at dd4dbe5): controls.
    line("S5", "!1 CR 0~ (control, built)", &sixel(&[RED, b"!1\r0~"]));
    line(
        "S5",
        "!1 DEL 0~ (control, built)",
        &sixel(&[RED, b"!1\x7f0~"]),
    );
    // S6: a pending repeat and the command after it.
    line("S6", "!5~ (control)", &sixel(&[RED, b"!5~"]));
    line(
        "S6",
        "!5#1~ (recognised command, control)",
        &sixel(&[RED, b"!5#1~"]),
    );
    line("S6", "!5%~ (unrecognised command)", &sixel(&[RED, b"!5%~"]));
    line("S6", "!5:~ (unrecognised command)", &sixel(&[RED, b"!5:~"]));
    // S7: repeat counts, through the decoder's own extent walk.
    for body in [&b"!0~"[..], b"!32767~", b"!32768~", b"!40000~", b"!70000~"] {
        println!(
            "S7 extent of {}: width {}",
            show(body),
            Sixel::extent(body).width.raw()
        );
    }
    // S8: no colour defined.
    line("S8", "~ with no # at all", &sixel(&[b"~"]));
    line("S8", "#1~, register 1 never defined", &sixel(&[b"#1~"]));
    // S9: colour commands STD 070-9 says to ignore whole, after a green register 1.
    line("S9", "green (control)", &sixel(&[GREEN_THEN, b"~"]));
    line(
        "S9",
        "then #5;2;150;0;0 (RGB above 100)",
        &sixel(&[GREEN_THEN, b"#5;2;150;0;0~"]),
    );
    line(
        "S9",
        "then #5;3;10;10;10 (Pu 3)",
        &sixel(&[GREEN_THEN, b"#5;3;10;10;10~"]),
    );
    line(
        "S9",
        "then #5;0;10;10;10 (Pu 0)",
        &sixel(&[GREEN_THEN, b"#5;0;10;10;10~"]),
    );
    line(
        "S9",
        "then #5;1;400;50;50 (hue above 360)",
        &sixel(&[GREEN_THEN, b"#5;1;400;50;50~"]),
    );
    // S10: a register past 255.
    line(
        "S10",
        "#300;2;100;0;0#300~",
        &sixel(&[b"#300;2;100;0;0#300~"]),
    );
    // S11: GR under ISO 8859-1.
    line(
        "S11",
        "7E under ISO 8859-1 (control)",
        &sixel_latin1(&[RED, b"~"]),
    );
    line(
        "S11",
        "FE (~ | 0x80) under ISO 8859-1",
        &sixel_latin1(&[RED, b"\xfe"]),
    );
    line(
        "S11",
        "! B1 B0 ~ (digits | 0x80) under ISO 8859-1",
        &sixel_latin1(&[RED, b"!\xb1\xb0~"]),
    );
    line("S11", "FE under UTF-8", &sixel(&[RED, b"\xfe"]));
    // S12: BEL.
    line("S12", "BEL in text (control)", b"\x07");
    line(
        "S12",
        "BEL between data in a Sixel",
        &sixel(&[RED, b"~\x07~"]),
    );
    // S13: ENQ.
    line("S13", "ENQ in text", b"\x05");
    line("S13", "!1 ENQ 0~ in a Sixel", &sixel(&[RED, b"!1\x050~"]));
    // S14: SUB.
    line(
        "S14",
        "~~ ? ~~ (what 070-9 reads SUB as, control)",
        &sixel(&[RED, b"~~?~~"]),
    );
    line("S14", "~~ SUB ~~", &sixel(&[RED, b"~~\x1a~~"]));
    // S15: CAN after data, then text.
    line(
        "S15",
        "~~ CAN, then Z",
        &[
            sixel(&[RED, b"~~"]).strip_suffix(b"\x1b\\").unwrap(),
            b"\x18Z",
        ]
        .concat(),
    );
    // S16: ESC that is not ST, after data.
    line(
        "S16",
        "~~ ESC [5C",
        &[
            sixel(&[RED, b"~~"]).strip_suffix(b"\x1b\\").unwrap(),
            b"\x1b[5C",
        ]
        .concat(),
    );
    // S17: a C1 under ISO 8859-1, after data: 85 is NEL.
    line(
        "S17",
        "~~ 85 (NEL) under ISO 8859-1",
        &[&b"\x1b%@\x90q"[..], RED, b"~~\x85Z"].concat(),
    );
    // S18: XON and XOFF inside a number.
    line(
        "S18",
        "!1 DC1 0~ and !1 DC3 0~",
        &sixel(&[RED, b"!1\x110~!1\x130~"]),
    );
    // S19: the aspect ratio.
    line("S19", "no raster attributes, no P1", &sixel(&[RED, b"~"]));
    line(
        "S19",
        "\"2;1;1;6, a 2:1 pixel",
        &sixel(&[b"\"2;1;1;6", RED, b"~"]),
    );
    line(
        "S19",
        "P1 = 9 (1:1 in both DEC texts)",
        b"\x1bP9q#1;2;100;0;0#1~\x1b\\",
    );
    // S20: NUL inside a number.
    line("S20", "!1 NUL 0~", &sixel(&[RED, b"!1\x000~"]));
}
