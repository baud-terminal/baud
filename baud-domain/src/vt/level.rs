//! The conformance level each DEC function needs, as one table.
//!
//! **What DECSCL's levels mean.** `[[source: dec-std-070]]` 070-4 DECSCL,
//! note 1 (PDF 226): DECSCL works at Level 1 as "an exception to the normal
//! rule that Level 2 functions only work in Level 2 operation". So a function
//! acts at the levels that define it and is not executed below them, and the
//! level that defines it is the lowest its page's `Levels:` field names, an
//! `x` there being an extension baud has: "Higher levels, by definition,
//! support the functionality of lower levels" (070-4 DA, PDF 221), and the
//! chapters whose fields stop at 2 or 3 predate Level 4. A function no DEC
//! level defines -- one the control sequences (ctlseqs) or another
//! protocol define, one only ECMA-48 has -- acts at every
//! level, and one only a DEC terminal's own manual defines is that terminal's
//! level, the VT510's and VT520's Level 4, whose DECSCL makes 62, 63 and 64
//! one "VT Level 4 mode" (`[[source: vt510-decscl]]`).
//!
//! **A table, built at compile time and indexed at runtime.** [`LEVELS`] has
//! one row a function, in [`Function`]'s order, and [`Function::introduced`]
//! is an index into it; a const assertion holds the order. The parsed forms
//! name their function ([`Csi::function`], [`Esc::function`],
//! [`PrivateMode::function`], [`AnsiMode::function`], [`Charset::function`]),
//! so a dispatcher asks one load whether the level in force admits it.
//! `baud-domain/tests/conformance_levels.rs` reads the table against the
//! `Levels:` fields transcribed from the standard.

use crate::*;

/// The levels a function acts at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Introduced {
    /// From this level up.
    Since(ConformanceLevel),
    /// From the first level through the second, where a note of the text
    /// removes it above: DECID's note b, "DECID is not recognized at Level 3
    /// or above" (070-4, PDF 232).
    Through(ConformanceLevel, ConformanceLevel),
    /// At every level: a function no DEC level defines.
    Extension,
}

impl Introduced {
    /// Whether a terminal operating at `at` executes it.
    #[inline]
    pub fn admits(self, at: ConformanceLevel) -> bool {
        match self {
            Introduced::Since(l) => at >= l,
            Introduced::Through(lo, hi) => at >= lo && at <= hi,
            Introduced::Extension => true,
        }
    }

    /// The lowest level it acts at.
    pub fn least(self) -> ConformanceLevel {
        match self {
            Introduced::Since(l) | Introduced::Through(l, _) => l,
            Introduced::Extension => ConformanceLevel::One,
        }
    }
}

/// A DEC terminal whose own manual is the only text defining a function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecTerminal {
    Vt100,
    Vt220,
    Vt340,
    Vt420,
    Vt510,
    Vt520,
}

/// Which text gives a row its level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LevelSource {
    /// The function's own `Levels:` field in `[[source: dec-std-070]]`, at
    /// this PDF page: the row is the lowest level the field names.
    Field(u16),
    /// A `Levels:` field, at this PDF page, that another passage of the text
    /// overrides; the row's comment names the passage.
    Overridden(u16),
    /// 070-4 Table 1 (PDF 223), the extension of this number, for a function
    /// whose own chapter carries no field.
    Extension(u16),
    /// One of Appendix D's documented exceptions, which a level may implement
    /// and need not (070-1 §1.5.3.1.1, rule 3).
    Exception,
    /// Only this terminal's manual defines it.
    Manual(DecTerminal),
}

macro_rules! functions {
    ($($f:ident $m:literal $i:expr, $s:expr;)*) => {
        /// A DEC function baud executes, by the mnemonic its text gives it.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Function { $($f,)* }

        /// One row a [`Function`], in its order.
        pub const LEVELS: &[LevelRow] = &[$(LevelRow { function: Function::$f, mnemonic: $m, introduced: $i, source: $s },)*];
    };
}

/// A [`LEVELS`] row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelRow {
    pub function: Function,
    /// The mnemonic, as `[[source: dec-std-070]]` spells it where it has the
    /// function.
    pub mnemonic: &'static str,
    pub introduced: Introduced,
    pub source: LevelSource,
}

