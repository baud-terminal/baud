//! A CFF2 font built to reach what the real CFF2 fixture never does.
//!
//! Writes `baud-font/tests/var/cff2-cases.otf`. Source Code VF
//! (`baud-font/tests/fonts/SourceCodeVF-Upright.otf`) and SFIndia carry no
//! hints, no hintmask, no flex, no global subroutines, one Font DICT, one
//! axis and no Private DICT vsindex; this font has all of those, so
//! HarfBuzz's drawing of it (the `cff2` oracle, `cff2-outlines.txt`)
//! decides the rules the real fonts leave untested:
//!
//! ```text
//! .notdef  a square, nothing varies
//! blend    rmoveto with one n=2 blend and k=3 deltas; hlineto and vlineto
//!          with n=1 blends; the contour ends away from its start, so the
//!          close is the specification's implicit lineto
//! stems    hstemhm and vstemhm whose operands carry blends, an implied
//!          vstem before the first mask (its operands blended too), a
//!          cntrmask, a hintmask, and a hintmask between two contours, in
//!          the text's order "hs* vs* cm* hm* {mt <path constructing
//!          data>}*": the mask length is the stem count after blending,
//!          nine stems and two bytes, where six without the implied ones
//!          would be one byte, so a wrong count misreads the path that
//!          follows
//! flex     flex, flex1 (both of its branches), hflex and hflex1, with
//!          Fixed (16.16) operands
//! subrs    local and global subroutines, nested three deep, one that only
//!          pushes operands, one that blends
//! vsindex  a charstring vsindex selecting the second data set (k=2), then
//!          an n=6 blend on rrcurveto
//! fd1      a glyph of the second Font DICT, whose Private DICT selects the
//!          third data set (k=1), and its own local subroutines
//! stacked  a value blended twice before its operator (the specification's
//!          "advanced uses of blend"), and a blend whose n is less than the
//!          operator's operand count
//! curves   hvcurveto and vhcurveto at 4, 5, 8, 9, 12 and 13 operands,
//!          hhcurveto and vvcurveto with a leading odd operand, rcurveline
//!          and rlinecurve
//! moves    a moveto that draws nothing, then a contour
//! ```
//!
//! Axes: wght 100-400-900 and wdth 50-100-200. Regions: R0 wght (0,1,1),
//! R1 wght (-1,-1,0), R2 wdth (0,1,1), R3 wght (0,0.5,1), R4 wght (0,1,1)
//! and wdth (0,1,1). Data sets: 0 = R0 R1 R2, 1 = R3 R4, 2 = R2. FDSelect
//! is format 4. Deterministic: the head timestamps are fixed.
//!
//! **Written here: the CFF2 table.** write-fonts 0.55.0 has no CFF2
//! writer, only an INDEX helper, so the table is laid out here from the
//! CFF2 chapter (`[[source: ms-opentype-cff2]]`), in this order: the
//! header, the Top DICT (FDSelect, FDArray, CharStrings, vstore), the
//! global subroutine INDEX, the variation store, FDSelect, the CharStrings
//! INDEX, the Font DICT INDEX, then each Font DICT's Private DICT followed
//! by its local subroutine INDEX. Every number takes its shortest
//! encoding, and the offsets are laid out again until they stop changing.

use write_fonts::types::{F2Dot14, GlyphId};

use super::{
    Extremes, bbox, bytes, cmap, font, fvar, head, hhea, hmtx, maxp_cff, name, os2, os2_bytes,
    post_v3,
};

/// Each operator's encoding.
fn op(name: &str) -> &'static [u8] {
    match name {
        "rlineto" => &[5],
        "hlineto" => &[6],
        "vlineto" => &[7],
        "rrcurveto" => &[8],
        "callsubr" => &[10],
        "vsindex" => &[15],
        "blend" => &[16],
        "hstemhm" => &[18],
        "hintmask" => &[19],
        "cntrmask" => &[20],
        "rmoveto" => &[21],
        "vstemhm" => &[23],
        "rcurveline" => &[24],
        "rlinecurve" => &[25],
        "vvcurveto" => &[26],
        "hhcurveto" => &[27],
        "callgsubr" => &[29],
        "vhcurveto" => &[30],
        "hvcurveto" => &[31],
        "hflex" => &[12, 34],
        "flex" => &[12, 35],
        "hflex1" => &[12, 36],
        "flex1" => &[12, 37],
        _ => panic!("no operator {name}"),
    }
}

