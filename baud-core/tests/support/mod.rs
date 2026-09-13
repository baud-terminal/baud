//! The driving loop, once, for the conformance suites.
//!
//! **Not a compatibility shim.** `baud-core2`'s suites each repeat five lines
//! of `read_slot`/`base`/`write`/`feed`/`commit_read`, and the temptation when
//! moving them was to re-spell that API on top of `write_at`/`feed` so the
//! files could come across untouched. That would have hidden the one
//! difference that matters: `viewport()` strides by `MAX_COLS` in core2 and by
//! a **fixed pitch of at least that** here -- `Term::stride`, chosen from the
//! cache line, not a number -- so `t.viewport()[..8]` and
//! `t.row(Row::from_index(1))` do not mean the same thing in the two crates.
//! Fifty-two `viewport()` call sites would have passed vacuously.
//!
//! So this is `baud-core`'s own loop factored once, and the ported tests were
//! read rather than rewritten by pattern.

#![allow(dead_code)]

use baud_core::*;

/// The geometry both cores start at, so a ported test measures the grid it was
/// written against. `baud-core2::Term::new` hard-codes it; here it is the
/// `WinSize` a caller passes, and the suites passed nothing.
pub const CORPUS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

/// Run `f` against a fresh terminal at `ws`.
///
/// A closure rather than a returned `Term`, because `Term<'b>` borrows its
/// storage: the `Owned` has to outlive it, and only the caller's frame can own
/// that. The borrow checker is stating the design -- storage is
/// caller-provided -- rather than getting in the way.
pub fn with_term<R>(ws: &WinSize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    f(&mut t)
}

/// Room to grow to `rows`, for a test that resizes upward.
///
/// **`Owned::new` provisions exactly the geometry it is given**, and
/// `Term::resize` returns `Truncated` rather than growing past it -- the grid
/// is caller-provided, so the library cannot allocate on the caller's behalf.
/// `baud-core2` can always grow to `MAX_ROWS` because its `Storage` is a fixed
/// `[PackedCell; GRID]`; here the caller says how much. A test that grows says
/// so here.
pub fn with_room<R>(rows: u16, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let big = WinSize {
        ws_row: rows,
        ws_col: CORPUS.ws_col,
    };
    let mut owned = Owned::new(&big);
    let mut t = Term::new(owned.storage(), &CORPUS);
    f(&mut t)
}

/// The same, at the corpus geometry.
pub fn with_corpus<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    with_term(&CORPUS, f)
}

/// Feed every byte, in `READ_CHUNK` reads.
///
/// The chunking is not decoration: a control sequence split across two reads
/// is the case this repo's chunk-invariance properties exist for, and a helper
/// that fed the whole slice at once would take the suites off that path.
pub fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
}

/// Boot at `ws`, feed `bytes`, and hand the terminal to `f`.
pub fn render<R>(ws: &WinSize, bytes: &[u8], f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    with_term(ws, |t| {
        feed(t, bytes);
        f(t)
    })
}

/// Shrink to `ws` and assert the resize was not truncated.
///
/// **Asserted, not discarded**, which is the rule the suites already carry:
/// `Resized::Truncated` means the geometry exceeded a bound, and swallowing it
/// is the silent-clamp bug the return value exists to catch. Here there are
/// two bounds rather than one -- `MAX_COLS`/`MAX_ROWS` and the row capacity the
/// caller provisioned -- so a helper that resized *up* from the corpus
/// geometry would trip on the second and be right to.
pub fn shrink(t: &mut Term<'_>, ws: &WinSize) {
    assert_eq!(
        t.resize(ws),
        Resized::Ok,
        "{}x{} is a shrink from the corpus geometry",
        ws.ws_row,
        ws.ws_col
    );
}

/// Assert a suite's `#[ignore]` list is exactly `want`.
///
/// **An `#[ignore]` nobody counts is a test nobody runs**, which is the
/// probe-that-cannot-fire this repo records eight times. Every ported suite
/// that still has gaps calls this, so the list is a pinned number in the same
/// sense a pinned gap is: a new ignore fails it, and **closing a
/// gap fails it too**, which is the direction that matters -- the pin has to
/// move when the gap does.
pub fn ignores_are(dir: &str, files: &[&str], want: &[&str]) {
    let mut found = Vec::new();
    for f in files {
        let p = format!("{}/tests/{dir}/{f}", env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("#[ignore") {
                continue;
            }
            let name = lines[i..]
                .iter()
                .find_map(|l| l.trim_start().strip_prefix("fn "))
                .map(|n| n.split('(').next().unwrap_or(n).to_string())
                .unwrap_or_else(|| "<no fn follows the attribute>".into());
            found.push(name);
        }
    }
    found.sort();
    let mut want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(
        found, want,
        "{dir}: the ignore list moved -- a gap was added or closed, and this \
         pin has to move with it"
    );
}