use ConformanceLevel::{Four, One, Three, Two};
use DecTerminal::*;
use Introduced::{Since, Through};
use LevelSource::{Exception, Extension as Ext, Field, Manual, Overridden};

functions! {
    // The C0 controls and the shifts, 070-3 and 070-5.
    Cr "CR" Since(One), Field(313);
    Lf "LF" Since(One), Field(314);
    Vt "VT" Since(One), Field(316);
    Ff "FF" Since(One), Field(317);
    Bs "BS" Since(One), Field(318);
    Ht "HT" Since(One), Field(322);
    Bel "BEL" Since(One), Field(369);
    Sub "SUB" Since(One), Field(387);
    Ls0 "LS0" Since(One), Field(178);
    Ls1 "LS1" Since(One), Field(179);
    Ss2 "SS2" Since(One), Field(185);
    Ss3 "SS3" Since(One), Field(186);
    // "Levels: 2, 3" on their pages, which the VT420's Table 4-1 follows,
    // where 070-3 §3.6.1.1 (PDF 166) lists LS0 to LS3 "in Level 1
    // operation".
    Ls2 "LS2" Since(Two), Field(180);
    Ls3 "LS3" Since(Two), Field(181);
    Ls1r "LS1R" Since(Two), Field(182);
    Ls2r "LS2R" Since(Two), Field(183);
    Ls3r "LS3R" Since(Two), Field(184);
    S7c1t "S7C1T" Since(Two), Field(176);
    S8c1t "S8C1T" Since(Two), Field(177);
    // "Levels: 2X, 3"; its algorithm acts "if (conformance_level > 2)"
    // (070-3 §3.8.1).
    Announce "ACS" Since(Three), Overridden(173);
    // Identification, 070-4.
    Da "DA" Since(One), Field(221);
    // "Levels: 1x" and an implicit extension to Level 1 (note 1).
    Decscl "DECSCL" Since(One), Field(226);
    Decid "DECID" Through(One, Two), Field(232);
    Decstr "DECSTR" Since(Two), Field(235);
    Dsr "DSR" Since(One), Field(239);
    Cpr "CPR" Since(One), Field(308);
    Decxcpr "DECXCPR" Since(Three), Field(310);
    // VT100 operation, the VT100's own report.
    Decreqtparm "DECREQTPARM" Since(One), Manual(Vt100);
    // Character cell display, 070-5.
    Dectcem "DECTCEM" Since(Two), Field(276);
    Decstbm "DECSTBM" Since(One), Field(280);
    Decslrm "DECSLRM" Since(Four), Field(282);
    Declrmm "DECLRMM" Since(Four), Field(284);
    Decom "DECOM" Since(One), Field(286);
    Decsclm "DECSCLM" Since(One), Field(287);
    Ind "IND" Since(One), Field(289);
    Ri "RI" Since(One), Field(291);
    Decfi "DECFI" Since(Four), Field(292);
    Decbi "DECBI" Since(Four), Field(294);
    Cuu "CUU" Since(One), Field(296);
    Cud "CUD" Since(One), Field(298);
    Cuf "CUF" Since(One), Field(300);
    Cub "CUB" Since(One), Field(302);
    Cup "CUP" Since(One), Field(304);
    Hvp "HVP" Since(One), Field(306);
    Lnm "LNM" Since(One), Field(312);
    Nel "NEL" Since(One), Field(319);
    Tbc "TBC" Since(One), Field(321);
    Hts "HTS" Since(One), Field(324);
    Deccolm "DECCOLM" Since(One), Field(326);
    Decscpp "DECSCPP" Since(Three), Field(328);
    Decslpp "DECSLPP" Since(Three), Field(330);
    // "Levels: 3x (Windowing), 4", the Windowing Extension's at Level 3
    // as DECRQDE, SU and SD are.
    Np "NP" Since(Three), Field(332);
    Pp "PP" Since(Three), Field(334);
    Ppa "PPA" Since(Three), Field(336);
    Ppr "PPR" Since(Three), Field(337);
    Ppb "PPB" Since(Three), Field(338);
    Decrqde "DECRQDE" Since(Three), Field(343);
    Su "SU" Since(Three), Field(346);
    Sd "SD" Since(Three), Field(347);
    Decscnm "DECSCNM" Since(One), Field(349);
    Decswl "DECSWL" Since(One), Field(351);
    Decdwl "DECDWL" Since(One), Field(352);
    Decdhl "DECDHL" Since(One), Field(354);
    Sgr "SGR" Since(One), Field(358);
    Scs "SCS" Since(One), Field(372);
    Decaupss "DECAUPSS" Since(Two), Field(378);
    Decrqupss "DECRQUPSS" Since(Two), Field(380);
    Decnrcm "DECNRCM" Since(One), Field(381);
    Srm "SRM" Since(One), Field(390);
    Irm "IRM" Since(One), Field(393);
    Ich "ICH" Since(Two), Field(397);
    Dch "DCH" Since(One), Field(399);
    Il "IL" Since(One), Field(401);
    Dl "DL" Since(One), Field(403);
    Decic "DECIC" Since(Four), Field(405);
    Decdc "DECDC" Since(Four), Field(406);
    Ech "ECH" Since(Two), Field(407);
    El "EL" Since(One), Field(409);
    Ed "ED" Since(One), Field(411);
    Decsel "DECSEL" Since(Two), Field(414);
    Decsed "DECSED" Since(Two), Field(417);
    Decsca "DECSCA" Since(Two), Field(421);
    Deccra "DECCRA" Since(Four), Field(424);
    Decfra "DECFRA" Since(Four), Field(425);
    Decera "DECERA" Since(Four), Field(426);
    Decsera "DECSERA" Since(Four), Field(427);
    Deccara "DECCARA" Since(Four), Field(428);
    Decrara "DECRARA" Since(Four), Field(430);
    Decsace "DECSACE" Since(Four), Field(432);
    Decrqcra "DECRQCRA" Since(Four), Field(434);
    DsrChecksum "DSR (checksum)" Since(Four), Field(435);
    DsrMacroSpace "DECMSR" Since(Four), Field(440);
    Decsc "DECSC" Since(One), Field(442);
    Decrc "DECRC" Since(One), Field(444);
    Decrqm "DECRQM" Since(Two), Field(448);
    Decrqss "DECRQSS" Since(Two), Field(451);
    Decrqpsr "DECRQPSR" Since(Two), Field(453);
    Decrsps "DECRSPS" Since(Two), Field(460);
    Decrqtsr "DECRQTSR" Since(Two), Field(461);
    Decrsts "DECRSTS" Since(Two), Field(463);
    // Keyboard processing, 070-6.
    Kam "KAM" Since(One), Field(526);
    Decarm "DECARM" Since(One), Field(528);
    Decckm "DECCKM" Since(One), Field(549);
    Deckpam "DECKPAM" Since(One), Field(553);
    Deckpnm "DECKPNM" Since(One), Field(554);
    Decnkm "DECNKM" Since(Two), Field(555);
    // The printer port, 070-7.
    DsrPrinter "DSR (printer)" Since(One), Field(711);
    Mc "MC" Since(One), Field(713);
    Decpff "DECPFF" Since(One), Field(716);
    Decpex "DECPEX" Since(One), Field(717);
    // User-defined keys, 070-11: the status report baud answers.
    DsrUdk "DSR (UDK)" Since(Two), Field(974);
    // The locator, 070-13.
    Decelr "DECELR" Since(One), Field(1020);
    Decrqlp "DECRQLP" Since(One), Field(1023);
    Decsle "DECSLE" Since(One), Field(1024);
    Decefr "DECEFR" Since(One), Field(1025);
    Declkd "DECLKD" Since(One), Field(1028);
    DsrLocator "DSR (locator)" Since(One), Field(1030);
    // The status display, 070-14.
    Decsasd "DECSASD" Since(Two), Field(1047);
    Decssdt "DECSSDT" Since(Two), Field(1048);
    // The graphics extensions, whose chapters carry no field.
    Regis "ReGIS" Since(One), Ext(3);
    Sixel "Sixel" Since(One), Ext(4);
    // Appendix D's exceptions, and 070-0A's VT52 entry: its page says "only
    // recognized in Level 1 and Level 2", and its change history A.9.1 "to
    // allow entry from any conformance level".
    Ris "RIS" Since(One), Exception;
    Decaln "DECALN" Since(One), Exception;
    Decawm "DECAWM" Since(One), Exception;
    Decanm "DECANM" Since(One), Exception;
    // Only a terminal's manual: as the VT420's "Control Functions Ignored in
    // Level 1" (`[[source: vt420-rm]]` Table 4-1) names them or not, the
    // keyboard report at Level 2 and the rest at Level 1, DECSDM and DECGEPM
    // with Sixel and the printer; the VT510's and VT520's controls at
    // Level 4.
    DsrKeyboard "DSR (keyboard)" Since(Two), Manual(Vt220);
    Decbkm "DECBKM" Since(One), Manual(Vt340);
    Decsdm "DECSDM" Since(One), Manual(Vt340);
    Decgepm "DECGEPM" Since(One), Manual(Vt340);
    DsrDataIntegrity "DSR (data integrity)" Since(One), Manual(Vt420);
    DsrSessions "DSR (sessions)" Since(One), Manual(Vt420);
    Decst8c "DECST8C" Since(Four), Manual(Vt510);
    Decncsm "DECNCSM" Since(Four), Manual(Vt510);
    Decrlcm "DECRLCM" Since(Four), Manual(Vt510);
    Dechem "DECHEM" Since(Four), Manual(Vt510);
    Decrlm "DECRLM" Since(Four), Manual(Vt510);
    // Its page names "the terminal is set to VT100 mode" for its 7-bit
    // behaviour; at Level 4 by LEVEL_4, the VT100 clause unreached.
    Dechebm "DECHEBM" Since(Four), Manual(Vt510);
    Decscusr "DECSCUSR" Since(Four), Manual(Vt520);
    Decswbv "DECSWBV" Since(Four), Manual(Vt520);
    Decsmbv "DECSMBV" Since(Four), Manual(Vt520);
    Dececm "DECECM" Since(Four), Manual(Vt520);
}

