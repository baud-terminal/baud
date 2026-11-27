//! **Find in the scrollback, for both drivers**: the keys that open it and
//! step it, what a key does while the field is open, and what the window
//! and the selection do when the current match moves. The matches and the
//! field are `baud_render`'s [`Find`]; the core reads the ring.
//!
//! The keys are the HIG's (`[[source: apple-hig-keyboards]]`): Command-F
//! "Open a Find window", Command-G "Find the next occurrence of the
//! selection", Shift-Command-G the previous, Command-E "Use the selection
//! for a find operation", and Esc "Cancel the current action or process".
//! The Linux driver opens it with Ctrl+Shift+F, Ghostty's and Alacritty's
//! binding (`[[source: ghostty-config-521cf2c]]`, `[[source:
//! alacritty-bindings-5421470]]`); foot's is Ctrl+Shift+R, its reverse
//! search (`[[source: foot-ini-cb27717]]`). In the field, Enter is Find
//! Next and Shift+Enter Find Previous, as Alacritty and iTerm2 bind them
//! (`[[source: alacritty-bindings-5421470]]`, `[[source:
//! iterm2-finddriver-654bb13]]`).
//!
//! **The current match is the selection**, as a macOS find selects what it
//! found, so Command-C (or Ctrl+Shift+C) copies it; every other match is
//! washed in yellow (`Renderer::set_find`).
//!
//! **The last search is the find pasteboard's** on macOS, "The pasteboard
//! that holds information about the current state of the active
//! application's find panel" (`[[source: apple-nspasteboard-name-find]]`):
//! the query goes there as it changes, and Command-F, Command-G or
//! Shift-Command-G with the field closed reopen it on that string,
//! selected so a key typed replaces it, and step from the match current
//! when it closed (`Finding::reopen`), as iTerm2's Find Next with no field
//! open searches it (`[[source: iterm2-finddriver-654bb13]]` 589-594,
//! 649-668). **An input method composes in the field** while it is open,
//! at its caret ([`Finding::look_typing`]), and its candidate window opens
//! there.

use baud_core::{Found, Term};
use baud_domain::{Codepoint, KeyCode, Modifiers, Rows};
use baud_render::{
    Caret, Field, Find, FindLook, Grain, Hints, Point, Query, Scrolled, Selection, Span, Typing,
};

/// What a chord does to the find field.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Chord {
    /// Open the field, or keep it open.
    Open,
    /// Open it on the selection's text.
    UseSelection,
    Next,
    Previous,
}

/// **A Command chord's find action on macOS**, by what the key types with
/// Command held (`NSEvent.characters`), as the copy the menu missed is
/// found (`menu_missed`): Command-F, Command-E, Command-G, and with Shift
/// Command-G the previous. Option or Control held makes another command.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn command_chord(characters: &str, shift: Shift) -> Option<Chord> {
    match (characters, shift) {
        ("f", Shift::Up) => Some(Chord::Open),
        ("e", Shift::Up) => Some(Chord::UseSelection),
        ("g", Shift::Up) => Some(Chord::Next),
        ("g" | "G", Shift::Down) => Some(Chord::Previous),
        _ => None,
    }
}

/// Whether Shift is held with a chord.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shift {
    Up,
    Down,
}

/// What a key does while the field is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FieldKey {
    Close,
    Next,
    Previous,
    /// Delete the query's last character, its marks with it.
    Delete,
    /// Add a character the key typed.
    Type(Codepoint),
    /// Nothing: the field keeps every other key from the program.
    Nothing,
}

/// **A key in the open field**, either driver's: Escape closes it, Enter
/// is Find Next and Shift+Enter Find Previous, Backspace deletes, and a
/// key that types a character with neither Control nor Alt adds it.
pub(crate) fn field_key(code: KeyCode, mods: Modifiers, text: Option<Codepoint>) -> FieldKey {
    let chord = mods.has(Modifiers::CTRL) || mods.has(Modifiers::ALT);
    match code {
        KeyCode::Escape => FieldKey::Close,
        KeyCode::Enter if mods.has(Modifiers::SHIFT) => FieldKey::Previous,
        KeyCode::Enter => FieldKey::Next,
        KeyCode::Backspace => FieldKey::Delete,
        _ => match (text, chord) {
            (Some(c), false) => FieldKey::Type(c),
            _ => FieldKey::Nothing,
        },
    }
}