/// A charstring operand: one byte for -107..=107, two for -1131..=1131,
/// else 28 and an i16.
fn cs_int(v: i32, out: &mut Vec<u8>) {
    match v {
        -107..=107 => out.push((v + 139) as u8),
        108..=1131 => {
            let w = v - 108;
            out.extend([(w >> 8) as u8 + 247, w as u8]);
        }
        -1131..=-108 => {
            let w = -v - 108;
            out.extend([(w >> 8) as u8 + 251, w as u8]);
        }
        _ => {
            out.push(28);
            out.extend(i16::try_from(v).expect("an i16 operand").to_be_bytes());
        }
    }
}

/// A charstring from its text: integers, Fixed (16.16) operands written
/// with a point (255 and the 32-bit value), operators by name, and the
/// bytes after a hintmask or cntrmask in hex between angle brackets.
fn charstring(lines: &[&str]) -> Vec<u8> {
    let mut out = vec![];
    for t in lines.iter().flat_map(|l| l.split_whitespace()) {
        if let Some(hex) = t.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
            for i in (0..hex.len()).step_by(2) {
                out.push(u8::from_str_radix(&hex[i..i + 2], 16).expect("a mask byte"));
            }
        } else if t.contains('.') {
            let v: f64 = t.parse().expect("a Fixed operand");
            out.push(255);
            out.extend(((v * 65536.0).round() as i32).to_be_bytes());
        } else if let Ok(v) = t.parse() {
            cs_int(v, &mut out);
        } else {
            out.extend(op(t));
        }
    }
    out
}

/// A DICT operand: one byte for -107..=107, two for -1131..=1131, three
/// (28) for an i16, else five (29).
fn dict_int(v: i32, out: &mut Vec<u8>) {
    match v {
        -1131..=1131 => cs_int(v, out),
        -32768..=32767 => {
            out.push(28);
            out.extend((v as i16).to_be_bytes());
        }
        _ => {
            out.push(29);
            out.extend(v.to_be_bytes());
        }
    }
}

/// A CFF2 INDEX: a 32-bit count, then (when there are items) the offset
/// size, the offsets from 1 and the data.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = (items.len() as u32).to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    let last = 1 + items.iter().map(Vec::len).sum::<usize>();
    let size = match last {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    out.push(size as u8);
    let put = |v: usize, out: &mut Vec<u8>| out.extend(&(v as u32).to_be_bytes()[4 - size..]);
    let mut at = 1usize;
    put(at, &mut out);
    for i in items {
        at += i.len();
        put(at, &mut out);
    }
    for i in items {
        out.extend(i);
    }
    out
}

const ORDER: [&str; 10] = [
    ".notdef", "blend", "stems", "flex", "subrs", "vsindex", "fd1", "stacked", "curves", "moves",
];

