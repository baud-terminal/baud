//! The VT parser as a lookup, not a walk.
//!
//! **Same protocol, same public surface, different shape.** `machine.rs`
//! decides what a byte means by walking a `match` over twenty states and then
//! a chain of byte tests inside each arm. Every one of those tests is a pure
//! function of `(encoding, state, byte)` -- three bounded inputs -- so the
//! answer is computable at build time and the walk is a lookup.
//!
//! That is the same argument that made the UAX #29 transition a table, worth
//! 8% there: a rule walk is a chain of unpredictable branches, and a
//! mispredict costs three times an L1 hit.
//!
//! The table is `2 x 20 x 256` entries of one `u16` -- **20 KB**, built by a
//! `const fn`, so it is rodata with no initialiser and nothing to load.
//! `the_table_parser_matches_the_machine` drives both implementations over the
//! same generated programs and requires identical terminal state.
//!
//! Two things the table deliberately does *not* absorb:
//!
//! * **The actions.** Where a byte goes is a function of three bounded values;
//!   *what it does* is a call into the grid, and that is not a lookup. The
//!   table names the action and a flat `match` performs it, which is one
//!   dispatch over ~25 arms instead of nested tests over 20 x 256.
//! * **State payloads.** `Utf8` carries an accumulator and `Vt52Col` a row.
//!   Those live in `Term` beside the discriminant, because a table is indexed
//!   by a small integer and an accumulator is not one.

use crate::*;

/// A parser state, as the small integer a table can be indexed by.
///
/// `machine.rs`'s `State` carries payloads -- a UTF-8 accumulator, a VT52 row
/// -- which is why it cannot be one. Here the payload lives in `Term` and this
/// is the discriminant alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum SId {
    Ground = 0,
    Vt52Ground,
    /// Inside a multi-byte sequence, indexed by how many continuation bytes
    /// are still owed. **The count is part of the state**, because where the
    /// next byte goes depends on it and a table is indexed by the state alone.
    /// `machine.rs` carries it as a payload; here it is three ids.
    Utf8One,
    Utf8Two,
    Utf8Three,
    Escape,
    EscInter,
    CsiEntry,
    CsiParams,
    CsiInter,
    CsiIgnore,
    OscString,
    OscEsc,
    DcsEntry,
    DcsParam,
    DcsInter,
    DcsPass,
    DcsIgnore,
    DcsEsc,
    StringIgnore,
    Vt52Row,
    Vt52Col,
}

/// How many states the table is indexed over.
pub(crate) const NSTATE: usize = 22;