// The table is in `Function`'s order, so `introduced` is an index.
const _: () = {
    let mut i = 0;
    while i < LEVELS.len() {
        assert!(LEVELS[i].function as usize == i);
        i += 1;
    }
};

impl Function {
    /// The levels it acts at: a load from [`LEVELS`].
    #[inline]
    pub fn introduced(self) -> Introduced {
        LEVELS[self as usize].introduced
    }

    /// Its row.
    pub fn row(self) -> &'static LevelRow {
        &LEVELS[self as usize]
    }
}

/// What a parsed form needs: its function's levels, or every level for a
/// form no DEC level defines.
#[inline]
pub fn introduced_by(f: Option<Function>) -> Introduced {
    match f {
        Some(f) => f.introduced(),
        None => Introduced::Extension,
    }
}

impl Csi {
    /// The DEC function this operation performs, or `None` for one no DEC
    /// level defines. SM, RM and the private modes are Level 1's and each
    /// mode has its own ([`PrivateMode::function`], [`AnsiMode::function`]);
    /// so do the device status reports ([`DecStatus::function`]).
    pub fn function(&self) -> Option<Function> {
        use Csi::*;
        Some(match *self {
            CursorUp(_) => Function::Cuu,
            CursorDown(_) => Function::Cud,
            CursorForward(_) => Function::Cuf,
            CursorBack(_) => Function::Cub,
            CursorPosition { .. } => Function::Cup,
            CharacterLinePosition { .. } => Function::Hvp,
            EraseInDisplay(_) | EraseEach(EraseList::Display) => Function::Ed,
            EraseInLine(_) | EraseEach(EraseList::Line) => Function::El,
            InsertLines(_) => Function::Il,
            DeleteLines(_) => Function::Dl,
            DeleteChars(_) => Function::Dch,
            EraseChars(_) => Function::Ech,
            InsertBlanks(_) => Function::Ich,
            SetScrollRegion { .. } => Function::Decstbm,
            SelectProtection { .. } => Function::Decsca,
            SelectiveEraseDisplay(_) | EraseEach(EraseList::SelectiveDisplay) => Function::Decsed,
            SelectiveEraseLine(_) | EraseEach(EraseList::SelectiveLine) => Function::Decsel,
            DeviceAttributes => Function::Da,
            DeviceStatus(Some(StatusKind::CursorPosition)) => Function::Cpr,
            DeviceStatus(_) => Function::Dsr,
            Query(QueryOp::TerminalParameters) => Function::Decreqtparm,
            Query(QueryOp::PreferredSupplemental) => Function::Decrqupss,
            Query(QueryOp::DisplayedExtent) => Function::Decrqde,
            Locator(LocatorOp::Enable) => Function::Decelr,
            Locator(LocatorOp::SelectEvents) => Function::Decsle,
            Locator(LocatorOp::FilterRectangle) => Function::Decefr,
            Locator(LocatorOp::RequestPosition) => Function::Decrqlp,
            SoftReset => Function::Decstr,
            SetConformanceLevel => Function::Decscl,
            TabClear(_) => Function::Tbc,
            TabsEveryEight => Function::Decst8c,
            ScrollUp(_) => Function::Su,
            ScrollDown(_) => Function::Sd,
            Position(PositionOp::Page(PageOp::Next), _) => Function::Np,
            Position(PositionOp::Page(PageOp::Preceding), _) => Function::Pp,
            Position(PositionOp::Page(PageOp::Absolute), _) => Function::Ppa,
            Position(PositionOp::Page(PageOp::Forward), _) => Function::Ppr,
            Position(PositionOp::Page(PageOp::Backward), _) => Function::Ppb,
            Mode { .. } | PrivateMode { .. } => return None,
            Sgr => Function::Sgr,
            CursorStyle(_) => Function::Decscusr,
            WarningBellVolume(_) => Function::Decswbv,
            MarginBellVolume(_) => Function::Decsmbv,
            MediaCopy(_) => Function::Mc,
            SetSideMargins { .. } => Function::Decslrm,
            InsertColumns(_) => Function::Decic,
            DeleteColumns(_) => Function::Decdc,
            SetColumnsPerPage(_) => Function::Decscpp,
            SelectStatusType(_) => Function::Decssdt,
            SelectActiveDisplay(_) => Function::Decsasd,
            RequestMode { .. } => Function::Decrqm,
            Window(WindowOp::Lines) => Function::Decslpp,
            Area(AreaOp::Copy) => Function::Deccra,
            Area(AreaOp::Fill) => Function::Decfra,
            Area(AreaOp::Erase) => Function::Decera,
            Area(AreaOp::SelectiveErase) => Function::Decsera,
            Area(AreaOp::ChangeAttrs) => Function::Deccara,
            Area(AreaOp::ReverseAttrs) => Function::Decrara,
            Area(AreaOp::SelectExtent) => Function::Decsace,
            Area(AreaOp::RequestChecksum) => Function::Decrqcra,
            RequestPresentationState => Function::Decrqpsr,
            RequestTerminalState => Function::Decrqtsr,
            // ECMA-48's alone, xterm's, kitty's and contour's.
            EraseSavedLines
            | SelectiveEraseSavedLines
            | EraseInArea(_)
            | EraseInField(_)
            | Repeat(_)
            | ColorSchemeQuery
            | Query(QueryOp::DecStatus | QueryOp::ModifyKeys | QueryOp::FormatKeys)
            | CursorForwardTab(_)
            | CursorBackTab(_)
            | CursorTabControl
            | Position(..)
            | SelectEditingExtent(_)
            | CursorLineTab(_)
            | CursorNextLine(_)
            | CursorPrevLine(_)
            | ColumnAbsolute(_)
            | CharacterPositionAbsolute(_)
            | LineAbsolute(_)
            | ColumnRelative(_)
            | LineRelative(_)
            | ColumnBackward(_)
            | LineBackward(_)
            | SaveCursor
            | RestoreCursor
            | ScrollLeft(_)
            | ScrollRight(_)
            | SelectCharacterPath { .. }
            | SelectPresentationDirections { .. }
            | SelectPresentationVariants
            | String(_)
            | RequestVersion
            | KeyControl(_)
            | ShiftEscape(_)
            | PointerMode(_)
            | KittySet { .. }
            | KittyQuery
            | KittyPush { .. }
            | KittyPop { .. }
            | SaveModes
            | RestoreModes
            | HighlightMouseTracking(_)
            | Window(_)
            | Graphics(_)
            | Area(AreaOp::ReportRendition | AreaOp::SelectChecksum)
            | Stack(_)
            | TitleModes(_)
            | Unsupported { .. } => return None,
        })
    }
}

