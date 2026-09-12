//! `morx` (`[[source: apple-ttrm-morx]]`): Apple's extended glyph
//! metamorphosis table, under each chain's default flags.
//!
//! **Types 1, 2 and 4.** Contextual substitution, ligatures and the
//! non-contextual swash table are what Apple Color Emoji enables by default
//! -- its insertion and rearrangement subtables sit behind feature flags no
//! default selects -- and what Menlo, Monaco, Courier, Helvetica and Times
//! carry at all. A subtable of another type is declined and counted rather
//! than half-run.
//!
//! **Default flags only.** The chain's `defaultFlags` select the subtables;
//! no feature request is folded in, because a terminal has no feature
//! menu. HarfBuzz reads the `feat` table and asks for `liga` and friends,
//! so the fixtures are shaped with those turned back off.
//!
//! **The state machine is HarfBuzz's** (`[[source: harfbuzz-hb-aat-layout-common]]`,
//! `[[source: harfbuzz-hb-aat-layout-morx-table]]`), transcribed: one
//! transition per glyph plus one at end of text, `dontAdvance` bounded by
//! an operation budget, a ligature's components popped from a ring in
//! reverse and its action offsets sign-extended from thirty bits, its
//! spent components left as deleted glyphs -- class 2 to every later
//! subtable -- until the whole table has run, while a glyph a lookup
//! deleted in place stays for the shaper to hide, which is what
//! `hb-shape` 13.2.1 does with both. One deliberate difference: a lookup
//! value of zero means "no change", as the specification says, where
//! HarfBuzz would write glyph zero.

use crate::buffer::{Buffer, Glyph};
use crate::bytes::{Bytes, Count, Index, Len, Offset};
use baud_domain::GlyphId;

/// Coverage bits of a subtable header.
mod coverage {
    pub const VERTICAL: u32 = 0x8000_0000;
    pub const BACKWARDS: u32 = 0x4000_0000;
    pub const ALL_DIRECTIONS: u32 = 0x2000_0000;
    pub const TYPE: u32 = 0xFF;
}

/// Subtable types.
mod kind {
    pub const CONTEXTUAL: u32 = 1;
    pub const LIGATURE: u32 = 2;
    pub const NONCONTEXTUAL: u32 = 4;
}

/// A row of the state array.
#[derive(Clone, Copy, PartialEq, Eq)]
struct State(u16);

impl State {
    const START_OF_TEXT: State = State(0);
}

/// A column of the state array: the class the lookup gives a glyph.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Class(u16);

/// The four classes every state table predefines.
mod class {
    use super::Class;
    pub const END_OF_TEXT: Class = Class(0);
    pub const OUT_OF_BOUNDS: Class = Class(1);
    pub const DELETED_GLYPH: Class = Class(2);
}

/// An entry's flag word.
#[derive(Clone, Copy)]
struct Flags(u16);

impl Flags {
    fn has(self, f: Flags) -> bool {
        self.0 & f.0 != 0
    }
}

/// Entry flags shared by the state-table subtables.
const DONT_ADVANCE: Flags = Flags(0x4000);
/// Contextual: make the current glyph the marked glyph.
const SET_MARK: Flags = Flags(0x8000);
/// Ligature: push the current glyph onto the component stack.
const SET_COMPONENT: Flags = Flags(0x8000);
/// Ligature: run the actions from `ligActionIndex`.
const PERFORM_ACTION: Flags = Flags(0x2000);
/// Ligature action: the last action of the group; implies a store.
const ACTION_LAST: u32 = 0x8000_0000;
/// Ligature action: store the ligature in place of the popped glyph.
const ACTION_STORE: u32 = 0x4000_0000;
/// Ligature action: the thirty-bit signed component offset.
const ACTION_OFFSET: u32 = 0x3FFF_FFFF;
/// An index that names no lookup.
const NO_LOOKUP: u16 = 0xFFFF;

/// How many components a ligature can gather, HarfBuzz's ring size.
const COMPONENTS: usize = 64;

/// The glyph a lookup writes to delete in place, `0xFFFF` in the table
/// itself. HarfBuzz 13.2.1 leaves such a glyph in the buffer -- a
/// variation selector deleted this way still comes out as the hidden
/// space -- so baud does too: it stays, classed as deleted for the
/// subtables after it, and the shaper hides it like any other ignorable.
const DELETED: u16 = 0xFFFF;