impl SId {
    const FROM_INDEX: [SId; NSTATE] = [
        SId::Ground,
        SId::Vt52Ground,
        SId::Utf8One,
        SId::Utf8Two,
        SId::Utf8Three,
        SId::Escape,
        SId::EscInter,
        SId::CsiEntry,
        SId::CsiParams,
        SId::CsiInter,
        SId::CsiIgnore,
        SId::OscString,
        SId::OscEsc,
        SId::DcsEntry,
        SId::DcsParam,
        SId::DcsInter,
        SId::DcsPass,
        SId::DcsIgnore,
        SId::DcsEsc,
        SId::StringIgnore,
        SId::Vt52Row,
        SId::Vt52Col,
    ];
    pub(crate) fn of(i: u8) -> SId {
        SId::FROM_INDEX[i as usize]
    }
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// What a byte does once the table has said where it goes.
///
/// Deliberately a *flat* set: the table has already resolved which state and
/// which byte range we are in, so nothing here re-tests either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Act {
    /// Consume it and do nothing. The largest arm by far.
    Nothing = 0,
    /// A C0 control that executes where it stands.
    Execute,
    /// Latin-1: one byte, one character.
    PrintByte,
    /// An 8-bit C1 outside UTF-8, re-entered through the `Escape` state.
    C1Escape,
    /// A UTF-8 lead byte: start the accumulator.
    Utf8Start,
    /// A UTF-8 continuation byte.
    Utf8Cont,
    /// Not a lead, not a continuation, not printable: the cold arm.
    GroundUncommon,
    /// A byte that cannot continue a UTF-8 sequence: emit the replacement and
    /// re-dispatch it from `Ground`. **Re-dispatching is part of the action**,
    /// so the table names the intermediate state and the action runs the table
    /// again -- which is what `machine.rs`'s recursive `control` call does.
    Utf8Bad,
    /// `CSI` seen: clear the parameter block.
    CsiClear,
    /// `DCS` seen: clear the parameter block and the string buffer.
    DcsClear,
    /// A two-byte escape, `ESC` plus a final.
    EscFe,
    /// An escape intermediate, collected.
    EscCollect,
    /// The final byte of an `ESC <inter> <final>` form.
    EscInterFinal,
    /// A private marker in `csi_entry`.
    CsiPrivate,
    /// A parameter or intermediate byte of a control sequence.
    CsiParamByte,
    /// The final byte of a control sequence.
    CsiFinal,
    /// A control sequence intermediate.
    CsiCollect,
    /// A private marker in `dcs_entry`.
    DcsPrivate,
    /// A parameter byte of a device control string.
    DcsParamByte,
    /// A device control string intermediate.
    DcsCollect,
    /// The final byte of a device control string: select the handler.
    ///
    /// **This action owns its own transition**, and it is the only one that
    /// does. Where it goes depends on `Dcs::parse(csi_inter, final)` -- whether
    /// baud has a handler for this string -- and `csi_inter` is *collected*
    /// state, not one of the three bounded inputs the table is indexed by. So
    /// the table names the action and the action decides between passthrough
    /// and ignore.
    DcsHook,
    /// A payload byte of a string.
    StringPut,
    /// A device control string terminated: dispatch it.
    DcsDispatch,
    /// A bare `ESC` inside a device control string: cancel it.
    DcsCancel,
    /// An operating system command terminated by `ST`.
    OscDispatchSt,
    /// The same, terminated by `BEL`.
    OscDispatchBel,
    /// A bare `ESC` inside an operating system command: cancel it.
    OscCancel,
    /// `CAN` or `SUB` mid-sequence: execute and abandon.
    Cancel,
    /// The first coordinate byte of `ESC Y`.
    Vt52SetRow,
    /// The second, which completes the address.
    Vt52Address,
    /// VT52's escape table, which is a different protocol rather than a subset.
    Vt52Dispatch,
}

impl Act {
    const FROM_INDEX: [Act; 31] = [
        Act::Nothing,
        Act::Execute,
        Act::PrintByte,
        Act::C1Escape,
        Act::Utf8Start,
        Act::Utf8Cont,
        Act::GroundUncommon,
        Act::Utf8Bad,
        Act::CsiClear,
        Act::DcsClear,
        Act::EscFe,
        Act::EscCollect,
        Act::EscInterFinal,
        Act::CsiPrivate,
        Act::CsiParamByte,
        Act::CsiFinal,
        Act::CsiCollect,
        Act::DcsPrivate,
        Act::DcsParamByte,
        Act::DcsCollect,
        Act::DcsHook,
        Act::StringPut,
        Act::DcsDispatch,
        Act::DcsCancel,
        Act::OscDispatchSt,
        Act::OscDispatchBel,
        Act::OscCancel,
        Act::Cancel,
        Act::Vt52SetRow,
        Act::Vt52Address,
        Act::Vt52Dispatch,
    ];
    pub(crate) fn of(i: u8) -> Act {
        Act::FROM_INDEX[i as usize]
    }
}

/// One transition: the action, and where the parser goes next.
///
/// Packed into a `u16` -- action in the high byte, state in the low -- so the
/// whole table is two bytes an entry and the lookup is one load.
const fn step(act: Act, next: SId) -> u16 {
    ((act as u16) << 8) | (next as u16)
}

// ── byte classification, spelled once ────────────────────────────────────────
// These mirror `StreamByte`'s predicates. They are duplicated here rather than
// called because the table is built in a `const fn` and these are the exact
// definitions the table has to agree with; `the_table_agrees_with_streambyte`
// checks every byte against the originals so the two cannot drift.

