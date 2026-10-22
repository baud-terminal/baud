//! The prompt block: the rows the shell will redraw, tracked so that a
//! redraw mark replaces exactly them.
//!
//! zsh keeps one number about the screen -- the rows from its prompt's
//! first line to the cursor -- and on `SIGWINCH` moves up that many,
//! erases to the end of the screen and redraws. That count goes stale
//! whenever a redraw was computed for a width the screen has left, and
//! it clamps at row 0 whenever the block is taller than the window. The
//! terminal knows where the block begins, as a ring row, so at the mark
//! the block is replaced from where it *is*: the cursor is put on its
//! first row, the window is moved back over any of its rows that
//! scrolled off, and every row of it is erased wherever it now sits.
//! Nothing above the block is touched and nothing the terminal made is
//! left in the history. The recordings that each rule answers to are in
//! `docs/research/notes/m-resize-and-reflow.md`; the tests that fail
//! without it are `a_block_never_moves_once_it_is_drawn`,
//! `no_more_than_two_blank_rows_come_between_consecutive_prompts`,
//! `a_gesture_leaves_one_prompt_on_screen` and
//! `a_gesture_on_a_fresh_terminal_creates_no_history`.

use crate::paint::Paint;
use crate::term::SurfaceLine;
use baud_domain::*;

/// Where the block the shell is at begins.
///
/// A ring row, so a scroll or a resize's rotation leaves it naming the
/// same cells; a screen row went stale at the first scroll. Meaningful
/// only while its screen's [`Shell::zone`] is [`crate::term::Zone::Prompt`].
#[derive(Clone, Copy)]
pub(crate) struct PromptBlock {
    start: RowRot,
}

impl PromptBlock {
    pub(crate) const NONE: PromptBlock = PromptBlock {
        start: RowRot::NONE,
    };

    pub(crate) fn start(self) -> RowRot {
        self.start
    }

    pub(crate) fn at(start: RowRot) -> PromptBlock {
        PromptBlock { start }
    }
}

/// What the marks have said about **one screen**: its zone, where its
/// block begins, what the command's `A` asked of the terminal, and where
/// the user's input began.
///
/// **One per screen, swapped by `?1049` as the kitty stacks are.** The
/// block start is a row of one surface's ring, so read against the other
/// ring it names rows the shell never drew: a redraw on the alternate
/// screen erased its rows by the primary's block start (a subtraction that
/// underflows whenever that start, a primary ring row, lies past the
/// alternate screen's cursor row plus its ring -- a primary turned three
/// rows with its prompt on the last of twenty is enough), a
/// block opened there was redrawn on the primary against the alternate
/// ring's numbers and took the history with it, and a mark there stopped
/// the *primary* re-cutting, because `Term::resize` asked the zone of
/// whichever screen last sent one. The alternate screen's state starts
/// fresh on every entry: `?1049` clears the screen, and `?47`, which keeps
/// it, keeps text a program drew, not a prompt the shell will count rows
/// from -- a block start left from the last visit is not carried across.
///
/// **On `StrBuf`, not `Cold`**, since the options came in: nothing here is
/// read per byte, and `Cold`'s size is pinned.
#[derive(Clone, Copy)]
pub(crate) struct Shell {
    pub(crate) zone: crate::term::Zone,
    pub(crate) block: PromptBlock,
    pub(crate) asks: Asks,
    /// Where `B` or `I` left the cursor: the input's first cell.
    pub(crate) input: Input,
    /// The newest row a `k=s` or `k=c` line began on, in this block.
    pub(crate) secondary: Later,
}

/// What the `A` or `N` that opened the command asked: kitty's `redraw=`,
/// the proposal's `cl=` and `move-keys=`, kitty's `click_events=` and
/// `special_key=`. **Reset at every new command**, as kitty's `screen.c`
/// does at each `A` (`self->prompt_settings.val = 0`): a setting belongs to
/// the command whose `A` carried it. kitty zeroes them at every `A`, a
/// redraw's and an `A;k=s` included; baud resets them only at an `A` or `N`
/// that opens a new command, and an `A` in the prompt zone adds its
/// options to the ones the command has.
#[derive(Clone, Copy)]
pub(crate) struct Asks {
    pub(crate) redraw: Redraw,
    pub(crate) click: Option<ClickToMove>,
    pub(crate) click_events: Option<ClickEvents>,
    pub(crate) special_key: SpecialKey,
    pub(crate) move_keys: Option<MoveKeys>,
}

