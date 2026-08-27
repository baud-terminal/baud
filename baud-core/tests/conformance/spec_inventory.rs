//! Every CSI final `[[source: ctlseqs]]` defines is accounted for.
//!
//! `docs/vt-coverage.md` is baud's own enumeration, and that is its weakness as
//! evidence: an operation the specification defines and baud never listed is
//! invisible in it. Not `todo`, not `out` -- absent. `coverage.rs` and
//! `enumerated.rs` both check the table against the code, and neither can see a
//! row that was never written.
//!
//! This checks the other direction: against the source rather than against
//! baud. ctlseqs documents 117 CSI sequences, many of them xterm extensions
//! baud deliberately does not implement, so the test is **not** that baud
//! handles them all. It is the weaker claim that actually matters --
//!
//! > every final byte the specification defines is either handled, or counted
//! > in `Term::unsupported`, and never silently consumed.
//!
//! Silent consumption is the `parsed` failure this project is built around:
//! "the terminal accepts a sequence, does nothing, and gives the application no
//! way to find out". Checked here against the specification's list, so a
//! sequence baud never thought about still cannot fall into it.

use baud_core::*;
use std::collections::BTreeSet;

/// Final bytes of the `CSI` entries in ctlseqs' "Functions using CSI" section.
fn spec_finals() -> BTreeSet<u8> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with("CSI ") {
            continue;
        }
        // The description begins after two spaces; the sequence precedes it.
        let seq = line.split("  ").next().unwrap_or(line).trim();
        if let Some(b) = plain_final(seq) {
            out.insert(b);
        }
    }
    out
}

/// Which finals `docs/vt-coverage.md` claims, in any namespace.
fn table_finals() -> BTreeSet<u8> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let l = line.trim();
        if !(l.starts_with("| `CSI") || l.starts_with("| ``CSI")) {
            continue;
        }
        let mut cell = l
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .to_string();

        // HPA's final byte *is* a backtick, so its row needs a **double**
        // backtick code span -- ``CSI ` ``. It was written `` `CSI \` ``
        // instead, which looks like an escape and is not one: CommonMark does
        // not process backslash escapes inside a code span, so it rendered as
        // `CSI \` and named no final byte at all. Both forms are read here;
        // only the double-backtick one renders.
        let mut spans: Vec<String> = Vec::new();
        while let Some(a) = cell.find("``") {
            let Some(rel) = cell[a + 2..].find("``") else {
                break;
            };
            let b = a + 2 + rel;
            spans.push(cell[a + 2..b].trim().to_string());
            cell.replace_range(a..b + 2, " ");
        }

        // The legacy escaped form, unescaped to a sentinel *before* splitting,
        // because splitting on backticks otherwise cuts it in half and yields
        // the backslash as the final byte.
        let cell = cell.replace("\\`", "\u{0}");
        // A cell may name several forms: `CSI H` / `CSI f`. Each backticked
        // span is one, and taking the last token of the joined string missed
        // `H` entirely.
        spans.extend(cell.split('`').skip(1).step_by(2).map(|f| f.to_string()));

        for form in spans {
            let form = form.replace('\u{0}', "`");
            if let Some(b) = plain_final(form.trim()) {
                out.insert(b);
            }
        }
    }
    out
}

/// The final byte of a **plain** `CSI` form: parameters only, no marker and no
/// intermediate.
///
/// Both directions need this and getting it wrong produced every one of the
/// first run's six failures. There is no bare `CSI p`: ctlseqs defines
/// `CSI ! p`, `CSI " p`, `CSI $ p`, `CSI # p` and `CSI > Ps p`, all of which
/// end in `p` and none of which is reached by `ESC [ p`. Taking the last token
/// regardless claimed the specification defined an operation it does not.
fn plain_final(seq: &str) -> Option<u8> {
    let rest = seq.strip_prefix("CSI")?;
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let (last, head) = toks.split_last()?;
    for t in head {
        // Parameter placeholders (`Ps`, `Pm`, `Pl`, `Pc`), separators and
        // literal digits are part of a plain form. Anything else -- a `?`, a
        // `>`, an `SP`, a `"` -- means this form is not one.
        let ok =
            t.starts_with('P') || *t == ";" || t.chars().all(|c| c.is_ascii_digit() || c == ';');
        if !ok {
            return None;
        }
    }
    let b = last.as_bytes();
    (b.len() == 1 && (0x40..=0x7e).contains(&b[0])).then(|| b[0])
}

