//! Operating System Commands: titles, the palette, hyperlinks, the clipboard.
//!
//! **The parsing is `baud_domain::Osc`.** What is here is the wiring --
//! accumulate a body, decide what it was, write the result somewhere the
//! embedder can read -- and that is the half this core has to get right on its
//! own. A core that recognised `OSC 8` and never set the link index would open
//! every URI it was sent and link no cell, and no test of the parser could see
//! it.
//!
//! **The two pinned differentials were structurally blind to all of this.**
//! The glyph differential read content and the rendition one read the
//! resolved `StyleWord`; a window title changes no cell and a link index rides
//! in `CellFlags`, which neither read. So the gate for this file was the
//! control-string differential against `baud-core2`, which compared the title,
//! the icon, the palette, the link index of a written cell, the URI it
//! resolves to, and the two refusal counters -- and it was perturbed both ways
//! before it was believed. All three went with that crate (bcefaae). The
//! golden capture that replaced them keeps cells, the cursor and the reply,
//! and no title, icon, palette or link, so what holds this file now is chiefly the
//! hand-written suites, `tests/xterm/osc.rs` and `tests/xterm/hyperlinks.rs`.

use crate::paint::Paint;
use crate::storage::{LINK_MAX, NLINK, OSC_MAX, StyleLog};
use crate::term::Cold;
use baud_domain::*;