/// **The open field**: the find, and whether its query is selected --
/// closing it drops both, and leaves [`Left`].
#[derive(Clone, Debug)]
pub(crate) struct Finding {
    find: Find,
    query: Query,
}

/// **What a closed field leaves**: its query and the match current when
/// it closed, which a field reopened on the same query starts from.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug)]
pub(crate) struct Left {
    query: Vec<Codepoint>,
    current: Option<Found>,
}

impl Finding {
    /// The field opened on `query`, the ring read for it.
    pub(crate) fn open(term: &Term<'_>, query: Vec<Codepoint>) -> Finding {
        let mut find = Find::new();
        find.set_query(term, query, term.window_back());
        Finding {
            find,
            query: Query::Typed,
        }
    }

    /// **The field reopened on `query`**, the find pasteboard's string --
    /// "The pasteboard that holds information about the current state of
    /// the active application's find panel" (`[[source:
    /// apple-nspasteboard-name-find]]`) -- selected, so the next key typed
    /// replaces it; and `chord` stepped from the match current when it
    /// closed, where `left` closed on the same query and the match is
    /// still there. Otherwise the search is a new one, and its current
    /// match the newest the reader can have seen, as a typed query's is,
    /// unstepped.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn reopen(
        term: &Term<'_>,
        query: Vec<Codepoint>,
        left: Option<&Left>,
        chord: Chord,
    ) -> Finding {
        let mut f = Finding::open(term, query);
        if !f.find.query().is_empty() {
            f.query = Query::Selected;
        }
        let resumed = left
            .filter(|l| l.query == f.find.query())
            .and_then(|l| l.current)
            .is_some_and(|m| f.find.select(m));
        if resumed {
            f.chord(chord);
        }
        f
    }

    /// What the field leaves when it closes.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn left(&self) -> Left {
        Left {
            query: self.find.query().to_vec(),
            current: self.find.current(),
        }
    }

    pub(crate) fn find(&self) -> &Find {
        &self.find
    }

    /// **A key in the field**: the query edited and read again, or the
    /// current match stepped; `Closed` when it closed.
    pub(crate) fn key(&mut self, term: &Term<'_>, key: FieldKey) -> Open {
        let mut query = self.edited();
        match key {
            FieldKey::Close => return Open::Closed,
            FieldKey::Next => self.find.next(),
            FieldKey::Previous => self.find.previous(),
            // A selected query is deleted whole.
            FieldKey::Delete if self.query == Query::Selected => {
                self.query = Query::Typed;
                self.find.set_query(term, Vec::new(), term.window_back());
            }
            FieldKey::Delete => {
                // Back over the marks, then the character they follow.
                while query.last().is_some_and(|&c| zero_width(c)) {
                    query.pop();
                }
                query.pop();
                self.find.set_query(term, query, term.window_back());
            }
            FieldKey::Type(c) => {
                self.query = Query::Typed;
                query.push(c);
                self.find.set_query(term, query, term.window_back());
            }
            FieldKey::Nothing => {}
        }
        Open::Open
    }

    /// The query a key typed adds to: none while it is selected.
    fn edited(&self) -> Vec<Codepoint> {
        match self.query {
            Query::Selected => Vec::new(),
            Query::Typed => self.find.query().to_vec(),
        }
    }

    /// Characters a driver's input method committed, or a paste, added to
    /// the query -- in place of it while it is selected.
    pub(crate) fn typed(&mut self, term: &Term<'_>, text: &[Codepoint]) {
        let mut query = self.edited();
        self.query = Query::Typed;
        query.extend_from_slice(text);
        self.find.set_query(term, query, term.window_back());
    }

    /// A chord: Find Next or Previous, or for Open nothing more.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn chord(&mut self, chord: Chord) {
        match chord {
            Chord::Next => self.find.next(),
            Chord::Previous => self.find.previous(),
            Chord::Open | Chord::UseSelection => {}
        }
    }

    /// **After a read**: the matches brought up to date; whether the
    /// current match moved, so a driver selects it again.
    pub(crate) fn refresh(&mut self, term: &Term<'_>) -> Moved {
        let was = self.find.current();
        self.find.refresh(term);
        match self.find.current() == was {
            true => Moved::No,
            false => Moved::Yes,
        }
    }

    /// **After a resize**: the matches read again on the re-laid grid,
    /// and the current match's selection, which a driver takes before its
    /// next frame. A new width re-cuts every line and a match's columns
    /// with it, and no read may come to bring them up to date.
    pub(crate) fn after_resize(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
    ) -> Option<Selection> {
        self.find.refresh(term);
        selection_of(term, scrolled, self.find.current()?)
    }

    /// **The window and the selection for the current match**: the window
    /// moved over the history until the match shows, and the match
    /// selected, so a copy takes it.
    pub(crate) fn show(&self, term: &mut Term<'_>, scrolled: Scrolled) -> Option<Selection> {
        if let Some(back) = self.find.window_for(term, term.window_back()) {
            if back != term.window_back() {
                term.set_window_back(back);
            }
        }
        selection_of(term, scrolled, self.find.current()?)
    }

    /// What the frame draws of it, on the window `back` lines up.
    #[cfg(test)]
    pub(crate) fn look(&self, term: &Term<'_>, back: Rows) -> FindLook {
        self.look_typing(term, back, &[], &[], Caret::End)
    }

    /// **What the frame draws of it with an input method composing**:
    /// `composing` drawn in the field after the query, in its `hints`
    /// (`marked::hints`) over the field's colours, with its cursor at
    /// `caret` (`Find::field_typing`), and nothing of it at the terminal's
    /// cursor -- the keys are the field's while it is open.
    pub(crate) fn look_typing(
        &self,
        term: &Term<'_>,
        back: Rows,
        composing: &[Codepoint],
        hints: &[Hints],
        caret: Caret,
    ) -> FindLook {
        FindLook {
            marks: self.find.marks(term, back),
            field: self.field(term, composing, hints, caret),
        }
    }

    /// **The field alone**: where a driver asks the input method's
    /// candidate window to open, at its caret.
    pub(crate) fn field(
        &self,
        term: &Term<'_>,
        composing: &[Codepoint],
        hints: &[Hints],
        caret: Caret,
    ) -> Field {
        self.find.field_typing(
            term.geometry().0,
            Typing {
                query: self.query,
                composing,
                hints,
                caret,
            },
        )
    }
}

