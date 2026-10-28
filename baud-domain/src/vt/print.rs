//! Printing: Media Copy and its modes, `[[source: dec-std-070]]` 070-7, the
//! Printer Port Extension, and the VT420's chapter 11 (`[[source:
//! vt420-rm]]`, pp. 209-215).
//!
//! **A program's print reaches a printer only through the embedder**, which
//! tells the core whether one is there ([`PrinterPort`]); the core turns a
//! print into 070-7's print stream and hands it over as a job. With no
//! printer since power-up or RIS every print function is ignored and
//! counted, 070-7 § 7.3.2's "all print commands which require the printer
//! port are ignored", and DSR `?15` says so; a printer lost after it was
//! there is "not ready", and its prints are queued.

use crate::*;

/// `MC`, `CSI Ps i` and `CSI ? Ps i`: the print functions baud acts on, and
/// the ones it refuses because answering is unsafe.
///
/// **Refused, and counted apart from the unknown**: `CSI 2 i`, the VT510's
/// "Sends screen data through host port" (`[[source: vt510-rm-b]]` p. 5-178),
/// would type the screen back into the program's input, the reason the title
/// reports are refused; `CSI ? 2 i`, "Graphics to Host", which sends what
/// ReGIS's hard copy prints to the host instead, the screen's image as a
/// reply, refused for the same reason; `CSI ? 8 i` and `CSI ? 9 i` stop and
/// start a printer-to-host session, which needs a printer that talks back;
/// xterm's `CSI 10 i` and `CSI 11 i` write HTML and SVG dumps to files
/// (`[[source: ctlseqs]]`). A `Ps` no text gives is [`Csi::Unsupported`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaCopy {
    /// `CSI i`, `CSI 0 i`: print page, the page or the scrolling region as
    /// DECPEX says.
    PrintPage,
    /// `CSI 4 i`: printer controller mode off.
    ControllerOff,
    /// `CSI 5 i`: printer controller mode on.
    ControllerOn,
    /// `CSI ? 1 i`: print the cursor's line.
    PrintLine,
    /// `CSI ? 4 i`: autoprint off.
    AutoPrintOff,
    /// `CSI ? 5 i`: autoprint on.
    AutoPrintOn,
    /// `CSI ? 10 i`: print the composed display, DECPEX ignored.
    PrintScreen,
    /// `CSI ? 11 i`: print all pages.
    PrintAllPages,
    /// `CSI ? i` and `CSI ? 0 i`, "Graphics to Printer": ReGIS's hard copy
    /// goes to the printer (`[[source: vt3xx-gp-002]]` ch. 16), the one
    /// place baud sends it, so nothing changes.
    GraphicsToPrinter,
    /// One of the four kinds above that are refused.
    Refused,
}

impl MediaCopy {
    /// `CSI Ps i`, the standard namespace, or `None` for a `Ps` no text
    /// gives (`CSI 6 i` and `CSI 7 i` among them: the Locator Port's
    /// controller mode in `[[source: dec-std-070]]` 070-13 and a
    /// printer-to-host session on the VT510, both counted as unsupported).
    pub fn standard(p: Option<Param>) -> Option<MediaCopy> {
        Some(match p.map_or(0, |x| x.0) {
            0 => MediaCopy::PrintPage,
            4 => MediaCopy::ControllerOff,
            5 => MediaCopy::ControllerOn,
            2 | 10 | 11 => MediaCopy::Refused,
            _ => return None,
        })
    }

    /// `CSI ? Ps i`, DEC's private namespace, or `None` for a `Ps` no text
    /// gives baud a function for: `?18` and `?19` assign and release the
    /// printer between two sessions.
    pub fn private(p: Option<Param>) -> Option<MediaCopy> {
        Some(match p.map_or(0, |x| x.0) {
            0 => MediaCopy::GraphicsToPrinter,
            2 => MediaCopy::Refused,
            1 => MediaCopy::PrintLine,
            4 => MediaCopy::AutoPrintOff,
            5 => MediaCopy::AutoPrintOn,
            10 => MediaCopy::PrintScreen,
            11 => MediaCopy::PrintAllPages,
            8 | 9 => MediaCopy::Refused,
            _ => return None,
        })
    }
}

/// Whether the embedder has a printer to hand a job to, which with whether
/// it had one since power-up or RIS is what DSR `?15` answers (070-7 p.
/// 7-32): `?10n` ready, `?11n` not ready, `?13n` "not connected".
/// **Absent until the embedder says otherwise**, so a core no embedder
/// told prints nothing and answers as it did before printing was built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrinterPort {
    Absent,
    Ready,
}

/// **Whether a DSR `?15` stops the read**, so the embedder can look at its
/// printer again before the answer goes (`Term::set_printer_hold`). **Off
/// by default**: the answer is then what the embedder last said, at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PrinterHold {
    #[default]
    Off,
    On,
}

/// Whether a DSR `?15` held under [`PrinterHold::On`] waits for its
/// answer: owed until the embedder says what printer it has.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PrinterAsked {
    #[default]
    No,
    Owed,
}

/// What a printer is collecting for: nothing, an autoprint run (`CSI ? 5 i`
/// to `CSI ? 4 i`) or a printer controller run (`CSI 5 i` to `CSI 4 i`),
/// during which the screen is not updated, so an embedder can say why.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintRun {
    Idle,
    AutoPrint,
    Controller,
}

/// `?18` DECPFF. "When Print Form Feed Mode is set, a Form Feed (FF)
/// control character is transmitted to the printer port at the conclusion
/// of each Print Page or Print Screen operation" (070-7 p. 7-37). **Off at
/// power-on and after `RIS`**, "Default: No form feed" (`[[source:
/// vt420-rm]]` p. 211); `DECSTR` keeps it, its list naming neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintFormFeed {
    Off,
    On,
}

