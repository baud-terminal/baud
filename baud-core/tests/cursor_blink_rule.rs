//! **`?12` and the configured blink: either one blinks the cursor, and the
//! style is saved across `?1049`**.
//!
//! baud folded `?12` into the one cursor style, so `?12l` turned the blink
//! off for good. vim sends `?12h ?12l` at start and helix ends its session on
//! `0 q 2 q ?12l` on the alternate screen, so after either one the shell's
//! cursor stopped blinking (`tests/cursor/README.md`). Two halves fix it, each
//! following a terminal that gets its case right:
//!
//! * **foot's OR for `?12`**: the drawn blink is `?12` set *or* the blink
//!   DECSCUSR (or the configuration) gave the style, so `?12l` cannot stop a
//!   cursor the style blinks, and `?12h` still blinks a steady one.
//! * **kitty's and wezterm's `?1049`**: the style and the `?12` bit are saved
//!   with the cursor at `?1049 h` and restored at `?1049 l`, so what a program
//!   sets on the alternate screen stays there.
//!
//! The answers stay truthful to what was asked: `DECRQSS SP q` reports the
//! style DECSCUSR set, and `DECRQM ?12` the `?12` bit, as foot and xterm
//! report it -- so an application that saves the mode and restores it gets its
//! own state back.
//!
//! Each rule is fed at read sizes 1, 3, 7 and whole.
mod support;
use baud_core::{CursorStyle, Owned, Term};

/// Feed `bytes` at every read size to a terminal configured with `start`, and
/// hand each terminal to `f`.
fn each_read_size(start: CursorStyle, bytes: &[u8], f: impl Fn(&mut Term<'_>)) {
    for chunk in [1, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        t.set_initial_cursor_style(start);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&mut t);
    }
}

/// The reply to `query`, fed to `t` after whatever it has seen.
fn ask(t: &mut Term<'_>, query: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, query);
    t.reply().to_vec()
}

const DECRQSS: &[u8] = b"\x1bP$q q\x1b\\";
const DECRQM_12: &[u8] = b"\x1b[?12$p";

fn capture(name: &str) -> Vec<u8> {
    let path = format!("{}/../tests/cursor/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The position just past the first occurrence of `needle` in `hay`.
fn after(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("{:?} not in the capture", String::from_utf8_lossy(needle)))
        + needle.len()
}

// ---- The recorded level ------------------------------------------------

/// **vim**: its start-up `?12h ?12l` no longer stops the configured blink,
/// inside the session or after it. A steady configuration stays steady, and
/// the terminal still answers the style DECSCUSR left.
#[test]
fn vim_keeps_the_configured_blink_inside_and_after() {
    let bytes = capture("vim-exit.raw");
    let nudge = after(&bytes, b"\x1b[?12l");
    assert_eq!(nudge, 57, "the capture's ?12h ?12l at 45 and 51");
    each_read_size(CursorStyle::BlinkingBlock, &bytes[..nudge], |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock, "inside vim");
    });
    each_read_size(CursorStyle::BlinkingBlock, &bytes, |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock, "after vim");
        assert_eq!(ask(t, DECRQSS), b"\x1bP1$r1 q\x1b\\");
        assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;2$y");
    });
    each_read_size(CursorStyle::SteadyBar, &bytes, |t| {
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBar, "a steady config");
    });
}

/// **helix**: its exit's `0 q 2 q ?12l` all land on the alternate screen, and
/// `?1049 l` gives the shell its own cursor back. Inside, the steady block it
/// asked for is what is drawn.
#[test]
fn helix_gives_the_shell_its_cursor_back() {
    let bytes = capture("helix-exit.raw");
    let leave = after(&bytes, b"\x1b[?1049l") - b"\x1b[?1049l".len();
    each_read_size(CursorStyle::BlinkingBlock, &bytes[..leave], |t| {
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBlock, "helix's own");
    });
    each_read_size(CursorStyle::BlinkingBlock, &bytes, |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock, "after helix");
        assert_eq!(ask(t, DECRQSS), b"\x1bP1$r1 q\x1b\\");
        assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;2$y");
    });
    each_read_size(CursorStyle::SteadyBar, &bytes, |t| {
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBar, "a steady config");
    });
}

// ---- The hand model ----------------------------------------------------