impl Paint<'_, '_> {
    /// Begin a control string: record which one, and empty the accumulator.
    ///
    /// **Reset here, at the introducer, and not at the terminator.** A string
    /// spans reads, so the accumulator outlives a `feed`; and a string that
    /// `CAN` or `SUB` cancels never reaches a terminator at all. Resetting
    /// where the string starts is one rule that covers both, and the
    /// alternative -- reset on completion, plus a second rule for cancellation
    /// -- is the shape that made `pre: Option<Classified>` break chunk
    /// invariance: a value carried between two functions and consumed by a
    /// later, different one, because the path has more exits than it looks
    /// like it has.
    ///
    /// **And the encoding it began under**, which `str_put` reads to keep an
    /// `OSC`'s or a Sixel's GR byte as its 7-bit byte under ISO 8859-1: taken once here,
    /// in the introducer's arm, rather than passed in the body's arm on
    /// every byte of every string, a Sixel's included.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_start(&mut self, b: StreamByte, enc: InputEncoding) {
        self.cold.str_kind = StrKind::of(b);
        self.cold.str_enc = enc;
        self.cold.str_len = ByteLen::ZERO;
    }

    /// One body byte, truncating rather than growing.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_put(&mut self, b: StreamByte) {
        // A Sixel body is far larger than `OSC_MAX` and goes to its own buffer.
        // The length rises past the buffer so an overflow is detectable at the
        // terminator (`sixel_len > sixel_body.len()`) and the image is refused
        // whole rather than decoded from a truncated body.
        if self.cold.str_kind == StrKind::Sixel {
            // **A C0 is dropped here, before the decoder.** The graph puts a DCS
            // body's `BEL` and its format effectors `08-0D`,
            // `[[source: williams]]`'s `dcs passthrough` row (388), and the
            // decoder reads a number up to its first non-digit, so any of them
            // inside one split it: `!1 BEL 0~` drew one column where `!10~` draws
            // ten, and `#1;2;10 BEL 0;0;0` set red to 10%. DEC's own Sixel text
            // gives the format effectors no meaning: `[[source: dec-std-070]]`
            // 070-9 §9.1, "These codes shall have no effect on the interpretation
            // of Sixel Control strings" (PDF p. 925). `[[source: vt3xx-sixel]]`
            // says nothing of C0. xterm skips `HT LF VT FF CR` as white space and
            // ends a number on `BS`, its departure
            // (`[[source: xterm-graphics-sixel]]` 541-555), and never sees a
            // `BEL`: its Sixel parser is handed only bytes its string table calls
            // `CASE_IGNORE` (`[[source: xterm-reset]]`'s file, 3408-3412), and
            // `BEL` is `CASE_BELL` (`[[source: xterm-vtprstbl-c]]` 7630). The
            // other C0 never get here, the graph ignoring them, and neither does
            // `DEL`, which the graph ignores in a DCS body as williams' row does
            // and as 070-9 §9.2 lists for Sixel, "7/15 (DELETE) Ignored" (926).
            // The `DEL` was stored until 2026-10-10 and this arm dropped it; xterm
            // hands its parser one, which ends a number there, its departure.
            //
            // **Under ISO 8859-1 a GR byte is its 7-bit byte, and a `SPACE` is
            // dropped with the C0** (`Sixel::body_byte`, which quotes 070-9
            // §8.1, §9.2 and §9.4). The body stored GR raw until 2026-10-13,
            // and the decoder skipped it, so `FE FE` drew nothing; and a
            // `SPACE` reached the decoder, which ended a number at it, so `!1
            // SPACE 0~` drew one column where `!10~` draws ten. xterm skips
            // `isspace` bytes before it reads a digit
            // (`[[source: xterm-graphics-sixel]]` 541-547), as this does. The
            // encoding is the one the introducer began under (`str_start`).
            //
            // `21-7F` is tested here and the rest out of line, in
            // `sixel_put_rare`, so the common byte costs what the `is_c0` this
            // replaced cost (`Sixel::body_byte_is_gl_past_space`); the `DEL`
            // in that range never arrives, the graph ignoring it.
            if !Sixel::body_byte_is_gl_past_space(b) {
                return self.sixel_put_rare(b);
            }
            return self.sixel_put(b);
        }
        // **A DECRQSS drops a format effector, `08-0D`, before its `Pt` is
        // kept.** The graph puts them in a DCS body for the handler to judge
        // (`graph::string`), and this one gives them no meaning:
        // `[[source: dec-std-070]]` 3.5.4 says their inclusion "is intended
        // for convenience in formatting and storing control strings" and
        // recommends "that these characters not affect the interpretation of
        // the control string" (PDF p. 133), and of DEC's practice
        // `[[source: williams]]` says a C0 with no meaning to the device
        // function is "silently ignored" (818-822). So `m CR` names SGR, as
        // it did while the graph dropped the byte, and the answer is
        // unchanged. xterm's `strcmp` against `m` fails on it
        // (`[[source: xterm-misc]]`'s file, 4935), practice and not followed;
        // `[[source: ctlseqs]]` leaves error recovery to the implementation
        // (133-137). The other C0 the graph puts, `BEL`, stays in the `Pt`,
        // and the request then names nothing, which is the owner's open call.
        if self.cold.str_kind == StrKind::Decrqss && b.is_format_effector() {
            return;
        }
        // **Under ISO 8859-1 an `OSC` keeps a GR byte as its 7-bit byte**,
        // its clipboard body included. `[[source: dec-std-070]]` 3.5.4.5: "GR
        // (8-bit) graphic characters in APC, OSC, and PM control strings will
        // be treated as their 7-bit equivalent (the eighth bit will be
        // ignored)"; `[[source: ecma48]]` §9 gives "the contents of the
        // command string" "the same meanings" as `02/00` to `07/14`; and
        // `[[source: williams]]` treats GR as GL "in all states" (462). The
        // body kept the byte as it came, so `ESC ] B0 BB t BEL` named no
        // `OSC`, a title in GR was stored in GR, and `B5 B2 BB` was not the
        // `52;` that routes a copy below. Above this, a Sixel folds its own,
        // as 070-9 §9.4 says, and a DECRQSS keeps its `Pt`'s, the owner's
        // open call (`dcs_passthrough`, `docs/vt-coverage.md`). Below it, `Other` is a
        // DCS no handler reads or an `APC`, `PM` or `SOS`, kept and dropped
        // unread at the terminator, so a strip there would move nothing; an
        // `APC` or `PM` that gains a reader takes the rule then. Under UTF-8
        // `gr_as_gl` maps nothing: the bytes are a character's.
        let b = match self.cold.str_kind {
            StrKind::Osc | StrKind::Clipboard => self.cold.str_enc.gr_as_gl(b),
            _ => b,
        };
        // An `OSC 52` body past its `52;`: decoded into the clipboard areas,
        // not kept in `pending`, whose 512 bytes hold under 400 of text.
        if self.cold.str_kind == StrKind::Clipboard {
            return self.clip_put(b);
        }
        let n = self.cold.str_len.index();
        if n < OSC_MAX {
            self.strs.pending[n] = b.raw_byte();
            self.cold.str_len = ByteLen::from_index(n + 1);
            // **`52;` routes the rest of the body to the clipboard**, whose
            // `Pd` is unbounded: kept here, a copy past 380-odd octets would
            // be cut at `OSC_MAX`. Matched on the spelling `Osc::parse`
            // matches, so no body it would call `52` stays behind.
            if n == 2 && self.cold.str_kind == StrKind::Osc && self.strs.pending[..3] == *b"52;" {
                self.clip_start();
            }
        } else {
            self.string_truncations += 1;
        }
    }

    /// Store one Sixel body byte the handler keeps. The length rises past
    /// the buffer, so an overflow is seen at the terminator.
    #[inline]
    fn sixel_put(&mut self, b: StreamByte) {
        let n = self.cold.sixel_len.index();
        if n < self.sixel_body.len() {
            self.sixel_body[n] = b.raw_byte();
        }
        self.cold.sixel_len = ReadLen::from_index(n + 1);
    }

    /// A Sixel body byte outside `21-7E`: kept, folded or dropped as
    /// `Sixel::body_byte` says, under the encoding the introducer began
    /// under.
    #[cold]
    #[inline(never)]
    fn sixel_put_rare(&mut self, b: StreamByte) {
        if let Some(b) = Sixel::body_byte(self.cold.str_enc, b) {
            self.sixel_put(b);
        }
    }

    /// The string is complete. Act on it if it is one this core acts on.
    ///
    /// Takes the terminating byte, because `[[source: ctlseqs]]` says a reply
    /// "uses the same terminator used in a query" -- so `BEL` and `ST` are one
    /// action with two shapes rather than two actions, and which one arrived
    /// has to travel with it.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_end(&mut self, b: StreamByte) {
        let term = match b.raw_byte() {
            0x07 => StringTerm::Bel,
            _ => StringTerm::St,
        };
        match self.cold.str_kind {
            StrKind::Osc => self.dispatch_osc(term),
            StrKind::Sixel => self.sixel_end(),
            StrKind::Clipboard => self.clip_end(),
            StrKind::Decrqss => {
                let n = self.cold.str_len.index();
                let target = StatusTarget::parse(&self.strs.pending[..n]);
                self.report_status(target);
            }
            // `SOS`, `PM` and `APC` have no defined content here, and `DCS`
            // has no prefix parser yet. **Consumed is the part that matters**:
            // a body that reached the screen as text would render an
            // application's private protocol as glyphs.
            StrKind::Other => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
        self.cold.str_len = ByteLen::ZERO;
        self.cold.sixel_len = ReadLen::ZERO;
    }

    /// A palette entry written, and the colour epoch moved when it changed:
    /// a renderer resolves the table again under a new epoch, and a write
    /// that leaves the entry as it was must not make it.
    fn set_palette(&mut self, i: Chan, w: ColorWord) {
        if self.strs.palette[i.index()] != w {
            self.strs.palette[i.index()] = w;
            self.style_log.colors_moved();
        }
    }

    /// Every `OSC 4` override cleared, which is every entry a rendition
    /// resolves through: `OSC 104` with no parameter, and both resets, as
    /// xterm's `ReallyReset` calls `ResetAnsiColorRequest` in its common path
    /// (`[[source: xterm-reset]]`). The dynamic colours are not touched --
    /// neither xterm path resets them. Through [`Paint::set_palette`], so the
    /// colour epoch moves only if some entry was overridden.
    pub(crate) fn reset_palette(&mut self) {
        clear_palette(self.strs.palette, self.style_log);
    }

    /// The same for a dynamic colour. `OSC 12`'s cursor colour is read where
    /// the cursor is drawn and no rendition resolves through it, so it moves
    /// no epoch.
    fn set_dynamic(&mut self, which: DynamicColor, w: ColorWord) {
        if self.cold.dynamic[which.index()] != w {
            self.cold.dynamic[which.index()] = w;
            if which != DynamicColor::Cursor {
                self.style_log.colors_moved();
            }
        }
    }

    fn dispatch_osc(&mut self, term: StringTerm) {
        let n = self.cold.str_len.index();
        // Split the borrow: `Osc::parse` holds the payload while the setters
        // want the other buffers. Every arm resolves to owned values first.
        match Osc::parse(&self.strs.pending[..n]) {
            // **No trailing-NUL trim, because a NUL cannot get here.**
            // `baud-core2` carried a `clip` helper for emitters that put a NUL
            // before the real terminator, and this core had one too until the
            // parser started ignoring C0 in the payload: `graph::string` drops
            // `00-06` outright, so there is no trailing NUL left to trim.
            // Neutering the helper changed no row of the differential, which
            // is a function that cannot fire -- the shape this repo has now
            // recorded seven times.
            Osc::SetTitle(t) => self.title_len = copy_bounded(self.strs.title, t),
            Osc::SetIcon(t) => self.icon_len = copy_bounded(self.strs.icon, t),
            Osc::SetIconAndTitle(t) => {
                self.title_len = copy_bounded(self.strs.title, t);
                self.icon_len = copy_bounded(self.strs.icon, t);
            }
            Osc::SetColor { index, spec } => match spec {
                XColor::Rgb(r, g, b) => self.set_palette(index, ColorWord::rgb(r, g, b)),
                XColor::Query => self.report_color(index, term),
                // A colour *name*. Resolving it needs X11's `rgb.txt` and this
                // repo has no citable copy, so it is left alone rather than
                // set to something plausible-but-wrong -- which is the same
                // reason the DEC line-drawing table was refused until a
                // machine-readable source was cited. **Counted**, because a
                // spec this core cannot resolve is a request it did not
                // honour, and `every_spec_osc_code_is_handled_or_counted`
                // probes with exactly such a name.
                XColor::Unresolved(_) => self.unsupported_total += 1,
            },
            Osc::ResetColor(Some(c)) => self.set_palette(c, ColorWord::DEFAULT),
            Osc::ResetColor(None) => self.reset_palette(),
            // **Under `?5` each names the colour shown in its role** (note V,
            // V17): every cell is drawn exchanged, so a default cell shows the
            // stored background as its text and the stored foreground as its
            // page, and the role maps to the other slot through the [`Video`]
            // the renderer draws by -- where xterm's `OSC 10`-family handlers
            // land, on its swapped tables (`[[source: xterm-misc]]` misc.c
            // 3568-3750). A query keeps the code the program sent, as xterm's
            // replies do.
            Osc::SetDynamic { which, spec } => match spec {
                XColor::Rgb(r, g, b) => {
                    let slot = self.cold.video().role(which);
                    self.set_dynamic(slot, ColorWord::rgb(r, g, b));
                }
                XColor::Query => self.report_dynamic(which, term),
                // Counted, as `OSC 4`'s unresolvable name is.
                XColor::Unresolved(_) => self.unsupported_total += 1,
            },
            Osc::ResetDynamic(which) => {
                let slot = self.cold.video().role(which);
                self.set_dynamic(slot, ColorWord::DEFAULT);
            }
            // **Inline, because the URI borrows the payload buffer.** Both
            // live in `strs`, so resolving the slot and the length first is
            // what lets the write happen at all -- the same borrow split
            // `baud-core2` made here for the same reason.
            //
            // **A slot is never overwritten and a clipped URI is never
            // interned**, because a click opens what the slot holds. The
            // table used to evict on collision like the style table, and
            // that is wrong for a link: a later `OSC 8` from *any* program
            // -- an empty run that writes no cell -- landed in the slot an
            // honest link already on screen resolves through, and the screen
            // did not change while its target did. The style table's
            // degradation is a wrong colour; this one is a wrong destination.
            Osc::Hyperlink { id, uri } => {
                let len = uri.len();
                if len == 0 {
                    self.link = LinkId::NONE;
                } else if len > LINK_MAX || n >= OSC_MAX {
                    // A link whose URI may be cut is not a link. The body is
                    // clipped at `OSC_MAX` *before* the URI is split out of
                    // it, so a long `id=` leaves a URI under `LINK_MAX` that
                    // still ends mid-path; and a URI past `LINK_MAX` used to
                    // be clipped here in silence. Either is a different URL
                    // from the one sent. Refused and counted; the run renders
                    // as text.
                    self.link = LinkId::NONE;
                    self.links_clipped += 1;
                } else {
                    let slot = LinkId::bucket(id, uri, NLINK);
                    let at = slot.index() * LINK_MAX;
                    let have = self.strs.link_len[slot.index()].index();
                    if have == 0 {
                        let mut tmp = [0u8; LINK_MAX];
                        tmp[..len].copy_from_slice(uri);
                        self.strs.links[at..at + len].copy_from_slice(&tmp[..len]);
                        self.strs.link_len[slot.index()] = ByteLen::from_index(len);
                        self.link = slot;
                    } else if self.strs.links[at..at + have] == *uri {
                        // The same URI is already here -- resent, or a second
                        // `id` hashing to the same slot -- and one slot for
                        // one target is exactly right.
                        self.link = slot;
                    } else {
                        // Occupied by a different URI: refused, and the cells
                        // that follow are plain text. The `ClusterStore` rule
                        // -- a wrong cluster is not a wrong colour -- and it
                        // bounds the table at 127 distinct targets for the
                        // storage's life, which `links_refused` says when it
                        // is reached. Freed only by `RIS`, which erases every
                        // cell that could name a slot first: freed while one
                        // did, a slot taken by a new link would give that cell
                        // the new target.
                        self.link = LinkId::NONE;
                        self.links_refused += 1;
                    }
                }
                self.refresh_flags();
            }
            // Both counters: `strings_ignored` is what an embedder reads to
            // find OSC codes it might want to implement, and
            // `unsupported_total` is the one the conformance inventory asks.
            // They answer different questions and the second was missing.
            // What the marks are acted on for: the prompt block, tracked so
            // a redraw replaces exactly it (`prompt.rs`), and the zone,
            // which `Term::resize` reads to leave the block's rows un-cut.
            Osc::Semantic(mark) => {
                // Only an initial line begins a block: `k=s`, `k=c` and `k=r`
                // are later lines of the same one. A mark in the prompt zone
                // is the shell redrawing the block it has.
                if matches!(mark, Semantic::PromptStart(PromptKind::Initial)) {
                    if self.cold.shell.zone == crate::term::Zone::Prompt {
                        self.replace_prompt_block();
                    }
                    self.open_prompt_block();
                }
                self.cold.shell.zone = match mark {
                    Semantic::PromptStart(_) | Semantic::CommandStart => crate::term::Zone::Prompt,
                    // `D` closes it as `C` does: Enter on an empty line sends
                    // no `C`, and the `P` after `D` is a fresh prompt, not a
                    // redraw -- and so is anything a `precmd` prints first.
                    Semantic::OutputStart | Semantic::CommandDone => crate::term::Zone::Output,
                    Semantic::Other => self.cold.shell.zone,
                };
                self.semantic_marks += 1;
            }
            Osc::Unsupported => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
    }

    /// Answer `OSC 4 ; c ; ?` in the same shape as the setter.
    ///
    /// `[[source: ctlseqs]]`: xterm "replies with a control sequence of the
    /// same form which can be used to set the corresponding color".
    ///
    /// **Every entry is answered**, as xterm's `ReportAnsiColorRequest` does:
    /// the application's override if it set one, else the embedder's table
    /// ([`crate::Term::set_default_palette`]), else nothing. The last is the
    /// truthful answer of a core no embedder told -- naming a colour the
    /// renderer does not use would be the wrong-shaped answer
    /// `docs/vt-coverage.md` forbids. The table is a second copy of the
    /// renderer's, kept in step by the embedder's one call beside
    /// [`crate::Term::set_default_colors`].
    fn report_color(&mut self, index: Chan, term: StringTerm) {
        let own = self.strs.palette[index.index()];
        let word = if own.is_default() {
            self.strs.palette[crate::storage::EMBEDDER_PALETTE + index.index()]
        } else {
            own
        };
        let Some((r, g, b)) = word.as_rgb() else {
            return;
        };
        self.want_reply(Report::Color {
            index,
            r,
            g,
            b,
            term,
        });
    }
}