impl Asks {
    pub(crate) const NONE: Asks = Asks {
        redraw: Redraw::Prompt,
        click: None,
        click_events: None,
        special_key: SpecialKey::Arrows,
        move_keys: None,
    };

    /// Take what `o` says and keep what it does not.
    fn apply(&mut self, o: &PromptOptions<'_>) {
        self.redraw = o.redraw.unwrap_or(self.redraw);
        self.click = o.click.or(self.click);
        self.click_events = o.click_events.or(self.click_events);
        self.special_key = o.special_key.unwrap_or(self.special_key);
        if let Some(k) = o.move_keys.and_then(MoveKeys::of) {
            self.move_keys = Some(k);
        }
    }
}

/// Where a `B` or an `I` put the input's first cell, a ring row so it
/// follows its cells through a scroll.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    None,
    At {
        row: RowRot,
        col: Col,
        end: InputEnd,
    },
}

/// A later prompt line's first row, a ring row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Later {
    None,
    At(RowRot),
}

/// An initial line's mark: a new block, or the shell redrawing the one it
/// is at -- an initial mark that arrives while the prompt zone is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InitialMark {
    Fresh,
    Redraw,
}

impl InitialMark {
    fn in_zone(zone: crate::term::Zone) -> InitialMark {
        match zone {
            crate::term::Zone::Prompt => InitialMark::Redraw,
            crate::term::Zone::Output => InitialMark::Fresh,
        }
    }
}

impl Shell {
    /// A screen no mark has spoken about: output, which is what a stream
    /// with no shell integration is.
    pub(crate) const FRESH: Shell = Shell {
        zone: crate::term::Zone::Output,
        block: PromptBlock::NONE,
        asks: Asks::NONE,
        input: Input::None,
        secondary: Later::None,
    };
}

impl Paint<'_, '_> {
    /// **`OSC 133`, acted on.** See [`Semantic`] for each letter's text.
    pub(crate) fn semantic(&mut self, mark: Semantic<'_>) {
        self.end_input_at_eol(mark);
        match mark {
            Semantic::FreshLine => self.fresh_line(),
            // "First do a fresh-line." For a later line in kitty's spelling
            // too: it is an `A`. **But not ahead of a redraw's erase**: the
            // erase puts the cursor on the block's first row, which it has
            // just emptied, so the line is fresh once the cursor is at its
            // start -- a line feed first would scroll a row into the history
            // that the erase then has to take back, or cannot.
            Semantic::Command(opener, o) => match o.kind {
                PromptKind::Initial => {
                    let mark = InitialMark::in_zone(self.strs.shell.zone);
                    let redraw = mark == InitialMark::Redraw;
                    if !redraw {
                        self.strs.shell.asks = Asks::NONE;
                    }
                    self.strs.shell.asks.apply(&o);
                    let erases = redraw && self.strs.shell.asks.redraw != Redraw::Never;
                    if !erases {
                        self.fresh_line();
                    }
                    self.initial_prompt(mark, Some((o.aid, opener)));
                    if erases {
                        self.carriage_return();
                    }
                }
                kind => {
                    self.fresh_line();
                    self.strs.shell.asks.apply(&o);
                    self.later_prompt(kind);
                }
            },
            Semantic::Prompt(PromptKind::Initial) => {
                self.initial_prompt(InitialMark::in_zone(self.strs.shell.zone), None);
            }
            Semantic::Prompt(kind) => self.later_prompt(kind),
            Semantic::Input(end) => {
                let (rot, ring) = (self.view.rotation(), self.view.ring());
                self.strs.shell.input = Input::At {
                    row: rot.advance(self.cy.index(), ring),
                    col: self.cx.col(),
                    end,
                };
                self.strs.shell.zone = crate::term::Zone::Prompt;
            }
            Semantic::OutputStart => self.output_began(),
            // `D` closes the zone as `C` does: Enter on an empty line sends
            // no `C`, and the `P` after `D` is a fresh prompt, not a redraw
            // -- and so is anything a `precmd` prints first.
            Semantic::CommandDone(done) => {
                if self.cold.screen == crate::term::Screen::Normal {
                    self.strs.prompts.close(done);
                }
                self.strs.shell.zone = crate::term::Zone::Output;
                self.strs.shell.input = Input::None;
            }
            Semantic::Other => {}
        }
    }