/// A glyph a ligature has spent: classed as deleted for the subtables
/// after it, then removed once the whole table has run, which is where
/// HarfBuzz's output differs from the in-place case. No face maps this id.
const SPENT: u16 = 0xFFFE;

fn is_deleted(g: GlyphId) -> bool {
    g.index() == usize::from(DELETED) || is_spent(g)
}

fn is_spent(g: GlyphId) -> bool {
    g.index() == usize::from(SPENT)
}

/// What a lookup answers: a class, a glyph, or zero for "no change".
#[derive(Clone, Copy)]
struct Value(u16);

/// A per-glyph lookup table (`[[source: apple-ttrm-morx]]`, "Lookup tables
/// in a 'morx'"), formats 0, 2, 4, 6, 8 and 10, answering `u16` values.
#[derive(Clone, Copy)]
struct Lookup<'a> {
    table: Bytes<'a>,
}

impl<'a> Lookup<'a> {
    fn get(self, g: GlyphId) -> Option<Value> {
        let t = self.table;
        let glyph = u16::try_from(g.index()).ok()?;
        let format: u16 = t.read(Offset::ZERO)?;
        let value: Option<u16> = match format {
            0 => t.read(Offset::of(2u32).at(Index::of(glyph), Len::of(2))?),
            2 | 4 | 6 => {
                // A binary-search header: unit size, unit count and the
                // three derived words nothing here reads.
                let unit = Len::of(u32::from(t.read::<u16>(Offset::of(2u32))?));
                let units = Count::of(t.read::<u16>(Offset::of(4u32))?);
                let base = Offset::of(12u32);
                let mut lo = 0u32;
                let mut hi = units.value();
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    let at = base.at(Index::of(mid), unit)?;
                    let key: u16 = t.read(at)?;
                    // Format 6 keys one glyph; 2 and 4 key a segment by
                    // its last glyph then its first.
                    let (last, first) = if format == 6 {
                        (key, key)
                    } else {
                        (key, t.read(at.plus(Len::of(2))?)?)
                    };
                    if glyph > last {
                        lo = mid + 1;
                    } else if glyph < first {
                        hi = mid;
                    } else {
                        let value_at = at.plus(Len::of(if format == 6 { 2 } else { 4 }))?;
                        let value: u16 = t.read(value_at)?;
                        return Some(Value(if format == 4 {
                            let off = Offset::of(u32::from(value));
                            t.read(off.at(Index::of(glyph.checked_sub(first)?), Len::of(2))?)?
                        } else {
                            value
                        }));
                    }
                }
                None
            }
            8 => {
                let first: u16 = t.read(Offset::of(2u32))?;
                let count: u16 = t.read(Offset::of(4u32))?;
                let i = glyph.checked_sub(first)?;
                if i >= count {
                    return None;
                }
                t.read(Offset::of(6u32).at(Index::of(i), Len::of(2))?)
            }
            10 => {
                let size: u16 = t.read(Offset::of(2u32))?;
                let first: u16 = t.read(Offset::of(4u32))?;
                let count: u16 = t.read(Offset::of(6u32))?;
                let i = glyph.checked_sub(first)?;
                if i >= count || !(1..=4).contains(&size) {
                    return None;
                }
                let at = Offset::of(8u32).at(Index::of(i), Len::of(u32::from(size)))?;
                let mut v = 0u32;
                let mut k = 0u32;
                while k < u32::from(size) {
                    let b: u8 = t.read(at.plus(Len::of(k))?)?;
                    v = (v << 8) | u32::from(b);
                    k += 1;
                }
                u16::try_from(v).ok()
            }
            _ => None,
        };
        value.map(Value)
    }

    /// A substitution: the looked-up glyph, or `None` when the table has
    /// nothing for it or answers zero, which the specification reserves
    /// for "no change".
    fn substitute(self, g: GlyphId) -> Option<GlyphId> {
        if is_deleted(g) {
            return None;
        }
        match self.get(g)? {
            Value(0) => None,
            Value(v) => Some(GlyphId::from_shaper(v)),
        }
    }
}

/// The extended state table header at the start of a state subtable.
#[derive(Clone, Copy)]
struct Machine<'a> {
    /// The subtable body, which every offset in the header counts from.
    body: Bytes<'a>,
    classes: Count,
    class_table: Lookup<'a>,
    states: Offset,
    entries: Offset,
    /// Bytes per entry: the two shared words plus the type's own.
    entry_len: Len,
}

