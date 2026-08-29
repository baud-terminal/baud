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
//! `the_table_sends_every_byte_where_the_machine_does` drives both over all
//! 10,240 transitions and requires the same landing state.
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

// The table and its proof land before the action dispatch that consumes them,
// so that the transition can be verified against `machine.rs` on its own. The
// switch is the next commit.
#![allow(dead_code)]

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

/// One transition: the action, and where the parser goes next.
///
/// Two `repr(u8)` enums side by side, so the entry is **two bytes** and the
/// lookup is one load -- the same size as packing them into a `u16`, and
/// without the shift, the mask and the two `as` casts each way. A packed word
/// was the first spelling and it cost four conversions per read to say what
/// the field names say for free.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub(crate) struct Step {
    pub(crate) act: Act,
    pub(crate) next: SId,
}

const fn step(act: Act, next: SId) -> Step {
    Step { act, next }
}

// ── byte classification ─────────────────────────────────────────────────────
// Everything below asks `StreamByte`. The predicates were duplicated here as
// raw-byte tests while the table was being written, which the type audit
// refused -- correctly: two spellings of one rule is how `0x7f` went missing
// from the AVX2 stop mask while the scalar tail had it. They are `const fn` on
// `StreamByte` now, so the table is built from the same definitions the rest of
// the parser uses and there is nothing to keep in sync.

/// How many continuations a lead byte owes, as a state.
const fn utf8_state(b: StreamByte) -> SId {
    match b.utf8_lead() {
        Some((3, _, _)) => SId::Utf8Three,
        Some((2, _, _)) => SId::Utf8Two,
        _ => SId::Utf8One,
    }
}

/// One byte of a control sequence's parameter string.
///
/// `csi_entry` and `csi_param` differ in exactly one transition -- the private
/// marker -- so they share this and the caller handles the difference. That is
/// how `machine.rs` spells it too, and the alternative was two arms that had
/// to be kept identical.
const fn csi_param(b: StreamByte) -> Step {
    if b.is_c0() {
        // Executed where it stands and the sequence continues: `CSI 1 <BEL> m`
        // is still an SGR.
        step(Act::Execute, SId::CsiParams)
    } else if b.is_csi_param_byte() {
        step(Act::CsiParamByte, SId::CsiParams)
    } else if FinalByte::from_stream(b).is_some() {
        step(Act::CsiFinal, SId::Ground)
    } else if b.is_csi_intermediate() {
        step(Act::CsiCollect, SId::CsiInter)
    } else if b.is_del() {
        step(Act::Nothing, SId::CsiParams)
    } else {
        // `3C-3F` past the first position. Consumed to the final byte rather
        // than returned to Ground, which printed the rest of the sequence.
        step(Act::Nothing, SId::CsiIgnore)
    }
}

/// One byte of a device control string's parameter string.
///
/// Shared by `dcs_entry` and `dcs_param` for the same reason, and differing
/// from `csi_param` in two rules: a C0 here is *ignored* rather than executed,
/// and there is no sub-parameter colon.
const fn dcs_param(b: StreamByte) -> Step {
    if b.is_c0() || b.is_del() {
        step(Act::Nothing, SId::DcsParam)
    } else if b.is_dcs_param_byte() {
        step(Act::DcsParamByte, SId::DcsParam)
    } else if b.is_csi_intermediate() {
        step(Act::DcsCollect, SId::DcsInter)
    } else if FinalByte::from_stream(b).is_some() {
        // `DcsHook` owns its own transition; see its documentation.
        step(Act::DcsHook, SId::DcsParam)
    } else {
        step(Act::Nothing, SId::DcsIgnore)
    }
}

/// The transition table, built at compile time.
///
/// Indexed `[encoding][state][byte]`, where encoding 0 is UTF-8 and 1 is
/// ISO 8859-1. 20 KB of rodata.
pub(crate) static UTF8_HALF: [Step; NSTATE * 256] = build(Half::Utf8);
pub(crate) static LATIN1_HALF: [Step; NSTATE * 256] = build(Half::Latin1);

impl Step {
    /// The transition for one byte: the whole parser, as one load.
    ///
    /// Typed at the boundary on purpose. The first spelling took
    /// `(bool, SId, u8)` and returned a `usize` index the caller then had to
    /// use correctly -- three raw values in one signature, and the audit was
    /// right to refuse it: an encoding is not a `bool` and a stream byte is
    /// not a number.
    #[inline(always)]
    pub(crate) fn of(enc: InputEncoding, st: SId, b: StreamByte) -> Step {
        // The encoding selects a table rather than an index, so the only
        // conversion left in this file is the state's row -- the one thing
        // Rust forces, since an array index is a `usize` and no newtype can
        // be one.
        let half = match enc {
            InputEncoding::Utf8 => &UTF8_HALF,
            InputEncoding::Latin1 => &LATIN1_HALF,
        };
        half[st as usize * 256 + b.index()]
    }
}

