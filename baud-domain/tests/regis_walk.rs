//! **ReGIS read a character at a time, against `[[source: dec-std-070]]`
//! 070-8's own examples.**
//!
//! A recording [`RegisOut`] stands in for the page, so what is held here is
//! the grammar and the device state: positions, the stack, macrographs,
//! quoting, numerics, the option nesting and what each instruction asks to
//! have drawn or answered.

use baud_domain::Regis;
use baud_domain::{
    GlyphSource, LineSpan, RegisEntry, RegisGlyph, RegisIndex, RegisNum, RegisOut, RegisPen,
    RegisPoint, RegisRect, RegisTicks, StreamByte, WriteMode,
};
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq)]
enum Op {
    Line(RegisPoint, RegisPoint),
    Fill(Vec<RegisPoint>, Vec<u16>),
    Glyph(Glyph, RegisPoint, (i32, i32)),
    Erase(RegisIndex),
    Recolor,
    Reply(Vec<u8>),
    Delay(u32),
    Interactive(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
enum Glyph {
    Builtin(char),
    Loaded(Vec<u16>, u8),
    Error,
}

#[derive(Default)]
struct Rec {
    ops: Vec<Op>,
    pens: Vec<RegisPen>,
}

impl RegisOut for Rec {
    fn line(&mut self, a: RegisPoint, b: RegisPoint, pen: &RegisPen, phase: &mut u32, _: LineSpan) {
        *phase += 1;
        self.ops.push(Op::Line(a, b));
        self.pens.push(*pen);
    }
    fn fill(&mut self, points: &[RegisPoint], ends: &[u16], pen: &RegisPen) {
        self.ops.push(Op::Fill(points.to_vec(), ends.to_vec()));
        self.pens.push(*pen);
    }
    fn glyph(&mut self, g: &RegisGlyph<'_>, pen: &RegisPen) {
        let s = match g.source {
            GlyphSource::Builtin { ch, .. } => Glyph::Builtin(ch),
            GlyphSource::Loaded { rows, w } => Glyph::Loaded(rows.to_vec(), w),
            GlyphSource::Error => Glyph::Error,
        };
        self.ops.push(Op::Glyph(s, g.at, g.unit));
        self.pens.push(*pen);
    }
    fn erase(&mut self, bg: RegisIndex) {
        self.ops.push(Op::Erase(bg));
    }
    fn recolor(&mut self) {
        self.ops.push(Op::Recolor);
    }
    fn reply(&mut self, bytes: &[u8]) {
        self.ops.push(Op::Reply(bytes.to_vec()));
    }
    fn delay(&mut self, ticks: RegisTicks) {
        self.ops.push(Op::Delay(ticks.get()));
    }
    fn interactive(&mut self, pos: &[u8]) {
        self.ops.push(Op::Interactive(pos.to_vec()));
    }
    fn input_shown(&mut self) {}
    fn unmark(&mut self, _: RegisRect) {}
    fn scroll(&mut self, _: baud_domain::RegisShift, _: RegisIndex) {}
}

fn run(body: &[u8]) -> (Box<Regis>, Rec) {
    let mut r = Box::new(Regis::NEW);
    let mut rec = Rec::default();
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in body {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    (r, rec)
}

fn replies(rec: &Rec) -> Vec<Vec<u8>> {
    rec.ops
        .iter()
        .filter_map(|o| match o {
            Op::Reply(b) => Some(b.clone()),
            _ => None,
        })
        .collect()
}

fn lines(rec: &Rec) -> Vec<(RegisPoint, RegisPoint)> {
    rec.ops
        .iter()
        .filter_map(|o| match o {
            Op::Line(a, b) => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

fn pt(x: i32, y: i32) -> RegisPoint {
    RegisPoint { x, y }
}

fn at(r: &Regis) -> (i32, i32) {
    let (x, y) = r.position();
    (x.round(), y.round())
}

// ---------------------------------------------------------------------------
// Position arguments, § 8.4.5.1 and its table on p. 8-53.
// ---------------------------------------------------------------------------

#[test]
fn the_position_arguments_resolve_as_the_tables_say() {
    // "assume the initial values of X and Y are 100 and 50".
    let cases: &[(&[u8], (i32, i32))] = &[
        (b"[0,0]", (0, 0)),
        (b"[30,23]", (30, 23)),
        (b"[200]", (200, 50)),
        (b"[,42]", (100, 42)),
        (b"[+10,-25]", (110, 25)),
        (b"[0,+10]", (0, 60)),
        (b"[15,20]", (15, 20)),
        (b"[10,20][+5]", (15, 20)),
    ];
    for &(arg, want) in cases {
        let mut body = b"P[100,50]P".to_vec();
        body.extend_from_slice(arg);
        let (r, _) = run(&body);
        assert_eq!(at(&r), want, "{}", String::from_utf8_lossy(arg));
    }
}

#[test]
fn pixel_vectors_move_as_the_table_on_8_55_says() {
    // Multiplier, pixel string, new position, from [100,50].
    let cases: &[(u32, &[u8], (i32, i32))] = &[
        (1, b"0", (101, 50)),
        (1, b"1", (101, 49)),
        (1, b"0002", (103, 49)),
        (7, b"01234567", (100, 50)),
        (5, b"667", (105, 65)),
        (23, b"5", (77, 73)),
        (1, b"0000000000", (110, 50)),
    ];
    for &(m, pv, want) in cases {
        let mut body = format!("W(M{m})P[100,50]").into_bytes();
        body.extend_from_slice(pv);
        let (r, _) = run(&body);
        assert_eq!(at(&r), want, "M{m} {}", String::from_utf8_lossy(pv));
    }
}

#[test]
fn a_pixel_vector_is_rightward_whatever_the_address_space() {
    // "pixel vector 0 is always to the right, even if the +X direction has
    // been defined to be to the left."
    let (r, _) = run(b"S(A[799,0][0,479])P[100,50]0");
    assert_eq!(at(&r), (99, 50));
    let (_, rec) = run(b"S(A[799,0][0,479])P[100,50]V0");
    assert_eq!(lines(&rec), vec![(pt(699, 50), pt(700, 50))]);
}

#[test]
fn the_address_space_maps_its_corners_to_the_screens() {
    let (_, rec) = run(b"S(A[0,0][1599,959])P[0,0]V[1599,959]");
    assert_eq!(lines(&rec), vec![(pt(0, 0), pt(799, 479))]);
    // The VT125's addressing on the VT300: "maps the default screen
    // addressing of the VT125 into the leftmost 768 pixel columns".
    let (_, rec) = run(b"S(A[0,0][767,479])P[767,479]V[0,0]");
    assert_eq!(lines(&rec), vec![(pt(767, 479), pt(0, 0))]);
}

#[test]
fn an_invalid_address_space_is_ignored() {
    for body in [
        &b"S(A[0,0])P[799,479]V[0,0]"[..],
        b"S(A[0,0][,479])P[799,479]V[0,0]",
        b"S(A[-5,0][799,479])P[799,479]V[0,0]",
        b"S(A[0,0][0,479])P[799,479]V[0,0]",
    ] {
        let (r, rec) = run(body);
        assert_eq!(lines(&rec), vec![(pt(799, 479), pt(0, 0))]);
        assert_eq!(r.counts().space_refused, 1);
    }
}

// ---------------------------------------------------------------------------
// Vectors and the position stack.
// ---------------------------------------------------------------------------

#[test]
fn a_vector_draws_from_the_position_and_an_empty_bracket_is_a_dot() {
    let (r, rec) = run(b"P[10,10]V[20,10][,20][]");
    assert_eq!(
        lines(&rec),
        vec![
            (pt(10, 10), pt(20, 10)),
            (pt(20, 10), pt(20, 20)),
            (pt(20, 20), pt(20, 20)),
        ]
    );
    assert_eq!(at(&r), (20, 20));
}

#[test]
fn a_bounded_block_closes_the_polygon_and_a_dummy_draws_nothing() {
    let (_, rec) = run(b"P[10,10]V(B)[20,10][20,20](E)");
    assert_eq!(lines(&rec).last(), Some(&(pt(20, 20), pt(10, 10))));
    let (r, rec) = run(b"P[10,10]V(S)[20,10](E)");
    assert_eq!(lines(&rec).len(), 1);
    assert_eq!(at(&r), (20, 10));
    // "P(E) causes the drawing position to be restored".
    let (r, _) = run(b"P[10,10](B)[300,300](E)");
    assert_eq!(at(&r), (10, 10));
}

#[test]
fn the_stack_holds_sixteen_and_counts_the_rest() {
    let mut body = b"P".to_vec();
    for _ in 0..17 {
        body.extend_from_slice(b"(B)");
    }
    body.extend_from_slice(b"R(E)");
    let (r, rec) = run(&body);
    assert_eq!(r.counts().stack_overflow, 1);
    // Error 7, "begin/start overflow".
    assert_eq!(replies(&rec), vec![b"\"7,0\"\r".to_vec()]);
    let (r, rec) = run(b"P(E)R(E)");
    assert_eq!(r.counts().stack_underflow, 1);
    assert_eq!(replies(&rec), vec![b"\"8,0\"\r".to_vec()]);
}

// ---------------------------------------------------------------------------
// Numerics, quoting and the nesting.
// ---------------------------------------------------------------------------

#[test]
fn an_e_after_a_number_is_its_exponent_where_it_can_be() {
    // "the interpretation of (Y1E2) would be as (Y100)".
    let (r, _) = run(b"W(M1E2)P[0,0]0");
    assert_eq!(at(&r), (100, 0));
    // An `E` with no digit after it is the option letter.
    let (_, rec) = run(b"S(I0E)");
    assert_eq!(rec.ops, vec![Op::Erase(RegisIndex::ZERO)]);
}

#[test]
fn fractions_are_kept_until_the_number_is_whole() {
    let (r, _) = run(b"P[1.02e2,.1E3]");
    assert_eq!(at(&r), (102, 100));
    let (r, _) = run(b"P[6.25,10.4]");
    let (x, _) = r.position();
    assert_eq!(x.raw(), RegisNum::int(6).raw() + 65536 / 4);
}

#[test]
fn spaces_and_controls_outside_quotes_change_nothing() {
    let a = run(b"P[100,50]V[200,50]").1.ops;
    let b = run(b"P [ 1\r\n00 , 5 0 ] V\t[200,\x0050]").1.ops;
    assert_eq!(a, b);
}

#[test]
fn quoted_strings_double_their_quote_and_controls_decide() {
    fn text(body: &[u8]) -> String {
        run(body)
            .1
            .ops
            .iter()
            .filter_map(|o| match o {
                Op::Glyph(Glyph::Builtin(c), ..) => Some(*c),
                _ => None,
            })
            .collect()
    }
    assert_eq!(text(b"T'don''t'"), "don't");
    assert_eq!(text(b"T\"ditto (\"\")\""), "ditto (\")");
    assert_eq!(text(b"T\"Stop \",\"Here\""), "Stop Here");
    // "if in the command stream T"text"<>"moretext" the token <> were
    // replaced by one of SP, BS, HT, LF, or CR ... T"textmoretext""; by any
    // other control, the quote is doubled.
    assert_eq!(text(b"T\"text\"\r\"more\""), "textmore");
    assert_eq!(text(b"T\"text\"\x01\"more\""), "text\"more");
    // A macrograph definition between them does not double them.
    assert_eq!(text(b"T\"text\"@:Ax@;\"more\""), "textmore");
}

#[test]
fn a_seventh_option_level_resynchronises() {
    // "X(A(B(C(D(E(F))))))" is the deepest legal nesting; one more pops to
    // the command level.
    let (r, rec) = run(b"P[5,5]S(A(B(C(D(E(F(G))))))) V[9,9]");
    assert_eq!(r.counts().nesting_overflow, 1);
    // The `V` after it is a command again.
    assert_eq!(lines(&rec), vec![(pt(5, 5), pt(9, 9))]);
    let (r, _) = run(b"S(A(B(C(D(E(F))))))");
    assert_eq!(r.counts().nesting_overflow, 0);
}

#[test]
fn a_semicolon_leaves_no_command_in_force() {
    // "the following argument is ignored as having no context".
    let (_, rec) = run(b"P[10,10]V[20,20];[30,30]");
    assert_eq!(lines(&rec), vec![(pt(10, 10), pt(20, 20))]);
}

#[test]
fn an_unknown_command_takes_its_arguments_with_it() {
    let (r, rec) = run(b"P[10,10]V[20,20]Z[30,30]V[40,40]");
    assert_eq!(
        lines(&rec),
        vec![(pt(10, 10), pt(20, 20)), (pt(20, 20), pt(40, 40))]
    );
    assert_eq!(r.counts().ignored, 1);
}

// ---------------------------------------------------------------------------
// Macrographs, § 8.4.4.
// ---------------------------------------------------------------------------

#[test]
fn a_macrograph_is_defined_once_and_played_where_invoked() {
    let (_, rec) = run(b"@:AV[+10,+0]@;P[0,0]@A@a");
    assert_eq!(
        lines(&rec),
        vec![(pt(0, 0), pt(10, 0)), (pt(10, 0), pt(20, 0))]
    );
}

#[test]
fn a_definition_anywhere_leaves_the_parser_where_it_was() {
    // "A macrograph definition may even occur within the extent of a
    // numeric argument without adversely affecting the interpretation".
    let (r, _) = run(b"P[1@:Bjunk@;00,5@:C@;0]");
    assert_eq!(at(&r), (100, 50));
}

#[test]
fn a_macrograph_may_play_a_piece_of_an_argument() {
    let (r, _) = run(b"@:X25@;P[1@X,@X]");
    assert_eq!(at(&r), (125, 25));
}

#[test]
fn a_macrograph_never_plays_itself() {
    let (r, rec) = run(b"@:AV[+1]@B@;@:BV[+1]@A@;P[0,0]@A");
    assert_eq!(lines(&rec).len(), 2);
    assert_eq!(r.counts().macro_recursion, 1);
}

#[test]
fn macrographs_nest_sixteen_deep() {
    let mut body = Vec::new();
    // A calls B calls ... calls Q: seventeen levels.
    for i in 0..16u8 {
        body.extend_from_slice(
            format!("@:{}V[+1]@{}@;", (b'A' + i) as char, (b'B' + i) as char).as_bytes(),
        );
    }
    body.extend_from_slice(b"@:QV[+1]@;P[0,0]@A");
    let (r, rec) = run(&body);
    assert_eq!(lines(&rec).len(), 16);
    assert_eq!(r.counts().macro_nesting, 1);
}

#[test]
fn a_full_store_fails_soft() {
    // "No other macrographs already stored will be affected. Any part ...
    // which is already stored will remain stored. The remainder of the
    // attempted definition will be discarded".
    let mut body = b"@:AV[+1]@;@:B".to_vec();
    body.extend(std::iter::repeat_n(b'1', 12_000));
    body.extend_from_slice(b"@;P[0,0]@AR(M(=))");
    let (r, rec) = run(&body);
    assert_eq!(r.counts().macro_overflow, 1);
    assert_eq!(lines(&rec).len(), 1);
    assert_eq!(replies(&rec), vec![b"\"0,10000\"\r".to_vec()]);
}

#[test]
fn a_redefinition_clears_first_and_at_dot_clears_all() {
    let (_, rec) = run(b"@:AV[+1]@;@:AV[+2]@;P[0,0]@A");
    assert_eq!(lines(&rec), vec![(pt(0, 0), pt(2, 0))]);
    let (r, rec) = run(b"@:AV[+1]@;@.P[0,0]@AR(M(=))");
    assert_eq!(lines(&rec), vec![]);
    assert_eq!(r.macro_free(), 10_000);
    assert_eq!(replies(&rec), vec![b"\"10000,10000\"\r".to_vec()]);
}

#[test]
fn an_at_inside_a_quoted_string_is_a_character() {
    let (_, rec) = run(b"@:AV[+1]@;T'a@Ab'");
    let n = rec
        .ops
        .iter()
        .filter(|o| matches!(o, Op::Glyph(..)))
        .count();
    assert_eq!(n, 4);
}

#[test]
fn a_macrograph_report_is_empty_whatever_it_holds() {
    // The owner's call, 2026-10-20: "macrograph reports answered empty as
    // 070-8 prefers" -- "The preferred fallback is the empty report, that
    // is, just the terminating CR character".
    let (_, rec) = run(b"@:Asecret@;R(M(A))R(M(B))R(M(AB))");
    assert_eq!(replies(&rec), vec![b"\r".to_vec(); 4]);
}

// ---------------------------------------------------------------------------
// Reports, § 8.5.7.
// ---------------------------------------------------------------------------

#[test]
fn the_position_report_is_in_the_imposed_space() {
    let (_, rec) = run(b"P[123,45]R(P)S(A[0,0][1599,959])P[1599,+10]R(P)");
    assert_eq!(
        replies(&rec),
        vec![b"[123,45]\r".to_vec(), b"[1599,10]\r".to_vec()]
    );
}

#[test]
fn an_interactive_position_report_without_a_hold_is_the_plain_one() {
    // § 8.8.5.3's first response, "the result from a non-interactive device",
    // for a caller that holds nothing.
    let (r, rec) = run(b"P[7,8]R(P(I))");
    assert_eq!(replies(&rec), vec![b"[7,8]\r".to_vec()]);
    assert_eq!(r.counts().inputs_unheld, 1);
}

/// **Under a hold a one-shot `R(P(I))` answers nothing and waits**, the
/// next character held; **in multiple mode it asks the caller for the
/// null button's report at once**, after `R(I1)`'s carriage return.
#[test]
fn an_interactive_report_waits_in_one_shot_and_goes_at_once_in_multiple() {
    let mut r = Box::new(Regis::NEW);
    r.set_hold(baud_domain::RegisHold::On);
    let mut rec = Rec::default();
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in b"P[7,8]R(P(I))" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    assert_eq!(rec.ops, vec![]);
    assert!(r.held(), "the next character is held");
    r.input_end();
    assert!(!r.held());
    let (r, rec) = run(b"P[7,8]R(I1)R(P(I))");
    assert_eq!(
        rec.ops,
        vec![
            Op::Reply(b"\r".to_vec()),
            Op::Interactive(b"[7,8]".to_vec())
        ]
    );
    assert_eq!(r.input_mode(), baud_domain::RegisInputMode::Multiple);
}

#[test]
fn an_unimplemented_report_is_a_carriage_return() {
    // "Requests for unimplemented reports are acknowledged by the device's
    // returning just a carriage return".
    let (_, rec) = run(b"R(Q)R(I0)");
    assert_eq!(replies(&rec), vec![b"\r".to_vec(), b"\r".to_vec()]);
}

#[test]
fn the_loading_alphabet_is_reported_by_name() {
    let (_, rec) = run(b"L(A2\"FONT\")R(L)L(A3)R(L)");
    assert_eq!(
        replies(&rec),
        vec![b"(A'FONT')\r".to_vec(), b"(A'')\r".to_vec()]
    );
}

#[test]
fn a_semicolon_clears_the_error_report() {
    let (_, rec) = run(b"P(E)R(E);R(E)");
    assert_eq!(
        replies(&rec),
        vec![b"\"8,0\"\r".to_vec(), b"\"0,0\"\r".to_vec()]
    );
}

// ---------------------------------------------------------------------------
// Curves, § 8.5.5.
// ---------------------------------------------------------------------------

fn endpoints(rec: &Rec) -> (RegisPoint, RegisPoint) {
    let l = lines(rec);
    (l.first().unwrap().0, l.last().unwrap().1)
}

#[test]
fn a_circle_goes_round_its_centre_and_leaves_the_position() {
    let (r, rec) = run(b"P[400,240]C[+100]");
    let (a, b) = endpoints(&rec);
    assert_eq!(a, pt(500, 240));
    assert_eq!(b, pt(500, 240));
    // Every point is a hundred pixels from the centre, within the pixel it
    // rounds to.
    for (p, _) in lines(&rec) {
        let d = (((p.x - 400).pow(2) + (p.y - 240).pow(2)) as f64).sqrt();
        assert!((d - 100.0).abs() < 0.71, "{p:?}");
    }
    assert_eq!(at(&r), (400, 240));
}

#[test]
fn an_arc_turns_counterclockwise_as_the_viewer_sees_it() {
    // From the right of the centre a quarter turn counterclockwise is the
    // top: up the screen.
    let (r, rec) = run(b"P[400,240]C(A90)[+100]");
    let (a, b) = endpoints(&rec);
    assert_eq!((a, b), (pt(500, 240), pt(400, 140)));
    assert_eq!(at(&r), (400, 240));
    let (_, rec) = run(b"P[400,240]C(A-90)[+100]");
    assert_eq!(endpoints(&rec).1, pt(400, 340));
    // "Inverting the address space by changing the sense defined in S(A)
    // does not change angle orientation".
    let (_, rec) = run(b"S(A[0,479][799,0])P[400,239]C(A90)[+100]");
    assert_eq!(endpoints(&rec), (pt(500, 240), pt(400, 140)));
}

#[test]
fn a_circumferential_arc_leaves_the_position_at_its_end() {
    // "the drawing position defines the start point of the arc, and is left
    // at the end of the arc when it is completed."
    let (r, rec) = run(b"P[500,240]C(CA90)[400,240]");
    assert_eq!(endpoints(&rec), (pt(500, 240), pt(400, 140)));
    assert_eq!(at(&r), (400, 140));
    // A whole circle's end is its start.
    let (r, _) = run(b"P[500,240]C(C)[400,240]");
    assert_eq!(at(&r), (500, 240));
}

#[test]
fn an_arc_past_a_turn_is_a_turn() {
    let a = run(b"P[400,240]C(A720)[+50]").1.ops;
    let b = run(b"P[400,240]C(A360)[+50]").1.ops;
    assert_eq!(a, b);
}

#[test]
fn an_open_curve_draws_from_its_second_point_to_its_penultimate() {
    // "at least three position arguments in addition to the current cursor
    // position must be given to result in a visible curve segment between
    // the second and third positions".
    let (_, rec) = run(b"P[100,100]C(S)[200,150][300,100](E)");
    assert!(lines(&rec).is_empty());
    let (r, rec) = run(b"P[100,100]C(S)[200,150][300,100][400,150](E)");
    assert_eq!(endpoints(&rec), (pt(200, 150), pt(300, 100)));
    // "the current cursor position is always maintained at the last
    // position argument".
    assert_eq!(at(&r), (400, 150));
}

#[test]
fn four_collinear_points_draw_a_straight_segment() {
    let (_, rec) = run(b"P[100,100]C(S)[200,100][300,100][400,100](E)");
    for (a, b) in lines(&rec) {
        assert_eq!((a.y, b.y), (100, 100));
        assert!((200..=300).contains(&a.x) && (200..=300).contains(&b.x));
    }
}

#[test]
fn a_curve_is_the_same_curve_backwards() {
    let fwd = lines(&run(b"P[100,100]C(S)[200,180][300,90][420,160][500,100](E)").1);
    let back = lines(&run(b"P[500,100]C(S)[420,160][300,90][200,180][100,100](E)").1);
    let mut f: Vec<_> = fwd.iter().flat_map(|&(a, b)| [a, b]).collect();
    let mut g: Vec<_> = back.iter().flat_map(|&(a, b)| [a, b]).collect();
    f.sort();
    f.dedup();
    g.sort();
    g.dedup();
    assert_eq!(f, g);
}

#[test]
fn a_closed_curve_returns_to_its_start() {
    let (r, rec) = run(b"P[400,100]C(B)[500,240][400,380][300,240](E)");
    let l = lines(&rec);
    // Four segments of a closed loop through four points: every point is
    // on it and it ends where it began.
    let pts: Vec<_> = l.iter().flat_map(|&(a, b)| [a, b]).collect();
    for p in [pt(400, 100), pt(500, 240), pt(400, 380), pt(300, 240)] {
        assert!(pts.contains(&p), "{p:?}");
    }
    assert_eq!(at(&r), (400, 100));
    // "The occurrence of the second C in this stream returns the ReGIS
    // parser to command level, however, voiding the actions of the (B)
    // option and resulting in some number of concentric circles."
    let (_, rec) = run(b"P[400,240]C(B)[+10]C[+20]");
    assert!(endpoints(&rec).0 == pt(420, 240));
}

// ---------------------------------------------------------------------------
// Fill, § 8.5.8.
// ---------------------------------------------------------------------------

fn fills(rec: &Rec) -> Vec<(Vec<RegisPoint>, Vec<u16>)> {
    rec.ops
        .iter()
        .filter_map(|o| match o {
            Op::Fill(p, e) => Some((p.clone(), e.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_fill_takes_its_vertices_from_what_v_would_draw() {
    let (r, rec) = run(b"P[10,10]F(V[100,10][100,100][10,100])");
    assert_eq!(
        fills(&rec),
        vec![(vec![pt(100, 10), pt(100, 100), pt(10, 100)], vec![3])]
    );
    // No perimeter, and the position is restored.
    assert!(lines(&rec).is_empty());
    assert_eq!(at(&r), (10, 10));
    // `V[]` puts the position in the list; `P` moves without a vertex.
    let (_, rec) = run(b"P[10,10]F(V[][100,10]P[500,500]V[100,100])");
    assert_eq!(
        fills(&rec)[0].0,
        vec![pt(10, 10), pt(100, 10), pt(100, 100)]
    );
}

#[test]
fn a_fill_cut_by_a_semicolon_draws_nothing() {
    let (r, rec) = run(b"F(V[100,10][100,100][10,100];");
    assert!(fills(&rec).is_empty());
    assert_eq!(r.counts().fill_abandoned, 1);
}

#[test]
fn a_complex_polygon_keeps_its_contours_apart() {
    let (_, rec) = run(b"F(V[0,0][100,0][100,100][0,100]F(V[25,25][75,25][75,75]))");
    let (p, e) = &fills(&rec)[0];
    assert_eq!(p.len(), 7);
    assert_eq!(e, &vec![4, 7]);
}

#[test]
fn a_filled_circle_is_its_outline_s_vertices() {
    let (_, rec) = run(b"P[400,240]F(C[+50])");
    let (p, _) = &fills(&rec)[0];
    assert!(p.len() > 16);
    for q in p {
        let d = (((q.x - 400).pow(2) + (q.y - 240).pow(2)) as f64).sqrt();
        assert!((d - 50.0).abs() < 0.71, "{q:?}");
    }
}

// ---------------------------------------------------------------------------
// Writing controls, § 8.5.3 and § 8.7.2.
// ---------------------------------------------------------------------------

#[test]
fn temporary_write_options_last_one_instruction() {
    let (_, rec) = run(b"V(W(R,I2))[10,10]V[20,20]");
    assert_eq!(rec.pens[0].mode, WriteMode::Replace);
    assert_eq!(rec.pens[0].fg.raw(), 2);
    assert_eq!(rec.pens[1].mode, WriteMode::Overlay);
    assert_eq!(rec.pens[1].fg.raw(), 7);
}

#[test]
fn a_colour_by_value_is_the_closest_entry() {
    // The VT340's map: 2 is red, 3 green, 1 blue.
    let (_, rec) = run(b"W(I(R))V[1]W(I(G))V[2]W(I(H0L50S60))V[3]");
    let fg: Vec<u8> = rec.pens.iter().map(|p| p.fg.raw()).collect();
    assert_eq!(fg, vec![2, 3, 1]);
}

#[test]
fn an_index_past_the_map_wraps() {
    let (_, rec) = run(b"W(I18)V[1]");
    assert_eq!(rec.pens[0].fg.raw(), 2);
}

#[test]
fn the_output_map_changes_where_its_group_closes() {
    let (r, rec) = run(b"S(M2(G)5(H120L50S100))");
    assert_eq!(rec.ops, vec![Op::Recolor, Op::Recolor]);
    let (gr, gg, gb) = r.colors()[2].rgb();
    assert_eq!((gr.get(), gg.get(), gb.get()), (0, 255, 0));
    let (rr, rg, rb) = r.colors()[5].rgb();
    assert_eq!((rr.get(), rg.get(), rb.get()), (255, 0, 0));
}

#[test]
fn patterns_are_the_vt300s_eight_bit_memory() {
    let (_, rec) = run(b"W(P101)V[1]W(P2)V[2]W(P1101101101)V[3]W(P4(M5))V[4]");
    let bits: Vec<u8> = rec.pens.iter().map(|p| p.pattern.bits()).collect();
    assert_eq!(
        bits,
        vec![0b1011_0110, 0b1111_0000, 0b0110_1101, 0b1010_1010]
    );
    assert_eq!(rec.pens[3].pattern_mul, 5);
}

#[test]
fn erase_uses_the_background_and_clears_the_stack() {
    let (r, rec) = run(b"P(B)S(I3)S(E)P(E)");
    assert_eq!(rec.ops[0], Op::Erase(RegisIndex::wrap(3)));
    assert_eq!(r.counts().stack_underflow, 1);
}

#[test]
fn a_time_delay_asks_for_its_ticks() {
    let (_, rec) = run(b"S(T60,T30)");
    assert_eq!(rec.ops, vec![Op::Delay(60), Op::Delay(30)]);
}

/// **The played bound is the host character's, held or not**: a
/// macrograph a delay stops and resumes is cut where the same macrograph
/// played through is, the same lines drawn.
#[test]
fn a_held_macrograph_is_cut_where_one_played_through_is() {
    // `B` is 72 characters and `A` plays it 1,000 times: 74,000 played
    // for one `A`, past the bound of 65,536.
    let mut body = b"@:BS(T1)V[+1,0]".to_vec();
    body.extend([b' '; 57]);
    body.extend_from_slice(b"@;@:A");
    for _ in 0..1000 {
        body.extend_from_slice(b"@B");
    }
    body.extend_from_slice(b"@;@A");
    let lines = |rec: &Rec| rec.ops.iter().filter(|o| matches!(o, Op::Line(..))).count();
    let (off, rec_off) = run(&body);
    let mut r = Box::new(Regis::NEW);
    let mut rec = Rec::default();
    r.set_hold(baud_domain::RegisHold::On);
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in &body {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    let mut resumes = 0;
    while r.playing() {
        assert!(r.take_wait().is_some());
        r.resume(&mut rec);
        resumes += 1;
    }
    assert_eq!(
        off.counts().macro_cut,
        1,
        "the control: played through, it is cut"
    );
    assert_eq!(r.counts().macro_cut, 1);
    assert_eq!(lines(&rec), lines(&rec_off));
    assert!(lines(&rec) < 1000, "{}", lines(&rec));
    assert!(resumes > 800, "{resumes}");
}

/// **Held, a delay stops a macrograph where it is**: the ticks are the
/// caller's to take, the rest plays on `resume`, and off, the delay is
/// counted and the macrograph plays through.
#[test]
fn a_held_delay_stops_a_macrograph_until_it_resumes() {
    let mut r = Box::new(Regis::NEW);
    let mut rec = Rec::default();
    r.set_hold(baud_domain::RegisHold::On);
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in b"@:AS(T5)V[10,0]S(T7)V[20,0]@;@A" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    let lines = |rec: &Rec| rec.ops.iter().filter(|o| matches!(o, Op::Line(..))).count();
    assert!(r.playing());
    assert_eq!(r.take_wait(), Some(RegisTicks::of(5)));
    assert_eq!(r.take_wait(), None, "taken once");
    assert_eq!(lines(&rec), 0);
    r.resume(&mut rec);
    assert_eq!((r.take_wait(), lines(&rec)), (Some(RegisTicks::of(7)), 1));
    r.resume(&mut rec);
    assert_eq!((r.take_wait(), lines(&rec)), (None, 2));
    assert!(!r.playing());
    assert_eq!(r.counts().delays_unheld, 0);
    // Off, the default: one put plays it through, each delay counted.
    let (r, rec) = run(b"@:AS(T5)V[10,0]S(T7)V[20,0]@;@A");
    assert_eq!((lines(&rec), r.counts().delays_unheld), (2, 2));
}

// ---------------------------------------------------------------------------
// Text and Load, § 8.5.6, § 8.7.3, § 8.7.5.
// ---------------------------------------------------------------------------

fn glyphs(rec: &Rec) -> Vec<(Glyph, RegisPoint)> {
    rec.ops
        .iter()
        .filter_map(|o| match o {
            Op::Glyph(g, p, _) => Some((g.clone(), *p)),
            _ => None,
        })
        .collect()
}

#[test]
fn text_steps_by_its_escapement_and_its_controls() {
    let (r, rec) = run(b"P[100,100]T'AB'");
    assert_eq!(
        glyphs(&rec),
        vec![
            (Glyph::Builtin('A'), pt(100, 100)),
            (Glyph::Builtin('B'), pt(109, 100))
        ]
    );
    assert_eq!(at(&r), (118, 100));
    // CR back to where the quoted extent began, LF down a cell and saved.
    let (_, rec) = run(b"P[100,100]T'A\rB\nC\rD'");
    let g = glyphs(&rec);
    assert_eq!(g[1].1, pt(100, 100));
    assert_eq!(g[2].1, pt(109, 120));
    assert_eq!(g[3].1, pt(109, 120));
    // An explicit escapement, always relative.
    let (_, rec) = run(b"P[100,100]T[35,25]'AB'");
    assert_eq!(glyphs(&rec)[1].1, pt(135, 125));
}

#[test]
fn a_standard_size_sets_the_cells_and_the_escapement() {
    let (_, rec) = run(b"P[0,0]T(S2)'AB'");
    assert_eq!(
        rec.ops[1],
        Op::Glyph(Glyph::Builtin('B'), pt(18, 0), (16, 30))
    );
    let (r, _) = run(b"T(S17)R(E)");
    assert_eq!(at(&r), (0, 0));
}

#[test]
fn text_pixel_vectors_move_half_a_cell() {
    let (r, _) = run(b"P[100,100]T2");
    assert_eq!(at(&r), (100, 90));
    let (r, _) = run(b"P[100,100]T(S2)0");
    assert_eq!(at(&r), (109, 100));
}

#[test]
fn a_loaded_cell_draws_from_its_alphabet() {
    let (_, rec) = run(b"L(A1)'A'FF,81,C,123T(A1)'AB'");
    let g = glyphs(&rec);
    // "If too few digits are specified to fill a row, they fill the low
    // order portion of the mask": `C` is 0C and the last `3` is 03.
    let mut rows = vec![0xff, 0x81, 0x0c, 0x12, 0x03];
    rows.resize(10, 0);
    assert_eq!(g[0].0, Glyph::Loaded(rows, 8));
    // A character not loaded is the error character.
    assert_eq!(g[1].0, Glyph::Error);
}

#[test]
fn the_storage_size_is_height_first() {
    // `L[h,w]`, § 8.8.5.1's order: a cell 4 high and 12 wide takes three
    // hex digits a row.
    let (_, rec) = run(b"L(A1)[4,12]'A'ABC,123T(A1)'A'");
    let g = glyphs(&rec);
    assert_eq!(g[0].0, Glyph::Loaded(vec![0xabc, 0x123, 0, 0], 12));
}

#[test]
fn alphabet_zero_cannot_be_loaded_and_the_extent_bounds_the_rest() {
    let (r, _) = run(b"L(A0)'A'FF;");
    assert_eq!(r.counts().load_refused, 1);
    let (r, rec) = run(b"L(A1,E10)'a'FF;T(A1)'a'");
    assert_eq!(r.counts().load_refused, 1);
    assert_eq!(glyphs(&rec)[0].0, Glyph::Error);
}

#[test]
fn the_built_in_alphabet_reads_through_its_sets() {
    // `é` as UTF-8, the default encoding: the code E9 of the in-use table,
    // Latin-1's in GR.
    let (_, rec) = run("T(A0(L\"00\"))'q'T(A0(L\"0B\",R\"1A\"))'é'".as_bytes());
    let g = glyphs(&rec);
    assert_eq!(g[0].0, Glyph::Builtin('\u{2500}'));
    assert_eq!(g[1].0, Glyph::Builtin('\u{e9}'));
    // DEC Supplemental in GR, the same code.
    let (_, rec) = run("T(A0(R\"0%5\"))'é'".as_bytes());
    assert_eq!(glyphs(&rec)[0].0, Glyph::Builtin('\u{e9}'));
}

#[test]
fn a_utf8_character_outside_the_table_is_the_error_character() {
    // U+2500 has no code in the 8-bit in-use table, and nor has U+0080, a
    // C1 control. Both draw the error character, and the text around them
    // is drawn (Jev's lean, DECODE_TO_TABLE).
    let (_, rec) = run("T'a─b\u{80}c'".as_bytes());
    let g: Vec<_> = glyphs(&rec).into_iter().map(|(g, _)| g).collect();
    assert_eq!(
        g,
        vec![
            Glyph::Builtin('a'),
            Glyph::Error,
            Glyph::Builtin('b'),
            Glyph::Error,
            Glyph::Builtin('c')
        ]
    );
}

#[test]
fn under_latin1_each_byte_is_its_code() {
    let mut r = Box::new(Regis::NEW);
    let mut rec = Rec::default();
    r.set_encoding(baud_domain::InputEncoding::Latin1);
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in b"T'\xe9'" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    assert_eq!(glyphs(&rec)[0].0, Glyph::Builtin('\u{e9}'));
}

// ---------------------------------------------------------------------------
// The envelope, § 8.9.1.
// ---------------------------------------------------------------------------

#[test]
fn a_resumed_envelope_goes_on_where_the_last_stopped() {
    let mut r = Box::new(Regis::NEW);
    let mut rec = Rec::default();
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in b"P[10,10]V[20" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    r.enter(RegisEntry::of(Some(0)), &mut rec);
    for &b in b",20]" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    assert_eq!(lines(&rec), vec![(pt(10, 10), pt(20, 20))]);
}

#[test]
fn a_resynchronising_envelope_starts_a_new_command() {
    let mut r = Box::new(Regis::NEW);
    let mut rec = Rec::default();
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in b"P[10,10]T'abc" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    r.enter(RegisEntry::of(Some(1)), &mut rec);
    for &b in b"V[20,20]" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    // The quoted string was closed and the `V` is a command.
    assert_eq!(lines(&rec).len(), 1);
    // A definition left open is closed too.
    r.enter(RegisEntry::of(None), &mut rec);
    for &b in b"@:Aabc" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    r.enter(RegisEntry::of(Some(3)), &mut rec);
    for &b in b"V[30,30]" {
        r.put(StreamByte::from_stream(b), &mut rec);
    }
    assert_eq!(lines(&rec).len(), 2);
}

// ---------------------------------------------------------------------------
// Properties.
// ---------------------------------------------------------------------------

/// A ReGIS command string from the shapes the grammar has.
fn atom() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        (any::<i16>(), any::<i16>()).prop_map(|(x, y)| format!("P[{x},{y}]").into_bytes()),
        (any::<i16>(), any::<i16>())
            .prop_map(|(x, y)| format!("V[{:+},{}]", x / 8, y / 8).into_bytes()),
        "[0-7]{1,6}".prop_map(|s| format!("V{s}").into_bytes()),
        (1u8..30, -400i16..400).prop_map(|(r, a)| format!("C(A{a})[+{r}]").into_bytes()),
        Just(b"C(S)[+10,+20][+30,-5][+12,+40][-7,+3](E)".to_vec()),
        Just(b"F(V[+50][,+50][-50])".to_vec()),
        "[a-z ]{0,6}".prop_map(|s| format!("T'{s}'").into_bytes()),
        (0u8..20).prop_map(|n| format!("W(I{n},P{n}(M3))").into_bytes()),
        Just(b"@:AV[+3,+3]@;@A".to_vec()),
        Just(b"R(P)".to_vec()),
        Just(b";".to_vec()),
        Just(b"S(E)".to_vec()),
        Just(b"P(B)[+5](E)".to_vec()),
    ]
}

proptest! {
    /// **Envelopes resume**: the same body cut into `DCS p` envelopes at any
    /// points draws and answers what it does in one.
    #[test]
    fn a_body_cut_into_resumed_envelopes_is_the_body(
        atoms in prop::collection::vec(atom(), 1..12),
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..6),
    ) {
        let body: Vec<u8> = atoms.concat();
        let whole = run(&body).1.ops;
        let mut at: Vec<usize> = cuts.iter().map(|i| i.index(body.len() + 1)).collect();
        at.sort();
        let mut r = Box::new(Regis::NEW);
        let mut rec = Rec::default();
        let mut from = 0;
        for &c in at.iter().chain(std::iter::once(&body.len())) {
            r.enter(RegisEntry::of(Some(0)), &mut rec);
            for &b in &body[from..c] {
                r.put(StreamByte::from_stream(b), &mut rec);
            }
            from = c;
        }
        prop_assert_eq!(rec.ops, whole);
    }

    /// **Every byte is read, and nothing panics or runs away**: an arbitrary
    /// body ends in bounded state.
    #[test]
    fn any_bytes_are_read_and_bounded(body in prop::collection::vec(any::<u8>(), 0..400)) {
        let (r, rec) = run(&body);
        prop_assert!(rec.ops.len() < 1_000_000);
        prop_assert!(r.macro_free() <= 10_000);
    }
}