impl Esc {
    /// The DEC function this `ESC Fe` performs, or `None`.
    pub fn function(self) -> Option<Function> {
        Some(match self {
            Esc::SaveCursor => Function::Decsc,
            Esc::RestoreCursor => Function::Decrc,
            Esc::ReverseIndex => Function::Ri,
            Esc::Index => Function::Ind,
            Esc::NextLine => Function::Nel,
            Esc::Reset => Function::Ris,
            Esc::KeypadApplication => Function::Deckpam,
            Esc::KeypadNumeric => Function::Deckpnm,
            Esc::TabSet => Function::Hts,
            Esc::Identify => Function::Decid,
            Esc::BackIndex => Function::Decbi,
            Esc::ForwardIndex => Function::Decfi,
            Esc::SingleShift2 => Function::Ss2,
            Esc::SingleShift3 => Function::Ss3,
            Esc::LockingShift2 => Function::Ls2,
            Esc::LockingShift3 => Function::Ls3,
            Esc::LockingShift1Right => Function::Ls1r,
            Esc::LockingShift2Right => Function::Ls2r,
            Esc::LockingShift3Right => Function::Ls3r,
            // ECMA-48's VTS, SPA, EPA, SSA, ESA and HTJ; ST, which performs
            // nothing here; HP's memory lock and unlock, which no DEC level
            // names.
            Esc::LineTabSet
            | Esc::StringTerminator
            | Esc::StartGuardedArea
            | Esc::EndGuardedArea
            | Esc::StartSelectedArea
            | Esc::EndSelectedArea
            | Esc::TabJustify
            | Esc::MemoryLock
            | Esc::MemoryUnlock
            | Esc::Unsupported => return None,
        })
    }
}