/// One transition.
#[derive(Clone, Copy)]
struct Entry {
    next: State,
    flags: Flags,
    /// The type's own words: `ligActionIndex`, or `markIndex` and
    /// `currentIndex`.
    data: [u16; 2],
}

impl<'a> Machine<'a> {
    fn parse(body: Bytes<'a>, entry_len: Len) -> Option<Machine<'a>> {
        let classes: u32 = body.read(Offset::ZERO)?;
        if classes < 4 {
            return None;
        }
        let class_off: u32 = body.read(Offset::of(4u32))?;
        let states: u32 = body.read(Offset::of(8u32))?;
        let entries: u32 = body.read(Offset::of(12u32))?;
        Some(Machine {
            body,
            classes: Count::of(classes),
            class_table: Lookup {
                table: body.tail(Offset::of(class_off))?,
            },
            states: Offset::of(states),
            entries: Offset::of(entries),
            entry_len,
        })
    }

    fn class_of(&self, g: GlyphId) -> Class {
        if is_deleted(g) {
            return class::DELETED_GLYPH;
        }
        self.class_table
            .get(g)
            .map_or(class::OUT_OF_BOUNDS, |v| Class(v.0))
    }

    fn entry(&self, state: State, class: Class) -> Option<Entry> {
        let class = if self.classes.holds(Index::of(class.0)) {
            class
        } else {
            class::OUT_OF_BOUNDS
        };
        let cell = u32::from(state.0)
            .checked_mul(self.classes.value())?
            .checked_add(u32::from(class.0))?;
        let index: u16 = self
            .body
            .read(self.states.at(Index::of(cell), Len::of(2))?)?;
        let at = self.entries.at(Index::of(index), self.entry_len)?;
        Some(Entry {
            next: State(self.body.read(at)?),
            flags: Flags(self.body.read(at.plus(Len::of(2))?)?),
            data: [
                self.body.read(at.plus(Len::of(4))?).unwrap_or(NO_LOOKUP),
                self.body.read(at.plus(Len::of(6))?).unwrap_or(NO_LOOKUP),
            ],
        })
    }
}

/// What a subtable's transition does with the buffer.
trait Transition {
    fn transition(&mut self, buf: &mut Buffer<'_>, at: &Index, entry: Entry) -> Option<()>;
}

/// Drives a state table over the buffer: one transition per glyph and one
/// more at end of text, with `dontAdvance` bounded so a hostile table
/// cannot spin.
fn drive(machine: &Machine<'_>, buf: &mut Buffer<'_>, t: &mut impl Transition) -> Option<()> {
    let mut state = State::START_OF_TEXT;
    let mut at = Index::ZERO;
    let mut budget = buf.len().value().saturating_mul(8).saturating_add(64);
    loop {
        let class = match buf.get(at) {
            Some(g) => machine.class_of(g.id),
            None => class::END_OF_TEXT,
        };
        let entry = machine.entry(state, class)?;
        t.transition(buf, &at, entry)?;
        state = entry.next;
        if !buf.len().holds(at) {
            return Some(());
        }
        if !entry.flags.has(DONT_ADVANCE) || budget == 0 {
            at = at.next();
        }
        budget = budget.saturating_sub(1);
    }
}

/// Type 1: substitute the marked and the current glyph through per-glyph
/// lookups the entry names.
struct Contextual<'a> {
    /// The array of offsets to lookups, each from the array's own start.
    lookups: Bytes<'a>,
    mark: Option<Index>,
}

impl Contextual<'_> {
    fn lookup(&self, i: Index) -> Option<Lookup<'_>> {
        let off: u32 = self.lookups.read(Offset::ZERO.at(i, Len::of(4))?)?;
        Some(Lookup {
            table: self.lookups.tail(Offset::of(off))?,
        })
    }
}

impl Transition for Contextual<'_> {
    fn transition(&mut self, buf: &mut Buffer<'_>, at: &Index, entry: Entry) -> Option<()> {
        let at_end = !buf.len().holds(*at);
        // CoreText applies neither substitution at end of text unless a
        // mark was set, and HarfBuzz follows it.
        if at_end && self.mark.is_none() {
            return Some(());
        }
        if entry.data[0] != NO_LOOKUP {
            let mark = self.mark.unwrap_or(Index::ZERO);
            if let Some(g) = buf.get(mark) {
                if let Some(out) = self.lookup(Index::of(entry.data[0]))?.substitute(g.id) {
                    buf.get_mut(mark)?.id = out;
                }
            }
        }
        if entry.data[1] != NO_LOOKUP {
            let cur = if at_end { buf.len().last()? } else { *at };
            if let Some(g) = buf.get(cur) {
                if let Some(out) = self.lookup(Index::of(entry.data[1]))?.substitute(g.id) {
                    buf.get_mut(cur)?.id = out;
                }
            }
        }
        if entry.flags.has(SET_MARK) {
            self.mark = Some(*at);
        }
        Some(())
    }
}

