//! The surface of baud-domain that baud-core, baud-render and the presenters
//! build against. Every item below is pinned: removing or renaming it, or
//! changing a pinned signature, stops this file compiling. Every type that
//! holds a rule is listed in `WITNESSED` with the error its `compile_fail`
//! witness expects, and the tests below read those witnesses from the source,
//! so a rule cannot lose its witness and a witness cannot lose its error code.

// A pin spells an entry point's whole signature, so a change to any part of
// it stops this file compiling.
#![allow(clippy::type_complexity)]

use std::fs;
use std::path::{Path, PathBuf};

// AmbiguousWidth
const _: baud_domain::AmbiguousWidth = baud_domain::AmbiguousWidth::Narrow;
const _: baud_domain::AmbiguousWidth = baud_domain::AmbiguousWidth::Wide;
const _: fn() -> baud_domain::AmbiguousWidth = <baud_domain::AmbiguousWidth as Default>::default;
// Attrs
const _: baud_domain::Attrs = baud_domain::Attrs::NONE;
const _: baud_domain::Attrs = baud_domain::Attrs::BOLD;
const _: baud_domain::Attrs = baud_domain::Attrs::REVERSE;
const _: baud_domain::Attrs = baud_domain::Attrs::UNDERLINES;
// Autodetect
const _: baud_domain::Autodetect = baud_domain::Autodetect::On;
const _: baud_domain::Autodetect = baud_domain::Autodetect::Off;
// BandsView
const _: fn(
    &'static [baud_domain::StyleSlot],
    baud_domain::BandSlots,
) -> baud_domain::BandsView<'static> = baud_domain::BandsView::of;
const _: fn(
    &baud_domain::BandsView<'static>,
    baud_domain::Band,
) -> baud_domain::LiveTable<'static> = baud_domain::BandsView::table;
const _: fn(&baud_domain::BandsView<'static>) -> &'static [baud_domain::StyleSlot] =
    baud_domain::BandsView::slots;
const _: fn(&baud_domain::BandsView<'static>) -> baud_domain::BandSlots =
    baud_domain::BandsView::band_slots;
// BandSlots
const _: baud_domain::BandSlots = baud_domain::BandSlots(1);
const _: fn(&baud_domain::BandsView<'static>) -> baud_domain::LiveTable<'static> =
    baud_domain::BandsView::one;
// BidiDirection
const _: baud_domain::BidiDirection = baud_domain::BidiDirection::Ltr;
const _: baud_domain::BidiDirection = baud_domain::BidiDirection::Rtl;
// BoxMirror
const _: baud_domain::BoxMirror = baud_domain::BoxMirror::Off;
const _: baud_domain::BoxMirror = baud_domain::BoxMirror::On;
// BytePos
const _: baud_domain::BytePos = baud_domain::BytePos::ZERO;
const _: fn(usize) -> baud_domain::BytePos = baud_domain::BytePos::from_index;
const _: fn(baud_domain::BytePos, baud_domain::ByteLen) -> baud_domain::BytePos =
    baud_domain::BytePos::advance;
const _: fn(baud_domain::BytePos, baud_domain::ReadLen) -> baud_domain::BytePos =
    baud_domain::BytePos::advance_read;
// CellFlags
const _: baud_domain::CellFlags = baud_domain::CellFlags::NONE;
const _: baud_domain::CellFlags = baud_domain::CellFlags::GRAPHICS;
const _: baud_domain::CellFlags = baud_domain::CellFlags::PROTECTED;
const _: fn(bool) -> baud_domain::CellFlags = baud_domain::CellFlags::protection;
// Chan
const _: fn(u8) -> baud_domain::Chan = baud_domain::Chan::from_u8;
const _: fn(f32) -> baud_domain::Chan = baud_domain::Chan::from_unit;
const _: fn(u16, u16) -> baud_domain::Chan = baud_domain::Chan::from_offset;
const _: fn(baud_domain::Chan) -> u8 = baud_domain::Chan::get;
// CharacterPath
const _: baud_domain::CharacterPath = baud_domain::CharacterPath::Default;
const _: baud_domain::CharacterPath = baud_domain::CharacterPath::LeftToRight;
const _: baud_domain::CharacterPath = baud_domain::CharacterPath::RightToLeft;
const _: fn() -> baud_domain::CharacterPath = <baud_domain::CharacterPath as Default>::default;
// ClusterBreak
const _: fn() -> baud_domain::ClusterBreak = baud_domain::ClusterBreak::after_control;
const _: fn() -> baud_domain::ClusterBreak = baud_domain::ClusterBreak::after_ascii_run;
const _: fn() -> baud_domain::ClusterBreak = baud_domain::ClusterBreak::new;
const _: fn(
    baud_domain::ClusterBreak,
    baud_domain::ClusterCells,
    baud_domain::GraphemeClass,
    baud_domain::Lookback,
    baud_domain::Codepoint,
) -> bool = baud_domain::ClusterBreak::parts_at;
// ClusterCells
const _: baud_domain::ClusterCells = baud_domain::ClusterCells::Legacy;
const _: baud_domain::ClusterCells = baud_domain::ClusterCells::LegacyMarksJoined;
const _: baud_domain::ClusterCells = baud_domain::ClusterCells::Extended;
const _: fn(baud_domain::GraphemeMode, baud_domain::SpacingMarks) -> baud_domain::ClusterCells =
    baud_domain::ClusterCells::of;
// ClusterEpoch
const _: fn() -> baud_domain::ClusterEpoch = <baud_domain::ClusterEpoch as Default>::default;
const _: for<'a> fn(&'a baud_domain::ClusterStore<'static>) -> baud_domain::ClusterEpoch =
    baud_domain::ClusterStore::epoch;
// ClusterSlot
const _: usize = baud_domain::ClusterSlot::CAPACITY;
const _: baud_domain::ClusterSlot = baud_domain::ClusterSlot::EMPTY;
const _: for<'a> fn(&'a baud_domain::ClusterSlot) -> &'a [u32] = baud_domain::ClusterSlot::cps;
const _: fn(baud_domain::ClusterSlot) -> bool = baud_domain::ClusterSlot::is_empty;
// ClusterStore
const _: usize = baud_domain::ClusterStore::SLOTS_MAX;
const _: usize = baud_domain::ClusterStore::PROBES;
const _: fn(&'static mut [baud_domain::ClusterSlot]) -> baud_domain::ClusterStore<'static> =
    baud_domain::ClusterStore::new;
const _: for<'a> fn(&'a baud_domain::ClusterStore<'static>) -> bool =
    baud_domain::ClusterStore::sweep_due;
// Codepoint
const _: fn(char) -> baud_domain::Codepoint = baud_domain::Codepoint::from_char;
const _: fn(u32) -> Option<baud_domain::Codepoint> = baud_domain::Codepoint::new;
const _: fn(u32) -> baud_domain::Codepoint = baud_domain::Codepoint::decoded;
const _: baud_domain::Codepoint = baud_domain::Codepoint::REPLACEMENT;
// Col
const _: fn(usize) -> baud_domain::Col = baud_domain::Col::at_index;
const _: baud_domain::Col = baud_domain::Col::ZERO;
const _: fn(baud_domain::Col) -> usize = baud_domain::Col::index;
// ColorWord
const _: baud_domain::ColorWord = baud_domain::ColorWord::DEFAULT;
const _: fn(baud_domain::Chan, baud_domain::Chan, baud_domain::Chan) -> baud_domain::ColorWord =
    baud_domain::ColorWord::rgb;
const _: baud_domain::ColorWord = baud_domain::ColorWord::AUTOMATIC;
const _: fn(baud_domain::Chan) -> baud_domain::ColorWord = baud_domain::ColorWord::indexed;
// Cols
const _: fn(usize) -> baud_domain::Cols = baud_domain::Cols::of_index;
const _: fn(baud_domain::Cols) -> usize = baud_domain::Cols::index;
const _: fn(baud_domain::Cols, baud_domain::Col) -> baud_domain::Room = baud_domain::Cols::room_at;
const _: fn(baud_domain::Cols, usize) -> baud_domain::Cols = baud_domain::Cols::clamp;
// Content
const _: baud_domain::Content = baud_domain::Content::EMPTY;
const _: fn(baud_domain::Codepoint) -> baud_domain::Content = baud_domain::Content::codepoint;
const _: baud_domain::Content = baud_domain::Content::DEFECTIVE;
const _: baud_domain::Content = baud_domain::Content::WIDE_TAIL;
// CopyDirection
const _: baud_domain::CopyDirection = baud_domain::CopyDirection::LeftToRight;
const _: baud_domain::CopyDirection = baud_domain::CopyDirection::RightToLeft;
// Count
const _: fn(usize) -> baud_domain::Count = baud_domain::Count::of;
const _: baud_domain::Count = baud_domain::Count::ONE;
const _: baud_domain::Count = baud_domain::Count::ZERO;
const _: fn(Option<baud_domain::Param>) -> baud_domain::Count = baud_domain::Count::from_param;
// CursorChange
const _: baud_domain::CursorChange = baud_domain::CursorChange::Same;
const _: baud_domain::CursorChange = baud_domain::CursorChange::Moved;
const _: baud_domain::CursorChange = baud_domain::CursorChange::Restyled;
// CursorStyle
const _: fn(baud_domain::CursorShape, baud_domain::CursorBlink) -> baud_domain::CursorStyle =
    baud_domain::CursorStyle::of;
const _: fn(Option<baud_domain::Param>) -> Option<baud_domain::CursorPick> =
    baud_domain::CursorStyle::from_param;
const _: fn(baud_domain::CursorStyle) -> u8 = baud_domain::CursorStyle::wire;
const _: fn(baud_domain::CursorStyle, bool) -> baud_domain::CursorStyle =
    baud_domain::CursorStyle::with_blink;
const _: baud_domain::CursorStyle = baud_domain::CursorStyle::SteadyBlock;
const _: baud_domain::CursorStyle = baud_domain::CursorStyle::BlinkingBlock;
// DecimalMark
const _: baud_domain::DecimalMark = baud_domain::DecimalMark::FullStop;
const _: baud_domain::DecimalMark = baud_domain::DecimalMark::Comma;
// DecodeBudget
const _: baud_domain::DecodeBudget = baud_domain::DecodeBudget::ALL;
const _: baud_domain::DecodeBudget = baud_domain::DecodeBudget::MIB;
const _: fn(usize) -> baud_domain::DecodeBudget = baud_domain::DecodeBudget::of;
// DecodeProgress
const _: baud_domain::DecodeProgress = baud_domain::DecodeProgress::Idle;
const _: baud_domain::DecodeProgress = baud_domain::DecodeProgress::More;
// DIRTY_ROWS
const _: usize = baud_domain::DIRTY_ROWS;
// DirtyRows
const _: baud_domain::DirtyRows = baud_domain::DirtyRows::NONE;
const _: fn(baud_domain::DirtyRows, baud_domain::Row) -> Option<baud_domain::DirtyRows> =
    baud_domain::DirtyRows::with;
const _: fn(
    baud_domain::DirtyRows,
    baud_domain::Row,
    baud_domain::Row,
) -> Option<baud_domain::DirtyRows> = baud_domain::DirtyRows::span;
const _: fn(baud_domain::DirtyRows, baud_domain::DirtyRows) -> baud_domain::DirtyRows =
    baud_domain::DirtyRows::union;
const _: fn(baud_domain::DirtyRows, baud_domain::Row) -> bool = baud_domain::DirtyRows::contains;
#[allow(dead_code)]
fn dirty_rows_iter(d: baud_domain::DirtyRows) {
    let _: &mut dyn Iterator<Item = baud_domain::Row> = &mut d.iter();
}
const _: () = assert!(core::mem::size_of::<baud_domain::DirtyRows>() == 88);
// DynamicColor
const _: usize = baud_domain::DynamicColor::COUNT;
const _: fn(baud_domain::DynamicColor) -> usize = baud_domain::DynamicColor::index;
const _: fn(u16) -> Option<baud_domain::DynamicColor> = baud_domain::DynamicColor::of_code;
const _: fn(baud_domain::DynamicColor) -> u16 = baud_domain::DynamicColor::code;
const _: baud_domain::DynamicColor = baud_domain::DynamicColor::Foreground;
// Found
const _: baud_domain::Found = baud_domain::Found {
    first: baud_domain::Spot {
        line: baud_domain::LineIdx::FIRST,
        col: baud_domain::Col::ZERO,
    },
    last: baud_domain::Spot {
        line: baud_domain::LineIdx::FIRST,
        col: baud_domain::Col::ZERO,
    },
};
// FrameDamage
const _: fn(baud_domain::WholeWhy) -> baud_domain::FrameDamage = baud_domain::FrameDamage::Whole;
const _: fn(
    baud_domain::DirtyRows,
    baud_domain::CursorChange,
    baud_domain::OverlayChanges,
) -> baud_domain::FrameDamage = |rows, cursor, overlays| baud_domain::FrameDamage::Rows {
    rows,
    cursor,
    overlays,
};
const _: fn() = || {
    fn copy<T: Copy>() {}
    copy::<baud_domain::FrameDamage>();
};
// FrameNo
const _: fn(u32) -> baud_domain::FrameNo = baud_domain::FrameNo::of;
const _: baud_domain::FrameNo = baud_domain::FrameNo::ROOT;
const _: fn(baud_domain::FrameNo) -> u32 = baud_domain::FrameNo::raw;
// GraphemeClass
const _: fn(baud_domain::Codepoint) -> (baud_domain::GraphemeClass, baud_domain::Lookback) =
    baud_domain::GraphemeClass::of_with_lookback;
const _: fn(baud_domain::Codepoint) -> baud_domain::GraphemeClass = baud_domain::GraphemeClass::of;
const _: [baud_domain::GraphemeClass; 16] = baud_domain::GraphemeClass::FROM_INDEX;
const _: baud_domain::GraphemeClass = baud_domain::GraphemeClass::Other;
// HeldCover, at the span SpanSet is pinned at
const _: for<'a> fn(
    &'a baud_domain::Tracked<baud_domain::SpanSet<Lines, ()>>,
) -> baud_domain::HeldCover<Lines> = baud_domain::HeldCover::<Lines>::of::<()>;
const _: for<'a, 'b> fn(
    &'a baud_domain::HeldCover<Lines>,
    (),
    u8,
    u8,
    &'b mut baud_domain::WalkSteps,
) -> bool = baud_domain::HeldCover::<Lines>::meets;
// HistoryErasures
const _: baud_domain::HistoryErasures = baud_domain::HistoryErasures::FIRST;
const _: fn(baud_domain::HistoryErasures) -> baud_domain::HistoryErasures =
    baud_domain::HistoryErasures::next;
// HistoryWipes
const _: baud_domain::HistoryWipes = baud_domain::HistoryWipes::FIRST;
const _: fn(baud_domain::HistoryWipes) -> baud_domain::HistoryWipes =
    baud_domain::HistoryWipes::next;
// HostTime
const _: baud_domain::HostTime = baud_domain::HostTime::ZERO;
const _: fn(u64) -> baud_domain::HostTime = baud_domain::HostTime::of_nanos;
const _: fn(baud_domain::HostTime, baud_domain::HostTime) -> Option<core::time::Duration> =
    baud_domain::HostTime::since;
const _: fn(baud_domain::HostTime, core::time::Duration) -> baud_domain::HostTime =
    baud_domain::HostTime::plus;
// ImageAlpha
const _: baud_domain::ImageAlpha = baud_domain::ImageAlpha::Partial;
const _: baud_domain::ImageAlpha = baud_domain::ImageAlpha::OpaqueOrClear;
const _: for<'a> fn(&'a [baud_domain::Pixel]) -> baud_domain::ImageAlpha =
    baud_domain::ImageAlpha::of;
const _: fn(baud_domain::ImageAlpha, baud_domain::Pixel) -> baud_domain::ImageAlpha =
    baud_domain::ImageAlpha::with;
// ImageBudget
const _: baud_domain::ImageBudget = baud_domain::ImageBudget { pixels: 0 };
const _: fn(usize) -> baud_domain::ImageBudget = baud_domain::ImageBudget::of;
const _: fn(baud_domain::ImageBudget) -> usize = baud_domain::ImageBudget::scratch;
const _: fn(baud_domain::ImageBudget) -> usize = baud_domain::ImageBudget::probe;
// ImageDraw
const _: fn(u32, u32, u32, u32) -> u32 = baud_domain::ImageDraw::nearest;
const _: baud_domain::ImageDraw = baud_domain::ImageDraw {
    src: baud_domain::PxRect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
    dest: baud_domain::PxRect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
};
// ImageGen
const _: baud_domain::ImageGen = baud_domain::ImageGen::ZERO;
const _: fn(baud_domain::ImageGen) -> u32 = baud_domain::ImageGen::raw;
// ImageGround
const _: baud_domain::ImageGround = baud_domain::ImageGround::Clear;
const _: baud_domain::ImageGround = baud_domain::ImageGround::Filled {
    data_w: baud_domain::ImagePx::ZERO,
    data_h: baud_domain::ImagePx::ZERO,
};
// ImageId
const _: baud_domain::ImageId = baud_domain::ImageId::NONE;
const _: fn(baud_domain::ImageId) -> bool = baud_domain::ImageId::is_none;
// ImageMotion
const _: baud_domain::ImageMotion = baud_domain::ImageMotion::Play;
const _: baud_domain::ImageMotion = baud_domain::ImageMotion::Hold;
const _: fn(
    &mut baud_domain::ImageStore<'static>,
    baud_domain::HostTime,
    baud_domain::ImageMotion,
) -> baud_domain::Animated = baud_domain::ImageStore::animate;
// ImagePx
const _: baud_domain::ImagePx = baud_domain::ImagePx::ZERO;
const _: fn(u16) -> baud_domain::ImagePx = baud_domain::ImagePx::of;
const _: fn(baud_domain::ImagePx) -> u16 = baud_domain::ImagePx::raw;
const _: fn(baud_domain::ImagePx, usize) -> baud_domain::ImagePx = baud_domain::ImagePx::span;
// ImageRead
const _: fn() = || {
    fn read<I: baud_domain::ImageRead + Copy>() {}
    read::<&'static baud_domain::ImageStore<'static>>();
};
#[allow(dead_code)]
fn image_read<I: baud_domain::ImageRead + 'static>(i: &I) {
    let _: for<'a> fn(&'a I) -> baud_domain::ImageGen = I::generation;
    let _: for<'a> fn(
        &'a I,
        baud_domain::ImageId,
        baud_domain::ImageAge,
        baud_domain::ImageGen,
    ) -> baud_domain::ImageSince = I::image_since;
    let _: for<'a> fn(&'a I, baud_domain::ImageId) -> Option<baud_domain::ImageView<'a>> = I::image;
    let _: for<'a> fn(&'a I, baud_domain::PlacementId) -> baud_domain::KittyRoot = I::kitty_root;
    let _: for<'a> fn(
        &'a I,
        baud_domain::ImageId,
    ) -> Option<(baud_domain::KittyId, baud_domain::KittyNumber)> = I::kitty_names;
    let _: for<'a> fn(&'a I, baud_domain::ImageId) -> Option<baud_domain::ImageAge> = I::born;
    let _: &mut dyn Iterator<Item = (baud_domain::PlacementId, &baud_domain::PlacementSlot)> =
        &mut i.placements();
}
// ImageScreen
const _: baud_domain::ImageScreen = baud_domain::ImageScreen::Primary;
const _: baud_domain::ImageScreen = baud_domain::ImageScreen::Alternate;
// ImageStore
const _: fn(
    &'static mut [baud_domain::Pixel],
    &'static mut [baud_domain::ImageSlot],
    &'static mut [baud_domain::PlacementSlot],
    &'static mut [baud_domain::PlacementId],
) -> baud_domain::ImageStore<'static> = baud_domain::ImageStore::new;
const _: fn(&baud_domain::ImageStore<'static>) -> bool = baud_domain::ImageStore::any_placed;
const _: fn(
    &baud_domain::ImageStore<'static>,
    baud_domain::ImageId,
    baud_domain::ImagePx,
    baud_domain::ImagePx,
) -> bool = baud_domain::ImageStore::is_page;
const _: fn(&mut baud_domain::ImageStore<'static>, baud_domain::ImageId) =
    baud_domain::ImageStore::free;
// IntervalMap, at LineIdx and u8
const _: for<'a, 'b> fn(
    &'a mut baud_domain::IntervalMap<baud_domain::LineIdx, u8>,
    baud_domain::LineInterval<baud_domain::LineIdx>,
    u8,
    &'b mut baud_domain::WalkSteps,
) = baud_domain::IntervalMap::<baud_domain::LineIdx, u8>::assign;
const _: for<'a, 'b> fn(
    &'a mut baud_domain::IntervalMap<baud_domain::LineIdx, u8>,
    baud_domain::LineInterval<baud_domain::LineIdx>,
    &'b mut baud_domain::WalkSteps,
) = baud_domain::IntervalMap::<baud_domain::LineIdx, u8>::erase;
fn _gaps_of(
    m: &baud_domain::IntervalMap<baud_domain::LineIdx, u8>,
    span: baud_domain::LineInterval<baud_domain::LineIdx>,
    out: &mut Vec<baud_domain::LineInterval<baud_domain::LineIdx>>,
    steps: &mut baud_domain::WalkSteps,
) {
    m.gaps_of(1, span, out, steps)
}
fn _remap(
    m: &mut baud_domain::IntervalMap<baud_domain::LineIdx, u8>,
    steps: &mut baud_domain::WalkSteps,
) -> baud_domain::OverlapRuns {
    m.remap(|i| i, baud_domain::RemapOverlap::Neither, steps)
}
// KeyCode
const _: fn(baud_domain::KeypadKey) -> baud_domain::KeyCode = baud_domain::KeyCode::Keypad;
const _: fn(baud_domain::Codepoint) -> baud_domain::KeyCode = baud_domain::KeyCode::Char;
const _: baud_domain::KeyCode = baud_domain::KeyCode::Right;
const _: baud_domain::KeyCode = baud_domain::KeyCode::Left;
// KeyEvent
const _: fn(baud_domain::KeyCode, baud_domain::Modifiers) -> baud_domain::KeyEvent =
    baud_domain::KeyEvent::press;
const _: fn(
    baud_domain::KeyEvent,
) -> (
    baud_domain::KeyCode,
    baud_domain::Modifiers,
    baud_domain::KeyEventKind,
    Option<baud_domain::Codepoint>,
) = |e| (e.code, e.mods, e.kind, e.text);
const _: fn(
    baud_domain::KeyEvent,
) -> (
    Option<baud_domain::Codepoint>,
    Option<baud_domain::Codepoint>,
    Option<baud_domain::Codepoint>,
    Option<baud_domain::Codepoint>,
) = |e| (e.chord_text, e.shifted, e.base_layout, e.control);
// KittyId
const _: baud_domain::KittyId = baud_domain::KittyId::NONE;
const _: fn(u32) -> baud_domain::KittyId = baud_domain::KittyId::of;
const _: fn(baud_domain::KittyId) -> u32 = baud_domain::KittyId::raw;
const _: fn(baud_domain::KittyId) -> bool = baud_domain::KittyId::is_none;
// LineBidi
const _: baud_domain::LineBidi = baud_domain::LineBidi::POWER_ON;
const _: fn(baud_domain::LineBidi) -> baud_domain::BidiParagraph = baud_domain::LineBidi::paragraph;
const _: fn(baud_domain::LineBidi, baud_domain::CharacterPath) -> baud_domain::LineBidi =
    baud_domain::LineBidi::with_path;
const _: fn(baud_domain::LineBidi) -> baud_domain::CharacterPath = baud_domain::LineBidi::path;
const _: () = assert!(core::mem::size_of::<baud_domain::LineBidi>() == 1);
// LineIdx
const _: baud_domain::LineIdx = baud_domain::LineIdx::FIRST;
const _: fn(u64) -> baud_domain::LineIdx = baud_domain::LineIdx::of;
const _: fn(baud_domain::LineIdx) -> u64 = baud_domain::LineIdx::value;
const _: fn(baud_domain::LineIdx, baud_domain::LineIdx) -> Option<baud_domain::Rows> =
    baud_domain::LineIdx::since;
// LineInterval, at LineIdx
const _: fn(
    baud_domain::LineIdx,
    baud_domain::LineIdx,
) -> baud_domain::LineInterval<baud_domain::LineIdx> =
    baud_domain::LineInterval::<baud_domain::LineIdx>::new;
const _: fn(baud_domain::LineIdx) -> baud_domain::LineInterval<baud_domain::LineIdx> =
    baud_domain::LineInterval::<baud_domain::LineIdx>::one;
// LineRendition
const _: baud_domain::LineRendition = baud_domain::LineRendition::Single;
const _: baud_domain::LineRendition = baud_domain::LineRendition::DoubleBottom;
const _: fn(baud_domain::LineRendition) -> bool = baud_domain::LineRendition::is_double;
const _: fn(baud_domain::LineRendition, baud_domain::Cols) -> baud_domain::Cols =
    baud_domain::LineRendition::end_of_line;
// LiveTable
const _: fn(&'static [baud_domain::StyleSlot]) -> baud_domain::LiveTable<'static> =
    baud_domain::LiveTable::of;
const _: fn(&baud_domain::LiveTable<'static>, baud_domain::StyleId) -> baud_domain::StyleWord =
    baud_domain::LiveTable::word;
const _: fn(&baud_domain::LiveTable<'static>, baud_domain::PackedCell) -> baud_domain::StyleWord =
    baud_domain::LiveTable::word_of;
const _: fn(&baud_domain::LiveTable<'static>) -> &'static [baud_domain::StyleSlot] =
    baud_domain::LiveTable::slots;
// MAX_ROWS
const _: usize = baud_domain::MAX_ROWS;
// Mirroring
const _: baud_domain::Mirroring = baud_domain::Mirroring::Mirrored;
const _: baud_domain::Mirroring = baud_domain::Mirroring::Cancelled;
// Modifiers
const _: baud_domain::Modifiers = baud_domain::Modifiers::SHIFT;
const _: fn(baud_domain::Modifiers, baud_domain::Modifiers) -> baud_domain::Modifiers =
    baud_domain::Modifiers::with;
const _: fn(baud_domain::Modifiers) -> u8 = baud_domain::Modifiers::wire;
const _: fn(u8) -> Option<baud_domain::Modifiers> = baud_domain::Modifiers::from_wire;
// OverlayChanges
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::NONE;
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::SELECTION;
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::FIND;
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::HOVER;
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::SCROLLBAR;
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::IME;
const _: baud_domain::OverlayChanges = baud_domain::OverlayChanges::SPLASH;
const _: fn(
    baud_domain::OverlayChanges,
    baud_domain::OverlayChanges,
) -> baud_domain::OverlayChanges = baud_domain::OverlayChanges::union;
const _: fn(baud_domain::OverlayChanges, baud_domain::OverlayChanges) -> bool =
    baud_domain::OverlayChanges::contains;
// PackedCell
const _: baud_domain::PackedCell = baud_domain::PackedCell::EMPTY;
const _: fn(
    baud_domain::Codepoint,
    baud_domain::StyleId,
    baud_domain::CellFlags,
) -> baud_domain::PackedCell = baud_domain::PackedCell::glyph;
const _: fn(baud_domain::StyleId) -> baud_domain::PackedCell = baud_domain::PackedCell::empty_with;
const _: fn(baud_domain::StreamByte, baud_domain::Charset) -> baud_domain::Codepoint =
    baud_domain::PackedCell::translate;
const _: () = assert!(core::mem::size_of::<baud_domain::PackedCell>() == 8);
// PaintRole
const _: baud_domain::PaintRole = baud_domain::PaintRole::Cursor;
const _: baud_domain::PaintRole = baud_domain::PaintRole::CursorText;
const _: baud_domain::PaintRole = baud_domain::PaintRole::SelectionBackground;
const _: baud_domain::PaintRole = baud_domain::PaintRole::SelectionForeground;
const _: baud_domain::PaintRole = baud_domain::PaintRole::VisualBell;
// PastEdge
const _: baud_domain::PastEdge = baud_domain::PastEdge::Held;
const _: baud_domain::PastEdge = baud_domain::PastEdge::Clear;
// Pixel
const _: fn(baud_domain::Chan, baud_domain::Chan, baud_domain::Chan) -> baud_domain::Pixel =
    baud_domain::Pixel::opaque;
const _: baud_domain::Pixel = baud_domain::Pixel::CLEAR;
const _: baud_domain::Pixel = baud_domain::Pixel::FILL;
const _: baud_domain::Pixel = baud_domain::Pixel::UNSAID;
const _: () = assert!(core::mem::size_of::<baud_domain::Pixel>() == 4);
// ProgramPaint
const _: baud_domain::ProgramPaint = baud_domain::ProgramPaint::Unset;
const _: fn((u8, u8, u8)) -> baud_domain::ProgramPaint = baud_domain::ProgramPaint::Colour;
const _: baud_domain::ProgramPaint = baud_domain::ProgramPaint::Automatic;
// PromptLine
const _: fn(baud_domain::Rows) -> baud_domain::PromptLine = baud_domain::PromptLine::Back;
const _: fn(baud_domain::Row) -> baud_domain::PromptLine = baud_domain::PromptLine::Screen;
// PromptMark
const _: baud_domain::PromptMark = baud_domain::PromptMark {
    line: baud_domain::PromptLine::Screen(baud_domain::Row::ZERO),
    status: baud_domain::ExitStatus::Unreported,
};
// ReadLen
const _: baud_domain::ReadLen = baud_domain::ReadLen::ONE;
const _: baud_domain::ReadLen = baud_domain::ReadLen::ZERO;
const _: fn(usize) -> baud_domain::ReadLen = baud_domain::ReadLen::from_index;
const _: fn(baud_domain::ByteLen) -> baud_domain::ReadLen = baud_domain::ReadLen::of_bytes;
// Reclaims
const _: baud_domain::Reclaims = baud_domain::Reclaims {
    refusals: 0,
    sweeps: 0,
    freed: 0,
    rewrites: 0,
};
const _: fn(baud_domain::Reclaims, baud_domain::Reclaims) -> bool =
    baud_domain::Reclaims::recolours_since;
// RemapOverlap
const _: baud_domain::RemapOverlap = baud_domain::RemapOverlap::Neither;
// Resets
const _: baud_domain::Resets = baud_domain::Resets::FIRST;
const _: fn(baud_domain::Resets) -> baud_domain::Resets = baud_domain::Resets::next;
// Row
const _: baud_domain::Row = baud_domain::Row::ZERO;
const _: fn(usize) -> baud_domain::Row = baud_domain::Row::from_index;
const _: fn(baud_domain::Row) -> usize = baud_domain::Row::index;
const _: fn(baud_domain::Row, baud_domain::Count, baud_domain::Rows) -> baud_domain::Row =
    baud_domain::Row::down;
// RowAt
const _: baud_domain::RowAt = baud_domain::RowAt {
    screen: baud_domain::Which::Primary,
    p: baud_domain::RowRot::NONE,
};
const _: fn(&baud_domain::WindowAt, baud_domain::Row) -> baud_domain::RowAt =
    baud_domain::WindowAt::at;
// RowExtent
const _: baud_domain::RowExtent = baud_domain::RowExtent::ZERO;
const _: fn(usize) -> baud_domain::RowExtent = baud_domain::RowExtent::of;
const _: usize = baud_domain::RowExtent::MAX_MARK;
const _: fn(baud_domain::RowExtent, usize) -> baud_domain::RowExtent =
    baud_domain::RowExtent::reached;
const _: () = assert!(core::mem::size_of::<baud_domain::RowExtent>() == 8);
// RowGens
const _: fn(&baud_domain::RowGens<'static>, baud_domain::Row, &baud_domain::Seal) -> bool =
    baud_domain::RowGens::after;
const _: fn(&baud_domain::RowGens<'static>) -> baud_domain::WindowAt = baud_domain::RowGens::window;
const _: for<'a> fn(
    &'a baud_domain::SurfaceStore<'static>,
    baud_domain::WindowAt,
) -> baud_domain::RowGens<'a> = baud_domain::SurfaceStore::row_gens;
// RowRot
const _: baud_domain::RowRot = baud_domain::RowRot::NONE;
const _: fn(baud_domain::RowRot, usize, baud_domain::Rows) -> baud_domain::RowRot =
    baud_domain::RowRot::advance;
const _: fn(baud_domain::RowRot, usize, baud_domain::Rows) -> baud_domain::RowRot =
    baud_domain::RowRot::retreat;
const _: fn(baud_domain::RowRot) -> usize = baud_domain::RowRot::index;
// RowStyles
const _: fn(baud_domain::LiveTable<'static>) -> baud_domain::RowStyles<'static> =
    baud_domain::RowStyles::Live;
const _: fn(baud_domain::FrozenTable<'static>) -> baud_domain::RowStyles<'static> =
    baud_domain::RowStyles::Frozen;
const _: fn(&baud_domain::RowStyles<'static>, usize) -> usize = baud_domain::RowStyles::base;
const _: fn(&baud_domain::RowStyles<'static>, baud_domain::StyleId) -> baud_domain::StyleWord =
    baud_domain::RowStyles::word;
// RowWritten
const _: baud_domain::RowWritten = baud_domain::RowWritten::InOrder;
const _: baud_domain::RowWritten = baud_domain::RowWritten::RightToLeft;
// Rows
const _: fn(usize) -> baud_domain::Rows = baud_domain::Rows::of_count;
const _: fn(usize) -> baud_domain::Rows = baud_domain::Rows::of_index;
const _: baud_domain::Rows = baud_domain::Rows::ZERO;
const _: fn(baud_domain::Rows) -> usize = baud_domain::Rows::index;
// STRING_DEPTH
const _: usize = baud_domain::STRING_DEPTH;
const _: () = assert!(baud_domain::STRING_DEPTH == 125);
// Screen
const _: baud_domain::Screen = baud_domain::Screen::Normal;
const _: baud_domain::Screen = baud_domain::Screen::Alternate;
// ScreenEpoch
const _: baud_domain::ScreenEpoch = baud_domain::ScreenEpoch::FIRST;
const _: fn(baud_domain::ScreenEpoch) -> baud_domain::ScreenEpoch = baud_domain::ScreenEpoch::next;
// Seal
const _: fn(&mut baud_domain::SurfaceStore<'static>) -> baud_domain::Seal =
    baud_domain::SurfaceStore::seal;
const _: fn(&baud_domain::SurfaceStore<'static>) -> baud_domain::Seal =
    baud_domain::SurfaceStore::sealed;
const _: fn(&baud_domain::SurfaceStore<'static>, &baud_domain::Seal) -> bool =
    baud_domain::SurfaceStore::wrote_since;
// SearchLines
const _: baud_domain::SearchLines = baud_domain::SearchLines {
    oldest: baud_domain::LineIdx::FIRST,
    top: baud_domain::LineIdx::FIRST,
    end: baud_domain::LineIdx::FIRST,
};
// Sixel
const _: fn(
    baud_domain::InputEncoding,
    baud_domain::StreamByte,
) -> Option<baud_domain::StreamByte> = baud_domain::Sixel::body_byte;
const _: fn(baud_domain::StreamByte) -> bool = baud_domain::Sixel::body_byte_is_gl_past_space;
const _: fn(&[u8], baud_domain::SixelAspect) -> baud_domain::SixelReport =
    baud_domain::Sixel::extent;
const _: fn(
    &[u8],
    baud_domain::SixelAspect,
    baud_domain::Pixel,
    &mut baud_domain::SixelCanvas<'_>,
) -> baud_domain::SixelReport = baud_domain::Sixel::decode_inked;
// SixelAspect
const _: fn(Option<u16>) -> baud_domain::SixelAspect = baud_domain::SixelAspect::from_macro;
const _: baud_domain::SixelAspect = baud_domain::SixelAspect::ONE_TO_ONE;
const _: fn(baud_domain::SixelAspect, u32) -> u32 = baud_domain::SixelAspect::top;
// SixelCanvas
const _: fn(
    &'static mut [baud_domain::Pixel],
    baud_domain::SixelPx,
) -> baud_domain::SixelCanvas<'static> = baud_domain::SixelCanvas::new;
const _: fn(
    &'static mut [baud_domain::Pixel],
    baud_domain::SixelPx,
    u16,
) -> baud_domain::SixelCanvas<'static> = baud_domain::SixelCanvas::shrunk;
// SixelClaim
const _: baud_domain::SixelClaim = baud_domain::SixelClaim::No;
const _: baud_domain::SixelClaim = baud_domain::SixelClaim::Yes;
// SixelPx
const _: baud_domain::SixelPx = baud_domain::SixelPx::ZERO;
const _: fn(u16) -> baud_domain::SixelPx = baud_domain::SixelPx::of;
const _: fn(baud_domain::SixelPx) -> usize = baud_domain::SixelPx::index;
const _: fn(baud_domain::SixelPx) -> u16 = baud_domain::SixelPx::raw;
// Slant
const _: baud_domain::Slant = baud_domain::Slant::AsFace;
const _: baud_domain::Slant = baud_domain::Slant::ByDirection;
// SlotSet
const _: fn() -> baud_domain::SlotSet = baud_domain::SlotSet::new;
const _: fn(&mut baud_domain::SlotSet, baud_domain::StyleId) = baud_domain::SlotSet::insert;
const _: fn(&mut baud_domain::SlotSet, &[baud_domain::PackedCell]) = baud_domain::SlotSet::mark;
const _: fn(&baud_domain::SlotSet, usize) -> bool = baud_domain::SlotSet::has;
const _: () = assert!(core::mem::size_of::<baud_domain::SlotSet>() == 8192);
// SpacingMarks
const _: baud_domain::SpacingMarks = baud_domain::SpacingMarks::Column;
const _: baud_domain::SpacingMarks = baud_domain::SpacingMarks::Joined;
// SpanSet, at a span of this file's
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Lines(u8, u8);
impl baud_domain::Spanned for Lines {
    type Lane = ();
    type Line = u8;
    fn lane(self) {}
    fn first(self) -> u8 {
        self.0
    }
    fn last(self) -> u8 {
        self.1
    }
}
const _: for<'a, 'b> fn(
    &'a mut baud_domain::SpanSet<Lines, ()>,
    Lines,
    (),
    &'b mut baud_domain::WalkSteps,
) -> baud_domain::Inserted = baud_domain::SpanSet::<Lines, ()>::insert;
const _: for<'a, 'b, 'c> fn(
    &'a mut baud_domain::SpanSet<Lines, ()>,
    &'b mut Vec<(Lines, ())>,
    &'c mut baud_domain::WalkSteps,
) = baud_domain::SpanSet::<Lines, ()>::extend_sorted;
const _: for<'a, 'b, 'c> fn(
    &'a baud_domain::SpanSet<Lines, ()>,
    &'b baud_domain::SpanSet<Lines, ()>,
    &'c mut baud_domain::WalkSteps,
) -> bool = baud_domain::SpanSet::<Lines, ()>::same;
const _: for<'a> fn(&'a baud_domain::SpanSet<Lines, ()>) -> baud_domain::SpanCount =
    baud_domain::SpanSet::<Lines, ()>::len;
fn _keys_of(s: &baud_domain::SpanSet<Lines, ()>, x: Lines) -> impl Iterator<Item = ()> + '_ {
    s.keys_of(x)
}
// Spot
const _: baud_domain::Spot = baud_domain::Spot {
    line: baud_domain::LineIdx::FIRST,
    col: baud_domain::Col::ZERO,
};
// StatusGen
const _: fn(&baud_domain::StatusGen, &baud_domain::Seal) -> bool = baud_domain::StatusGen::after;
// StatusLine
const _: fn(baud_domain::LiveTable<'static>) -> baud_domain::StatusLine<'static> =
    |styles| baud_domain::StatusLine {
        cells: &[],
        rendition: baud_domain::LineRendition::Single,
        cursor: None,
        styles,
    };
// Stride
const _: fn(usize) -> baud_domain::Stride = baud_domain::Stride::of_cells;
const _: fn(baud_domain::Stride) -> usize = baud_domain::Stride::index;
const _: fn(baud_domain::Stride) -> usize = baud_domain::Stride::bytes;
// StringId
const _: baud_domain::StringId = baud_domain::StringId::NONE;
const _: () = assert!(baud_domain::StringId::CAPACITY == 255);
const _: fn(usize) -> Option<baud_domain::StringId> = baud_domain::StringId::from_index;
const _: fn(baud_domain::StringId) -> usize = baud_domain::StringId::index;
const _: () = assert!(core::mem::size_of::<baud_domain::StringId>() == 1);
// StyleId
const _: fn(usize) -> baud_domain::StyleId = baud_domain::StyleId::from_index;
const _: baud_domain::StyleId = baud_domain::StyleId::DEFAULT;
const _: fn(baud_domain::StyleWord, usize) -> usize = baud_domain::StyleId::bucket;
const _: fn(baud_domain::StyleId) -> usize = baud_domain::StyleId::index;
const _: () = assert!(core::mem::size_of::<baud_domain::StyleId>() == 2);
// StyleWord
const _: baud_domain::StyleWord = baud_domain::StyleWord::DEFAULT;
const _: baud_domain::StyleWord = baud_domain::StyleWord::TOMBSTONE;
const _: fn(baud_domain::StyleWord) -> baud_domain::StringId = baud_domain::StyleWord::string;
const _: fn(baud_domain::StyleWord) -> u64 = baud_domain::StyleWord::key;
const _: () = assert!(core::mem::size_of::<baud_domain::StyleWord>() == 20);
// StyleWrites
const _: baud_domain::StyleWrites = baud_domain::StyleWrites::NEVER;
const _: fn(baud_domain::StyleWrites) -> baud_domain::StyleWrites = baud_domain::StyleWrites::next;
const _: fn(baud_domain::StyleWrites, baud_domain::StyleWrites, usize) -> baud_domain::Replay =
    baud_domain::StyleWrites::replay;
const _: fn(baud_domain::StyleWrites, usize) -> bool = baud_domain::StyleWrites::past;
// SurfaceLine
const _: fn(baud_domain::Rows) -> baud_domain::SurfaceLine = baud_domain::SurfaceLine::Above;
const _: fn(baud_domain::Row) -> baud_domain::SurfaceLine = baud_domain::SurfaceLine::At;
// TermRead
#[allow(dead_code)]
fn term_read<T: baud_domain::TermRead + 'static>() {
    use baud_domain::*;
    // geometry and cursor
    let _: for<'a> fn(&'a T) -> (Cols, Rows) = T::geometry;
    let _: for<'a> fn(&'a T) -> Stride = T::stride;
    let _: for<'a> fn(&'a T) -> (Row, Col) = T::cursor;
    let _: for<'a> fn(&'a T) -> bool = T::cursor_visible;
    let _: for<'a> fn(&'a T) -> CursorStyle = T::cursor_style;
    let _: for<'a> fn(&'a T) -> WrapPending = T::wrap_pending;
    let _: for<'a> fn(&'a T) -> Rows = T::window_back;
    let _: for<'a> fn(&'a T, Rows) -> Rows = T::looking_back;
    let _: for<'a> fn(&'a T) -> Rows = T::status_rows;
    // rows and views
    let _: for<'a> fn(&'a T, Row) -> Option<&'a [PackedCell]> = T::row;
    let _: for<'a> fn(&'a T, Rows) -> Viewport<'a> = T::view;
    let _: for<'a> fn(&'a T) -> Viewport<'a> = T::viewport;
    let _: for<'a> fn(&'a T, Rows) -> RowGens<'a> = T::row_gens;
    let _: for<'a> fn(&'a T) -> Option<Viewport<'a>> = T::status_view;
    let _: for<'a> fn(&'a T) -> Option<StatusLine<'a>> = T::status_line;
    let _: for<'a> fn(&'a T) -> Option<StatusGen> = T::status_gen;
    let _: for<'a> fn(&'a T, Rows, Row) -> BidiDirection = T::line_direction;
    let _: for<'a> fn(&'a T, Rows) -> ImageOrigin = T::image_origin;
    // change signals
    let _: for<'a> fn(&'a T) -> Seal = T::sealed;
    let _: for<'a, 'k> fn(&'a T, &'k Seal) -> bool = T::wrote_since;
    let _: for<'a> fn(&'a T) -> Screen = T::screen;
    let _: for<'a> fn(&'a T) -> ScreenEpoch = T::screen_epoch;
    let _: for<'a> fn(&'a T) -> Resets = T::resets;
    let _: for<'a> fn(&'a T) -> HistoryErasures = T::history_erasures;
    let _: for<'a> fn(&'a T) -> HistoryWipes = T::history_wipes;
    let _: for<'a> fn(&'a T) -> ClusterEpoch = T::cluster_epoch;
    let _: for<'a> fn(&'a T) -> ColorEpoch = T::color_epoch;
    let _: for<'a> fn(&'a T) -> StyleWrites = T::style_writes;
    let _: for<'a> fn(&'a T) -> TableSerial = T::style_table;
    let _: for<'a> fn(&'a T, StyleWrites) -> Written<'a> = T::styles_written;
    let _: for<'a> fn(&'a T) -> Option<Rows> = T::band_rows;
    let _: for<'a> fn(&'a T, Band) -> TableSerial = T::band_style_table;
    let _: for<'a> fn(&'a T, Band) -> StyleWrites = T::band_style_writes;
    let _: for<'a> fn(&'a T, Band, StyleWrites) -> Written<'a> = T::band_styles_written;
    // renditions and colours
    let _: for<'a> fn(&'a T) -> BandsView<'a> = T::styles;
    let _: for<'a, 's> fn(&'a T, RowStyles<'s>, PackedCell) -> StyleWord = T::style_in;
    let _: for<'a> fn(&'a T) -> Reclaims = T::style_reclaims;
    let _: for<'a> fn(&'a T, Band) -> Reclaims = T::band_style_reclaims;
    let _: for<'a> fn(&'a T, Chan) -> Option<(u8, u8, u8)> = T::palette;
    let _: for<'a> fn(&'a T, DynamicColor) -> Option<(u8, u8, u8)> = T::dynamic_color;
    let _: for<'a> fn(&'a T, SpecialColor) -> Option<(u8, u8, u8)> = T::special_color;
    let _: for<'a> fn(&'a T, Video) -> Option<(u8, u8, u8)> = T::cursor_color_in;
    let _: for<'a> fn(&'a T, PaintRole) -> ProgramPaint = T::program_paint;
    let _: for<'a> fn(&'a T) -> ColorModes = T::color_modes;
    let _: for<'a> fn(&'a T) -> bool = T::reverse_video;
    // text and clusters
    let _: for<'a> fn(&'a T, PackedCell) -> Content = T::base_of;
    let _: for<'a> fn(&'a T) -> ClusterCells = T::cluster_cells;
    let _: for<'a> fn(&'a T) -> AmbiguousWidth = T::ambiguous_width;
    let _: for<'a> fn(&'a T) -> UnicodeWidths = T::unicode_widths;
    let _: for<'a> fn(&'a T) -> Vs16Width = T::vs16_width;
    let _: for<'a> fn(&'a T) -> bool = T::variants_seen;
    let _: for<'a> fn(&'a T) -> BidiSupport = T::bidi_support;
    let _: for<'a> fn(&'a T) -> CopyDirection = T::copy_direction;
    let _: for<'a> fn(&'a T, StringId) -> Option<StringEntry> = T::string_entry;
    let _: for<'a> fn(&'a T) -> bool = T::strings_seen;
    // search and prompts
    let _: for<'a> fn(&'a T) -> SearchLines = T::search_lines;
    let _: for<'a> fn(&'a T, LineIdx) -> LineIdx = T::line_start;
    let _: for<'a> fn(&'a T, LineIdx) -> Option<SurfaceLine> = T::surface_line;
    let _: for<'a> fn(&'a T) -> Option<Row> = T::prompt_row;
    let _: for<'a> fn(&'a T) -> Zone = T::primary_zone;
    // images
    let _: for<'a> fn(&'a T) -> T::Images<'a> = T::images;
    let _: for<'a> fn(&'a T, KittyId, KittyPlacement) -> Option<KittyVirtual> = T::kitty_virtual;
    let _: for<'a> fn(&'a T) -> bool = T::kitty_virtuals;
    // present planning
    let _: for<'a> fn(&'a T) -> Option<CursorGuide> = T::cursor_guide;
    let _: for<'a> fn(&'a T) -> Option<Highlighted> = T::highlight;
    let _: for<'a> fn(&'a T) -> RegisBlink = T::regis_blink;
}
/// The four reads that return an iterator and the one that takes a
/// closure: their types cannot be spelled as a fn pointer, so each is
/// pinned by a call whose result must have the pinned item.
#[allow(dead_code)]
fn term_read_iterators<T: baud_domain::TermRead>(
    t: &T,
    cell: baud_domain::PackedCell,
    styles: baud_domain::RowStyles<'_>,
    needle: &baud_domain::Needle,
    lines: (baud_domain::LineIdx, baud_domain::LineIdx),
) {
    fn runs<I: Iterator<Item = baud_domain::PlaceholderCell>>(_: baud_domain::PlaceholderRuns<I>) {}
    let _: &mut dyn Iterator<Item = u32> = &mut t.cluster_marks(cell);
    let _: &mut dyn Iterator<Item = baud_domain::PromptMark> = &mut t.prompts();
    let _: &mut dyn Iterator<Item = baud_domain::PromptMark> =
        &mut t.prompts_from(baud_domain::Rows::ZERO);
    runs(t.placeholder_runs(&[], styles));
    t.find(needle, lines.0, lines.1, |_: baud_domain::Found| {});
    fn images<I: baud_domain::ImageRead + Copy>(_: I) {}
    images(t.images());
}
// TextMoved
const _: fn(baud_domain::Rows) -> baud_domain::TextMoved = baud_domain::TextMoved::Up;
const _: fn(baud_domain::Rows) -> baud_domain::TextMoved = baud_domain::TextMoved::Down;
// Tracked
const _: fn(u8) -> baud_domain::Tracked<u8> = baud_domain::Tracked::<u8>::new;
const _: for<'a> fn(&'a baud_domain::Tracked<u8>) -> &'a u8 = baud_domain::Tracked::<u8>::get;
const _: for<'a> fn(&'a baud_domain::Tracked<u8>) -> baud_domain::PendingGen =
    baud_domain::Tracked::<u8>::generation;