fn unsupported_after(bytes: &[u8]) -> u64 {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    t.unsupported.count()
}

/// **No CSI final the specification defines is silently consumed.**
#[test]
fn every_spec_csi_final_is_handled_or_counted() {
    let spec = spec_finals();
    let table = table_finals();
    assert!(
        spec.len() > 30,
        "only {} finals parsed from ctlseqs -- the extractor stopped matching",
        spec.len()
    );
    assert!(
        table.len() > 20,
        "only {} finals parsed from vt-coverage.md",
        table.len()
    );
    let mut unaccounted = Vec::new();
    for b in &spec {
        let counted = unsupported_after(&[0x1b, b'[', *b]);
        let claimed = table.contains(b);
        if claimed && counted != 0 {
            unaccounted.push(format!(
                "CSI {} is in vt-coverage.md but reached Unsupported",
                *b as char
            ));
        }
        if !claimed && counted == 0 {
            unaccounted.push(format!(
                "CSI {} is defined by ctlseqs, absent from vt-coverage.md, and consumed in silence",
                *b as char
            ));
        }
    }
    assert!(
        unaccounted.is_empty(),
        "{} unaccounted CSI final(s) of {} in the specification:\n  {}",
        unaccounted.len(),
        spec.len(),
        unaccounted.join("\n  ")
    );
}

/// A `CSI` form as the specification writes it: optional marker, parameters,
/// optional intermediate, final byte.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Form {
    marker: Option<u8>,
    inter: Option<u8>,
    final_byte: u8,
}

impl Form {
    /// The bytes an application would send. `1` stands in for any parameter --
    /// a real value, and the default for every counted operation.
    fn wire(self) -> Vec<u8> {
        let mut v = vec![0x1b, b'['];
        if let Some(m) = self.marker {
            v.push(m);
        }
        v.push(b'1');
        if let Some(i) = self.inter {
            v.push(i);
        }
        v.push(self.final_byte);
        v
    }
}

/// Parse one `CSI ...` line into a [`Form`].
///
/// Tokens are a marker (`?`, `>`, `<`, `=`), parameter placeholders (`Ps`,
/// `Pm`, `Pl`, `Pc`, `Pt`) and separators, an intermediate (`SP` or a byte in
/// `20..2F`), then the final. Anything else -- a `Pt` string form, a multi-byte
/// tail -- returns `None` rather than being guessed at.
fn parse_form(seq: &str) -> Option<Form> {
    let rest = seq.strip_prefix("CSI")?;
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let (last, head) = toks.split_last()?;
    let fb = last.as_bytes();
    if fb.len() != 1 || !(0x40..=0x7e).contains(&fb[0]) {
        return None;
    }
    let (mut marker, mut inter) = (None, None);
    for (n, t) in head.iter().enumerate() {
        let b = t.as_bytes();
        if *t == "SP" {
            inter = Some(0x20);
            continue;
        }
        if t.starts_with('P') || *t == ";" || t.chars().all(|c| c.is_ascii_digit() || c == ';') {
            continue;
        }
        if b.len() != 1 {
            return None; // something this parser does not model
        }
        if n == 0 && matches!(b[0], b'?' | b'>' | b'<' | b'=') {
            marker = Some(b[0]);
        } else if (0x20..=0x2f).contains(&b[0]) {
            inter = Some(b[0]);
        } else {
            return None;
        }
    }
    Some(Form {
        marker,
        inter,
        final_byte: fb[0],
    })
}

/// Every `CSI` form the specification defines, in any namespace.
fn spec_forms() -> BTreeSet<Form> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with("CSI ") {
            continue;
        }
        let seq = line.split("  ").next().unwrap_or(line).trim();
        if let Some(f) = parse_form(seq) {
            out.insert(f);
        }
    }
    out
}