/// `tput cnorm` on the primary screen -- `?12l ?25h` in this machine's
/// `xterm-256color` -- keeps a blinking configuration blinking.
#[test]
fn cnorm_on_the_primary_keeps_the_configured_blink() {
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[?12l\x1b[?25h", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock);
    });
}

/// A program that sets a steady cursor and leaves gives the shell the one it
/// had.
#[test]
fn a_program_that_sets_a_steady_cursor_and_leaves_hands_back_the_shells() {
    each_read_size(
        CursorStyle::BlinkingBlock,
        b"\x1b[?1049h\x1b[2 q\x1b[?12l\x1b[?1049l",
        |t| assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock),
    );
}

/// **A pin, not a red-first test: rule F's residual.** tmux holds the outer
/// alternate screen for its whole session and sends the outer `Se` -- `2 q`
/// here -- when a pane returns to the default style. Nothing leaves that
/// screen, so the steady block stays. A terminfo entry whose `Se` is `0 q`
/// is what would move this (an open question).
#[test]
fn a_steady_block_on_a_screen_that_stays_up_is_steady() {
    each_read_size(
        CursorStyle::BlinkingBlock,
        b"\x1b[?1049h\x1b[?12l\x1b[?25h\x1b[2 q",
        |t| assert_eq!(t.cursor_style(), CursorStyle::SteadyBlock),
    );
}

// ---- The unit level ----------------------------------------------------

/// The drawn blink is `?12` or the style's own: `?12l` does not stop a style
/// that blinks, `?12h` blinks a steady one, and the shape is the style's.
#[test]
fn twelve_ors_with_the_styles_blink() {
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[?12l", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock);
    });
    each_read_size(CursorStyle::SteadyBar, b"\x1b[?12h", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar);
    });
    each_read_size(CursorStyle::SteadyBar, b"\x1b[?12h\x1b[?12l", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBar);
    });
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[2 q\x1b[?12h", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock, "2 q ?12h");
    });
    // foot's rule: `?12` wins over a later steady DECSCUSR, since the two
    // are separate inputs and the drawn blink is either.
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[?12h\x1b[4 q", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingUnderline, "?12h 4 q");
    });
}

/// **DECSCUSR `0` and `7` leave `?12`**, as foot's `0 q` does: under a
/// steady configuration a `?12h` still blinks the configured cursor after a
/// reset of the style to it.
#[test]
fn decscusr_zero_and_seven_leave_twelve() {
    for reset in [&b"\x1b[0 q"[..], b"\x1b[7 q"] {
        let bytes = [&b"\x1b[?12h"[..], reset].concat();
        each_read_size(CursorStyle::SteadyBar, &bytes, |t| {
            assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar);
            assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;1$y");
        });
    }
}

/// `DECRQM ?12` answers the `?12` bit, not the drawn blink: reset at power-on
/// while a blinking configuration blinks, and set only by `?12h`.
#[test]
fn decrqm_twelve_answers_the_mode() {
    each_read_size(CursorStyle::BlinkingBlock, b"", |t| {
        assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;2$y", "power-on");
    });
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[?12h", |t| {
        assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;1$y");
    });
    each_read_size(CursorStyle::SteadyBar, b"\x1b[?12h\x1b[?12l", |t| {
        assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;2$y");
    });
}

/// `DECRQSS SP q` answers the style DECSCUSR set, whatever `?12` draws.
#[test]
fn decrqss_answers_the_style_decscusr_set() {
    each_read_size(CursorStyle::SteadyBar, b"\x1b[?12h", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar, "drawn");
        assert_eq!(ask(t, DECRQSS), b"\x1bP1$r6 q\x1b\\", "asked");
    });
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[?12h\x1b[2 q", |t| {
        assert_eq!(ask(t, DECRQSS), b"\x1bP1$r2 q\x1b\\");
    });
}

/// `?1049 h` saves the style and the `?12` bit with the cursor, and `?1049 l`
/// restores both.
#[test]
fn the_style_and_twelve_ride_the_1049_save() {
    each_read_size(
        CursorStyle::BlinkingBlock,
        b"\x1b[5 q\x1b[?1049h\x1b[2 q\x1b[?12h\x1b[?1049l",
        |t| {
            assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar);
            assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;2$y");
            assert_eq!(ask(t, DECRQSS), b"\x1bP1$r5 q\x1b\\");
        },
    );
    each_read_size(
        CursorStyle::SteadyBar,
        b"\x1b[?12h\x1b[?1049h\x1b[?12l\x1b[?1049l",
        |t| {
            assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar, "?12 restored");
            assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;1$y");
        },
    );
}