const _: for<'a> fn(&'a mut baud_domain::Tracked<u8>) -> &'a mut u8 =
    baud_domain::Tracked::<u8>::edit;
// UnicodeWidths
const _: baud_domain::UnicodeWidths = baud_domain::UnicodeWidths::Current;
const _: baud_domain::UnicodeWidths = baud_domain::UnicodeWidths::Eight;
const _: fn(baud_domain::UnicodeWidths) -> u8 = baud_domain::UnicodeWidths::version;
// Variants
const _: baud_domain::Variants = baud_domain::Variants::DEFAULT;
const _: baud_domain::Variants = baud_domain::Variants::TOMBSTONE;
const _: fn(baud_domain::Variants) -> bool = baud_domain::Variants::is_default;
const _: fn(baud_domain::Variants) -> Option<baud_domain::Form> = baud_domain::Variants::form;
const _: () = assert!(core::mem::size_of::<baud_domain::Variants>() == 2);
// Video
const _: baud_domain::Video = baud_domain::Video::Normal;
const _: baud_domain::Video = baud_domain::Video::Reverse;
const _: fn(
    baud_domain::ColorWord,
    baud_domain::ColorWord,
    baud_domain::ColorWord,
) -> baud_domain::ColorWord = baud_domain::Video::cursor_across;
const _: fn(baud_domain::Video, baud_domain::Attrs, u32, u32) -> (u32, u32) =
    baud_domain::Video::drawn::<u32>;