/// Everything a form could observably change.
///
/// "It replied" is not the only way to act, and treating it as the only one
/// made the first version of this test assert nothing. A form that moves the
/// cursor, writes a cell, switches a mode or bumps a counter has acted; one
/// that changes none of them and is not counted has been *silently consumed*,
/// which is the failure being looked for.
#[derive(PartialEq)]
struct Snapshot {
    grid: Vec<PackedCell>,
    cursor: (usize, usize),
    reply: Vec<u8>,
    unsupported: u64,
    unsupported_param: u64,
    replacements: u64,
    // Everything an operation can change that a grid comparison cannot see.
    cursor_visible: bool,
    cursor_style: CursorStyle,
    cursor_keys: bool,
    keypad: KeypadMode,
    screen: Screen,
    charset: Charset,
    terminal_mode: TerminalMode,
    mouse: MouseTracking,
    bracketed_paste: bool,
    synchronized: bool,
    column_mode: bool,
    smooth_scroll: bool,
    reverse_video: bool,
    auto_repeat: bool,
    meta_escape: bool,
    title: Vec<u8>,
    icon: Vec<u8>,
    bells: u64,
    palette: Vec<Option<(u8, u8, u8)>>,
    // All four G-slots, not just the invoked one: a designation into G1 is
    // recorded and was invisible from outside.
    g: [Charset; 4],
    encoding: InputEncoding,
    c1: C1Transmit,
}

/// A prepared terminal: one palette entry set, text on two rows, and the
/// cursor in the middle.
///
/// A form fed to a *blank* grid at the origin can legitimately do nothing --
/// `CSI 1 A` at row 0 is a no-op -- so a blank start would report real
/// operations as silent. The palette entry is there for the same reason:
/// `OSC 104` resets one, and resetting a colour nobody set changes nothing.
const PREAMBLE: &[u8] = b"\x1b]4;1;rgb:12/34/56\x07hello world\r\nsecond line\r\n\x1b[2;5H";

/// Written **after** the form, so that changes which only show on the next cell
/// are visible.
///
/// Without it, `CSI 1 m` looked silent: SGR sets the pending rendition and
/// nothing on the grid moves until something is written with it. So did
/// DECSCA.
///
/// But the probe has the opposite blind spot -- it writes *at the cursor*, so
/// it overwrites whatever an operation just did to that cell, and `CSI 1 X`
/// (ECH) looked silent because the erase was immediately covered. So both
/// snapshots are taken, and a form counts as silent only if it changed neither.
const PROBE: &[u8] = b"X";

fn snapshot(extra: &[u8], probe: bool) -> Snapshot {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let tail: &[u8] = if probe { PROBE } else { b"" };
    for chunk in [PREAMBLE, extra, tail] {
        if chunk.is_empty() {
            continue;
        }
        let mut slot = t.read_slot();
        let base = slot.base();
        let w = slot.write(StreamByte::slice_from(chunk));
        t.feed(base, w.len());
        t.commit_read(w.len());
    }
    let (r, c) = t.cursor();
    Snapshot {
        grid: t.viewport().to_vec(),
        cursor: (r.index(), c.index()),
        reply: t.reply().to_vec(),
        unsupported: t.unsupported.count(),
        unsupported_param: t.unsupported_param.count(),
        replacements: t.replacements.count(),
        cursor_visible: t.cursor_visible(),
        cursor_style: t.cursor_style(),
        cursor_keys: t.cursor_keys_application(),
        keypad: t.keypad(),
        screen: t.screen(),
        charset: t.charset(),
        terminal_mode: t.terminal_mode(),
        mouse: t.mouse_tracking(),
        bracketed_paste: t.bracketed_paste(),
        synchronized: t.synchronized_output(),
        column_mode: t.column_mode(),
        smooth_scroll: t.smooth_scroll(),
        reverse_video: t.reverse_video(),
        auto_repeat: t.auto_repeat(),
        meta_escape: t.meta_sends_escape(),
        title: t.title().to_vec(),
        icon: t.icon_name().to_vec(),
        bells: t.bells(),
        palette: (0..=255u8).map(|i| t.palette(Chan::from_u8(i))).collect(),
        g: [
            t.charset_in(GSlot::G0),
            t.charset_in(GSlot::G1),
            t.charset_in(GSlot::G2),
            t.charset_in(GSlot::G3),
        ],
        encoding: t.input_encoding(),
        c1: t.c1_transmit(),
    }
}