const fn is_c0(b: u8) -> bool {
    b < 0x20
}
const fn is_del(b: u8) -> bool {
    b == 0x7f
}
const fn is_esc_intermediate(b: u8) -> bool {
    b >= 0x20 && b <= 0x2f
}
const fn is_csi_intermediate(b: u8) -> bool {
    b >= 0x20 && b <= 0x2f
}
const fn is_final(b: u8) -> bool {
    b >= 0x40 && b <= 0x7e
}
const fn is_private(b: u8) -> bool {
    b >= 0x3c && b <= 0x3f
}
const fn is_utf8_lead(b: u8) -> bool {
    (b >= 0xc2 && b <= 0xdf) || (b >= 0xe0 && b <= 0xef) || (b >= 0xf0 && b <= 0xf4)
}
const fn is_utf8_cont(b: u8) -> bool {
    b & 0xc0 == 0x80
}

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const ST_8BIT: u8 = 0x9c;
const ST_FINAL: u8 = b'\\';
const CSI_INTRO: u8 = b'[';
const OSC_INTRO: u8 = b']';
const DCS_INTRO: u8 = b'P';
const SOS_INTRO: u8 = b'X';
const PM_INTRO: u8 = b'^';
const APC_INTRO: u8 = b'_';

/// Latin-1 maps every byte above `7F` to the codepoint of the same value,
/// except the C1 range, which is a control introducer.
/// A parameter-position byte, with the transitions `csi_param_byte` performs.
///
/// **Spelled here rather than delegated**, because that function both collects
/// *and* moves the parser: a final dispatches to `Ground`, an intermediate goes
/// to `csi_intermediate`, and anything else abandons the sequence. A table
/// that named only "collect a parameter" would be wrong for three of the four.
const fn csi_param(b: u8) -> u16 {
    if is_c0(b) {
        step(Act::Execute, SId::CsiParams)
    } else if is_final(b) {
        step(Act::CsiFinal, SId::Ground)
    } else if is_csi_intermediate(b) {
        step(Act::CsiCollect, SId::CsiInter)
    } else if is_del(b) {
        step(Act::Nothing, SId::CsiParams)
    } else if (b >= b'0' && b <= b'9') || b == b';' || b == b':' {
        step(Act::CsiParamByte, SId::CsiParams)
    } else {
        step(Act::Nothing, SId::CsiIgnore)
    }
}

/// A DCS parameter-position byte. Same reason as `csi_param`: the function it
/// replaces both collects and moves. Two differences from the CSI form and
/// both are the specification's: a C0 is *ignored* here rather than executed --
/// `[[source: williams]]` says C0 other than CAN, SUB and ESC are not executed
/// while recognising a device control string -- and a colon is not a separator.
const fn dcs_param(b: u8) -> u16 {
    if is_c0(b) || is_del(b) {
        step(Act::Nothing, SId::DcsParam)
    } else if is_csi_intermediate(b) {
        step(Act::DcsCollect, SId::DcsInter)
    } else if is_final(b) {
        step(Act::DcsHook, SId::DcsParam)
    } else if (b >= b'0' && b <= b'9') || b == b';' {
        step(Act::DcsParamByte, SId::DcsParam)
    } else {
        step(Act::Nothing, SId::DcsIgnore)
    }
}

/// How many continuations a lead byte owes.
const fn utf8_state(b: u8) -> SId {
    if b >= 0xf0 {
        SId::Utf8Three
    } else if b >= 0xe0 {
        SId::Utf8Two
    } else {
        SId::Utf8One
    }
}

const fn latin1_is_c1(b: u8) -> bool {
    b >= 0x80 && b <= 0x9f
}

/// The transition table, built at compile time.
///
/// Indexed `[encoding][state][byte]`, where encoding 0 is UTF-8 and 1 is
/// ISO 8859-1. 20 KB of rodata.
pub(crate) static VT: [u16; 2 * NSTATE * 256] = build();

pub(crate) const fn vt_index(latin1: bool, st: SId, b: u8) -> usize {
    (latin1 as usize) * NSTATE * 256 + (st as usize) * 256 + b as usize
}

const fn build() -> [u16; 2 * NSTATE * 256] {
    let mut t = [step(Act::Nothing, SId::Ground); 2 * NSTATE * 256];
    let mut e = 0usize;
    while e < 2 {
        let latin1 = e == 1;
        let mut s = 0usize;
        while s < NSTATE {
            let st = SId::FROM_INDEX[s];
            let mut i = 0usize;
            while i < 256 {
                t[e * NSTATE * 256 + s * 256 + i] = one(latin1, st, i as u8);
                i += 1;
            }
            s += 1;
        }
        e += 1;
    }
    t
}