// Data set 0 has k = 3 (R0 R1 R2), data set 1 k = 2 (R3 R4), data set 2
// k = 1 (R2). Every INDEX here holds fewer than 1240 subroutines, so the
// bias is 107: subroutine 0 is called as -107 and 1 as -106.
const PROGRAMS: [&[&str]; 10] = [
    // .notdef
    &["100 0 rmoveto 400 700 -400 hlineto"],
    // blend: rmoveto 120 40, its two operands blended in one n=2 blend.
    &[
        "120 40 30 -20 10 5 0 -8 2 blend rmoveto",
        "300 60 -40 25 1 blend hlineto",
        "500 -50 30 0 1 blend vlineto",
        "-150 20 0 0 1 blend -200 rlineto",
    ],
    // stems
    &[
        // Two blended horizontal stems (4 operands, n=4 blend), two plain.
        "0 80 300 80 10 0 0 5 0 0 -10 0 0 5 0 3 4 blend 200 60 100 40 hstemhm",
        // Two vertical stems defined by vstemhm, the first blended.
        "100 70 20 0 0 0 0 5 2 blend 300 40 vstemhm",
        // The implied vstemhm before the first mask, a cntrmask, which
        // "cm*" puts before every hintmask: three pairs, the first blended
        // -- nine stems in all, so two mask bytes, where the six defined
        // without it would make one. Stem 9's bit is set in the second
        // byte of the last mask, so a reader that takes one byte reads
        // 0x80 as the number -11 and the rmoveto after it as malformed.
        "400 70 15 0 0 0 0 0 2 blend 500 70 100 30 cntrmask <5000>",
        "hintmask <a880>",
        "50 0 rmoveto 600 700 -600 hlineto",
        "hintmask <5480>",
        "200 -500 10 0 0 0 0 0 2 blend rmoveto 200 300 -200 hlineto",
    ],
    // flex
    &[
        "100 300 rmoveto",
        "50.5 10.25 50 10 50 0 50 0 50 -10 50.75 -10 50 flex",
        // flex1 whose summed dx dominates: the last operand is dx.
        "50 20 50 20 50 0 50 -20 50 -20 40 flex1",
        // flex1 whose summed dy dominates: the last operand is dy.
        "10 50 10 50 0 50 -10 50 -10 50 -30 flex1",
        "-60 20.5 30 -50 -50 -20.5 -60 hflex",
        "-50 10 -50 20 -50 -50 -50 -20 -50 hflex1",
        "-200 -400 15 -10 0 0 0 0 2 blend rlineto",
    ],
    // subrs: gsubr 0 pushes the moveto's operands; lsubr 0 draws, and
    // calls lsubr 1, which calls gsubr 1.
    &["-107 callgsubr rmoveto", "-107 callsubr", "-300 0 rlineto"],
    // vsindex
    &[
        "1 vsindex",
        "100 100 30 0 0 0 2 blend rmoveto",
        "100 0 100 100 0 100 10 5 0 0 5 5 0 0 10 0 0 10 6 blend rrcurveto",
        "-200 0 rlineto",
    ],
    // fd1: the Private DICT selects data set 2 (k = 1): every blend has one
    // delta per value.
    &[
        "150 150 40 -40 2 blend rmoveto",
        "-107 callsubr",
        "-250 20 1 blend hlineto",
    ],
    // stacked
    &[
        // 100 is blended by (5,-5,7), and the result blended again by
        // (3,2,-1): both deltas reach the moveto's dx.
        "100 5 -5 7 1 blend 3 2 -1 1 blend 100 rmoveto",
        // rrcurveto's six operands with two separate n=1 blends.
        "50 10 0 0 1 blend 50 50 0 0 -10 20 0 5 1 blend rrcurveto",
        "-100 -100 rlineto",
    ],
    // curves
    &[
        "100 100 rmoveto",
        "50 20 30 40 hvcurveto",
        "40 20 30 50 10 vhcurveto",
        "50 20 30 40 40 20 30 50 hvcurveto",
        "40 20 30 50 50 20 30 40 10 vhcurveto",
        "50 20 30 40 40 20 30 50 50 20 30 40 hvcurveto",
        "40 20 30 50 50 20 30 40 40 20 30 50 10 vhcurveto",
        "5 30 20 30 40 hhcurveto",
        "5 30 20 30 40 vvcurveto",
        "30 10 20 20 10 30 40 0 rcurveline",
        "20 0 0 20 10 10 20 20 10 30 rlinecurve",
        "-300 -300 rlineto",
    ],
    // moves
    &["50 50 rmoveto", "100 100 rmoveto 200 200 -200 hlineto"],
];

const GSUBRS: [&[&str]; 2] = [&["200 100 40 -20 5 0 0 0 2 blend"], &["0 100 rlineto"]];

/// FD 0's local subroutines: 0 draws and calls 1, 1 calls gsubr 1.
const LSUBRS0: [&[&str]; 2] = [
    &["100 20 0 0 1 blend 0 0 100 rlineto -106 callsubr"],
    &["100 0 rlineto -106 callgsubr"],
];

/// FD 1's: one subroutine that blends with its Private DICT's k = 1.
const LSUBRS1: [&[&str]; 1] = [&["200 30 1 blend vlineto 100 -10 1 blend hlineto"]];

