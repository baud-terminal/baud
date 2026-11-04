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
    /// Whether the window changed size since the last initial mark.
    pub(crate) resize: SinceMark,
}

/// **Whether a resize came since the last initial mark**: a shell redraws
/// its prompt on `SIGWINCH`, and the next initial mark in the prompt zone
/// is that redraw wherever the cursor is -- readline's goes to the
/// input's last line first, below any line break inside the input, where
/// a mark with no resize before it is the prompt printed again below a
/// list. Set by `Term::resize` on both screens; the mark that follows
/// takes it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SinceMark {
    Unresized,
    Resized,
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

/// An initial line's mark: a new block; the shell redrawing the one it is
/// at, a mark that arrives in the prompt zone with the cursor still on the
/// input's line or above it; or the same prompt drawn again below what the
/// shell wrote under its input -- a completion list, after which bash's
/// readline and zsh, when the list does not fit below the prompt, print
/// the prompt again rather than move back up to it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InitialMark {
    Fresh,
    Redraw,
    Again,
}

/// **How many prompts have opened on the primary**: the times its zone
/// turned from output to [`crate::term::Zone::Prompt`], as a reading to
/// compare. An embedder that samples the zone once a read cannot see a
/// command whose `C`, output and next `A` came in one read, which leaves
/// the zone at `Prompt` on both sides; the count has moved. A redraw's mark,
/// which arrives in the prompt zone, opens nothing, and a mark on the
/// alternate screen is the program's, not the shell's. Not reset by `RIS`,
/// so a reading taken before one never equals one taken after a prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromptsOpened(u32);

impl PromptsOpened {
    pub(crate) const NONE: PromptsOpened = PromptsOpened(0);

