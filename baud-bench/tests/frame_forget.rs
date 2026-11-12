//! **`frame`'s `FRAME_FORGET` arm: off unless asked, and work to do on the
//! recordings it is run on.** The bench compiles `baud-app`'s `paths.rs`,
//! whose own tests need the whole app, so no test here can build it; what
//! the arm does is read from the bench's source instead, and what it is
//! given to do from the core's record of each read. Off, the default, takes
//! no record and forgets nothing, so it is the row of a build before the
//! arm; on, one function takes each read's record and calls the app's
//! `Erased::of` and `Paths::forget_erased` on both screens. vim's recording
//! erases lines whole in its reads, so the arm forgets there; btop's erases
//! none, the arm's control.

use baud_core::{Owned, Screen, Term};
use baud_domain::{BytePos, ReadLen, WinSize};

/// The bench's source with every comment line left out, so a name in a
/// doc is not a call.
fn code() -> String {
    include_str!("../benches/frame.rs")
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of `src` from `start` to the close of the first brace block
/// opened after it.
fn block<'a>(src: &'a str, start: &str) -> &'a str {
    let at = src
        .find(start)
        .unwrap_or_else(|| panic!("`{start}` is in the bench"));
    let open = at + src[at..].find('{').expect("a block after it");
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[at..=open + i];
                }
            }
            _ => {}
        }
    }
    panic!("`{start}`'s block is never closed")
}

/// **`FRAME_FORGET` is off unless asked, and off forgets nothing**: the
/// switch reads off when unset or `off` and on only for `on`; a frame takes
/// the primary's top as its read begins only under on, and the one call
/// of `forget_read` is under that; and the record is taken, `Erased::of`
/// called and `Paths::forget_erased` called nowhere but in `forget_read`,
/// on both screens. Named break: the switch on when unset -- the first
/// assertion.
#[test]
fn the_forget_arm_is_off_unless_asked_and_off_forgets_nothing() {
    let src = code();
    let parse = block(&src, "let forget = match std::env::var(\"FRAME_FORGET\")");
    assert!(
        parse.contains("Ok(\"off\") | Err(_) => Switch::Off"),
        "off when unset: {parse}"
    );
    assert!(parse.contains("Ok(\"on\") => Switch::On"), "{parse}");
    let began = block(&src, "let began = match forget");
    assert!(began.contains("Switch::On if n > 0 => Some("), "{began}");
    assert!(
        began.contains("Switch::On | Switch::Off => None"),
        "{began}"
    );
    assert_eq!(
        src.matches("forget_read(").count(),
        2,
        "defined and called once"
    );
    assert!(
        block(&src, "if let Some(base) = began").contains("forget_read("),
        "the call is under on"
    );
    let body = block(&src, "fn forget_read(");
    for name in ["take_record(", "Erased::of(", ".forget_erased("] {
        assert_eq!(src.matches(name).count(), 1, "{name} once");
        assert!(body.contains(name), "{name} in forget_read");
    }
    assert!(
        body.contains("Screen::Normal") && body.contains("Screen::Alternate"),
        "both screens"
    );
}

/// The runs the core's record says each 4096-byte read of `name` erased
/// whole, on both screens, at the bench's 200x50, and the records that
/// held more than they have room for.
fn erasures(name: &str) -> (usize, usize) {
    let data = std::fs::read(format!(
        "{}/../tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the recording");
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let (mut runs, mut overflowed) = (0, 0);
    for chunk in data.chunks(4096) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        let rec = t.take_record();
        for screen in [Screen::Normal, Screen::Alternate] {
            match rec.erased(screen) {
                Some(spans) => runs += spans.count(),
                None => overflowed += 1,
            }
        }
    }
    (runs, overflowed)
}

/// **vim's recording erases lines whole, and btop's none**: the runs the
/// `FRAME_FORGET=on` arm forgets come from these records, so on vim it has
/// forgets to make, and on btop, which redraws by writing over its cells,
/// none -- the arm's control, where on differs from off by the record
/// alone.
#[test]
fn vim_erases_lines_whole_and_btop_none() {
    let (vim, _) = erasures("utf8/vim-unicode.raw");
    assert!(vim > 0, "no read of vim's erases a line whole");
    assert_eq!(erasures("utf8/btop.raw"), (0, 0), "btop's");
}