/// Seven blended BlueValues pairs: 14 + 14 * 3 + 1 = 57 operands before
/// one DICT operator, past CFF's limit of 48 and within CFF2's 513. A
/// reader that keeps CFF's limit loses every key the scan meets after it.
const BLUES: [i32; 14] = [
    -12, 0, 450, 462, 500, 512, 600, 612, 650, 662, 700, 712, 750, 762,
];
/// Each blue's deltas over data set 0's three regions, carried by the
/// delta-encoded operand it is stored as.
const BLUE_DELTAS: [i32; 3] = [2, -1, 1];

/// Private DICT 0: the blended BlueValues, then Subrs at `subrs`.
fn private0(subrs: i32) -> Vec<u8> {
    let mut out = vec![];
    let mut prev = 0;
    for b in BLUES {
        dict_int(b - prev, &mut out);
        prev = b;
    }
    for _ in BLUES {
        for d in BLUE_DELTAS {
            dict_int(d, &mut out);
        }
    }
    dict_int(BLUES.len() as i32, &mut out);
    out.push(23); // blend
    out.push(6); // BlueValues
    dict_int(subrs, &mut out);
    out.push(19); // Subrs
    out
}

/// Private DICT 1: vsindex 2, then Subrs at `subrs`.
fn private1(subrs: i32) -> Vec<u8> {
    let mut out = vec![];
    dict_int(2, &mut out);
    out.push(22); // vsindex
    dict_int(subrs, &mut out);
    out.push(19); // Subrs
    out
}

/// A Private DICT whose Subrs follow it: the offset is its own length,
/// found by laying it out until the two agree.
fn private(make: fn(i32) -> Vec<u8>) -> Vec<u8> {
    let mut len = 0;
    loop {
        let p = make(len);
        if p.len() as i32 == len {
            return p;
        }
        len = p.len() as i32;
    }
}

/// The variation store: format 1, the region list, then data sets 0, 1
/// and 2, none with items, behind its 16-bit length.
fn vstore() -> Vec<u8> {
    let f = |v: f32| F2Dot14::from_f32(v).to_be_bytes();
    let none = (0.0, 0.0, 0.0);
    // (wght, wdth) per region.
    let regions = [
        ((0.0, 1.0, 1.0), none),
        ((-1.0, -1.0, 0.0), none),
        (none, (0.0, 1.0, 1.0)),
        ((0.0, 0.5, 1.0), none),
        ((0.0, 1.0, 1.0), (0.0, 1.0, 1.0)),
    ];
    let sets: [&[u16]; 3] = [&[0, 1, 2], &[3, 4], &[2]];
    let mut list = vec![];
    list.extend(2u16.to_be_bytes());
    list.extend((regions.len() as u16).to_be_bytes());
    for (w, d) in regions {
        for (s, p, e) in [w, d] {
            list.extend(f(s));
            list.extend(f(p));
            list.extend(f(e));
        }
    }
    let data: Vec<Vec<u8>> = sets
        .iter()
        .map(|s| {
            let mut v = vec![0, 0, 0, 0];
            v.extend((s.len() as u16).to_be_bytes());
            for r in *s {
                v.extend(r.to_be_bytes());
            }
            v
        })
        .collect();
    let header = 2 + 4 + 2 + 4 * data.len();
    let mut store = vec![];
    store.extend(1u16.to_be_bytes());
    store.extend((header as u32).to_be_bytes());
    store.extend((data.len() as u16).to_be_bytes());
    let mut at = header + list.len();
    for d in &data {
        store.extend((at as u32).to_be_bytes());
        at += d.len();
    }
    store.extend(&list);
    for d in &data {
        store.extend(d);
    }
    let mut out = (store.len() as u16).to_be_bytes().to_vec();
    out.extend(store);
    out
}