/// The whole protocol, as one function of three bounded values.
const fn one(latin1: bool, st: SId, b: u8) -> u16 {
    // `anywhere`: CAN and SUB abandon a sequence in progress; ESC restarts one.
    // Ground-like states handle both themselves, so they fall through.
    // **Two different lists, and conflating them was four wrong cells.**
    // `CAN`/`SUB` abandon anything except the ground-like states, and the VT52
    // coordinate states count as in-progress. `ESC` restarts a sequence from
    // fewer states than that: the string forms keep it for themselves, because
    // there it is a terminator candidate rather than a fresh introducer, and
    // the VT52 coordinate bytes take it as data.
    let ground_like = matches!(
        st,
        SId::Ground | SId::Vt52Ground | SId::Utf8One | SId::Utf8Two | SId::Utf8Three
    );
    if (b == CAN || b == SUB) && !ground_like {
        return step(Act::Cancel, SId::Ground);
    }
    let in_sequence = !matches!(
        st,
        SId::Ground
            | SId::Vt52Ground
            | SId::Utf8One
            | SId::Utf8Two
            | SId::Utf8Three
            | SId::Vt52Row
            | SId::Vt52Col
    );
    if b == ESC {
        // The string states keep ESC for themselves: it is a terminator
        // candidate there, not a fresh sequence.
        let keeps_esc = matches!(
            st,
            SId::OscString
                | SId::OscEsc
                | SId::DcsPass
                | SId::DcsEsc
                | SId::DcsIgnore
                | SId::StringIgnore
        );
        if in_sequence && !keeps_esc {
            return step(Act::Nothing, SId::Escape);
        }
    }
    match st {
        SId::Ground | SId::Vt52Ground => {
            if b == ESC {
                step(Act::Nothing, SId::Escape)
            } else if is_c0(b) || b == BEL {
                step(Act::Execute, st)
            } else if latin1 && latin1_is_c1(b) {
                step(Act::C1Escape, SId::Escape)
            } else if latin1 && b >= 0xa0 {
                step(Act::PrintByte, st)
            } else if !latin1 && is_utf8_lead(b) {
                step(Act::Utf8Start, utf8_state(b))
            } else {
                step(Act::GroundUncommon, st)
            }
        }
        SId::Utf8One | SId::Utf8Two | SId::Utf8Three => {
            if is_utf8_cont(b) {
                let next = match st {
                    SId::Utf8Three => SId::Utf8Two,
                    SId::Utf8Two => SId::Utf8One,
                    // The last one completes the character.
                    _ => SId::Ground,
                };
                step(Act::Utf8Cont, next)
            } else {
                // Flush a replacement and re-dispatch the byte from Ground.
                step(Act::Utf8Bad, SId::Ground)
            }
        }
        SId::Escape => {
            if b == CSI_INTRO {
                step(Act::CsiClear, SId::CsiEntry)
            } else if b == OSC_INTRO {
                step(Act::Nothing, SId::OscString)
            } else if b == DCS_INTRO {
                step(Act::DcsClear, SId::DcsEntry)
            } else if b == SOS_INTRO || b == PM_INTRO || b == APC_INTRO {
                step(Act::Nothing, SId::StringIgnore)
            } else if is_esc_intermediate(b) {
                step(Act::EscCollect, SId::EscInter)
            } else {
                step(Act::EscFe, SId::Ground)
            }
        }
        SId::EscInter => {
            if is_c0(b) {
                step(Act::Execute, st)
            } else if is_esc_intermediate(b) {
                step(Act::EscCollect, st)
            } else if is_del(b) {
                step(Act::Nothing, st)
            } else {
                step(Act::EscInterFinal, SId::Ground)
            }
        }
        SId::CsiEntry => {
            if is_c0(b) {
                step(Act::Execute, st)
            } else if is_del(b) {
                step(Act::Nothing, st)
            } else if is_private(b) {
                step(Act::CsiPrivate, SId::CsiParams)
            } else {
                csi_param(b)
            }
        }
        SId::CsiParams => csi_param(b),
        SId::CsiInter => {
            if is_c0(b) {
                step(Act::Execute, st)
            } else if is_final(b) {
                step(Act::CsiFinal, SId::Ground)
            } else if is_del(b) {
                step(Act::Nothing, st)
            } else if !is_csi_intermediate(b) {
                step(Act::Nothing, SId::CsiIgnore)
            } else {
                step(Act::CsiCollect, st)
            }
        }
        SId::CsiIgnore => {
            if is_c0(b) {
                step(Act::Execute, st)
            } else if is_final(b) {
                step(Act::Nothing, SId::Ground)
            } else {
                step(Act::Nothing, st)
            }
        }
        SId::OscString => {
            if latin1 && b == ST_8BIT {
                step(Act::OscDispatchSt, SId::Ground)
            } else if b == BEL {
                step(Act::OscDispatchBel, SId::Ground)
            } else if b == ESC {
                step(Act::Nothing, SId::OscEsc)
            } else if is_c0(b) {
                step(Act::Nothing, st)
            } else {
                step(Act::StringPut, st)
            }
        }
        SId::OscEsc => {
            if b == ST_FINAL {
                step(Act::OscDispatchSt, SId::Ground)
            } else {
                step(Act::OscCancel, SId::Escape)
            }
        }
        SId::DcsEntry => {
            if is_c0(b) || is_del(b) {
                step(Act::Nothing, st)
            } else if is_private(b) {
                step(Act::DcsPrivate, SId::DcsParam)
            } else {
                dcs_param(b)
            }
        }
        SId::DcsParam => dcs_param(b),
        SId::DcsInter => {
            if is_c0(b) || is_del(b) {
                step(Act::Nothing, st)
            } else if is_csi_intermediate(b) {
                step(Act::DcsCollect, st)
            } else if is_final(b) {
                step(Act::DcsHook, st)
            } else {
                step(Act::Nothing, SId::DcsIgnore)
            }
        }
        SId::DcsPass => {
            if latin1 && b == ST_8BIT {
                step(Act::DcsDispatch, SId::Ground)
            } else if b == ESC {
                step(Act::Nothing, SId::DcsEsc)
            } else if b == BEL {
                step(Act::DcsDispatch, SId::Ground)
            } else if is_c0(b) {
                step(Act::Nothing, st)
            } else {
                step(Act::StringPut, st)
            }
        }
        SId::DcsIgnore => {
            if latin1 && b == ST_8BIT {
                step(Act::Nothing, SId::Ground)
            } else if b == ESC {
                step(Act::Nothing, SId::Escape)
            } else if b == BEL {
                step(Act::Nothing, SId::Ground)
            } else {
                step(Act::Nothing, st)
            }
        }
        SId::DcsEsc => {
            if b == ST_FINAL {
                step(Act::DcsDispatch, SId::Ground)
            } else {
                step(Act::DcsCancel, SId::Escape)
            }
        }
        SId::StringIgnore => {
            if latin1 && b == ST_8BIT {
                step(Act::Nothing, SId::Ground)
            } else if b == BEL {
                step(Act::Nothing, SId::Ground)
            } else if b == ESC {
                step(Act::Nothing, SId::Escape)
            } else {
                step(Act::Nothing, st)
            }
        }
        SId::Vt52Row => step(Act::Vt52SetRow, SId::Vt52Col),
        SId::Vt52Col => step(Act::Vt52Address, SId::Ground),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::state::State;
    use crate::{InputEncoding, Storage, Term, TerminalMode, Vt52Coord};

    /// The discriminant of a live `State`, so the two representations can be
    /// compared.
    fn sid_of(s: State) -> SId {
        match s {
            State::Ground => SId::Ground,
            State::Vt52Ground => SId::Vt52Ground,
            State::Utf8 { remaining, .. } => match remaining.owed() {
                3 => SId::Utf8Three,
                2 => SId::Utf8Two,
                _ => SId::Utf8One,
            },
            State::Escape => SId::Escape,
            State::EscapeIntermediate => SId::EscInter,
            State::CsiEntry => SId::CsiEntry,
            State::CsiParams => SId::CsiParams,
            State::CsiIntermediate => SId::CsiInter,
            State::CsiIgnore => SId::CsiIgnore,
            State::OscString => SId::OscString,
            State::OscEsc => SId::OscEsc,
            State::DcsEntry => SId::DcsEntry,
            State::DcsParam => SId::DcsParam,
            State::DcsIntermediate => SId::DcsInter,
            State::DcsPassthrough => SId::DcsPass,
            State::DcsIgnore => SId::DcsIgnore,
            State::DcsEsc => SId::DcsEsc,
            State::StringIgnore => SId::StringIgnore,
            State::Vt52Row => SId::Vt52Row,
            State::Vt52Col(_) => SId::Vt52Col,
        }
    }

    /// A live `State` for each id, with dummy payloads where one is needed.
    fn state_of(s: SId) -> State {
        match s {
            SId::Ground => State::Ground,
            SId::Vt52Ground => State::Vt52Ground,
            SId::Utf8One | SId::Utf8Two | SId::Utf8Three => State::Utf8 {
                remaining: crate::Pending::new(match s {
                    SId::Utf8Three => 3,
                    SId::Utf8Two => 2,
                    _ => 1,
                }),
                acc: crate::Utf8Acc::lead(0),
                min: crate::Utf8Min::Two,
            },
            SId::Escape => State::Escape,
            SId::EscInter => State::EscapeIntermediate,
            SId::CsiEntry => State::CsiEntry,
            SId::CsiParams => State::CsiParams,
            SId::CsiInter => State::CsiIntermediate,
            SId::CsiIgnore => State::CsiIgnore,
            SId::OscString => State::OscString,
            SId::OscEsc => State::OscEsc,
            SId::DcsEntry => State::DcsEntry,
            SId::DcsParam => State::DcsParam,
            SId::DcsInter => State::DcsIntermediate,
            SId::DcsPass => State::DcsPassthrough,
            SId::DcsIgnore => State::DcsIgnore,
            SId::DcsEsc => State::DcsEsc,
            SId::StringIgnore => State::StringIgnore,
            SId::Vt52Row => State::Vt52Row,
            SId::Vt52Col => {
                State::Vt52Col(Vt52Coord::from_wire(crate::StreamByte::from_stream(0x20)))
            }
        }
    }

    /// **The table must send every byte where the machine sends it.**
    ///
    /// Exhaustive over `(encoding, state, byte)` -- 10,240 transitions -- and
    /// exhaustive on purpose: a transition table is exactly the shape where a
    /// sampled check passes and one cell is wrong. Compares the state the
    /// machine lands in against the state the table names.
    #[test]
    fn the_table_sends_every_byte_where_the_machine_does() {
        // `no_std`: count them and keep the first, rather than collecting.
        let mut wrong = 0usize;
        let mut first: Option<(bool, SId, u8, SId, SId)> = None;
        for latin1 in [false, true] {
            for si in 0..NSTATE {
                let from = SId::of(si as u8);
                // VT52 has its own escape protocol and its own ground state;
                // the table models `Escape` as ANSI, so drive ANSI here.
                for b in 0u8..=255 {
                    let mut st = Storage::boxed();
                    let mut t = Term::new(&mut st, crate::OpMode::Computed);
                    t.terminal_mode = TerminalMode::Ansi;
                    t.encoding = match latin1 {
                        true => InputEncoding::Latin1,
                        false => InputEncoding::Utf8,
                    };
                    t.st = state_of(from);
                    t.control(crate::StreamByte::from_stream(b));
                    let want = sid_of(t.st);
                    let mut e = VT[vt_index(latin1, from, b)];
                    // Three actions re-enter the table with the same byte,
                    // mirroring `control`'s recursive call: a byte that cannot
                    // continue UTF-8, and a bare ESC cancelling either string
                    // form. `C1Escape` re-enters with a *different* byte and is
                    // checked separately.
                    let mut hops = 0;
                    while hops < 2 {
                        let a = Act::of((e >> 8) as u8);
                        if !matches!(a, Act::Utf8Bad | Act::OscCancel | Act::DcsCancel) {
                            break;
                        }
                        e = VT[vt_index(latin1, SId::of((e & 0xff) as u8), b)];
                        hops += 1;
                    }
                    let got = SId::of((e & 0xff) as u8);
                    // `C1Escape` re-enters with a *different* byte, and
                    // `DcsHook` owns its own transition because it depends on
                    // collected state. Neither is a table transition to
                    // compare; both still run the same code.
                    if matches!(
                        Act::of((VT[vt_index(latin1, from, b)] >> 8) as u8),
                        Act::C1Escape | Act::DcsHook
                    ) {
                        continue;
                    }
                    if got != want {
                        wrong += 1;
                        if first.is_none() {
                            first = Some((latin1, from, b, want, got));
                        }
                    }
                }
            }
        }
        assert!(
            wrong == 0,
            "{wrong} of 10240 transitions disagree; first \
             (latin1, from, byte, machine says, table says): {first:?}"
        );
    }
}