    /// `L`, and the first step of `A` and `N`: "If the cursor is the
    /// initial column ... do nothing. Otherwise, do the equivalent of
    /// `\r\n`" -- the initial column being where `CR` puts it, so under
    /// side margins the left margin, as Ghostty reads it.
    fn fresh_line(&mut self) {
        let at = self.cx;
        self.carriage_return();
        if self.cx != at {
            self.line_feed(crate::paint::Feed::Unprinted);
        }
    }

    /// An initial line: a new block, or a redraw of the one the shell is at.
    /// A redraw under `redraw=0` erases nothing: "the shell will not redraw
    /// the prompt on resize so it should not erase it". `redraw=last` erases
    /// as `1` does: under it an initial mark reaches a block only where the
    /// shell redraws a line that begins with one (Jev's lean, AS_ONE).
    fn initial_prompt(&mut self, mark: InitialMark, command: Option<(Aid, Opener)>) {
        let redraw = mark == InitialMark::Redraw;
        if redraw && self.strs.shell.asks.redraw != Redraw::Never {
            self.replace_prompt_block();
        }
        self.open_prompt_block();
        let shell = &mut self.strs.shell;
        shell.zone = crate::term::Zone::Prompt;
        shell.input = Input::None;
        shell.secondary = Later::None;
        if self.cold.screen != crate::term::Screen::Normal {
            return;
        }
        let line = self.book.top.plus(Rows::of_count(self.cy.index()));
        match (redraw, command) {
            (true, _) => self.strs.prompts.redrawn(line),
            (false, Some((aid, opener))) => self.strs.prompts.open(line, aid, opener),
            (false, None) => self.strs.prompts.open_bare(line),
        }
    }

    /// A later line of the block: `k=s`, `k=c` or `k=r`.
    fn later_prompt(&mut self, kind: PromptKind) {
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        let shell = &mut self.strs.shell;
        shell.zone = crate::term::Zone::Prompt;
        if matches!(kind, PromptKind::Secondary | PromptKind::Continuation) {
            shell.secondary = Later::At(rot.advance(self.cy.index(), ring));
        }
    }

    /// **`I`'s input ends at the end of its line**, read when the next mark
    /// comes rather than at every line feed, which would put a test on the
    /// write path. "If the cursor is on a fresh (empty) line and we see
    /// either `P` or `I` then this is the start of a continuation input
    /// line. If we see anything else, it is the start of the output area":
    /// so a line that ended since the `I` began the output, unless this
    /// mark is a `P` or an `I` at the start of a line.
    fn end_input_at_eol(&mut self, mark: Semantic<'_>) {
        if !self.input_line_ended() {
            return;
        }
        let continues =
            matches!(mark, Semantic::Prompt(_) | Semantic::Input(_)) && self.cx.col() == Col::ZERO;
        if !continues {
            self.output_began();
        }
    }

    /// The command's output began: `C`, or the end of an `I` input's line.
    fn output_began(&mut self) {
        self.strs.shell.zone = crate::term::Zone::Output;
        self.strs.shell.input = Input::None;
        if self.cold.screen == crate::term::Screen::Normal {
            self.strs.prompts.output_began();
        }
    }