impl Paint<'_, '_> {
    /// Answer `OSC 10 ; ?` / `OSC 11 ; ?`: the application's override if it
    /// set one, else the embedder's default if it supplied one, else nothing
    /// -- the `OSC 4` refusal, for the same reason. Of the colour shown in
    /// the role asked, under the code asked (`Cold::shown`): xterm's
    /// `ReportColorRequest` reports "the original color index, but reporting
    /// the opposite color which would be used" (`[[source: xterm-misc]]`
    /// 3576-3580).
    fn report_dynamic(&mut self, which: DynamicColor, term: StringTerm) {
        let Some((r, g, b)) = self.cold.shown(which).as_rgb() else {
            return;
        };
        self.want_reply(Report::DynamicColor {
            which,
            r,
            g,
            b,
            term,
        });
    }
}

/// Every `OSC 4` override cleared -- the first [`EMBEDDER_PALETTE`] entries
/// of the palette slice -- with the colour epoch moved once if any entry
/// changed. The one spelling of the clear, for `OSC 104`, both resets and an
/// embedder's [`Overrides::Drop`](crate::Overrides::Drop).
///
/// [`EMBEDDER_PALETTE`]: crate::storage::EMBEDDER_PALETTE
pub(crate) fn clear_palette(palette: &mut [ColorWord], log: &mut StyleLog<'_>) {
    let mut moved = false;
    for w in &mut palette[..crate::storage::EMBEDDER_PALETTE] {
        if !w.is_default() {
            *w = ColorWord::DEFAULT;
            moved = true;
        }
    }
    if moved {
        log.colors_moved();
    }
}