impl AnsiMode {
    /// The DEC function this mode is, or `None` for ECMA-48's ERM and TSM.
    pub fn function(self) -> Option<Function> {
        Some(match self {
            AnsiMode::InsertReplace => Function::Irm,
            AnsiMode::LineFeedNewLine => Function::Lnm,
            AnsiMode::KeyboardAction => Function::Kam,
            AnsiMode::SendReceive => Function::Srm,
            AnsiMode::Erasure
            | AnsiMode::TabulationStop
            | AnsiMode::BiDirectionalSupport
            | AnsiMode::DeviceComponentSelect
            | AnsiMode::CharacterEditing
            | AnsiMode::LineEditing
            | AnsiMode::GuardedAreaTransfer
            | AnsiMode::MultipleAreaTransfer
            | AnsiMode::TransferTermination
            | AnsiMode::SelectedAreaTransfer
            | AnsiMode::FormatEffectorTransfer
            | AnsiMode::Other(_) => return None,
        })
    }
}

impl PrivateMode {
    /// The DEC function this mode is, or `None` for xterm's and the rest.
    /// `?44` and `?45` are xterm's margin bell and reverse wrap here, the
    /// meanings baud gives them, not the VT340's.
    pub fn function(self) -> Option<Function> {
        Some(match self {
            PrivateMode::CursorKeys => Function::Decckm,
            PrivateMode::AnsiMode => Function::Decanm,
            PrivateMode::Column132 => Function::Deccolm,
            PrivateMode::SmoothScroll => Function::Decsclm,
            PrivateMode::ReverseVideo => Function::Decscnm,
            PrivateMode::Origin => Function::Decom,
            PrivateMode::AutoWrap => Function::Decawm,
            PrivateMode::AutoRepeat => Function::Decarm,
            PrivateMode::PrintFormFeed => Function::Decpff,
            PrivateMode::PrintExtent => Function::Decpex,
            PrivateMode::CursorVisible => Function::Dectcem,
            PrivateMode::GraphicsExpandedPrint => Function::Decgepm,
            PrivateMode::NationalReplacement => Function::Decnrcm,
            PrivateMode::NumericKeypad => Function::Decnkm,
            PrivateMode::BackarrowKey => Function::Decbkm,
            PrivateMode::LeftRightMargin => Function::Declrmm,
            PrivateMode::SixelDisplay => Function::Decsdm,
            PrivateMode::NoClearOnColumn => Function::Decncsm,
            PrivateMode::RightToLeftCopy => Function::Decrlcm,
            PrivateMode::HebrewEncoding => Function::Dechem,
            PrivateMode::CursorRightToLeft => Function::Decrlm,
            PrivateMode::HebrewKeyboard => Function::Dechebm,
            PrivateMode::EraseColor => Function::Dececm,
            _ => return None,
        })
    }
}