    /// Whether an `I` input's line has ended: a row from its row to the
    /// cursor's that did not wrap, or its row gone from the screen.
    pub(crate) fn input_line_ended(&self) -> bool {
        let Input::At {
            row,
            end: InputEnd::AtEndOfLine,
            ..
        } = self.strs.shell.input
        else {
            return false;
        };
        let y = row
            .retreat(self.view.rotation().index(), self.view.ring())
            .index();
        let cy = self.cy.index();
        y > cy || (y..cy).any(|r| !self.view.extent(Row::from_index(r)).wrapped())
    }

    /// The block begins on the cursor's row: an initial mark.
    pub(crate) fn open_prompt_block(&mut self) {
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        self.strs.shell.block = PromptBlock {
            start: rot.advance(self.cy.index(), ring),
        };
    }

    /// The shell is redrawing the block it has: replace it where it is.
    ///
    /// Refused only when the block's first row is further back than the
    /// history holds -- a stale start is not this block's -- or on the
    /// alternate screen, whose ring keeps no history. Rows of the block
    /// behind the window are taken back by moving the window over them
    /// when the rows it gives up below show nothing (the shell's `ED`
    /// has just cleared them); otherwise they are erased where they lie.
    pub(crate) fn replace_prompt_block(&mut self) {
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        let (cy, r, rows) = (self.cy.index(), ring.index(), self.rows.index());
        let start = self.strs.shell.block.start();
        // Rows from the block's first row to the cursor, in ring order.
        let d = (rot.advance(cy, ring).index() + r - start.index()) % r;
        let above = d.saturating_sub(cy);
        let retained =
            self.cold.screen == crate::term::Screen::Normal && above <= self.book.hist.index();
        if above > 0 && !retained {
            return;
        }
        let gives_up = rows - above.min(rows)..rows;
        let back = above > 0
            && gives_up
                .into_iter()
                .all(|y| self.view.row_is_blank(Row::from_index(y)));
        if back {
            // The rows full-screen scrolls swept are found by where they lie,
            // and the window is about to move back.
            if self.past_edge() {
                self.clear_swept();
            }
            self.view.retreat_rot(above);
            let hist = self.book.hist.index() - above;
            self.book.hist = Rows::of_count(hist);
            self.book.hidden =
                Rows::of_count((self.book.hidden.index() + above).min(r - rows - hist));
            // The window's top line moves back with it -- `above` is within
            // the history, and the top line is never below the history's
            // depth, so it cannot run out -- and rows the window gave up past
            // what the ring keeps below take their images with them.
            self.book.top = self
                .book
                .top
                .minus(Rows::of_count(above))
                .unwrap_or(LineIdx::FIRST);
            // A window over the history the block's rows came back over is
            // on the live screen from here (`Book::window_taken_back`).
            self.book.window_taken_back();
            if self.cold.placement.is_placed() {
                self.primary_placement_moved(None);
            }
            if self.images.any_placed() {
                self.expire_primary_images(None);
            }
            self.book.swept = self.book.top;
        }
        let blank = self.blank();
        for k in 0..=d {
            let at = start.advance(k, ring);
            if self.past_edge() {
                self.clear_erased_past_edge(at, blank);
            }
            let n = self.view.fill_at(at, blank);
            self.cells = self.cells.plus(n.index());
        }
        // **The block's lines are erased, so a selection on one goes**, as
        // any erase's: tested against the watch on a selection's lines with
        // the window where the erase left it -- taken back over the block's
        // first rows, which then run from the top row down, or left where it
        // was, with the block's first rows in the history -- so a read that
        // retires the lines taken back again nets no movement and still
        // drops a selection on them. The rows given up below are blank, and
        // come back blank. A block wholly on the screen was erased with no
        // test until 2026-10-17.
        let (from, to) = match (back, above) {
            (true, _) => (SurfaceLine::At(Row::ZERO), (cy + above).min(rows - 1)),
            (false, 0) => (SurfaceLine::At(Row::from_index(cy - d)), cy),
            (false, _) => (SurfaceLine::Above(Rows::of_count(above)), cy),
        };
        self.cold
            .test_watch_lines(from, SurfaceLine::At(Row::from_index(to)), self.book.top);
        self.set_row(Row::from_index(cy.saturating_sub(d)));
        self.prompt_erases += 1;
    }
}