/// **No CSI form the specification defines is silently consumed.**
#[test]
fn every_spec_csi_form_is_handled_or_counted() {
    let forms = spec_forms();
    assert!(
        forms.len() > 90,
        "only {} forms parsed from ctlseqs -- the extractor stopped matching",
        forms.len()
    );
    // Forms that are implemented but whose effect this harness cannot see, or
    // which genuinely do nothing from the prepared state. Each names why.
    //
    // Kept tiny and reasoned, as `coverage.rs` does: a growing list means the
    // snapshot has stopped observing rather than that baud has stopped acting.
    const UNOBSERVABLE: &[(&[u8], &str)] = &[
        (
            b"\x1b[1s",
            "SCOSC saves the cursor; nothing exposes the saved slot",
        ),
        (
            b"\x1b[1u",
            "SCORC with no prior save is inert by design -- scorc_without_a_save_is_inert",
        ),
        (
            b"\x1b[1 q",
            "DECSCUSR 1 is blinking block, which is already the default",
        ),
        (
            b"\x1b[?1l",
            "DECCKM reset, and cursor-keys mode starts reset",
        ),
    ];

    let (bare, probed) = (snapshot(b"", false), snapshot(b"", true));
    let mut silent = Vec::new();
    for f in &forms {
        let a = snapshot(&f.wire(), false);
        let b = snapshot(&f.wire(), true);
        if a == bare && b == probed && !UNOBSERVABLE.iter().any(|(w, _)| *w == f.wire()) {
            silent.push(format!("{:02x?}  {f:?}", f.wire()));
        }
    }
    assert!(
        silent.is_empty(),
        "{} of {} specification CSI forms are consumed in silence -- neither \
         acted on nor counted:\n  {}",
        silent.len(),
        forms.len(),
        silent.join("\n  ")
    );
}

/// A plain two-byte `ESC Fe` form: no intermediate.
///
/// The list in ctlseqs' "Controls beginning with ESC" mixes single-byte finals
/// (`ESC D`) with intermediate forms (`ESC # 8`, `ESC ( C`, `ESC SP F`) and
/// with the 7-bit representations of C1 introducers (`ESC [`, `ESC ]`,
/// `ESC P`, `ESC \`). The last group are not operations -- they *begin* one --
/// so they are excluded rather than fed as if they were.
fn plain_esc(seq: &str) -> Option<u8> {
    let rest = seq.strip_prefix("ESC ")?;
    let toks: Vec<&str> = rest.split_whitespace().collect();
    if toks.len() != 1 || toks[0].len() != 1 {
        return None;
    }
    let b = toks[0].as_bytes()[0];
    // Introducers: these open CSI, OSC, DCS, SOS, PM and APC. Feeding one
    // alone leaves the parser mid-sequence, which is not "silently consumed".
    if matches!(b, b'[' | b']' | b'P' | b'X' | b'^' | b'_' | b'\\') {
        return None;
    }
    (0x30..=0x7e).contains(&b).then_some(b)
}

