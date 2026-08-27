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
        if !l.starts_with("| `CSI") {
            continue;
        }
        let cell = l.trim_matches('|').split('|').next().unwrap_or("");
        // HPA's final byte *is* a backtick, written as a markdown escape.
        // Unescaped to a sentinel **before** splitting, because splitting on
        // backticks otherwise cuts the escape in half and yields the backslash
        // as the final byte.
        let cell = cell.replace("\\`", "\u{0}");
        // A cell may name several forms: `CSI H` / `CSI f`. Each backticked
        // span is one, and taking the last token of the joined string missed
        // `H` entirely.
        for form in cell.split('`').skip(1).step_by(2) {
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
}

/// A prepared terminal: text on two rows and the cursor in the middle.
///
/// A form fed to a *blank* grid at the origin can legitimately do nothing --
/// `CSI 1 A` at row 0 is a no-op -- so a blank start would report real
/// operations as silent.
const PREAMBLE: &[u8] = b"hello world\r\nsecond line\r\n\x1b[2;5H";

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