/// The alternate screen starts with the primary's style, as xterm, wezterm
/// and ghostty's shape do.
#[test]
fn the_alternate_screen_starts_with_the_primarys_style() {
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[3 q\x1b[?1049h", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingUnderline);
    });
}

/// A second `?1049 h` while the alternate screen is up keeps the first save,
/// as it keeps the first cursor.
#[test]
fn a_second_1049h_keeps_the_first_save() {
    each_read_size(
        CursorStyle::BlinkingBlock,
        b"\x1b[?1049h\x1b[2 q\x1b[?1049h\x1b[4 q\x1b[?1049l",
        |t| assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock),
    );
}

/// `?1049 l` on the primary with nothing saved leaves the style alone.
#[test]
fn a_1049l_with_nothing_saved_leaves_the_style() {
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[3 q\x1b[?1049l", |t| {
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingUnderline);
    });
}

/// `?47` shares the one cursor, and so its style: it saves and restores none.
#[test]
fn _47_neither_saves_nor_restores_the_style() {
    each_read_size(
        CursorStyle::BlinkingBlock,
        b"\x1b[?47h\x1b[2 q\x1b[?12h\x1b[?47l",
        |t| {
            assert_eq!(t.cursor_style(), CursorStyle::BlinkingBlock, "drawn");
            assert_eq!(ask(t, DECRQSS), b"\x1bP1$r2 q\x1b\\");
            assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;1$y");
        },
    );
}

/// A `?1049 h` save left pending by a `?47` exit is restored, style and all,
/// by the `?1049 l` that follows on the primary.
#[test]
fn a_pending_save_restores_its_style() {
    each_read_size(
        CursorStyle::BlinkingBlock,
        b"\x1b[3 q\x1b[?1049h\x1b[?47l\x1b[2 q\x1b[?1049l",
        |t| assert_eq!(t.cursor_style(), CursorStyle::BlinkingUnderline),
    );
}

/// Both resets restore the configured style and clear `?12`, from either
/// screen. The last case saves a style that is not the configured one, so
/// `RIS` has to leave the alternate screen -- restoring that save -- *before*
/// it resets, or the save would win.
#[test]
fn both_resets_clear_twelve() {
    for reset in [&b"\x1b[!p"[..], b"\x1bc"] {
        for before in [
            &b"\x1b[2 q\x1b[?12h"[..],
            b"\x1b[?1049h\x1b[2 q\x1b[?12h",
            b"\x1b[2 q\x1b[?12h\x1b[?1049h",
        ] {
            let bytes = [before, reset].concat();
            each_read_size(CursorStyle::SteadyBar, &bytes, |t| {
                assert_eq!(t.cursor_style(), CursorStyle::SteadyBar);
                assert_eq!(ask(t, DECRQM_12), b"\x1b[?12;2$y");
            });
        }
    }
}

// ---- A style changed under a running terminal ---------------------------

/// **A reloaded cursor style reaches a cursor no program styled**: an
/// embedder that reads its settings again changes the style in force, and
/// the one `DECSCUSR 0` restores.
#[test]
fn a_reconfigured_style_reaches_a_cursor_no_program_styled() {
    each_read_size(CursorStyle::BlinkingBlock, b"", |t| {
        t.reconfigure_cursor_style(CursorStyle::SteadyBar);
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBar);
        support::feed(t, b"\x1b[2 q\x1b[0 q");
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBar, "DECSCUSR 0");
    });
}

/// **A style a program set stays the program's**, and a reset gives the
/// reloaded one: helix's `5 q` survives a reload, its `0 q` at exit returns
/// to the new configured style, not the launch's.
#[test]
fn a_program_styled_cursor_keeps_its_style_over_a_reconfigure() {
    each_read_size(CursorStyle::BlinkingBlock, b"\x1b[4 q", |t| {
        t.reconfigure_cursor_style(CursorStyle::SteadyBar);
        assert_eq!(t.cursor_style(), CursorStyle::SteadyUnderline);
        support::feed(t, b"\x1b[0 q");
        assert_eq!(t.cursor_style(), CursorStyle::SteadyBar);
    });
}