/// The component stack of a ligature subtable: a ring of buffer
/// positions, as HarfBuzz keeps it, so an overlong group wraps rather
/// than overflows.
struct Stack {
    at: [Index; COMPONENTS],
    depth: Count,
}

impl Stack {
    fn get(&self, depth: Count) -> Index {
        self.at[depth.slot() % COMPONENTS]
    }

    fn push(&mut self, at: Index) {
        self.at[self.depth.slot() % COMPONENTS] = at;
        self.depth = Count::of(self.depth.value().saturating_add(1));
    }

    fn top(&self) -> Option<Index> {
        self.depth.last().map(|d| self.get(Count::of(d.value())))
    }

    fn pop(&mut self) -> Option<Index> {
        let d = self.depth.last()?;
        self.depth = Count::of(d.value());
        Some(self.get(self.depth))
    }
}

/// Type 2: gather components, then pop them into a ligature.
struct Ligature<'a> {
    body: Bytes<'a>,
    actions: Offset,
    components: Offset,
    ligatures: Offset,
    stack: Stack,
}

impl Transition for Ligature<'_> {
    fn transition(&mut self, buf: &mut Buffer<'_>, at: &Index, entry: Entry) -> Option<()> {
        if entry.flags.has(SET_COMPONENT) {
            // Never the same position twice, in case dontAdvance was used.
            if self.stack.top() == Some(*at) {
                self.stack.pop();
            }
            self.stack.push(*at);
        }
        if !entry.flags.has(PERFORM_ACTION) {
            return Some(());
        }
        if self.stack.depth.is_zero() || !buf.len().holds(*at) {
            return Some(());
        }
        let mut cursor = self.stack.depth;
        let mut action_at = self.actions.at(Index::of(entry.data[0]), Len::of(4))?;
        let mut ligature_index = 0u32;
        loop {
            let Some(below) = cursor.last() else {
                // Stack underflow: clear it.
                self.stack.depth = Count::ZERO;
                break;
            };
            cursor = Count::of(below.value());
            let pos = self.stack.get(cursor);
            let action: u32 = self.body.read(action_at)?;
            let mut offset = action & ACTION_OFFSET;
            if offset & 0x2000_0000 != 0 {
                offset |= 0xC000_0000;
            }
            let glyph = buf.get(pos)?.id;
            let component =
                i64::from(u16::try_from(glyph.index()).ok()?) + i64::from(offset.cast_signed());
            let component = u32::try_from(component).ok()?;
            let part: u16 = self
                .body
                .read(self.components.at(Index::of(component), Len::of(2))?)?;
            ligature_index = ligature_index.checked_add(u32::from(part))?;
            if action & (ACTION_STORE | ACTION_LAST) != 0 {
                let lig: u16 = self
                    .body
                    .read(self.ligatures.at(Index::of(ligature_index), Len::of(2))?)?;
                // The ligature spans every cell its later components did;
                // those stay behind as deleted glyphs so no position moves.
                let mut last = buf.get(pos)?.last;
                while self.stack.depth.value() > cursor.value().saturating_add(1) {
                    let Some(spent) = self.stack.pop() else {
                        break;
                    };
                    if let Some(g) = buf.get_mut(spent) {
                        last = last.max(g.last);
                        g.id = GlyphId::from_shaper(SPENT);
                    }
                }
                let g: &mut Glyph = buf.get_mut(pos)?;
                g.id = GlyphId::from_shaper(lig);
                g.last = last.max(g.last);
            }
            action_at = action_at.plus(Len::of(4))?;
            if action & ACTION_LAST != 0 {
                break;
            }
        }
        Some(())
    }
}

#[derive(Clone, Copy)]
pub struct Morx<'a> {
    data: Bytes<'a>,
    chains: Count,
}