/// Whether the field is still open after a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Open {
    Open,
    Closed,
}

/// Whether the current match moved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Moved {
    Yes,
    No,
}

fn zero_width(c: Codepoint) -> bool {
    baud_domain::Classified::of(c).width(baud_domain::WidthPolicy::DEFAULT)
        == baud_domain::Width::ZERO
}

/// **A match as a selection**: a stream from its first cell to its last,
/// numbered by `scrolled` as every selection is; `None` for a match the
/// live surface no longer holds.
pub(crate) fn selection_of(term: &Term<'_>, scrolled: Scrolled, m: Found) -> Option<Selection> {
    let first = scrolled.line_of(term.surface_line(m.first.line)?);
    let last = scrolled.line_of(term.surface_line(m.last.line)?);
    let at = |p: Point| Span { start: p, end: p };
    let mut sel = Selection::at(at(Point::new(first, m.first.col)), Grain::Cell);
    sel.extend(at(Point::new(last, m.last.col)));
    Some(sel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{CacheGeometry, Depth, Owned};
    use baud_domain::{BytePos, ReadLen, WinSize};
    use baud_render::Current;

    const WS: WinSize = WinSize {
        ws_col: 30,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    fn with_term<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
        let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(40));
        let mut t = Term::new(owned.storage(), &WS);
        f(&mut t)
    }

    fn typed(f: &mut Finding, t: &Term<'_>, s: &str) {
        for c in s.chars() {
            let key = field_key(
                KeyCode::Char(Codepoint::from_char(c)),
                Modifiers::NONE,
                Some(Codepoint::from_char(c)),
            );
            assert_eq!(f.key(t, key), Open::Open);
        }
    }

    /// The HIG's chords (`[[source: apple-hig-keyboards]]`), and nothing
    /// else under Command.
    #[test]
    fn the_hig_chords_open_use_the_selection_and_step() {
        assert_eq!(command_chord("f", Shift::Up), Some(Chord::Open));
        assert_eq!(command_chord("e", Shift::Up), Some(Chord::UseSelection));
        assert_eq!(command_chord("g", Shift::Up), Some(Chord::Next));
        assert_eq!(command_chord("G", Shift::Down), Some(Chord::Previous));
        assert_eq!(command_chord("g", Shift::Down), Some(Chord::Previous));
        assert_eq!(command_chord("f", Shift::Down), None);
        assert_eq!(command_chord("c", Shift::Up), None);
        assert_eq!(command_chord("v", Shift::Up), None);
    }

    #[test]
    fn a_key_in_the_field_closes_steps_deletes_or_types() {
        let none = Modifiers::NONE;
        let a = Codepoint::from_char('a');
        assert_eq!(field_key(KeyCode::Escape, none, None), FieldKey::Close);
        assert_eq!(field_key(KeyCode::Enter, none, None), FieldKey::Next);
        assert_eq!(
            field_key(KeyCode::Enter, none.with(Modifiers::SHIFT), None),
            FieldKey::Previous
        );
        assert_eq!(field_key(KeyCode::Backspace, none, None), FieldKey::Delete);
        assert_eq!(
            field_key(KeyCode::Char(a), none, Some(a)),
            FieldKey::Type(a)
        );
        let upper = Codepoint::from_char('A');
        assert_eq!(
            field_key(KeyCode::Char(a), none.with(Modifiers::SHIFT), Some(upper)),
            FieldKey::Type(upper)
        );
        assert_eq!(
            field_key(KeyCode::Char(a), none.with(Modifiers::CTRL), None),
            FieldKey::Nothing
        );
        assert_eq!(
            field_key(KeyCode::Char(a), none.with(Modifiers::ALT), Some(a)),
            FieldKey::Nothing
        );
        assert_eq!(field_key(KeyCode::Up, none, None), FieldKey::Nothing);
    }

    /// **What is found is what is copied**: the current match, the newest,
    /// is the selection, and its text the match's.
    #[test]
    fn typing_selects_the_newest_match_and_copying_takes_it() {
        with_term(|t| {
            feed(t, "one Straße\r\ntwo STRASSE\r\nthree".as_bytes());
            let mut f = Finding::open(t, Vec::new());
            assert!(f.find().current().is_none());
            typed(&mut f, t, "strasse");
            let sel = f.show(t, Scrolled::START).expect("a match selected");
            assert_eq!(sel.text(t, Scrolled::START), "STRASSE");
            assert_eq!(t.window_back(), Rows::ZERO, "the match shows on the screen");
            assert_eq!(f.key(t, FieldKey::Next), Open::Open);
            let sel = f.show(t, Scrolled::START).unwrap();
            assert_eq!(
                sel.text(t, Scrolled::START),
                "Straße",
                "Find Next goes older"
            );
            assert_eq!(f.key(t, FieldKey::Close), Open::Closed);
        });
    }

    #[test]
    fn find_next_moves_the_window_up_the_history_to_the_match() {
        with_term(|t| {
            feed(t, b"needle\r\n");
            for i in 0..20 {
                feed(t, format!("x{i}\r\n").as_bytes());
            }
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "needle");
            let sel = f.show(t, Scrolled::START).unwrap();
            assert!(t.window_back().index() > 0, "the window looks back");
            let look = f.look(t, t.window_back());
            let current: Vec<_> = look
                .marks
                .iter()
                .filter(|m| m.current == Current::Yes)
                .collect();
            assert_eq!(current.len(), 1, "the match is in the window drawn");
            let shown = sel.highlight(
                Scrolled::START.top(t.window_back()),
                Rows::of_count(6),
                baud_domain::Cols::of_index(30),
            );
            let h = shown.expect("the selection is on the window");
            assert_eq!(
                (baud_render::WindowLine::of(h.from.0), h.from.1),
                (current[0].line, current[0].from)
            );
        });
    }

    /// The frame's records: every match marked, the current one flagged
    /// for the selection, and the field along the top row.
    #[test]
    fn the_look_marks_every_match_and_carries_the_field() {
        with_term(|t| {
            feed(t, b"ab ab ab");
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "ab");
            let look = f.look(t, Rows::ZERO);
            assert_eq!(look.marks.len(), 3);
            let flagged: Vec<Current> = look.marks.iter().map(|m| m.current).collect();
            assert_eq!(flagged, [Current::No, Current::No, Current::Yes]);
            let text: String = look
                .field
                .text
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect();
            assert!(text.starts_with(" Find ab"), "{text:?}");
            assert!(text.ends_with(" 1 of 3 "), "{text:?}");
        });
    }

    /// Command-G and Shift-Command-G step the open field as Enter and
    /// Shift+Enter do; Command-F on an open field changes nothing.
    #[test]
    fn a_chord_steps_the_open_field() {
        with_term(|t| {
            feed(t, b"m1\r\nm2");
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "m");
            let line = |f: &Finding| f.find().current().unwrap().first.line.value();
            let newest = line(&f);
            f.chord(Chord::Open);
            assert_eq!(line(&f), newest);
            f.chord(Chord::Next);
            assert!(line(&f) < newest, "Command-G goes older");
            f.chord(Chord::Previous);
            assert_eq!(line(&f), newest);
        });
    }

    #[test]
    fn delete_takes_a_character_with_its_marks() {
        with_term(|t| {
            let mut f = Finding::open(t, Vec::new());
            f.typed(
                t,
                &[
                    Codepoint::from_char('x'),
                    Codepoint::from_char('e'),
                    Codepoint::new(0x301).unwrap(),
                ],
            );
            assert_eq!(f.key(t, FieldKey::Delete), Open::Open);
            assert_eq!(f.find().query(), &[Codepoint::from_char('x')]);
        });
    }

    /// **A resize with nothing read** leaves the marks and the selection
    /// those of a find opened afresh on the re-laid grid: the match a new
    /// width moved to the next row is marked there, not past the last
    /// column.
    #[test]
    fn a_resize_without_a_read_marks_what_a_fresh_find_marks() {
        with_term(|t| {
            feed(t, b"------------------------needle\r\nx\r\n");
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "needle");
            let _ = t.resize(&WinSize {
                ws_col: 20,
                ws_row: 6,
            });
            let sel = f.after_resize(t, Scrolled::START);
            let fresh = Finding::open(t, f.find().query().to_vec());
            assert_eq!(
                f.look(t, t.window_back()).marks,
                fresh.look(t, t.window_back()).marks
            );
            let sel = sel.expect("the current match selected again");
            assert_eq!(sel.text(t, Scrolled::START), "needle");
        });
    }

    #[test]
    fn output_after_a_read_keeps_the_current_match_until_its_line_changes() {
        with_term(|t| {
            feed(t, b"needle\r\nother");
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "needle");
            feed(t, b"\r\nmore output\r\n");
            assert_eq!(f.refresh(t), Moved::No);
            feed(t, b"\x1b[1;1H\x1b[2K");
            feed(t, b"\x1b[3;1Hneedle again");
            assert_eq!(f.refresh(t), Moved::Yes);
        });
    }

    /// **Command-G after Escape finds the last query again**, from the
    /// match that was current when the field closed, as a Find Next with
    /// no field open searches the find pasteboard's string (`[[source:
    /// iterm2-finddriver-654bb13]]` 589-594, 649-668): reopened on it,
    /// Command-G steps on to the next older match, and Shift-Command-G
    /// back. The reopened query is selected, so a key typed replaces it.
    /// A different string -- another application's -- starts from the
    /// newest match, as a fresh query does.
    #[test]
    fn command_g_after_escape_steps_on_from_where_the_field_closed() {
        with_term(|t| {
            feed(t, b"m1\r\nm2\r\nm3\r\nm4");
            let line = |f: &Finding| f.find().current().unwrap().first.line.value();
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "m");
            let lines: Vec<u64> = f
                .find()
                .matches()
                .iter()
                .map(|m| m.first.line.value())
                .collect();
            assert_eq!(lines.len(), 4);
            f.chord(Chord::Next);
            assert_eq!(line(&f), lines[2]);
            let left = f.left();
            assert_eq!(f.key(t, FieldKey::Close), Open::Closed);
            let m = vec![Codepoint::from_char('m')];
            let mut again = Finding::reopen(t, m.clone(), Some(&left), Chord::Next);
            assert_eq!(line(&again), lines[1], "on from where it closed");
            let back = Finding::reopen(t, m.clone(), Some(&left), Chord::Previous);
            assert_eq!(line(&back), lines[3], "and back");
            let same = Finding::reopen(t, m.clone(), Some(&left), Chord::Open);
            assert_eq!(line(&same), lines[2], "Command-F: where it closed");
            // A new search -- another string, nothing left, or the match it
            // closed on gone -- starts on the newest, unstepped.
            let upper = vec![Codepoint::from_char('M')];
            for chord in [Chord::Next, Chord::Previous] {
                let other = Finding::reopen(t, upper.clone(), Some(&left), chord);
                assert_eq!(line(&other), lines[3], "another string, {chord:?}");
                let fresh = Finding::reopen(t, m.clone(), None, chord);
                assert_eq!(line(&fresh), lines[3], "nothing left, {chord:?}");
            }
            typed(&mut again, t, "x");
            assert_eq!(
                again.find().query(),
                &[Codepoint::from_char('x')],
                "replaced"
            );
            typed(&mut again, t, "y");
            assert_eq!(again.find().query().len(), 2, "then added to");
            let mut cleared = Finding::reopen(t, m, Some(&left), Chord::Open);
            assert_eq!(cleared.key(t, FieldKey::Delete), Open::Open);
            assert!(cleared.find().query().is_empty(), "Delete clears it");
        });
    }

    /// **An input method composes in the open field**: its text after the
    /// query, underlined in the field's colours, the field's caret at the
    /// input method's cursor, and nothing of it at the terminal's cursor
    /// (`marked::composition` is not asked while the field is open). Its
    /// hints, `text-input-v3`'s `preedit_hint`s, are drawn in the field as
    /// at the cursor (`Composition`), over the field's selection colours:
    /// a hint the input method gives replaces the underline, and a
    /// codepoint past the hints it gives has none.
    #[test]
    fn a_composition_is_drawn_in_the_field_at_its_caret() {
        use baud_render::Caret;
        with_term(|t| {
            feed(t, b"ab");
            let mut f = Finding::open(t, Vec::new());
            typed(&mut f, t, "ab");
            let kana: Vec<Codepoint> = "かな".chars().map(Codepoint::from_char).collect();
            let plain = f.look(t, Rows::ZERO).field;
            let field = f.look_typing(t, Rows::ZERO, &kana, &[], Caret::At(1)).field;
            let text: String = field
                .text
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect();
            assert!(text.starts_with(" Find abかな"), "{text:?}");
            let under = Hints::SELECTION.with(Hints::WHOLE);
            let at = " Find ab".chars().count();
            assert_eq!(field.hints[at..at + 2], [under, under]);
            assert_eq!(field.hints[at - 1], Hints::SELECTION, "the query is not");
            assert_eq!(
                field.caret.index(),
                plain.caret.index() + 2,
                "after か, two columns past the query"
            );
            let end = f.look_typing(t, Rows::ZERO, &kana, &[], Caret::End).field;
            assert_eq!(end.caret.index(), plain.caret.index() + 4);
            // A prediction on か and nothing on な: italic where it says,
            // plain past it, both in the field's colours.
            let hinted = [Hints::PREDICTION];
            let field = f
                .look_typing(t, Rows::ZERO, &kana, &hinted, Caret::End)
                .field;
            assert_eq!(
                field.hints[at..at + 2],
                [Hints::SELECTION.with(Hints::PREDICTION), Hints::SELECTION],
                "the input method's hints reach the field"
            );
        });
    }
}
