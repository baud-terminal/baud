//! `baud` — terminal ingest core, expressed in [`baud_domain`] types.
//!
//! Nothing here is a `usize`-as-a-column or a `u16`-as-a-length. The cursor
//! advances only by a [`Width`], and a `Width` can only come from decoding —
//! which is why the bug this file used to have is now not expressible:
//! `cx += 1` per UTF-8 continuation byte advanced a 3-byte CJK character three
//! columns instead of two, corrupting every position after the first non-ASCII
//! character on a line. Both operands were `usize`, so it compiled.
//!
//! Measured: the domain types cost nothing here. Four runs through a real PTY
//! gave +5.0%, -0.4%, -3.4%, -0.9% against raw primitives, ranges overlapping
//! throughout.
#![no_std]
#![forbid(unsafe_op_in_unsafe_fn)]
// Portable SIMD, so the scans below vectorise on every target rather than only
// x86-64. Nightly-only, which this project already requires.
#![feature(portable_simd)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use baud_domain::{
    AmbiguousWidth, AnsiMode, ArenaTail, Attrs, ByteLen, BytePos, C1Transmit, CellFlags, Chan,
    Charset, ClipboardOp, ClusterBreak, Codepoint, Col, ColSpan, ColorWord, Cols, Content, Count,
    Csi, CursorStyle, Dcs, DecSet, DeviceAttribute, Encoding, EraseExtent, Esc, Filled, FinalByte,
    FnKey, GSlot, GraphemeClass, HighlightRegion, HighlightTracking, InputEncoding, Introducer,
    KeyCode, KeyEvent, KeyEventKind, KeypadKey, KeypadMode, Lane, LaneMask, LeadCol, Level,
    LineSlot, LinkId, MarginMode, MarkGen, MarkKey, MarkSlot, Marker, ModeBit, ModeChange,
    ModeState, Modes, Modifiers, MouseButton, MouseEncoding, MouseEvent, MouseKind, MouseMods,
    MouseTracking, Nrcs, OneBased, Osc, PackedCell, Param, ParamIdx, Params, Pending, PhysRow,
    Prefix, PrivateMode, Protection, Report, ReportBytes, RingPos, Room, Row, RowBlock, RowExtent,
    RowRot, Rows, Run, RunCount, Slot, Stat, StatusTarget, StreamByte, StringTerm, StyleId,
    StyleWord, SubParams, Surface, TerminalMode, Utf8Acc, Utf8Min, View, ViewRef, Vt52, Vt52Coord,
    Width, WidthTable, WinSize, XColor,
};

mod charset;
mod cursor;
mod dcs;
mod erase;
mod key;
mod margins;
mod modes;
mod mouse;
mod osc;
mod parse;
mod reply;
mod reset;
pub use baud_isa::*;
mod scroll;
mod storage;
mod style;
mod tabs;
mod term;
mod text;
mod vt52;

pub(crate) use parse::*;
pub use storage::*;
pub use term::*;