impl<'a> Morx<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<Morx<'a>> {
        let version: u16 = data.read(Offset::ZERO)?;
        if !(2..=3).contains(&version) {
            return None;
        }
        let chains: u32 = data.read(Offset::of(4u32))?;
        Some(Morx {
            data,
            chains: Count::of(chains),
        })
    }

    /// Runs every chain's default subtables over the buffer, in order, and
    /// answers how many subtables were declined for their type.
    pub fn apply(&self, buf: &mut Buffer<'_>) -> Count {
        let mut declined = 0u32;
        let mut at = Offset::of(8u32);
        let mut chain = 0u32;
        while chain < self.chains.value() {
            let Some(next) = self.apply_chain(at, buf, &mut declined) else {
                break;
            };
            at = next;
            chain += 1;
        }
        // What the ligatures spent comes out now, from the back so earlier
        // positions stay valid.
        let mut i = buf.len().as_index();
        while let Some(p) = i.prev() {
            i = p;
            if buf.get(i).is_some_and(|g| is_spent(g.id)) {
                buf.remove(i);
            }
        }
        Count::of(declined)
    }

    /// One chain; answers the offset of the next.
    fn apply_chain(&self, at: Offset, buf: &mut Buffer<'_>, declined: &mut u32) -> Option<Offset> {
        let d = self.data;
        let flags: u32 = d.read(at)?;
        let length: u32 = d.read(at.plus(Len::of(4))?)?;
        let features: u32 = d.read(at.plus(Len::of(8))?)?;
        let subtables: u32 = d.read(at.plus(Len::of(12))?)?;
        let mut sub = at.plus(Len::of(16))?.at(Index::of(features), Len::of(12))?;
        let mut i = 0u32;
        while i < subtables {
            let sub_len: u32 = d.read(sub)?;
            let coverage: u32 = d.read(sub.plus(Len::of(4))?)?;
            let sub_flags: u32 = d.read(sub.plus(Len::of(8))?)?;
            if sub_len < 12 {
                return None;
            }
            let body = d.slice(sub.plus(Len::of(12))?, Len::of(sub_len - 12))?;
            let horizontal =
                coverage & coverage::ALL_DIRECTIONS != 0 || coverage & coverage::VERTICAL == 0;
            if sub_flags & flags != 0 && horizontal {
                let backwards = coverage & coverage::BACKWARDS != 0;
                if backwards {
                    buf.as_mut_slice().reverse();
                }
                let ran = match coverage & coverage::TYPE {
                    kind::CONTEXTUAL => Self::contextual(body, buf),
                    kind::LIGATURE => Self::ligature(body, buf),
                    kind::NONCONTEXTUAL => Self::noncontextual(body, buf),
                    _ => None,
                };
                if backwards {
                    buf.as_mut_slice().reverse();
                }
                if ran.is_none() {
                    *declined = declined.saturating_add(1);
                }
            }
            sub = sub.plus(Len::of(sub_len))?;
            i += 1;
        }
        at.plus(Len::of(length))
    }

    fn contextual(body: Bytes<'_>, buf: &mut Buffer<'_>) -> Option<()> {
        let machine = Machine::parse(body, Len::of(8))?;
        let off: u32 = body.read(Offset::of(16u32))?;
        let mut t = Contextual {
            lookups: body.tail(Offset::of(off))?,
            mark: None,
        };
        drive(&machine, buf, &mut t)
    }

    fn ligature(body: Bytes<'_>, buf: &mut Buffer<'_>) -> Option<()> {
        let machine = Machine::parse(body, Len::of(6))?;
        let actions: u32 = body.read(Offset::of(16u32))?;
        let components: u32 = body.read(Offset::of(20u32))?;
        let ligatures: u32 = body.read(Offset::of(24u32))?;
        let mut t = Ligature {
            body,
            actions: Offset::of(actions),
            components: Offset::of(components),
            ligatures: Offset::of(ligatures),
            stack: Stack {
                at: [Index::ZERO; COMPONENTS],
                depth: Count::ZERO,
            },
        };
        drive(&machine, buf, &mut t)
    }

    fn noncontextual(body: Bytes<'_>, buf: &mut Buffer<'_>) -> Option<()> {
        let lookup = Lookup { table: body };
        let mut at = Index::ZERO;
        while let Some(g) = buf.get(at) {
            if let Some(out) = lookup.substitute(g.id) {
                buf.get_mut(at)?.id = out;
            }
            at = at.next();
        }
        Some(())
    }
}