// Vowels
const _: baud_domain::Vowels = baud_domain::Vowels::Marked;
const _: baud_domain::Vowels = baud_domain::Vowels::Hidden;
// Vs16Width
const _: baud_domain::Vs16Width = baud_domain::Vs16Width::Two;
const _: baud_domain::Vs16Width = baud_domain::Vs16Width::One;
const _: fn(baud_domain::Vs16Width, baud_domain::ClusterCells) -> bool =
    baud_domain::Vs16Width::widens_in;
// WalkSteps
const _: baud_domain::WalkSteps = baud_domain::WalkSteps::ZERO;
const _: fn(baud_domain::WalkSteps) -> u64 = baud_domain::WalkSteps::get;
// WatchedLines
const _: baud_domain::WatchedLines = baud_domain::WatchedLines {
    first: baud_domain::SurfaceLine::At(baud_domain::Row::ZERO),
    last: baud_domain::SurfaceLine::At(baud_domain::Row::ZERO),
    moved: baud_domain::TextMoved::Up(baud_domain::Rows::ZERO),
};
// WholeWhy
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Resized;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Screen;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Reset;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Erased;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Wiped;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Colours;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Clusters;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Renditions;
const _: baud_domain::WholeWhy = baud_domain::WholeWhy::Overflow;
// Width
const _: baud_domain::Width = baud_domain::Width::WIDE;
const _: baud_domain::Width = baud_domain::Width::NARROW;
const _: baud_domain::Width = baud_domain::Width::ZERO;
const _: fn(baud_domain::Codepoint, baud_domain::AmbiguousWidth) -> baud_domain::Width =
    baud_domain::Width::of;