/// Which half of the table an encoding selects.
///
/// The table is built for two encodings and `InputEncoding` has exactly two
/// variants, so this is a rename -- but it is the *table's* axis rather than
/// the terminal's mode, and naming it stops `latin1: bool` from reappearing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Half {
    Utf8 = 0,
    Latin1 = 1,
}

impl Half {
    /// Latin-1 gives `80..9F` to the C1 controls and `A0..FF` to graphics;
    /// UTF-8 gives both to the decoder. That is the only rule the halves
    /// differ by, and asking it by name keeps the builder from testing a flag.
    const fn is_latin1(self) -> bool {
        matches!(self, Half::Latin1)
    }
}

const fn build(half: Half) -> [Step; NSTATE * 256] {
    let mut t = [step(Act::Nothing, SId::Ground); NSTATE * 256];
    let mut s = 0usize;
    while s < NSTATE {
        let st = SId::FROM_INDEX[s];
        let mut i = 0usize;
        while i < 256 {
            t[s * 256 + i] = one(half, st, StreamByte::ALL[i]);
            i += 1;
        }
        s += 1;
    }
    t
}

/// The whole protocol, as one function of three bounded values.
const fn one(half: Half, st: SId, b: StreamByte) -> Step {
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
    if (b.is(StreamByte::CAN) || b.is(StreamByte::SUB)) && !ground_like {
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
    if b.is(StreamByte::ESC) {
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
            if b.is(StreamByte::ESC) {
                step(Act::Nothing, SId::Escape)
            } else if b.is_c0() || b.is(StreamByte::BEL) {
                step(Act::Execute, st)
            } else if half.is_latin1() && b.is_c1() {
                step(Act::C1Escape, SId::Escape)
            } else if half.is_latin1() && b.is_latin1_graphic() {
                step(Act::PrintByte, st)
            } else if !half.is_latin1() && b.utf8_lead().is_some() {
                step(Act::Utf8Start, utf8_state(b))
            } else {
                step(Act::GroundUncommon, st)
            }
        }
        SId::Utf8One | SId::Utf8Two | SId::Utf8Three => {
            if b.utf8_continuation().is_some() {
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
            if b.is(StreamByte::CSI_INTRO) {
                step(Act::CsiClear, SId::CsiEntry)
            } else if b.is(StreamByte::OSC_INTRO) {
                step(Act::Nothing, SId::OscString)
            } else if b.is(StreamByte::DCS_INTRO) {
                step(Act::DcsClear, SId::DcsEntry)
            } else if b.is(StreamByte::SOS_INTRO)
                || b.is(StreamByte::PM_INTRO)
                || b.is(StreamByte::APC_INTRO)
            {
                step(Act::Nothing, SId::StringIgnore)
            } else if b.is_esc_intermediate() {
                step(Act::EscCollect, SId::EscInter)
            } else {
                step(Act::EscFe, SId::Ground)
            }
        }
        SId::EscInter => {
            if b.is_c0() {
                step(Act::Execute, st)
            } else if b.is_esc_intermediate() {
                step(Act::EscCollect, st)
            } else if b.is_del() {
                step(Act::Nothing, st)
            } else {
                step(Act::EscInterFinal, SId::Ground)
            }
        }
        SId::CsiEntry => {
            if b.is_c0() {
                step(Act::Execute, st)
            } else if b.is_del() {
                step(Act::Nothing, st)
            } else if Marker::from_private(b).is_some() {
                step(Act::CsiPrivate, SId::CsiParams)
            } else {
                csi_param(b)
            }
        }
        SId::CsiParams => csi_param(b),
        SId::CsiInter => {
            if b.is_c0() {
                step(Act::Execute, st)
            } else if FinalByte::from_stream(b).is_some() {
                step(Act::CsiFinal, SId::Ground)
            } else if b.is_del() {
                step(Act::Nothing, st)
            } else if !b.is_csi_intermediate() {
                step(Act::Nothing, SId::CsiIgnore)
            } else {
                step(Act::CsiCollect, st)
            }
        }
        SId::CsiIgnore => {
            if b.is_c0() {
                step(Act::Execute, st)
            } else if FinalByte::from_stream(b).is_some() {
                step(Act::Nothing, SId::Ground)
            } else {
                step(Act::Nothing, st)
            }
        }
        SId::OscString => {
            if half.is_latin1() && b.is(StreamByte::ST_8BIT) {
                step(Act::OscDispatchSt, SId::Ground)
            } else if b.is(StreamByte::BEL) {
                step(Act::OscDispatchBel, SId::Ground)
            } else if b.is(StreamByte::ESC) {
                step(Act::Nothing, SId::OscEsc)
            } else if b.is_c0() {
                step(Act::Nothing, st)
            } else {
                step(Act::StringPut, st)
            }
        }
        SId::OscEsc => {
            if b.is(StreamByte::ST_FINAL) {
                step(Act::OscDispatchSt, SId::Ground)
            } else {
                step(Act::OscCancel, SId::Escape)
            }
        }
        SId::DcsEntry => {
            if b.is_c0() || b.is_del() {
                step(Act::Nothing, st)
            } else if Marker::from_private(b).is_some() {
                step(Act::DcsPrivate, SId::DcsParam)
            } else {
                dcs_param(b)
            }
        }
        SId::DcsParam => dcs_param(b),
        SId::DcsInter => {
            if b.is_c0() || b.is_del() {
                step(Act::Nothing, st)
            } else if b.is_csi_intermediate() {
                step(Act::DcsCollect, st)
            } else if FinalByte::from_stream(b).is_some() {
                step(Act::DcsHook, st)
            } else {
                step(Act::Nothing, SId::DcsIgnore)
            }
        }
        SId::DcsPass => {
            if half.is_latin1() && b.is(StreamByte::ST_8BIT) {
                step(Act::DcsDispatch, SId::Ground)
            } else if b.is(StreamByte::ESC) {
                step(Act::Nothing, SId::DcsEsc)
            } else if b.is(StreamByte::BEL) {
                step(Act::DcsDispatch, SId::Ground)
            } else if b.is_c0() {
                step(Act::Nothing, st)
            } else {
                step(Act::StringPut, st)
            }
        }
        SId::DcsIgnore => {
            if half.is_latin1() && b.is(StreamByte::ST_8BIT) {
                step(Act::Nothing, SId::Ground)
            } else if b.is(StreamByte::ESC) {
                step(Act::Nothing, SId::Escape)
            } else if b.is(StreamByte::BEL) {
                step(Act::Nothing, SId::Ground)
            } else {
                step(Act::Nothing, st)
            }
        }
        SId::DcsEsc => {
            if b.is(StreamByte::ST_FINAL) {
                step(Act::DcsDispatch, SId::Ground)
            } else {
                step(Act::DcsCancel, SId::Escape)
            }
        }
        SId::StringIgnore => {
            // `9C` terminates only outside UTF-8, where it is not a
            // continuation byte; `BEL` terminates in either encoding. Two
            // rules, one outcome.
            if (half.is_latin1() && b.is(StreamByte::ST_8BIT)) || b.is(StreamByte::BEL) {
                step(Act::Nothing, SId::Ground)
            } else if b.is(StreamByte::ESC) {
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
        let mut first: Option<(InputEncoding, SId, u8, SId, SId)> = None;
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for si in 0..NSTATE {
                let from = SId::FROM_INDEX[si];
                // VT52 has its own escape protocol and its own ground state;
                // the table models `Escape` as ANSI, so drive ANSI here.
                for raw in 0u8..=255 {
                    let b = crate::StreamByte::from_stream(raw);
                    let mut st = Storage::boxed();
                    let mut t = Term::new(&mut st, crate::OpMode::Computed);
                    t.terminal_mode = TerminalMode::Ansi;
                    t.encoding = enc;
                    t.st = state_of(from);
                    t.control(b);
                    let want = sid_of(t.st);
                    let entry = Step::of(enc, from, b);
                    // Three actions re-enter the table with the same byte,
                    // mirroring `control`'s recursive call: a byte that cannot
                    // continue UTF-8, and a bare ESC cancelling either string
                    // form. `C1Escape` re-enters with a *different* byte and is
                    // checked separately.
                    let mut e = entry;
                    let mut hops = 0;
                    while hops < 2 {
                        if !matches!(e.act, Act::Utf8Bad | Act::OscCancel | Act::DcsCancel) {
                            break;
                        }
                        e = Step::of(enc, e.next, b);
                        hops += 1;
                    }
                    let got = e.next;
                    // `C1Escape` re-enters with a *different* byte, and
                    // `DcsHook` owns its own transition because it depends on
                    // collected state. Neither is a table transition to
                    // compare; both still run the same code.
                    if matches!(entry.act, Act::C1Escape | Act::DcsHook) {
                        continue;
                    }
                    if got != want {
                        wrong += 1;
                        if first.is_none() {
                            first = Some((enc, from, raw, want, got));
                        }
                    }
                }
            }
        }
        assert!(
            wrong == 0,
            "{wrong} of 10240 transitions disagree; first \
             (encoding, from, byte, machine says, table says): {first:?}"
        );
    }
}