    fn next(self) -> PromptsOpened {
        PromptsOpened(self.0.wrapping_add(1))
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
        resize: SinceMark::Unresized,
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
                    let mark = self.initial_mark();
                    if mark == InitialMark::Fresh {
                        self.strs.shell.asks = Asks::NONE;
                    }
                    self.strs.shell.asks.apply(&o);
                    let erases =
                        mark == InitialMark::Redraw && self.strs.shell.asks.redraw != Redraw::Never;
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
                let mark = self.initial_mark();
                self.initial_prompt(mark, None);
            }
            Semantic::Prompt(kind) => self.later_prompt(kind),
            Semantic::Input(end) => {
                // The prompt is drawn, whatever its height: pin it now.
                if self.strs.shell.zone == crate::term::Zone::Prompt {
                    self.pin_prompt_block();
                }
                let (rot, ring) = (self.view.rotation(), self.view.ring());
                self.strs.shell.input = Input::At {
                    row: rot.advance(self.cy.index(), ring),
                    col: self.cx.col(),
                    end,
                };
                self.prompt_zone();
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

    /// **What an initial mark is**: a new block after output; in the prompt
    /// zone, a redraw of the block after a resize, or while the cursor is on
    /// the input's line or above it, where a shell that redraws moves it
    /// back to before it draws -- zsh up by its count, readline a `CR` and
    /// an `EL` on the input's line (`tests/prompt/bash-grow`) -- and the
    /// same prompt drawn again once the cursor has passed the end of the
    /// input's line with no resize since the last initial mark. A shell
    /// passes it only by writing below its input, and what it wrote there
    /// is its own, which the redraw's erase would take: a completion list.
    /// With no `B` or `I` yet the block's input is not known, and the mark
    /// is a redraw.
    fn initial_mark(&self) -> InitialMark {
        let shell = &self.strs.shell;
        match shell.zone {
            crate::term::Zone::Output => InitialMark::Fresh,
            crate::term::Zone::Prompt if shell.resize == SinceMark::Resized => InitialMark::Redraw,
            crate::term::Zone::Prompt if self.past_input() => InitialMark::Again,
            crate::term::Zone::Prompt => InitialMark::Redraw,
        }
    }

    /// Whether the cursor is past the end of the input's line: below a row
    /// from the input's row to the cursor's that did not wrap. With the
    /// input's row gone above the screen, the rows from the top to the
    /// cursor's are the ones read: zsh's count clamps at the top row of a
    /// block taller than the window, and readline redraws from the input's
    /// last row, both on rows the input wrapped onto.
    fn past_input(&self) -> bool {
        let Input::At { row, .. } = self.strs.shell.input else {
            return false;
        };
        let y = row
            .retreat(self.view.rotation().index(), self.view.ring())
            .index();
        let cy = self.cy.index();
        if y >= self.rows.index() {
            return (0..cy).any(|r| !self.view.extent(Row::from_index(r)).wrapped());
        }
        y < cy && (y..cy).any(|r| !self.view.extent(Row::from_index(r)).wrapped())
    }

    /// An initial line: a new block, a redraw of the one the shell is at, or
    /// the block drawn again below the shell's own lines, which erases
    /// nothing and keeps the command's place in the prompt log. A redraw
    /// under `redraw=0` erases nothing: "the shell will not redraw the
    /// prompt on resize so it should not erase it". `redraw=last` erases as
    /// `1` does: under it an initial mark reaches a block only where the
    /// shell redraws a line that begins with one.
    fn initial_prompt(&mut self, mark: InitialMark, command: Option<(Aid, Opener)>) {
        if mark == InitialMark::Redraw && self.strs.shell.asks.redraw != Redraw::Never {
            self.replace_prompt_block();
        }
        self.open_prompt_block();
        self.prompt_zone();
        let shell = &mut self.strs.shell;
        shell.input = Input::None;
        shell.secondary = Later::None;
        shell.resize = SinceMark::Unresized;
        if self.cold.screen != crate::term::Screen::Normal {
            return;
        }
        let line = self.book.top.plus(Rows::of_count(self.cy.index()));
        match (mark, command) {
            (InitialMark::Redraw | InitialMark::Again, _) => self.strs.prompts.redrawn(line),
            (InitialMark::Fresh, Some((aid, opener))) => self.strs.prompts.open(line, aid, opener),
            (InitialMark::Fresh, None) => self.strs.prompts.open_bare(line),
        }
    }

    /// The live screen's zone turns to the prompt; on the primary, from
    /// output, that is a prompt opening ([`PromptsOpened`]).
    fn prompt_zone(&mut self) {
        let opens = self.strs.shell.zone != crate::term::Zone::Prompt
            && self.cold.screen == crate::term::Screen::Normal;
        if opens {
            self.strs.prompts_opened = self.strs.prompts_opened.next();
        }
        self.strs.shell.zone = crate::term::Zone::Prompt;
    }

    /// A later line of the block: `k=s`, `k=c` or `k=r`.
    fn later_prompt(&mut self, kind: PromptKind) {
        self.prompt_zone();
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        let shell = &mut self.strs.shell;
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

    /// **The prompt walks down to the window's last row**, under
    /// [`PromptAtBottom::On`]: the rows from `from` to the cursor's move
    /// down over the blank rows directly below the cursor, which open above
    /// them, so the prompt lands with its last line on the last row and
    /// nothing above it moves -- the page zsh4humans' shell-side
    /// `prompt-at-bottom` leaves, output where it was written and the blank
    /// rows between it and the prompt. A move of `IL` at `from` in the band
    /// down to the last blank row, with no cell holding text pushed off it,
    /// and the selection, the images, the block start, the input and the
    /// prompt log following the rows as they follow a region scroll.
    ///
    /// `from` is the block's first row, and the walk is taken once the
    /// prompt is drawn ([`Paint::pin_prompt_block`]). **Only where a mark
    /// said a prompt is**, and only on the primary with no scrolling region
    /// or side margins: a program that set either has taken the layout
    /// over, as a full-screen one has, and the alternate screen is never
    /// touched.
    fn pin_prompt(&mut self, from: Row) {
        if self.strs.prompt_at_bottom != PromptAtBottom::On
            || self.cold.screen != crate::term::Screen::Normal
            || self.cold.top != Row::ZERO
            || self.cold.bot.index() + 1 != self.rows.index()
            || self.fenced()
            || self.on_status()
        {
            return;
        }
        let (cy, rows) = (self.cy.index(), self.rows.index());
        let k = (cy + 1..rows)
            .take_while(|&y| self.view.row_is_blank(Row::from_index(y)))
            .count();
        if k == 0 || from.index() > cy {
            return;
        }
        let (n, by) = (Count::of(k), Rows::of_count(k));
        let bot = Row::from_index(cy + k);
        self.cold
            .band_moved((from, bot), by, crate::ops::Scroll::Down, self.book.top);
        if self.past_edge() {
            self.clear_swept();
        }
        self.view
            .insert_lines_in(from, n, bot, PackedCell::empty_with(StyleId::DEFAULT));
        if self.past_edge() {
            self.lines_moved_past_edge(from, n, bot, BandMove::Down);
        }
        if self.images.any_placed() {
            self.band_images_moved(from, bot, by, BandMove::Down);
        }
        if self.cold.guard_open.is_some() {
            self.guard_band_moved(from, bot, by, BandMove::Down);
        }
        if self.strs.print.areas.held() {
            self.areas_rows_moved(from, bot, by, BandMove::Down);
        }
        if self.cold.placement.is_placed() {
            self.placement_band_moved(from, bot, by, BandMove::Down);
        }
        self.cells = self
            .cells
            .plus((bot.index() + 1 - from.index()) * self.cols.index());
        // The marks' rows in the band go down with it.
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        let moved = |r: RowRot| {
            let y = r.retreat(rot.index(), ring).index();
            match (from.index()..=cy).contains(&y) {
                true => r.advance(k, ring),
                false => r,
            }
        };
        let shell = &mut self.strs.shell;
        shell.block = PromptBlock::at(moved(shell.block.start()));
        if let Input::At { row, col, end } = shell.input {
            shell.input = Input::At {
                row: moved(row),
                col,
                end,
            };
        }
        if let Later::At(r) = shell.secondary {
            shell.secondary = Later::At(moved(r));
        }
        let top = self.book.top;
        self.strs.prompts.moved_down(
            top.plus(Rows::of_count(from.index())),
            top.plus(Rows::of_count(bot.index())),
            by,
        );
        self.set_row(Row::from_index(cy + k));
    }

    /// **The drawn prompt walks down**, from its block's first row to the
    /// cursor's: at the input mark, `B` or `I`, and at the setter. Not at
    /// the initial mark, before the shell has drawn the prompt: there the
    /// walk would put the prompt's first line on the last row, and every
    /// later line of a taller prompt would scroll a row of output into the
    /// history, at every redraw again. A block whose first row is not on
    /// the screen is left where it is.
    pub(crate) fn pin_prompt_block(&mut self) {
        let start = self.strs.shell.block.start();
        let (rot, ring) = (self.view.rotation(), self.view.ring());
        let y = start.retreat(rot.index(), ring).index();
        if y <= self.cy.index() {
            self.pin_prompt(Row::from_index(y));
        }
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
            // A selected area with an end on a row given up is below the
            // screen, and the line feed that brings the row back erases it.
            if self.strs.print.areas.held() {
                self.areas_scrolled_down();
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