/// **No `ESC Fe` the specification defines is silently consumed.**
///
/// The CSI inventory found that full-screen scrolling never moved the grid.
/// This is the same question asked of the other half of the escape surface,
/// which had only baud's own table to check it.
#[test]
fn every_spec_esc_form_is_handled_or_counted() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let mut finals = BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with("ESC ") {
            continue;
        }
        let seq = line.split("  ").next().unwrap_or(line).trim();
        if let Some(b) = plain_esc(seq) {
            finals.insert(b);
        }
    }
    assert!(
        finals.len() > 8,
        "only {} ESC finals parsed -- the extractor stopped matching",
        finals.len()
    );
    // Implemented, but their effect is invisible to this harness or genuinely
    // nil from the prepared state. Each names which.
    const UNOBSERVABLE: &[(u8, &str)] = &[
        (
            b'7',
            "DECSC saves the cursor; nothing exposes the saved slot",
        ),
        (b'8', "DECRC with no prior save restores nothing"),
        (
            b'>',
            "DECKPNM selects the numeric keypad, which is the default",
        ),
        (
            b'H',
            "HTS sets a tab stop at the cursor; the snapshot has no tab-stop state, \
             and a stop at the cursor does not move the next tab from there",
        ),
    ];

    let (bare, probed) = (snapshot(b"", false), snapshot(b"", true));
    let mut silent = Vec::new();
    for b in &finals {
        if UNOBSERVABLE.iter().any(|(u, _)| u == b) {
            continue;
        }
        let bytes = [0x1b, *b];
        let a = snapshot(&bytes, false);
        let c = snapshot(&bytes, true);
        if a == bare && c == probed {
            silent.push(format!("ESC {}", *b as char));
        }
    }
    assert!(
        silent.is_empty(),
        "{} of {} specification ESC forms are consumed in silence:\n  {}",
        silent.len(),
        finals.len(),
        silent.join("\n  ")
    );
}

/// Every `Ps` value ctlseqs lists under `OSC Ps ; Pt`.
///
/// The document writes them spaced -- `Ps = 1 0 4` is 104 -- so the digits are
/// joined rather than parsed as separate numbers.
fn spec_osc_codes() -> BTreeSet<u32> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let mut out = BTreeSet::new();
    let mut in_osc = false;
    for line in src.lines() {
        if line.starts_with("OSC ") {
            in_osc = true;
            continue;
        }
        // A new top-level section ends the list.
        if in_osc && !line.starts_with(' ') && !line.trim().is_empty() {
            in_osc = false;
        }
        if !in_osc {
            continue;
        }
        let t = line.trim();
        let Some(rest) = t.strip_prefix("Ps = ") else {
            continue;
        };
        let digits: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ' ')
            .filter(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse::<u32>() {
            out.insert(n);
        }
    }
    out
}

/// **No `OSC Ps` the specification defines is silently consumed.**
///
/// A string protocol needs a different probe from a fixed-length control: the
/// payload has to be plausible or the operation declines for the wrong reason.
/// `1` is used, which is a valid colour index for the palette operations and
/// harmless text for the rest.
#[test]
fn every_spec_osc_code_is_handled_or_counted() {
    let codes = spec_osc_codes();
    assert!(
        codes.len() > 15,
        "only {} OSC codes parsed -- the extractor stopped matching",
        codes.len()
    );
    // Implemented, but with nothing this harness can observe.
    const UNOBSERVABLE: &[(u32, &str)] = &[(
        52,
        "clipboard read answers with silence on purpose -- any reply, empty \
         included, tells a remote program the feature works",
    )];
    let (bare, probed) = (snapshot(b"", false), snapshot(b"", true));
    let mut silent = Vec::new();
    for ps in &codes {
        if UNOBSERVABLE.iter().any(|(u, _)| u == ps) {
            continue;
        }
        let mut bytes = b"\x1b]".to_vec();
        bytes.extend_from_slice(ps.to_string().as_bytes());
        bytes.extend_from_slice(b";1\x07");
        let a = snapshot(&bytes, false);
        let c = snapshot(&bytes, true);
        if a == bare && c == probed {
            silent.push(format!("OSC {ps}"));
        }
    }
    assert!(
        silent.is_empty(),
        "{} of {} specification OSC codes are consumed in silence:\n  {}",
        silent.len(),
        codes.len(),
        silent.join("\n  ")
    );
}