/// Every `OSC 10`/`11`/`12` override cleared, with the colour epoch moved
/// when the foreground or background changed -- the cursor colour moves none,
/// as in `set_dynamic`.
pub(crate) fn clear_dynamic(cold: &mut Cold, log: &mut StyleLog<'_>) {
    for which in [
        DynamicColor::Foreground,
        DynamicColor::Background,
        DynamicColor::Cursor,
    ] {
        let w = &mut cold.dynamic[which.index()];
        if !w.is_default() {
            *w = ColorWord::DEFAULT;
            if which != DynamicColor::Cursor {
                log.colors_moved();
            }
        }
    }
}

fn copy_bounded(dst: &mut [u8], src: &[u8]) -> ByteLen {
    let n = src.len().min(OSC_MAX);
    dst[..n].copy_from_slice(&src[..n]);
    ByteLen::from_index(n)
}

/// Which control string is being accumulated.
///
/// Five states for five introducers. `SOS`, `PM` and `APC` share `Other`,
/// with any `DCS` this core has no prefix parser for, because it acts on none
/// of them and their bodies are consumed identically. `Decrqss` and `Sixel`
/// are separate because each is reached through a **prefix** -- `$ q`, or a
/// bare `q`, after the introducer and any parameters -- rather than through
/// the introducer alone, so `StrKind::of` cannot name them; `Act::DcsQuery`
/// and `Act::SixelStart` set them when the prefix arrives. `Clipboard` is the
/// same again inside an OSC body, set by `str_put` at `52;`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StrKind {
    Osc,
    /// A `DCS $ q`, the status request `report_status` answers.
    Decrqss,
    /// A `DCS Pa;Pb;Ph q` Sixel: the body is raster data, accumulated into
    /// `sixel_body` and decoded at the terminator. Reached through the bare
    /// `q` prefix, the `Decrqss` shape one prefix over.
    Sixel,
    /// An `OSC 52` past its `52;`: the body goes to the clipboard areas,
    /// decoded as it arrives (`clip.rs`). Reached through the body's own
    /// first bytes, which `StrKind::of` cannot see either.
    Clipboard,
    Other,
}

impl StrKind {
    pub(crate) const fn of(b: StreamByte) -> StrKind {
        match b.raw_byte() {
            // **Both spellings of the same introducer.** `ESC ]` is the
            // seven-bit form and `9D` the eight-bit one, and the graph reaches
            // the same node from either -- a Latin-1 `9D` is routed as
            // `escape(9D - 0x40)`, which is `]`. But this reads the byte back
            // out of the arena to name the kind, so it saw `9D` and answered
            // `Other`: the string parsed, its body was consumed, and the title
            // was silently dropped.
            //
            // The graph's delegation is what makes one row serve both, and
            // this is the one place downstream that still had to be told.
            //
            // **And `DD`, the same `]` in GR under ISO 8859-1**, where the
            // escape state reads a GR byte as its 7-bit byte (`graph::one`,
            // `[[source: williams]]` 462): `ESC DD` is `ESC ]`, and this read
            // `Other` for it, consuming the string and dropping its title.
            // Under UTF-8 the graph names `StrStart` for no `DD`.
            0x5d | 0x9d | 0xdd => StrKind::Osc,
            _ => StrKind::Other,
        }
    }
}