/// `?19` DECPEX: set, the whole page; reset, "only the area of the display
/// within the scrolling region (Top Margin to Bottom Margin inclusive)"
/// (070-7 p. 7-38). **The scrolling region at power-on and after `RIS`**,
/// a source conflict: `[[source: vt420-rm]]` p. 210 and
/// `[[source: vt510-rm-b]]` p. 5-81 say "Default: Scrolling region", and
/// xterm's `printerExtent` is false; the VT420's own Table 11-5 marks the
/// page "(D)", and STD 070's `host_state_init` sets `PRINT_DISPLAY` from
/// NVM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintExtent {
    ScrollingRegion,
    Page,
}

/// How many bytes of print stream the core's area holds: every job the
/// embedder has not yet taken, and the one being collected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrintBudget {
    bytes: usize,
}

impl PrintBudget {
    /// Room for `bytes` of print stream.
    pub const fn of_bytes(bytes: usize) -> PrintBudget {
        PrintBudget { bytes }
    }

    /// The area's length.
    pub const fn bytes(self) -> usize {
        self.bytes
    }
}

/// `?43` DECGEPM, graphics expanded print mode: "selects whether the
/// terminal sends a compressed or expanded graphics image to the printer"
/// (`[[source: vt3xx-gp-002]]` ch. 16), a Level 2 dump's grid size 6 or 9
/// decipoints (`[[source: dec-std-070]]` 070-7 § 7.8.2.4). **Compressed at
/// power-on and after `RIS`**, "Default: Compressed"; `DECSTR` keeps it, the
/// VT330/VT340's Table 13-1 naming no graphics print mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintSize {
    Compressed,
    Expanded,
}

/// Monochrome or colour sixel printing, which "the user may select" (070-7
/// § 7.8.2.1): the VT340's DECGPCM, whose number `?44` is xterm's margin
/// bell here. "Monochrome Sixels are formed from the multiple plane image
/// by a logical OR of all pixels which are selected for any color"; in
/// colour "the terminal will send out color specifiers at the beginning of
/// each Sixel dump". **Monochrome by default**, the text's "Default: Black
/// and white".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintColour {
    Mono,
    Colour,
}

/// The colour coordinates a colour dump's specifiers use, DECGPCS: HLS,
/// "Default: HLS", or RGB (`[[source: vt3xx-gp-002]]` ch. 16).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintSyntax {
    Hls,
    Rgb,
}

/// Whether a colour dump prints the background, DECGPBM: "If PRINT
/// BACKGROUND is selected, all pixels with no bits set in any plane are
/// printed in the color value selected for index 0" (070-7 § 7.8.2.1);
/// "Default: No background", and only in colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintBackground {
    Off,
    On,
}

/// Whether a dump is turned a quarter, DECGRPM: "rotates the image by 90
/// degrees so that an expanded image can fit on a single 8-1/2 inch wide
/// page (rotated images are always expanded)", counter-clockwise as the
/// VT240 does (070-7 § 7.8.2.3). "Default: compressed".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintRotation {
    Upright,
    Rotated,
}

/// **The graphics print choices the user makes under local control**: 070-7
/// § 7.8.2 lets the user select colour, the background and rotation, and
/// the VT340 the colour syntax, each also a DEC private mode (`?44`-`?47`)
/// whose number xterm, and so baud, gives to the margin bell, reverse
/// wraparound, logging and the alternate screen. The embedder sets them
/// ([`crate::GraphicsPrint::DEFAULT`] the texts' defaults).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GraphicsPrint {
    pub colour: PrintColour,
    pub syntax: PrintSyntax,
    pub background: PrintBackground,
    pub rotation: PrintRotation,
}

impl GraphicsPrint {
    /// Black and white, HLS, no background, upright: every default the
    /// VT330/VT340 manual gives.
    pub const DEFAULT: GraphicsPrint = GraphicsPrint {
        colour: PrintColour::Mono,
        syntax: PrintSyntax::Hls,
        background: PrintBackground::Off,
        rotation: PrintRotation::Upright,
    };
}

/// Whether a job is all its program sent to the printer, or lost bytes
/// past the area's bound, counted in [`PrintRefusals::dropped`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintCut {
    Whole,
    Cut,
}

/// One finished print job: 070-7's print stream, in UTF-8.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrintJob<'a> {
    pub bytes: &'a [u8],
    pub cut: PrintCut,
}

/// The print functions that printed nothing, as readings to compare.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PrintRefusals {
    /// A print function while the embedder had no printer since power-up
    /// or RIS: 070-7 § 7.3.2 ignores them.
    pub no_printer: u32,
    /// `CSI 2 i`, a printer-to-host session and xterm's dumps, refused
    /// because answering is unsafe.
    pub unsafe_kinds: u32,
    /// Bytes past the area's bound, dropped from the job collecting them.
    pub dropped: u64,
    /// Jobs finished while the core already held [`PRINT_QUEUE`] the
    /// embedder had not taken, each joined to the last of them after a
    /// form feed.
    pub merged: u32,
    /// ReGIS hard copies the area had no working room for, which printed
    /// nothing.
    pub hard_copies_cut: u32,
}

impl PrintRefusals {
    pub const NONE: PrintRefusals = PrintRefusals {
        no_printer: 0,
        unsafe_kinds: 0,
        dropped: 0,
        merged: 0,
        hard_copies_cut: 0,
    };
}

/// How many finished jobs the core keeps for the embedder to take after a
/// read: a read that finishes more joins the rest to the last, after a form
/// feed, so nothing printed is lost.
pub const PRINT_QUEUE: usize = 8;