/// **No `DCS` form the specification defines is silently consumed.**
///
/// Each entry is fed with an empty payload and `ST`, which is the shortest
/// well-formed string of that shape.
#[test]
fn every_spec_dcs_form_is_handled_or_counted() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let mut forms = BTreeSet::new();
    for line in src.lines() {
        let Some(rest) = line.strip_prefix("DCS ") else {
            continue;
        };
        let seq = rest.split("  ").next().unwrap_or(rest).trim();
        // Keep the literal bytes between `DCS` and the payload placeholder:
        // `$ q`, `! u`, `+ q`. Parameter and payload placeholders are dropped.
        let lit: Vec<&str> = seq
            .split_whitespace()
            .take_while(|t| !t.starts_with("Pt") && !t.starts_with("Ps.."))
            .filter(|t| !t.starts_with('P') && *t != ";" && *t != "|" && *t != "ST")
            .collect();
        if lit.is_empty() || lit.iter().any(|t| t.len() != 1) {
            continue;
        }
        forms.insert(lit.join(""));
    }
    assert!(
        forms.len() > 3,
        "only {} DCS forms parsed -- the extractor stopped matching",
        forms.len()
    );
    let (bare, probed) = (snapshot(b"", false), snapshot(b"", true));
    let mut silent = Vec::new();
    for f in &forms {
        let mut bytes = b"\x1bP".to_vec();
        bytes.extend_from_slice(f.as_bytes());
        bytes.extend_from_slice(b"\x1b\\");
        let a = snapshot(&bytes, false);
        let c = snapshot(&bytes, true);
        if a == bare && c == probed {
            silent.push(format!("DCS {f}"));
        }
    }
    assert!(
        silent.is_empty(),
        "{} of {} specification DCS forms are consumed in silence:\n  {}",
        silent.len(),
        forms.len(),
        silent.join("\n  ")
    );
}

/// An `ESC` form with an intermediate: `ESC # 8`, `ESC ( C`, `ESC SP F`,
/// `ESC % G`.
///
/// The plain check above takes only two-byte forms, so this half of the escape
/// surface -- every charset designation among it -- had nothing but baud's own
/// table checking it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct EscForm {
    inter: u8,
    final_byte: u8,
}

impl EscForm {
    fn wire(self) -> Vec<u8> {
        vec![0x1b, self.inter, self.final_byte]
    }
}

/// Parse `ESC <inter> <final>`, expanding the placeholder finals ctlseqs uses.
///
/// `ESC ( C` is written with `C` standing for "any of the character sets listed
/// below", not a literal `C`. Feeding the letter would test nothing, so the
/// listed finals are substituted -- which is what makes this check cover every
/// charset designation rather than one arbitrary byte.
fn parse_esc_form(seq: &str, charset_finals: &BTreeSet<u8>) -> Vec<EscForm> {
    let Some(rest) = seq.strip_prefix("ESC ") else {
        return Vec::new();
    };
    let toks: Vec<&str> = rest.split_whitespace().collect();
    if toks.len() != 2 {
        return Vec::new();
    }
    let inter = if toks[0] == "SP" {
        0x20
    } else if toks[0].len() == 1 {
        toks[0].as_bytes()[0]
    } else {
        return Vec::new();
    };
    if !(0x20..=0x2f).contains(&inter) {
        return Vec::new();
    }
    // `ESC ( C` and friends: `C` is a placeholder for the charset list.
    if toks[1] == "C" && matches!(inter, b'(' | b')' | b'*' | b'+') {
        return charset_finals
            .iter()
            .map(|f| EscForm {
                inter,
                final_byte: *f,
            })
            .collect();
    }
    if toks[1].len() != 1 {
        return Vec::new();
    }
    let f = toks[1].as_bytes()[0];
    if !(0x30..=0x7e).contains(&f) {
        return Vec::new();
    }
    vec![EscForm {
        inter,
        final_byte: f,
    }]
}

/// The single-byte charset finals ctlseqs lists under `ESC ( C`.
fn charset_finals(src: &str) -> BTreeSet<u8> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("C = ") else {
            continue;
        };
        let head: Vec<&str> = rest
            .split("->")
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        for tok in head {
            if tok == "or" {
                continue;
            }
            let b = tok.as_bytes();
            if b.len() == 1 && (0x30..=0x7e).contains(&b[0]) {
                out.insert(b[0]);
            }
        }
    }
    out
}