impl DecStatus {
    /// The DEC function each private device status report is.
    pub fn function(self) -> Function {
        match self {
            DecStatus::ExtendedCursor => Function::Decxcpr,
            DecStatus::Printer => Function::DsrPrinter,
            DecStatus::UserKeys => Function::DsrUdk,
            DecStatus::Keyboard => Function::DsrKeyboard,
            DecStatus::LocatorStatus | DecStatus::LocatorType => Function::DsrLocator,
            DecStatus::MacroSpace => Function::DsrMacroSpace,
            DecStatus::MemoryChecksum { .. } => Function::DsrChecksum,
            DecStatus::DataIntegrity => Function::DsrDataIntegrity,
            DecStatus::Sessions => Function::DsrSessions,
        }
    }
}

impl Charset {
    /// The levels designating this set acts at: SCS's own character-set
    /// table (070-5 pp. 5-119 and 5-120). ASCII, DEC Line Drawing and note
    /// 4's UK at Level 1, and the NRC sets, Table 1's extension 9 ("1-4")
    /// and DECNRCM's "1x", as the VT340 and VT420 have them; "if
    /// (conformance level >= LEVEL 2)" DEC Supplemental, under `<` and
    /// `% 5`; ISO Latin-1 Supplemental at Level 2 as 070-4's implicit 8-bit
    /// extension there; the Technical set from Level 2, Table 1's extension
    /// 15 ("2-4"); JIS-Roman and Katakana at Level 1, extension 5 ("1-3");
    /// the VT500's sets at Level 4. A final no text lists acts as ever.
    pub fn introduced(self) -> Introduced {
        match self {
            Charset::Ascii | Charset::Uk | Charset::DecGraphics => Since(One),
            Charset::National(Nrcs::Greek | Nrcs::Turkish | Nrcs::Hebrew) => Since(Four),
            Charset::National(_) => Since(One),
            Charset::Dec(
                DecSet::Supplemental | DecSet::SupplementalGraphics | DecSet::Technical,
            ) => Since(Two),
            Charset::Dec(DecSet::JisKatakana | DecSet::JisRoman) => Since(One),
            Charset::Dec(_) => Since(Four),
            Charset::Iso96(Iso96::Latin1) => Since(Two),
            Charset::Iso96(_) => Since(Four),
            Charset::Other => Introduced::Extension,
        }
    }
}

impl GSlot {
    /// "In Level 1 operation character sets can only be designated into G0
    /// and G1" (070-5 SCS note 1, p. 5-118).
    pub fn designable(self, at: ConformanceLevel) -> bool {
        at >= Two || matches!(self, GSlot::G0 | GSlot::G1)
    }
}

/// An SGR parameter's levels, by the value that starts it (070-5 SGR, pp.
/// 5-103 to 5-106): colour is §5.6.3's Color Text extension, which "may be
/// provided at Level 2 or higher", and 22, 24, 25, 27 and 28 act from Level
/// 2, as the algorithm's "if (conformance level == LEVEL_2)" and the
/// VT520's Table 5-15 have them. The rest are Level 1's or no DEC level's.
pub fn sgr_introduced(v: u16) -> Introduced {
    match v {
        22 | 24 | 25 | 27 | 28 | 30..=39 | 40..=49 | 58 | 59 | 90..=97 | 100..=107 => Since(Two),
        0 | 1 | 4 | 5 | 7 | 8 => Since(One),
        _ => Introduced::Extension,
    }
}