/// FDSelect format 4: fd1 in Font DICT 1, every other glyph in 0, as
/// ranges of (first glyph, Font DICT) and a sentinel.
fn fd_select() -> Vec<u8> {
    let fds: Vec<u16> = ORDER.iter().map(|n| u16::from(*n == "fd1")).collect();
    let mut ranges = vec![];
    for (g, &fd) in fds.iter().enumerate() {
        if g == 0 || fds[g - 1] != fd {
            ranges.push((g as u32, fd));
        }
    }
    let mut out = vec![4];
    out.extend((ranges.len() as u32).to_be_bytes());
    for (first, fd) in ranges {
        out.extend(first.to_be_bytes());
        out.extend(fd.to_be_bytes());
    }
    out.extend((ORDER.len() as u32).to_be_bytes());
    out
}

/// The CFF2 table.
fn cff2() -> Vec<u8> {
    let programs = |p: &[&[&str]]| -> Vec<Vec<u8>> { p.iter().map(|p| charstring(p)).collect() };
    let gsubrs = index(&programs(&GSUBRS));
    let charstrings = index(&programs(&PROGRAMS));
    let vstore = vstore();
    let fd_select = fd_select();
    let privates = [
        (private(private0), index(&programs(&LSUBRS0))),
        (private(private1), index(&programs(&LSUBRS1))),
    ];
    // Lay out until the Top DICT's and the Font DICTs' offsets stop
    // changing size.
    let (mut top_len, mut fd_len) = (0usize, 0usize);
    loop {
        let top_at = 5;
        let gsubrs_at = top_at + top_len;
        let vstore_at = gsubrs_at + gsubrs.len();
        let fd_select_at = vstore_at + vstore.len();
        let charstrings_at = fd_select_at + fd_select.len();
        let fd_array_at = charstrings_at + charstrings.len();
        let mut at = fd_array_at + fd_len;
        let mut fds = vec![];
        for (p, s) in &privates {
            let mut d = vec![];
            dict_int(p.len() as i32, &mut d);
            dict_int(at as i32, &mut d);
            d.push(18); // Private
            fds.push(d);
            at += p.len() + s.len();
        }
        let fd_array = index(&fds);
        let mut top = vec![];
        dict_int(fd_select_at as i32, &mut top);
        top.extend([12, 37]); // FDSelect
        dict_int(fd_array_at as i32, &mut top);
        top.extend([12, 36]); // FDArray
        dict_int(charstrings_at as i32, &mut top);
        top.push(17); // CharStrings
        dict_int(vstore_at as i32, &mut top);
        top.push(24); // vstore
        if top.len() == top_len && fd_array.len() == fd_len {
            let mut out = vec![2, 0, 5];
            out.extend((top.len() as u16).to_be_bytes());
            out.extend(top);
            out.extend(&gsubrs);
            out.extend(&vstore);
            out.extend(&fd_select);
            out.extend(&charstrings);
            out.extend(fd_array);
            for (p, s) in &privates {
                out.extend(p);
                out.extend(s);
            }
            return out;
        }
        (top_len, fd_len) = (top.len(), fd_array.len());
    }
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let metrics = [(800, 0); ORDER.len()];
    let (hmtx, long) = hmtx(&metrics);
    let map: Vec<(u32, GlyphId)> = (1..ORDER.len())
        .map(|i| (0x40 + i as u32, GlyphId::new(i as u32)))
        .collect();
    // The extremes of the drawn outlines at the default instance.
    let extremes = Extremes {
        advance_width_max: 800,
        min_lsb: 0,
        min_rsb: -325,
        x_max_extent: 1125,
    };
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(-29, 0, 1225, 1235), None)),
        ),
        (b"hhea", bytes(&hhea(800, -200, &extremes, long))),
        (b"maxp", bytes(&maxp_cff(ORDER.len() as u16))),
        (b"OS/2", os2_bytes(&os2(800, 0x41, 0x49, 0))),
        (
            b"name",
            bytes(&name("baud CFF2 cases", "Regular", &["Weight", "Width"])),
        ),
        (b"cmap", bytes(&cmap(&map))),
        (b"post", bytes(&post_v3())),
        (b"CFF2", cff2()),
        (b"hmtx", bytes(&hmtx)),
        (
            b"fvar",
            bytes(&fvar(&[
                (b"wght", 100.0, 400.0, 900.0),
                (b"wdth", 50.0, 100.0, 200.0),
            ])),
        ),
    ]);
    vec![("baud-font/tests/var/cff2-cases.otf", font)]
}