/// **No `ESC` form with an intermediate is silently consumed.**
#[test]
fn every_spec_esc_intermediate_form_is_handled_or_counted() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let charsets = charset_finals(&src);
    assert!(
        charsets.len() > 10,
        "only {} charset finals parsed -- the extractor stopped matching",
        charsets.len()
    );
    let mut forms = BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with("ESC ") {
            continue;
        }
        let seq = line.split("  ").next().unwrap_or(line).trim();
        for f in parse_esc_form(seq, &charsets) {
            forms.insert(f);
        }
    }
    assert!(
        forms.len() > 20,
        "only {} intermediate forms parsed -- the extractor stopped matching",
        forms.len()
    );
    // **Satisfied, not silent**, and one category rather than six exceptions:
    // each asks for what baud already does, so there is nothing to change and
    // nothing withheld. Same distinction `ESC SP F` forced when the terminfo
    // fixtures showed dtterm opening with it -- counting a request baud
    // already meets as unsupported claims a refusal that never happened.
    let already_true = |f: &EscForm| -> bool {
        matches!(
            (f.inter, f.final_byte),
            // S7C1T: 7-bit replies, the default.
            (0x20, b'F')
            // Select UTF-8, the default.
            | (b'%', b'G')
            // Designate ASCII into a slot that already holds it.
            | (b'(' | b')' | b'*' | b'+', b'B')
        )
    };

    let (bare, probed) = (snapshot(b"", false), snapshot(b"", true));
    let mut silent = Vec::new();
    for f in &forms {
        if already_true(f) {
            continue;
        }
        let a = snapshot(&f.wire(), false);
        let c = snapshot(&f.wire(), true);
        if a == bare && c == probed {
            silent.push(format!("ESC {} {}", f.inter as char, f.final_byte as char));
        }
    }
    assert!(
        silent.is_empty(),
        "{} of {} specification ESC-intermediate forms are consumed in silence:\n  {}",
        silent.len(),
        forms.len(),
        silent.join("\n  ")
    );
}

/// **A final byte the specification gives two arities must mean two things.**
///
/// Every other check here asks whether a sequence reached *a* handler. None
/// asked whether it reached the *right* one, and that is how
/// `CSI Ps ; Ps ; Ps ; Ps ; Ps T` came to scroll the screen: ctlseqs gives the
/// final byte `T` to SD with one parameter and to XTHIMOUSE with five, baud
/// read both as SD, and every "was it dispatched" check said yes. It was
/// dispatched to the wrong operation.
///
/// ctlseqs gives exactly two finals more than one arity -- `T` and `s` -- so
/// this is a small check rather than a framework. Feeding both arities must
/// not produce the same state: if it does, one of them is being read as the
/// other.
#[test]
fn a_final_with_two_arities_means_two_things() {
    // `[[source: ctlseqs]]`, extracted by arity:
    //   T -> 1 (SD) and 5 (XTHIMOUSE)
    //   s -> 0 (SCOSC) and 2 (DECSLRM)
    //
    // DECSLRM is only honoured with DECLRMM set, so the `s` pair is fed with
    // `?69h` first -- otherwise both arities *are* SCOSC, correctly, and the
    // comparison would be measuring the mode rather than the arity.
    const PAIRS: &[(&[u8], &[u8], &str)] = &[
        (b"\x1b[1T", b"\x1b[1;2;3;4;5T", "SD against XTHIMOUSE"),
        (
            b"\x1b[?69h\x1b[s",
            b"\x1b[?69h\x1b[1;2s",
            "SCOSC against DECSLRM",
        ),
    ];
    for (low, high, what) in PAIRS {
        let a = snapshot(low, false);
        let b = snapshot(high, false);
        assert!(
            a != b,
            "{what}: both arities left the terminal in the same state, so one \
             is being read as the other"
        );
    }
}