// WinSize
const _: baud_domain::WinSize = baud_domain::WinSize {
    ws_row: 24,
    ws_col: 80,
};
const _: fn(&baud_domain::WinSize) -> (baud_domain::Cols, baud_domain::Rows) =
    baud_domain::WinSize::geometry;
// WindowAt
const _: for<'a> fn(&'a baud_domain::WindowAt, baud_domain::Row) -> baud_domain::RowAt =
    baud_domain::WindowAt::at;
const _: fn(
    baud_domain::Which,
    baud_domain::RowRot,
    baud_domain::Rows,
    baud_domain::Rows,
) -> baud_domain::WindowAt = |screen, top, ring, rows| baud_domain::WindowAt {
    screen,
    top,
    ring,
    rows,
};
// Wrap
const _: baud_domain::Wrap = baud_domain::Wrap::Continues;
const _: baud_domain::Wrap = baud_domain::Wrap::Ended;
// WrapMargins
const _: fn(baud_domain::Col, baud_domain::Col) -> baud_domain::WrapMargins =
    baud_domain::WrapMargins::of;
const _: fn(baud_domain::WrapMargins) -> baud_domain::Col = baud_domain::WrapMargins::left;
const _: fn(baud_domain::WrapMargins) -> baud_domain::Col = baud_domain::WrapMargins::right;
const _: baud_domain::WrapMargins = baud_domain::WrapMargins::NONE;
const _: () = assert!(core::mem::size_of::<baud_domain::WrapMargins>() == 4);
// WrapPending
const _: baud_domain::WrapPending = baud_domain::WrapPending::No;
const _: baud_domain::WrapPending = baud_domain::WrapPending::Yes;
// WrappedAt
const _: baud_domain::WrappedAt = baud_domain::WrappedAt::Margins;
const _: baud_domain::WrappedAt = baud_domain::WrappedAt::Edge;
// Written
const _: fn([&'static [baud_domain::StyleId]; 2]) -> baud_domain::Written<'static> =
    baud_domain::Written::Slots;
const _: baud_domain::Written<'static> = baud_domain::Written::Every;
// Zone
const _: baud_domain::Zone = baud_domain::Zone::Prompt;
const _: baud_domain::Zone = baud_domain::Zone::Output;
const _: fn() -> baud_domain::Zone = <baud_domain::Zone as Default>::default;
// is_default_ignorable
const _: fn(baud_domain::Codepoint) -> bool = baud_domain::is_default_ignorable;
// is_placeholder
const _: fn(baud_domain::Codepoint) -> bool = baud_domain::is_placeholder;
// is_table_rule
const _: fn(char) -> bool = baud_domain::is_table_rule;
// order_strings
const _: for<'a, 'b, 'c, 'd> fn(
    &'a [baud_domain::StringId],
    fn(usize) -> bool,
    usize,
    baud_domain::BidiDirection,
    &'b fn(baud_domain::StringId) -> Option<baud_domain::StringEntry>,
    &'c mut [u16],
    &'d mut [baud_domain::BidiDirection],
) = baud_domain::order_strings;
// premultiply
const _: fn(u32) -> u32 = baud_domain::premultiply;
// themefile::CellPaint
const _: fn(u8) -> baud_domain::themefile::CellPaint<u8> =
    baud_domain::themefile::CellPaint::Colour;
const _: baud_domain::themefile::CellPaint<u8> = baud_domain::themefile::CellPaint::Text;
const _: baud_domain::themefile::CellPaint<u8> = baud_domain::themefile::CellPaint::Page;
const _: fn(baud_domain::themefile::CellPaint<u8>) -> Option<u8> =
    baud_domain::themefile::CellPaint::colour;
// themefile::CursorText
const _: baud_domain::themefile::CursorText =
    baud_domain::themefile::CursorText::UnderACursorColour;
const _: baud_domain::themefile::CursorText = baud_domain::themefile::CursorText::UnderEveryCursor;
// themefile::SelectionText
const _: baud_domain::themefile::SelectionText = baud_domain::themefile::SelectionText::Reverse;
const _: baud_domain::themefile::SelectionText = baud_domain::themefile::SelectionText::Unchanged;
// themefile::gamut::Space
const _: baud_domain::themefile::gamut::Space = baud_domain::themefile::gamut::Space::Srgb;
const _: baud_domain::themefile::gamut::Space = baud_domain::themefile::gamut::Space::DisplayP3;
const _: baud_domain::themefile::gamut::Space = baud_domain::themefile::gamut::Space::Calibrated;
// themefile::gamut::to_srgb
const _: fn(
    baud_domain::themefile::gamut::Space,
    [f64; 3],
) -> ([u8; 3], baud_domain::themefile::gamut::Fit) = baud_domain::themefile::gamut::to_srgb;

// WcSlice
const _: fn(&'static mut [u32]) -> baud_domain::WcSlice<'static, u32> = baud_domain::WcSlice::new;
const _: for<'a> fn(&'a baud_domain::WcSlice<'static, u32>) -> usize = baud_domain::WcSlice::len;
const _: for<'a> fn(&'a mut baud_domain::WcSlice<'static, u32>, usize, u32) =
    baud_domain::WcSlice::set;
const _: for<'a> fn(
    &'a mut baud_domain::WcSlice<'static, u32>,
    core::ops::Range<usize>,
) -> baud_domain::WcSlice<'a, u32> = baud_domain::WcSlice::slice;
const _: for<'a, 'c> fn(&'a mut baud_domain::WcSlice<'static, u32>, &'c [u32]) =
    baud_domain::WcSlice::copy_from;
// dbus::Sig
const _: fn(&'static [u8]) -> Result<baud_domain::dbus::Sig<'static>, baud_domain::dbus::SigError> =
    baud_domain::dbus::Sig::parse;
const _: fn(&'static str) -> baud_domain::dbus::Sig<'static> = baud_domain::dbus::Sig::lit;
const _: baud_domain::dbus::Sig<'static> = baud_domain::dbus::Sig::lit("a{sv}");
// dbus::SigCodes
const _: baud_domain::dbus::SigCodes = baud_domain::dbus::SigCodes::EMPTY;
const _: for<'a> fn(&'a baud_domain::dbus::SigCodes) -> baud_domain::dbus::Sig<'a> =
    baud_domain::dbus::SigCodes::as_sig;
const _: fn(
    baud_domain::dbus::SigCodes,
    baud_domain::dbus::SigCodes,
) -> baud_domain::dbus::SigCodes = baud_domain::dbus::SigCodes::concat;
const _: () = assert!(baud_domain::dbus::SigCodes::eq(
    baud_domain::dbus::SigCodes::EMPTY,
    baud_domain::dbus::Sig::EMPTY
));
// dbus::SigLen, dbus::Depth
const _: fn(baud_domain::dbus::SigLen) -> u8 = baud_domain::dbus::SigLen::get;
const _: baud_domain::dbus::Depth = baud_domain::dbus::Depth::NESTING;
// dbus names
const _: fn(&'static str) -> Result<baud_domain::dbus::Str<'static>, baud_domain::dbus::NameError> =
    baud_domain::dbus::Str::parse;
const _: fn(&'static str) -> baud_domain::dbus::Str<'static> = baud_domain::dbus::Str::lit;
const _: fn(
    &'static str,
) -> Result<baud_domain::dbus::ObjectPath<'static>, baud_domain::dbus::NameError> =
    baud_domain::dbus::ObjectPath::parse;
const _: fn(&'static str) -> baud_domain::dbus::ObjectPath<'static> =
    baud_domain::dbus::ObjectPath::lit;
const _: fn(
    &'static str,
) -> Result<baud_domain::dbus::InterfaceName<'static>, baud_domain::dbus::NameError> =
    baud_domain::dbus::InterfaceName::parse;
const _: fn(&'static str) -> baud_domain::dbus::InterfaceName<'static> =
    baud_domain::dbus::InterfaceName::lit;
const _: fn(
    &'static str,
) -> Result<baud_domain::dbus::MemberName<'static>, baud_domain::dbus::NameError> =
    baud_domain::dbus::MemberName::parse;
const _: fn(&'static str) -> baud_domain::dbus::MemberName<'static> =
    baud_domain::dbus::MemberName::lit;
const _: fn(
    &'static str,
) -> Result<baud_domain::dbus::ErrorName<'static>, baud_domain::dbus::NameError> =
    baud_domain::dbus::ErrorName::parse;
const _: fn(&'static str) -> baud_domain::dbus::ErrorName<'static> =
    baud_domain::dbus::ErrorName::lit;
const _: fn(
    &'static str,
) -> Result<baud_domain::dbus::BusName<'static>, baud_domain::dbus::NameError> =
    baud_domain::dbus::BusName::parse;
const _: fn(&'static str) -> baud_domain::dbus::BusName<'static> = baud_domain::dbus::BusName::lit;
const _: for<'a> fn(&'a baud_domain::dbus::OwnedBusName) -> baud_domain::dbus::BusName<'a> =
    baud_domain::dbus::OwnedBusName::as_name;
// dbus values
const _: fn(u8) -> baud_domain::dbus::Byte = baud_domain::dbus::Byte::new;
const _: fn(i32) -> baud_domain::dbus::I32 = baud_domain::dbus::I32::new;
const _: fn(u32) -> baud_domain::dbus::U32 = baud_domain::dbus::U32::new;
const _: fn(f64) -> baud_domain::dbus::Double = baud_domain::dbus::Double::new;
const _: baud_domain::dbus::Boolean = baud_domain::dbus::Boolean::False;
const _: baud_domain::dbus::Boolean = baud_domain::dbus::Boolean::True;
const _: baud_domain::dbus::FdIndex = baud_domain::dbus::FdIndex::ZERO;
const _: baud_domain::dbus::FdCount = baud_domain::dbus::FdCount::ZERO;
const _: fn(baud_domain::dbus::UnixFd) -> baud_domain::dbus::FdIndex =
    baud_domain::dbus::UnixFd::index;
const _: fn(
    &'static [baud_domain::dbus::U32],
) -> baud_domain::dbus::Array<'static, baud_domain::dbus::U32> = baud_domain::dbus::Array::new;
const _: fn(&'static [u8]) -> baud_domain::dbus::Bytes<'static> = baud_domain::dbus::Bytes::new;
const _: fn(
    &'static [(
        baud_domain::dbus::Str<'static>,
        baud_domain::dbus::Str<'static>,
    )],
) -> baud_domain::dbus::FlatPairs<'static> = baud_domain::dbus::FlatPairs::new;
const _: fn(
    &'static [(
        baud_domain::dbus::Str<'static>,
        baud_domain::dbus::Variant<'static>,
    )],
) -> baud_domain::dbus::Dict<
    'static,
    baud_domain::dbus::Str<'static>,
    baud_domain::dbus::Variant<'static>,
> = baud_domain::dbus::Dict::new;
const _: fn((baud_domain::dbus::U32,)) -> baud_domain::dbus::Struct<(baud_domain::dbus::U32,)> =
    baud_domain::dbus::Struct::new;
const _: baud_domain::dbus::Align = <baud_domain::dbus::U32 as baud_domain::dbus::Marshal>::ALIGN;
const _: () = assert!(baud_domain::dbus::SigCodes::eq(
    <baud_domain::dbus::NotifyArgs<'static> as baud_domain::dbus::Args>::SIG,
    baud_domain::dbus::Sig::lit("susssasa{sv}i")
));
// dbus body and messages
const _: fn(&'static baud_domain::dbus::Body) -> baud_domain::dbus::Sig<'static> =
    baud_domain::dbus::Body::sig;
const _: fn(&baud_domain::dbus::Body) -> baud_domain::dbus::FdCount = baud_domain::dbus::Body::fds;
const _: fn(
    &(baud_domain::dbus::U32,),
) -> Result<baud_domain::dbus::Body, baud_domain::dbus::EncodeError> =
    baud_domain::dbus::Body::of::<(baud_domain::dbus::U32,)>;
const _: baud_domain::dbus::EncodeError = baud_domain::dbus::EncodeError::TooLong;
const _: fn(usize) -> Option<baud_domain::dbus::MessageLen> = baud_domain::dbus::MessageLen::new;
const _: fn(usize) -> Option<baud_domain::dbus::ArrayLen> = baud_domain::dbus::ArrayLen::new;
const _: fn(
    &baud_domain::dbus::MethodCall<'static>,
    &baud_domain::dbus::Body,
    &mut baud_domain::dbus::Serials,
) -> Result<baud_domain::dbus::Encoded, baud_domain::dbus::EncodeError> =
    baud_domain::dbus::MethodCall::encode;
const _: fn(
    &baud_domain::dbus::Emit<'static>,
    &baud_domain::dbus::Body,
    &mut baud_domain::dbus::Serials,
) -> Result<baud_domain::dbus::Encoded, baud_domain::dbus::EncodeError> =
    baud_domain::dbus::Emit::encode;
const _: for<'a> fn(&'a baud_domain::dbus::Encoded) -> &'a [u8] = baud_domain::dbus::Encoded::bytes;
const _: fn(&baud_domain::dbus::Encoded) -> baud_domain::dbus::Serial =
    baud_domain::dbus::Encoded::serial;
const _: fn(&baud_domain::dbus::Encoded) -> baud_domain::dbus::FdCount =
    baud_domain::dbus::Encoded::fds;
const _: fn(baud_domain::dbus::Encoded) -> Vec<u8> = baud_domain::dbus::Encoded::into_bytes;
const _: baud_domain::dbus::Serial = baud_domain::dbus::Serial::HELLO;
const _: fn() -> baud_domain::dbus::Serials = baud_domain::dbus::Serials::new;
const _: fn(&mut baud_domain::dbus::Serials) -> baud_domain::dbus::Serial =
    baud_domain::dbus::Serials::next;
const _: &[u8] = baud_domain::dbus::HELLO;
const _: fn(&baud_domain::dbus::HeaderField<'static>) -> baud_domain::dbus::FieldCode =
    baud_domain::dbus::HeaderField::code;
const _: baud_domain::dbus::MessageKind = baud_domain::dbus::MessageKind::MethodCall;
const _: baud_domain::dbus::Flags = baud_domain::dbus::Flags::NONE;
/// Each type that holds a rule, and the error code its witness expects.
const WITNESSED: &[(&str, &str)] = &[
    ("HeaderField", "E0308"),
    ("Body", "E0451"),
    ("Body", "E0277"),
    ("MethodCall", "E0560"),
    ("MethodCall", "E0308"),
    ("Dict", "E0277"),
    ("Dict", "E0425"),
    ("Variant", "E0599"),
    ("BodyWriter", "E0624"),
    ("BodyWriter", "E0616"),
    ("Boolean", "E0277"),
    ("Serial", "E0423"),
    ("Serial", "E0308"),
    ("UnixFd", "E0423"),
    ("MessageLen", "E0423"),
    ("ArrayLen", "E0423"),
    ("Sig", "E0080"),
    ("SigCodes", "E0080"),
    ("Str", "E0080"),
    ("ObjectPath", "E0080"),
    ("InterfaceName", "E0080"),
    ("MemberName", "E0080"),
    ("ErrorName", "E0080"),
    ("BusName", "E0080"),
    ("WcSlice", "E0369"),
    ("WcSlice", "E0599"),
    ("WcSlice", "E0608"),
    ("Attrs", "E0369"),
    ("BytePos", "E0369"),
    ("CellFlags", "E0603"),
    ("Chan", "E0603"),
    ("ClusterBreak", "E0451"),
    ("ClusterEpoch", "E0423"),
    ("ClusterSlot", "E0451"),
    ("ClusterStore", "E0616"),
    ("Codepoint", "E0603"),
    ("ColorWord", "E0603"),
    ("Cols", "E0603"),
    ("Content", "E0603"),
    ("DirtyRows", "E0423"),
    ("DirtyRows", "E0608"),
    ("DirtyRows", "E0616"),
    ("DecodeBudget", "E0423"),
    ("HeldCover", "E0451"),
    ("HostTime", "E0369"),
    ("ImageGen", "E0423"),
    ("ImageId", "E0423"),
    ("ImageRead", "E0599"),
    ("ImagePx", "E0369"),
    ("ImageStore", "E0616"),
    ("IntervalMap", "E0616"),
    ("IntervalMap", "E0599"),
    ("IntervalMap", "E0061"),
    ("KittyId", "E0308"),
    ("LineBidi", "E0423"),
    ("LineIdx", "E0369"),
    ("LineInterval", "E0451"),
    ("Modifiers", "E0423"),
    ("OverlapRuns", "E0423"),
    ("OverlayChanges", "E0423"),
    ("PendingGen", "E0423"),
    ("ReadLen", "E0369"),
    ("RemapOverlap", "E0599"),
    ("Row", "E0369"),
    ("RowExtent", "E0616"),
    ("RowGens", "E0277"),
    ("RowRot", "E0603"),
    ("Rows", "E0308"),
    ("Seal", "E0369"),
    ("Seal", "E0451"),
    ("SixelAspect", "E0451"),
    ("SixelCanvas", "E0451"),
    ("SlotSet", "E0599"),
    ("SpanCount", "E0423"),
    ("SpanSet", "E0616"),
    ("SpanSet", "E0599"),
    ("StatusGen", "E0451"),
    ("Stride", "E0423"),
    ("StringId", "E0423"),
    ("StyleId", "E0616"),
    ("StyleWord", "E0451"),
    ("StyleWrites", "E0423"),
    ("TermRead", "E0038"),
    ("TermRead", "E0599"),
    ("Tracked", "E0616"),
    ("Tracked", "E0599"),
    ("Variants", "E0603"),
    ("WalkSteps", "E0423"),
    ("Width", "E0603"),
    ("WrapMargins", "E0451"),
];

/// One `compile_fail` block: the code it expects, its body, and where it is.
struct Witness {
    code: Option<String>,
    body: String,
    at: String,
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).expect("baud-domain/src is readable") {
        let p = e.expect("a directory entry").path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every `compile_fail` block in the doc comments of baud-domain/src.
fn witnesses() -> Vec<Witness> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut found = Vec::new();
    for f in files {
        let text = fs::read_to_string(&f).expect("a source file is readable");
        let mut open: Option<Witness> = None;
        for (n, line) in text.lines().enumerate() {
            let doc = line.trim_start();
            let Some(doc) = doc.strip_prefix("///").or_else(|| doc.strip_prefix("//!")) else {
                continue;
            };
            let doc = doc.trim();
            match open.as_mut() {
                None => {
                    if let Some(tag) = doc.strip_prefix("```compile_fail") {
                        let code = tag.split(',').map(str::trim).find(|t| t.starts_with('E'));
                        open = Some(Witness {
                            code: code.map(str::to_owned),
                            body: String::new(),
                            at: format!("{}:{}", f.display(), n + 1),
                        });
                    }
                }
                Some(w) => {
                    if doc.starts_with("```") {
                        found.push(open.take().expect("an open block"));
                    } else {
                        w.body.push_str(doc);
                        w.body.push('\n');
                    }
                }
            }
        }
    }
    found
}

fn names(body: &str, ty: &str) -> bool {
    body.match_indices(ty).any(|(i, _)| {
        let before = body[..i].chars().next_back();
        let after = body[i + ty.len()..].chars().next();
        let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        edge(before) && edge(after)
    })
}

#[test]
fn every_ruled_type_has_a_witness_with_its_error_code() {
    let all = witnesses();
    let missing: Vec<_> = WITNESSED
        .iter()
        .filter(|(ty, code)| {
            !all.iter()
                .any(|w| w.code.as_deref() == Some(*code) && names(&w.body, ty))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "rules with no witness naming the type and expecting the code: {missing:?}"
    );
}

#[test]
fn every_compile_fail_names_its_error_code() {
    let bare: Vec<_> = witnesses()
        .into_iter()
        .filter(|w| w.code.is_none())
        .map(|w| w.at)
        .collect();
    assert!(
        bare.is_empty(),
        "a compile_fail with no error code passes on any error: {bare:?}"
    );
}

#[test]
fn the_witness_reader_finds_a_known_witness() {
    let all = witnesses();
    assert!(all.len() >= WITNESSED.len(), "read {} witnesses", all.len());
    assert!(
        all.iter()
            .any(|w| w.code.as_deref() == Some("E0451") && names(&w.body, "Seal"))
    );
    assert!(!names("SealEpoch", "Seal"));
}
