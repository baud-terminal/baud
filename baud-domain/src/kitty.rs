//! The terminal graphics protocol (APC G): control data, replies, and a
//! transmission decoded as it arrives (`[[source: kitty-graphics]]`).
//!
//! **Everything that is protocol is here, and the core only routes bytes.**
//! An `APC G` body reaches [`KittyStream`] a byte at a time: the control
//! data up to `;` is kept and parsed by [`KittyCommand::parse`] against the
//! "Control data reference" table, and the payload is base64 decoded into
//! where its format puts it -- `f=24` and `f=32` straight into the image's
//! reserved arena span, `o=z` through [`Inflate::feed`] into that span (or
//! into the scratch for a PNG), and a PNG's bytes into the scratch, because
//! [`Png`] reads its container whole. The terminator finishes a chunk, and
//! the last chunk finishes the image.
//!
//! **Decoding a PNG, and turning inflated bytes into pixels, is deferred and
//! stepped**: the image is reserved and placed, and the cursor moved, when its
//! size is known -- from `s` and `v`, or from the PNG's `IHDR` -- and
//! [`KittyStream::step`] advances the rest a bounded amount at a time. The image
//! is not shown until it is whole, as "terminals must not display anything, until
//! the entire sequence is received and validated" ("Remote client"). Its reply is
//! sent when it completes. A query (`a=q`) is decoded at its terminator, into the
//! store's probe outside the arena when it fits there (a query "will not replace
//! an existing image with the same id, nor will it store the image"), before the
//! next byte, because "terminal emulators that support the graphics protocol, must
//! reply to query actions immediately without processing other input" ("Querying
//! support"). A command that arrives while a decode is pending finishes that
//! decode first, so the replies leave in the order the commands came.
//!
//! **What a chunk may carry.** "Subsequent chunks must have only the m and
//! optionally q keys" binds the client; a continuation chunk's other keys
//! are ignored here and a chunk may be any length, except that a delete
//! aborts the upload: "If an image is being loaded in chunks and the upload
//! is not complete when any delete command is received, the partial upload
//! must be aborted" ("Deleting images").
//!
//! **Replies** are
//! `ESC _ G i=<id>[,I=<number>][,p=<placement>];<OK or error> ESC \`, sent
//! only for a command that named an image by `i` or `I` ("If it does so,
//! the terminal emulator will reply"), in the shape the text writes,
//! "`<ESC>_Gi=<id>;OK<ESC>\\`": the stream keeps each body, and the core
//! frames it with `APC` and `ST` in the form S7C1T/S8C1T chose, 9F ... 9C
//! after S8C1T, which governs every C1 control the terminal sends.
//! `q=1` silences `OK` and `q=2` everything, as the placeholder section's
//! "(q=2) so that there are no responses" and kitty's
//! `finish_command_response` (`[[source: kitty-graphics-c]]`) have it. The
//! error codes are the ones the text names -- ENOENT for a put naming no
//! image, EINVAL for `i` and `I` together, kitty's one EBADF string for a
//! file medium -- and kitty's own for the rest (EINVAL, ENODATA, EFBIG,
//! ENOSPC, EBADPNG), practice recorded as such. **A placement not made is
//! never answered `OK`**: the reply to `a=p` and `a=T` is the placement's
//! outcome, and a full table, on screen or virtual, answers ENOSPC -- a
//! case the text names no code for and kitty, whose placements are not in a
//! table, never meets; ENOSPC is the code the text gives for storage run
//! out. kitty builds an `a=T` reply before it places
//! (`[[source: kitty-graphics-c]]`), so a refused placement there is
//! answered `OK`, practice not followed, as the text's EINVAL for a
//! relative virtual placement already was not.
//!
//! **The file mediums `t=f`, `t=t` and `t=s` are the embedder's**, through
//! [`KittyFiles`]: the payload is the path, or the shared-memory name,
//! collected to [`KITTY_PATH_MAX`] bytes; the terminator hands it out with
//! `O`, `S` and the most bytes the transmission can use
//! ([`KittyFileRequest`]), and the bytes the embedder reads go where a direct
//! payload's decoded octets go, from there on the `t=d` path. A stream no
//! embedder serves ([`KittyMediums::Direct`], the default) answers each with
//! the text's single error and touches nothing. A file medium is one chunk,
//! whatever its `m`: the text chunks the data a remote client sends, and
//! kitty reads the file at the chunk's end (`handle_add_command`,
//! `[[source: kitty-graphics-c]]`). **Once the reader is asked (a load still
//! open at the terminator), every refusal is that one error** -- missing,
//! unreadable, special, sensitive, past the bound, "smaller than the client
//! claimed", and as much a PNG that does not decode, an `IHDR` past the arena
//! or a placement refused after the decode, each of which tells the client
//! the file exists and something of what it holds: "the terminal emulator
//! must not allow such a program to use the responses to these commands to
//! learn anything about files it cannot read itself" ("The transmission
//! medium"). Each keeps nothing, as a missing file does: the image is freed
//! and the cursor stays, so a file medium's image is decoded whole at its
//! terminator and only then placed, and the reply comes before the next byte.
//! kitty answers a decode failure EBADPNG (practice not followed). A refusal
//! decided before the read, from the control data and the path alone, keeps
//! its code ([`KittyFileRead`]).
//!
//! **A virtual placement (`U=1`) is a prototype, kept apart from the
//! placements on screen** (`[[source: kitty-graphics]]`, "Unicode
//! placeholders"): "Virtual placements are invisible and only play the role
//! of prototypes for real images", so it is held in the stream's own table
//! ([`KITTY_VIRTUALS`]), never in the store's placement table, whose every
//! scroll, erase and expiry is about a place on the screen. It holds a
//! reference on its image ([`ImageStore::hold`]), so the quota does not
//! delete an image placeholders show. It moves no cursor, whatever `C`
//! says, having "no physical location on the screen", and draws the whole
//! image fitted into its `c` x `r` cells with its aspect kept and centred,
//! `x`, `y`, `w`, `h`, `X` and `Y` unused (kitty's `grman_put_cell_image`,
//! `[[source: kitty-graphics-c]]`). Only `d=i`, `I`, `n`, `N`, `r` and `R`
//! delete one; the same image and placement id replace one. The placeholder
//! cells that show it are `placeholder.rs`'s.
//!
//! **A relative placement (`P`, `Q`, `H`, `V`) is placed against its
//! parent** ("Relative placements"): at the parent's top-left cell plus
//! the offsets, in the store's table with a link to its parent
//! ([`KittyParent`]), which the store keeps it at as the parent moves and
//! deletes it with ([`ImageStore::kitty_root_of`]). Its parent is named in
//! [`KittyStream::place`] in kitty's order, and refused with kitty's codes:
//! ENOPARENT, EINVAL, ECYCLE, ETOODEEP. A virtual parent is in this
//! stream's table, which tells the store when one goes, and for a delete
//! by a place is told where its placeholders are on the screen
//! ([`KittyStream::mark_drawn`]), so a placement under it is tested where
//! it is drawn.
//!
//! **Animation** ("Animation"): an `a=f` frame's data is transmitted as an
//! image's is, into an image of its own with no name, and at its last
//! chunk -- or when its deferred decode completes -- composed into its
//! image's frame ([`ImageStore::kitty_frame`]) and answered with the
//! frame's number in `r`. `a=c` composes ([`ImageStore::kitty_compose`]),
//! `d=f` and `d=F` delete a frame ([`ImageStore::kitty_delete_frame`]),
//! and `a=a` sets the state the store's clock runs
//! ([`ImageStore::kitty_animation`], [`ImageStore::animate`]), answered
//! only with an error, as kitty answers it.

use crate::KittyBase64;
use crate::image::{
    FrameBlend, FrameCompose, FrameControl, FrameDeleted, FrameDepth, FrameLoad, FrameRefusal,
};
use crate::image::{
    ImageAlpha, ImageId, ImageOwner, ImagePx, ImageRefusal, ImageScreen, ImageStore, KittyFit,
    KittyFree, KittyId, KittyNumber, KittyParent, KittyPlace, KittyPlacement, LinkRefusal,
    ParentOn, Pixel, PlaceSerial, PlacementSlot, ZIndex,
};
use crate::png::{Png, PngError, PngProgress, PngSteps};
use crate::pos::LineIdx;
use crate::quantity::{Col, Cols, Rows};
use crate::style::Chan;
use crate::zlib::Inflate;
use zerocopy::IntoBytes;

/// The overall action, the `a` key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyAction {
    /// `t`, the default: transmit.
    Transmit,
    /// `T`: transmit and display.
    TransmitPut,
    /// `q`: load and answer, storing nothing.
    Query,
    /// `p`: display an image already transmitted.
    Put,
    /// `d`: delete.
    Delete,
    /// `f`: an animation frame.
    Frame,
    /// `a`: control an animation.
    Animate,
    /// `c`: compose animation frames.
    Compose,
}

/// The pixel format, the `f` key: 24, 32 (the default) or 100.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyFormat {
    Rgb,
    Rgba,
    Png,
}

impl KittyFormat {
    fn bytes_per_pixel(self) -> usize {
        match self {
            KittyFormat::Rgb => 3,
            KittyFormat::Rgba | KittyFormat::Png => 4,
        }
    }
}

/// The transmission medium, the `t` key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyMedium {
    Direct,
    File,
    TempFile,
    SharedMemory,
}

/// The `o` key: no compression, or zlib.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyCompression {
    None,
    Zlib,
}

/// The `q` key: which replies are sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyQuiet {
    /// 0: every reply.
    Loud,
    /// 1: errors only.
    ErrorsOnly,
    /// 2: none.
    Silent,
}

/// The `m` key: whether more chunks follow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyMore {
    Last,
    More,
}

/// The `C` key: whether a placement moves the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyCursor {
    Move,
    Stay,
}

/// What a delete names, the `d` key's letter without its case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyDelete {
    /// `a`: every placement on screen.
    Visible,
    /// `i`: by image id, and placement id.
    Id,
    /// `n`: the newest image with a number, and placement id.
    Number,
    /// `c`: what covers the cursor's cell.
    Cursor,
    /// `f`: animation frames.
    Frames,
    /// `p`: what covers a cell.
    Cell,
    /// `q`: what covers a cell at a z-index.
    CellZ,
    /// `r`: images whose id is in a range.
    Range,
    /// `x`: what covers a column.
    Column,
    /// `y`: what covers a row.
    Row,
    /// `z`: placements at a z-index.
    Z,
}

/// A refusal, as a reply's message: a code and a fixed text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyError {
    code: &'static str,
    text: &'static str,
}

/// **What a short transmission had**, in bytes: kitty's ENODATA names both,
/// "Insufficient image data: <have> < <need>" (`ABRT_INSUFFICIENT_DATA`,
/// `[[source: kitty-graphics-c]]`). Kept on the stream for the one reply
/// that says it, not in [`KittyError`]: carried there it widened every
/// load and command the stream copies, and a raw `f=32` upload of a
/// 776x488 picture read 0.90 ms [0.90-0.92] against the base's 0.85
/// [0.84-0.86], and 0.85 [0.85-0.87] kept here (`kitty_image`, medians of
/// five runs of three rounds of twenty, the arms rotated, the first run
/// dropped; the stored pixels' FNV-1a equal on all three).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shortfall {
    Unsaid,
    Bytes { have: usize, need: usize },
}

impl KittyError {
    const fn new(code: &'static str, text: &'static str) -> KittyError {
        KittyError { code, text }
    }

    /// "Specifying both i and I keys in any command is an error. The terminal
    /// must reply with an EINVAL error message".
    pub const BOTH_IDS: KittyError =
        KittyError::new("EINVAL", "Must not specify both image id and image number");
    /// The spec's single error for every file medium, kitty's string.
    pub const FILE: KittyError = KittyError::new("EBADF", "Failed to read image file");
    pub const NO_IMAGE: KittyError =
        KittyError::new("ENOENT", "Put command refers to non-existent image");
    pub const NO_DATA: KittyError = KittyError::new("ENODATA", "Insufficient image data");
    pub const TOO_MUCH: KittyError = KittyError::new("EFBIG", "Too much data");
    pub const NO_SPACE: KittyError =
        KittyError::new("ENOSPC", "Failed to store image data in the image arena");
    pub const TOO_LARGE: KittyError =
        KittyError::new("EINVAL", "Image too large for the image arena");
    pub const ZERO_SIZE: KittyError = KittyError::new("EINVAL", "Zero width/height not allowed");
    pub const CLEARED: KittyError =
        KittyError::new("ENOENT", "Image deleted before its decode completed");
    pub const BAD_PNG: KittyError = KittyError::new("EBADPNG", "Failed to decode PNG data");
    pub const INFLATE: KittyError = KittyError::new("EINVAL", "Failed to inflate image data");
    pub const CONTROL: KittyError = KittyError::new("EINVAL", "Malformed graphics control data");
    pub const ACTION: KittyError = KittyError::new("EINVAL", "Unknown graphics command action");
    pub const FORMAT: KittyError = KittyError::new("EINVAL", "Unknown image format");
    pub const MEDIUM: KittyError = KittyError::new("EINVAL", "Unknown transmission type");
    pub const COMPRESSION: KittyError = KittyError::new("EINVAL", "Unknown image compression");
    pub const DELETE: KittyError = KittyError::new("EINVAL", "Unknown delete action");
    pub const PNG_SIZE: KittyError =
        KittyError::new("EINVAL", "PNG data size too large for the image scratch");
    /// "If a client attempts to make a virtual placement relative the
    /// terminal must respond with the EINVAL error code", kitty's text.
    pub const VIRTUAL_RELATIVE: KittyError = KittyError::new(
        "EINVAL",
        "Put command creating a virtual placement cannot refer to a parent",
    );
    /// "If a client attempts to create a reference to a placement that
    /// does not exist the terminal must respond with the ENOPARENT error
    /// code": no image with the id `P` names, kitty's text.
    pub const NO_PARENT_IMAGE: KittyError = KittyError::new(
        "ENOPARENT",
        "Put command refers to a parent image that does not exist",
    );
    /// ENOPARENT: the parent image has no placement.
    pub const PARENT_UNPLACED: KittyError = KittyError::new(
        "ENOPARENT",
        "Put command refers to a parent image that has no placements",
    );
    /// ENOPARENT: no placement of the parent image has the id `Q` names.
    pub const NO_PARENT: KittyError = KittyError::new(
        "ENOPARENT",
        "Put command refers to a parent image placement that does not exist",
    );
    /// A placement named as its own parent, kitty's code and text.
    pub const SELF_PARENT: KittyError =
        KittyError::new("EINVAL", "Put command refers to itself as its own parent");
    /// "the terminal must respond with the ECYCLE error code".
    pub const CYCLE: KittyError =
        KittyError::new("ECYCLE", "This parent reference creates a cycle");
    /// An animation command naming no image, kitty's code and text.
    pub const NO_ANIMATED: KittyError =
        KittyError::new("ENOENT", "Animation command refers to non-existent image");
    /// An animation command naming no image by id or number, which
    /// kitty answers nothing ("Add frame data command without image id or
    /// number"): never sent, as a command naming no image is never
    /// answered.
    pub const FRAME_NAMELESS: KittyError =
        KittyError::new("EINVAL", "Animation command without image id or number");
    /// Frame data wider than its image, kitty's code and text.
    pub const FRAME_WIDE: KittyError =
        KittyError::new("EINVAL", "Frame width larger than image width");
    /// Taller.
    pub const FRAME_TALL: KittyError =
        KittyError::new("EINVAL", "Frame height larger than image height");
    /// A base frame `c` that does not exist.
    pub const NO_BASE: KittyError = KittyError::new("EINVAL", "No frame with that number found");
    /// No room for a frame, kitty's code and text.
    pub const FRAME_SPACE: KittyError =
        KittyError::new("ENOSPC", "Failed to cache data for image frame");
    /// "If the frames or the image are not found the terminal emulator
    /// must respond with ENOENT."
    pub const NO_SOURCE: KittyError =
        KittyError::new("ENOENT", "No source frame number exists in image");
    pub const NO_DEST: KittyError =
        KittyError::new("ENOENT", "No destination frame number exists in image");
    /// "If the rectangles go out of bounds of the image the terminal must
    /// respond with EINVAL."
    pub const DEST_OUT: KittyError =
        KittyError::new("EINVAL", "The destination rectangle is out of bounds");
    pub const SOURCE_OUT: KittyError =
        KittyError::new("EINVAL", "The source rectangle is out of bounds");
    /// "If the source and destination frames are the same and the
    /// rectangles overlap, the terminal must respond with EINVAL."
    pub const OVERLAP: KittyError = KittyError::new(
        "EINVAL",
        "The source and destination rectangles overlap and the src and destination frames are the same",
    );
    /// "the terminal must respond with the ETOODEEP error code".
    pub const TOO_DEEP: KittyError =
        KittyError::new("ETOODEEP", "Too many levels of parent references");
    /// **A placement the store's full table refuses.** The text names no
    /// code for it, and kitty has no such table to fill; `ENOSPC` is the
    /// code it names for storage that has run out ("there is not enough
    /// storage space, kitty will respond with ENOSPC"), in its error form.
    pub const PLACEMENTS_FULL: KittyError =
        KittyError::new("ENOSPC", "No room for another placement");
    /// A virtual placement the stream's full table ([`KITTY_VIRTUALS`])
    /// refuses, as [`KittyError::PLACEMENTS_FULL`].
    pub const VIRTUALS_FULL: KittyError =
        KittyError::new("ENOSPC", "No room for another virtual placement");
    /// A path past [`KITTY_PATH_MAX`]: kitty's code and text
    /// (`load_image_data`), which say nothing of any file.
    pub const PATH: KittyError = KittyError::new("EINVAL", "Filename too long");

    /// The code, `EINVAL` and the like.
    pub fn code(self) -> &'static str {
        self.code
    }
}

/// **One command's control data**, parsed: every key of the "Control data
/// reference" table with its default where it was absent. "All integers are
/// 32-bit"; a value past that, a value outside its key's set, or an entry
/// with no `=` is a malformed command, kept in [`KittyCommand::error`] while
/// the rest is still read, so the reply can name the image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyCommand {
    /// `a`, `None` where absent (which transmits).
    pub action: Option<KittyAction>,
    pub quiet: KittyQuiet,
    pub format: KittyFormat,
    pub medium: KittyMedium,
    /// `s`, the width of the data sent.
    pub width: u32,
    /// `v`, its height.
    pub height: u32,
    /// `S`, the size of the data.
    pub size: u32,
    /// `O`, the offset to read from (files only).
    pub offset: u32,
    pub id: KittyId,
    pub number: KittyNumber,
    pub placement: KittyPlacement,
    pub compression: KittyCompression,
    pub more: KittyMore,
    /// `x`; in a delete, a 1-based column (or the range's low id).
    pub x: u32,
    /// `y`; in a delete, a 1-based row (or the range's high id).
    pub y: u32,
    /// `w`, the source rectangle's width.
    pub w: u32,
    /// `h`, its height.
    pub h: u32,
    /// `X`, the offset inside the first cell.
    pub cell_x: u32,
    /// `Y`.
    pub cell_y: u32,
    /// `c`, the columns to display over.
    pub cols: u32,
    /// `r`, the rows to display over.
    pub rows: u32,
    pub cursor: KittyCursor,
    /// `C` as sent: in `a=c`, the composition mode, "`1` means a simple
    /// overwrite".
    pub compose: u32,
    /// `U`: non-zero for a Unicode placeholder's virtual placement.
    pub virtual_place: u32,
    /// `z`.
    pub z: i32,
    /// `d`'s letter, and whether its case frees the data.
    pub delete: (KittyDelete, KittyFree),
    /// `P`, a parent image for a relative placement.
    pub parent: u32,
    /// `Q`.
    pub parent_placement: u32,
    /// `H`, the cells right of the parent's top-left cell.
    pub parent_x: i32,
    /// `V`, the cells below it.
    pub parent_y: i32,
    /// The first malformed key, if any.
    pub error: Option<KittyError>,
    /// Whether a file medium's reader was asked: from then on every
    /// refusal is the single error.
    pub read: KittyFileRead,
}

/// **Whether a file medium was read**: once the embedder's reader has been
/// asked, any refusal -- a read that failed, a PNG that does not decode, an
/// `IHDR` past the arena, a placement refused after the decode -- would
/// tell the client the file exists and something of what it holds, so
/// every one is the text's single error ("all failures to read an image
/// file ... must be reported with a single, identical, error response",
/// "The transmission medium"), and keeps nothing: the image is freed, as
/// a missing file leaves none. A refusal decided before the read, from
/// the control data and the path alone, keeps its own code: it says
/// nothing of any file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyFileRead {
    /// `t=d`, or a file medium refused before its reader was asked.
    Unasked,
    /// A file medium whose reader was asked.
    Asked,
}

impl KittyCommand {
    /// Every key at its default.
    pub const DEFAULT: KittyCommand = KittyCommand {
        action: None,
        quiet: KittyQuiet::Loud,
        format: KittyFormat::Rgba,
        medium: KittyMedium::Direct,
        width: 0,
        height: 0,
        size: 0,
        offset: 0,
        id: KittyId::NONE,
        number: KittyNumber::NONE,
        placement: KittyPlacement::NONE,
        compression: KittyCompression::None,
        more: KittyMore::Last,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        cell_x: 0,
        cell_y: 0,
        cols: 0,
        rows: 0,
        cursor: KittyCursor::Move,
        compose: 0,
        virtual_place: 0,
        z: 0,
        delete: (KittyDelete::Visible, KittyFree::Placements),
        parent: 0,
        parent_placement: 0,
        parent_x: 0,
        parent_y: 0,
        error: None,
        read: KittyFileRead::Unasked,
    };

    /// The action this command performs: `t` where `a` was absent.
    pub fn act(&self) -> KittyAction {
        self.action.unwrap_or(KittyAction::Transmit)
    }

    /// Parse the control data, the bytes before `;`. "trailing or leading
    /// commas are undefined implementations may ignore them", and an empty
    /// entry is ignored here; a key the table does not list is ignored too,
    /// as a key from a later revision of the protocol would be.
    pub fn parse(ctrl: &[u8]) -> KittyCommand {
        let mut c = KittyCommand::DEFAULT;
        for entry in ctrl.split(|&b| b == b',') {
            if entry.is_empty() {
                continue;
            }
            let (&key, rest) = match entry.split_first() {
                Some(kv) => kv,
                None => continue,
            };
            let value = match rest.split_first() {
                Some((b'=', v)) => v,
                _ => {
                    c.fail(KittyError::CONTROL);
                    continue;
                }
            };
            c.key(key, value);
        }
        if !c.id.is_none() && !c.number.is_none() {
            c.fail(KittyError::BOTH_IDS);
        }
        c
    }

    fn fail(&mut self, e: KittyError) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }

    fn key(&mut self, key: u8, v: &[u8]) {
        let one = match v {
            [b] => Some(*b),
            _ => None,
        };
        match key {
            b'a' => {
                self.action = match one {
                    Some(b't') => Some(KittyAction::Transmit),
                    Some(b'T') => Some(KittyAction::TransmitPut),
                    Some(b'q') => Some(KittyAction::Query),
                    Some(b'p') => Some(KittyAction::Put),
                    Some(b'd') => Some(KittyAction::Delete),
                    Some(b'f') => Some(KittyAction::Frame),
                    Some(b'a') => Some(KittyAction::Animate),
                    Some(b'c') => Some(KittyAction::Compose),
                    _ => return self.fail(KittyError::ACTION),
                }
            }
            b't' => {
                self.medium = match one {
                    Some(b'd') => KittyMedium::Direct,
                    Some(b'f') => KittyMedium::File,
                    Some(b't') => KittyMedium::TempFile,
                    Some(b's') => KittyMedium::SharedMemory,
                    _ => return self.fail(KittyError::MEDIUM),
                }
            }
            b'o' => {
                self.compression = match one {
                    Some(b'z') => KittyCompression::Zlib,
                    _ => return self.fail(KittyError::COMPRESSION),
                }
            }
            b'd' => {
                let Some(b) = one else {
                    return self.fail(KittyError::DELETE);
                };
                let free = match b.is_ascii_uppercase() {
                    true => KittyFree::Data,
                    false => KittyFree::Placements,
                };
                let what = match b.to_ascii_lowercase() {
                    b'a' => KittyDelete::Visible,
                    b'i' => KittyDelete::Id,
                    b'n' => KittyDelete::Number,
                    b'c' => KittyDelete::Cursor,
                    b'f' => KittyDelete::Frames,
                    b'p' => KittyDelete::Cell,
                    b'q' => KittyDelete::CellZ,
                    b'r' => KittyDelete::Range,
                    b'x' => KittyDelete::Column,
                    b'y' => KittyDelete::Row,
                    b'z' => KittyDelete::Z,
                    _ => return self.fail(KittyError::DELETE),
                };
                self.delete = (what, free);
            }
            b'z' => match signed(v) {
                Some(n) => self.z = n,
                None => self.fail(KittyError::CONTROL),
            },
            // `H` and `V`, a relative placement's offsets, signed: "Negative
            // values move left and up".
            b'H' => match signed(v) {
                Some(n) => self.parent_x = n,
                None => self.fail(KittyError::CONTROL),
            },
            b'V' => match signed(v) {
                Some(n) => self.parent_y = n,
                None => self.fail(KittyError::CONTROL),
            },
            _ => {
                let Some(n) = unsigned(v) else {
                    // A key the table does not list is ignored whatever its
                    // value; a listed one with a bad number is malformed.
                    if b"qfsvSOiIpmNxywhXYcrCUPQ".contains(&key) {
                        self.fail(KittyError::CONTROL);
                    }
                    return;
                };
                match key {
                    b'q' => {
                        self.quiet = match n {
                            0 => KittyQuiet::Loud,
                            1 => KittyQuiet::ErrorsOnly,
                            _ => KittyQuiet::Silent,
                        }
                    }
                    b'f' => {
                        self.format = match n {
                            24 => KittyFormat::Rgb,
                            32 => KittyFormat::Rgba,
                            100 => KittyFormat::Png,
                            _ => return self.fail(KittyError::FORMAT),
                        }
                    }
                    b's' => self.width = n,
                    b'v' => self.height = n,
                    b'S' => self.size = n,
                    b'O' => self.offset = n,
                    b'i' => self.id = KittyId::of(n),
                    b'I' => self.number = KittyNumber::of(n),
                    b'p' => self.placement = KittyPlacement::of(n),
                    b'm' => {
                        self.more = match n {
                            0 => KittyMore::Last,
                            _ => KittyMore::More,
                        }
                    }
                    b'x' => self.x = n,
                    b'y' => self.y = n,
                    b'w' => self.w = n,
                    b'h' => self.h = n,
                    b'X' => self.cell_x = n,
                    b'Y' => self.cell_y = n,
                    b'c' => self.cols = n,
                    b'r' => self.rows = n,
                    b'C' => {
                        self.compose = n;
                        self.cursor = match n {
                            1 => KittyCursor::Stay,
                            _ => KittyCursor::Move,
                        }
                    }
                    b'U' => self.virtual_place = n,
                    b'P' => self.parent = n,
                    b'Q' => self.parent_placement = n,
                    // `N`, the usage hints, which a terminal "is also free to
                    // ignore".
                    _ => {}
                }
            }
        }
    }
}

/// A decimal `u32`, or `None` past 32 bits or with any other byte.
fn unsigned(v: &[u8]) -> Option<u32> {
    if v.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &b in v {
        let d = b.checked_sub(b'0').filter(|d| *d <= 9)?;
        n = n.checked_mul(10)?.checked_add(u32::from(d))?;
    }
    Some(n)
}

/// A decimal `i32`, with an optional `-`.
fn signed(v: &[u8]) -> Option<i32> {
    match v.split_first() {
        Some((b'-', rest)) => {
            let n = i64::from(unsigned(rest)?);
            i32::try_from(-n).ok()
        }
        _ => i32::try_from(unsigned(v)?).ok(),
    }
}

/// The longest reply's body: `G`, four numbers of ten digits with their
/// keys -- `i`, `I`, `p` and an animation command's `r` -- `;` and the
/// longest message ([`KittyError::OVERLAP`]'s). The core frames it as
/// `APC` ... `ST` in the controls S7C1T/S8C1T chose, at most four bytes
/// more.
const REPLY_MAX: usize = 188;

/// Replies owed, in order, each its length in a byte and its body: two at
/// most per call -- a pending decode's, finished before a new command, and
/// the new command's.
const OUTBOX: usize = 2 * (1 + REPLY_MAX);

const _: () = assert!(
    REPLY_MAX <= u8::MAX as usize,
    "a body's length fits its byte"
);

/// The control data kept, past which a command is malformed.
pub const KITTY_CONTROL_MAX: usize = 256;

/// **The longest body a chunk may reach with no terminator yet**, from its
/// `G` on, keys and payload: kitty's `MAX_ESCAPE_CODE_LENGTH`, a quarter of
/// its 1 MiB read buffer, past which "escape code too long ... ignoring it"
/// drops the string (its accumulate_st_terminated_esc_code, `[[source:
/// kitty-vt-parser-c]]`). kitty measures it only when a read ends with the
/// string still open, so where its parser resumes depends on its reads;
/// here the chunk is dropped at the byte that passes it and the rest is
/// ignored up to the terminator, wherever the reads fell -- a stated
/// difference from kitty. The other: a continuation past it that feeds an
/// inflate (`o=z`) drops its upload, where kitty ignores that chunk alone.
pub const KITTY_BODY_MAX: usize = 262_144;

/// Decoded octets held for one inflate feed: a feed per quantum costs the
/// inflate's setup three bytes at a time.
const STAGE: usize = 512;

/// Octets [`KittyStream::put_run`] decodes into the stack before it hands
/// them on: 2048 encoded bytes a call.
const RUN_OCTETS: usize = 1536;

/// The work [`KittyStream::step`] may do in one call, in bytes: compressed
/// input fed, or scanline and pixel bytes converted. A newtype so an
/// embedder's budget cannot be read as a pixel count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecodeBudget(usize);

impl DecodeBudget {
    /// No bound: decode to completion.
    pub const ALL: DecodeBudget = DecodeBudget(usize::MAX);

    /// A mebibyte of work: what the app spends a display-link tick on.
    pub const MIB: DecodeBudget = DecodeBudget(1 << 20);

    /// About `bytes` bytes of work, at least one step.
    pub fn of(bytes: usize) -> DecodeBudget {
        DecodeBudget(bytes.max(1))
    }
}

/// Whether a deferred decode remains after a step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeProgress {
    /// Nothing is pending.
    Idle,
    /// A decode is pending: call again.
    More,
}

/// The longest path or shared-memory name a file medium may carry, in
/// decoded bytes: kitty's bound (`load_image_data`, "Filename too long").
pub const KITTY_PATH_MAX: usize = 2048;

/// Which mediums the stream serves: `t=d` alone, or the file mediums too,
/// through the [`KittyFiles`] its embedder lends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyMediums {
    /// `t=d` alone: `t=f`, `t=t` and `t=s` answer the single error at once.
    Direct,
    /// The file mediums too, read by the embedder at the terminator.
    Local,
}

/// What a read does with a file longer than the transmission can use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyPastLimit {
    /// Read to the limit: raw pixels, whose bytes past `s * v` would be
    /// dropped as a direct payload's are, so reading them is pointless
    /// (kitty's `max_to_read`).
    Stop,
    /// Refuse before reading: a PNG or zlib data, which past the bound
    /// cannot decode into what the store was given.
    Refuse,
}

/// **The one error a file medium answers**, whatever went wrong: the
/// embedder writes the reason where only the local user reads it, as kitty
/// writes it to its log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyFileFailed;

/// Whether the read goes on after a chunk: `Stop` once the transmission is
/// refused, so a file is not read past a refusal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyFlow {
    More,
    Stop,
}

/// **What a file medium asks the embedder to read**: the medium, the
/// decoded payload -- a path for `t=f` and `t=t`, a shared-memory name for
/// `t=s` -- the `O` and `S` keys, and the most bytes the transmission can
/// use, decided before anything is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyFileRequest<'a> {
    pub medium: KittyMedium,
    /// The payload's bytes, not checked as UTF-8: a POSIX path is bytes.
    pub path: &'a [u8],
    /// `O`: where to start.
    pub offset: u32,
    /// `S`: how many bytes to read, zero for the rest of the object.
    pub size: u32,
    /// The most the transmission can use, and what a longer read does.
    pub limit: usize,
    pub past: KittyPastLimit,
}

impl KittyFileRequest<'_> {
    /// The bytes from `O` to the end of an object of `len` bytes: none when
    /// `O` is past the end, as kitty reads it (`read_img_file`).
    pub fn available(&self, len: u64) -> u64 {
        len.saturating_sub(u64::from(self.offset))
    }

    /// **How many bytes to read** from `available` bytes past `O`, decided
    /// before reading: `S` when given, no more than the object holds; the
    /// rest of the object otherwise; and past
    /// [`KittyFileRequest::limit`], the limit or a refusal as
    /// [`KittyFileRequest::past`] says. A claim past the object is a
    /// bound, as kitty reads it: `if (sz > max_to_read) sz = max_to_read`
    /// and a read that stops at the end of the file
    /// (`[[source: kitty-graphics-c]]` `read_img_file`), the image the
    /// bytes leave short judged after, the one file error. kitten's
    /// detection leans on it, sending `S` as its `t=t` path's length, 43
    /// for a three-byte pixel (`[[source: kitty-icat-detect]]`).
    pub fn read_len(&self, available: u64) -> Result<usize, KittyFileFailed> {
        let want = match self.size {
            0 => available,
            s => u64::from(s).min(available),
        };
        let want = usize::try_from(want).unwrap_or(usize::MAX);
        match (want > self.limit, self.past) {
            (false, _) => Ok(want),
            (true, KittyPastLimit::Stop) => Ok(self.limit),
            (true, KittyPastLimit::Refuse) => Err(KittyFileFailed),
        }
    }
}

/// **The embedder's half of the file mediums.** Called at the terminator of
/// a `t=f`, `t=t` or `t=s` command, inside the read that carried it, so the
/// reply is owed before the next byte is parsed -- "terminal emulators that
/// support the graphics protocol, must reply to query actions immediately
/// without processing other input" ("Querying support") -- and a placement
/// moves the cursor before the text after it. The core never opens a file:
/// it hands out the request and takes the bytes.
pub trait KittyFiles {
    /// Read what `req` names, handing the bytes to `sink` in order, as many
    /// calls as the embedder likes, and stopping when `sink` answers
    /// [`KittyFlow::Stop`]. Any failure is [`KittyFileFailed`]. Deleting a
    /// `t=t` file and unlinking a `t=s` object are the embedder's too.
    fn read(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed>;
}

/// The compressed bytes a zlib stream of `n` inflated bytes can take: an
/// eighth over and 64 bytes, past what stored blocks (five bytes a 65535),
/// the header and the Adler-32 (six) add.
fn zlib_bound(n: usize) -> usize {
    n.saturating_add(n / 8).saturating_add(64)
}

/// Where the terminal is when a command completes: what a placement anchors
/// to and what a delete naming a cell is read against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyAt {
    /// The live screen.
    pub screen: ImageScreen,
    /// The absolute line at its window's top row.
    pub top: LineIdx,
    /// The cursor's row below `top`.
    pub row: Rows,
    /// The cursor's column.
    pub col: Col,
    /// The display columns the cursor's cell covers from `col`: one, and
    /// two on a line not single-width, where a character is drawn twice as
    /// wide. What a delete by the cursor's cell intersects.
    pub span: Cols,
    /// The screen's rows.
    pub rows: Rows,
    /// The screen's columns.
    pub cols: Cols,
    /// The cell's width in pixels; zero when the embedder has not said.
    pub cell_w: ImagePx,
    /// Its height.
    pub cell_h: ImagePx,
}

/// How far a placement moves the cursor: "to the right by the number of
/// cols in the image placement rectangle and down by the number of rows"
/// ("Controlling displayed image layout").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyMove {
    pub rows: Rows,
    pub cols: Cols,
}

/// What the protocol did, counted. Never silent: a refused command, an
/// ignored one and a cancelled chunk each have a number.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KittyCounts {
    /// Commands completed, one per command and not per chunk.
    pub commands: usize,
    /// Commands refused, whether or not a reply was sent: one naming no
    /// image is answered nothing, and `q` may silence the error.
    pub refused: usize,
    /// Commands no handler takes: none the text names reaches here since
    /// animation was built, so a count here is a defect to find.
    pub ignored: usize,
    /// Uploads abandoned before their last chunk: a PNG loading across
    /// chunks when another stream takes the scratch
    /// ([`KittyStream::yield_scratch`]), and a chunk still open when another
    /// `APC G` begins. No byte inside a command leaves it open since it is
    /// read as kitty reads it, where only `BEL` and `ESC \` end it.
    pub cancelled: usize,
    /// Images stored whole.
    pub images: usize,
    /// Animation frames made or edited by `a=f`.
    pub frames: usize,
    /// Payload octets past what `s` and `v` declared, dropped as kitty
    /// drops them.
    pub excess: usize,
    /// Replies dropped whole because the outbox had no room for them: an
    /// embedder that did not drain [`KittyStream::replies`] between calls.
    /// The stream's own count, beside the core's `ReplyCounts::overflows` for its
    /// reply buffer: a different buffer, emptied by a different caller.
    pub replies_dropped: usize,
    /// Virtual placements (`U=1`) not made because every slot of the
    /// stream's table held one ([`KITTY_VIRTUALS`]), answered ENOSPC, or
    /// because their image has no id a placeholder could name.
    pub virtuals_refused: usize,
    /// Chunks dropped for a body past [`KITTY_BODY_MAX`] with no
    /// terminator yet, kitty's "escape code too long ... ignoring it".
    pub too_long: usize,
}

/// **How many virtual placements a stream holds**: `kitten icat` makes one
/// an image, and each is a few dozen bytes in the stream's own memory. A
/// table full refuses the next and counts it
/// ([`KittyCounts::virtuals_refused`]).
pub const KITTY_VIRTUALS: usize = 256;

/// One virtual placement: its image, by slot and by the client's id, its
/// placement id, the counts `c` and `r` as given, and its `z`. Empty when
/// [`Virtual::image`] is [`ImageId::NONE`].
#[derive(Clone, Copy)]
struct Virtual {
    image: ImageId,
    id: KittyId,
    placement: KittyPlacement,
    cols: Cols,
    rows: Rows,
    z: ZIndex,
    /// When it was made, from the store's clock: a relative placement's
    /// name for it, and which of an image's placements is the oldest.
    serial: PlaceSerial,
    /// The least line and the least display column of the placeholders
    /// showing it on the live screen, found for a delete by a place
    /// ([`KittyStream::mark_drawn`]); `None` when none was found.
    drawn: Option<(LineIdx, Col)>,
}

impl Virtual {
    const EMPTY: Virtual = Virtual {
        image: ImageId::NONE,
        id: KittyId::NONE,
        placement: KittyPlacement::NONE,
        cols: Cols(0),
        rows: Rows::ZERO,
        z: ZIndex::ZERO,
        serial: PlaceSerial::NONE,
        drawn: None,
    };
}

/// **What a placeholder run draws** ([`KittyStream::virtual_of`]): the
/// image, the virtual placement's geometry -- the whole image fitted into
/// its cells, [`KittyFit::Box`] -- and its `z`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyVirtual {
    pub image: ImageId,
    pub place: KittyPlace,
    pub z: ZIndex,
    /// Its serial: what a relative placement under it names it by.
    pub serial: PlaceSerial,
}

/// Where the current chunk stands between its `G` and its terminator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Chunk {
    /// No chunk open.
    Closed,
    /// Reading control data.
    Control,
    /// Past `;`, reading payload.
    Payload,
}

/// **Where the load stood when a chunk began**, for a chunk dropped past
/// [`KITTY_BODY_MAX`]: kitty ignores that command whole, so the upload goes
/// on as if the chunk had never come. Direct raw data and a PNG's bytes
/// roll back to here; an inflate cannot, and goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChunkMark {
    /// No upload: the chunk began the command, which is dropped with it.
    Fresh,
    /// Raw pixels, as [`Load::Raw`] held them, and a continuation's `q`.
    Raw {
        image: ImageId,
        at: usize,
        carry: [u8; 4],
        held: usize,
        alpha: ImageAlpha,
        quiet: KittyQuiet,
    },
    /// A PNG's bytes in the scratch, as [`Load::Png`] held them, and a
    /// continuation's `q`: the bytes past `len` are written over by the
    /// next chunk's, and its header is checked again when it passes 24.
    Png { len: usize, quiet: KittyQuiet },
    /// A refused upload, which a dropped chunk leaves refused.
    Refused,
    /// An inflate (`o=z`), whose state does not roll back.
    Fixed,
}

/// A transmission in flight across chunks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Load {
    /// None.
    Idle,
    /// `f=24`/`f=32`, base64 straight into the span: pixels written and the
    /// bytes of the pixel in progress.
    Raw {
        image: ImageId,
        at: usize,
        carry: [u8; 4],
        held: usize,
        alpha: ImageAlpha,
    },
    /// `f=24`/`f=32` with `o=z`: inflating into the span's bytes.
    RawZ { image: ImageId },
    /// `f=100`: the PNG's bytes into the scratch.
    Png { len: usize },
    /// `f=100` with `o=z`: inflating into the scratch's first `need` bytes.
    PngZ { need: usize },
    /// Refused: chunks are consumed to the last, which replies.
    Refused(KittyError),
}

/// A finished transmission whose pixels are still to be made.
#[derive(Clone, Copy)]
enum Deferred {
    Idle,
    /// A PNG in the scratch's first `len` bytes, its scanlines after them;
    /// the decode's own state is [`KittyStream`]'s `png`.
    Png {
        image: ImageId,
        len: usize,
    },
    /// Bytes in the span becoming pixels: from the last down for `f=24`,
    /// whose four-byte pixels outgrow their three-byte source, and from the
    /// first up otherwise; `left` pixels remain.
    Convert {
        image: ImageId,
        format: KittyFormat,
        left: usize,
        alpha: ImageAlpha,
    },
}

/// **The protocol's state, in caller memory**: the chunk being read, the
/// transmission in flight, the decode deferred, and the replies owed.
/// About 21 KB, most of it two inflate states, the virtual placements'
/// table and the 2 KiB path buffer; an embedder keeps one beside
/// the Sixel stream.
pub struct KittyStream {
    ctrl: [u8; KITTY_CONTROL_MAX],
    ctrl_len: usize,
    chunk: Chunk,
    /// Whether the open chunk continues `start`'s transmission.
    continues: Continues,
    /// The command whose transmission is in flight, or just ended.
    start: KittyCommand,
    load: Load,
    b64: KittyBase64,
    /// The next ENODATA's numbers, taken by [`KittyStream::reply`].
    short: Shortfall,
    /// Where the load stood when the open chunk began.
    mark: ChunkMark,
    /// The open chunk's body so far, from its `G` on: what kitty measures
    /// against [`KITTY_BODY_MAX`].
    body: usize,
    /// The excess count when the open chunk began: a chunk dropped past
    /// [`KITTY_BODY_MAX`] takes back what its bytes added, kitty never having
    /// read the command.
    excess_at: usize,
    stage: [u8; STAGE],
    stage_len: usize,
    z: Inflate,
    deferred: Deferred,
    /// A deferred PNG's decode, beside `deferred` rather than in it, since
    /// it is most of the stream's size.
    png: Option<PngSteps>,
    /// The command the deferred decode answers, and where to place it.
    owed: KittyCommand,
    next_id: u32,
    outbox: [u8; OUTBOX],
    out_len: usize,
    /// **The embedder's reader for the file mediums**, behind the stream's
    /// own pointer rather than beside it in the core's `Storage`: a field
    /// there moved every field after it, and the text path's code with
    /// them. `'static` because the stream is kept in caller memory with no
    /// lifetime of its own; an embedder leaks the reader as it leaks the
    /// storage.
    files: Option<&'static mut dyn KittyFiles>,
    /// A file medium's payload, decoded: the path or the name.
    path: [u8; KITTY_PATH_MAX],
    path_len: usize,
    /// One past the highest scratch byte a PNG wrote since its pages were
    /// last given back: the PNG's bytes, inflated for `o=z`, and the
    /// scanlines its decode made after them. Given back
    /// ([`ImageStore::set_release`]) once no PNG holds the scratch.
    scratch_hi: usize,
    /// The virtual placements (`U=1`), in their own table.
    virtuals: [Virtual; KITTY_VIRTUALS],
    /// How many slots of `virtuals` are filled, so a renderer asks one
    /// compare before it looks for placeholders.
    virtuals_live: usize,
    /// What it did.
    pub counts: KittyCounts,
    /// How the caller fed it, beside what it did.
    pub feed: KittyFeed,
}

/// **How a caller handed a stream its body bytes**, kept apart from
/// [`KittyCounts`]: where the reads fell decides it, so it is not a count
/// of what the protocol did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KittyFeed {
    /// The calls that handed it body bytes, [`KittyStream::put`]'s and
    /// [`KittyStream::put_run`]'s.
    pub runs: usize,
}

/// What [`KittyStream::put`] did with a byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyByte {
    /// Taken.
    Taken,
    /// The `;` ending the control data: the command was decided, and a
    /// decode it finished first may owe a reply ([`KittyStream::replies`]).
    Separator,
}

/// Whether a chunk continues a transmission.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Continues {
    No,
    Yes,
}

impl KittyStream {
    /// Nothing begun.
    pub const NEW: KittyStream = KittyStream {
        ctrl: [0; KITTY_CONTROL_MAX],
        ctrl_len: 0,
        chunk: Chunk::Closed,
        continues: Continues::No,
        start: KittyCommand::DEFAULT,
        load: Load::Idle,
        b64: KittyBase64::NEW,
        short: Shortfall::Unsaid,
        mark: ChunkMark::Fresh,
        body: 0,
        excess_at: 0,
        stage: [0; STAGE],
        stage_len: 0,
        z: Inflate::NEW,
        deferred: Deferred::Idle,
        png: None,
        owed: KittyCommand::DEFAULT,
        next_id: 1,
        outbox: [0; OUTBOX],
        out_len: 0,
        feed: KittyFeed { runs: 0 },
        files: None,
        path: [0; KITTY_PATH_MAX],
        path_len: 0,
        scratch_hi: 0,
        virtuals: [Virtual::EMPTY; KITTY_VIRTUALS],
        virtuals_live: 0,
        counts: KittyCounts {
            commands: 0,
            refused: 0,
            ignored: 0,
            cancelled: 0,
            images: 0,
            frames: 0,
            excess: 0,
            replies_dropped: 0,
            virtuals_refused: 0,
            too_long: 0,
        },
    };

    /// Forget everything in flight: `RIS`, after the store was reset. The
    /// counts stay, and the scratch a PNG wrote is given back.
    pub fn reset(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        self.give_back(store, scratch);
        let (counts, next_id, feed) = (self.counts, self.next_id, self.feed);
        let files = self.files.take();
        *self = KittyStream::NEW;
        self.counts = counts;
        self.next_id = next_id;
        self.feed = feed;
        self.files = files;
    }

    /// **Lend the reader for the file mediums**: from here `t=f`, `t=t`
    /// and `t=s` are read through `files` at their terminator. An embedder
    /// setting, which `RIS` keeps.
    pub fn set_files(&mut self, files: &'static mut dyn KittyFiles) {
        self.files = Some(files);
    }

    /// Which mediums are served: the file mediums once a reader is lent.
    pub fn mediums(&self) -> KittyMediums {
        match self.files {
            Some(_) => KittyMediums::Local,
            None => KittyMediums::Direct,
        }
    }

    /// **The replies owed since [`KittyStream::clear_replies`]**, each a
    /// body, `G` to its message, for the caller to frame as `APC` ... `ST`
    /// in the controls S7C1T/S8C1T chose.
    pub fn replies(&self) -> KittyReplies<'_> {
        KittyReplies {
            rest: &self.outbox[..self.out_len],
        }
    }

    /// Drop the replies once the caller has sent them.
    pub fn clear_replies(&mut self) {
        self.out_len = 0;
    }

    /// **Whether a PNG holds the scratch**: one loading across chunks, or
    /// one waiting for its deferred decode. Another stream that writes the
    /// scratch -- iTerm2's inline images -- asks first.
    pub fn holds_scratch(&self) -> bool {
        matches!(self.load, Load::Png { .. } | Load::PngZ { .. })
            || matches!(self.deferred, Deferred::Png { .. })
    }

    /// **Give the scratch up to another stream**: a deferred decode is
    /// finished first, so its picture shows and its reply is owed
    /// ([`KittyStream::replies`]), and a PNG still loading across chunks is
    /// abandoned, as a cancelled chunk's upload is, and counted cancelled.
    pub fn yield_scratch(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        self.drain(store, scratch);
        if matches!(self.load, Load::Png { .. } | Load::PngZ { .. }) {
            self.counts.cancelled += 1;
            self.abort(store);
        }
        self.settle(store, scratch);
    }

    /// Whether a deferred decode is pending.
    pub fn pending(&self) -> DecodeProgress {
        match self.deferred {
            Deferred::Idle => DecodeProgress::Idle,
            _ => DecodeProgress::More,
        }
    }

    /// **An `APC G` began.** A chunk still open was abandoned with no
    /// terminator, and its upload goes with it: half a chunk is not data the
    /// client sent. Inside a command every byte but its terminator is its
    /// payload, as kitty reads it, so none leaves a chunk open.
    pub fn begin(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        if self.chunk != Chunk::Closed {
            self.counts.cancelled += 1;
            self.abort(store);
            self.settle(store, scratch);
        }
        self.chunk = Chunk::Control;
        self.ctrl_len = 0;
        // The `G`.
        self.body = 1;
        self.excess_at = self.counts.excess;
        self.mark = match self.load {
            Load::Idle => ChunkMark::Fresh,
            Load::Raw {
                image,
                at,
                carry,
                held,
                alpha,
            } => ChunkMark::Raw {
                image,
                at,
                carry,
                held,
                alpha,
                quiet: self.start.quiet,
            },
            Load::Png { len } => ChunkMark::Png {
                len,
                quiet: self.start.quiet,
            },
            Load::Refused(_) => ChunkMark::Refused,
            Load::RawZ { .. } | Load::PngZ { .. } => ChunkMark::Fixed,
        };
    }

    /// **`n` more body bytes, and whether they took the chunk past
    /// [`KITTY_BODY_MAX`]**: then the chunk is dropped and what follows up
    /// to the terminator is ignored, as kitty ignores the string ("escape
    /// code too long ... ignoring it"), and the terminator finds the chunk
    /// closed and runs nothing. kitty never saw the command, so an upload
    /// the chunk continued goes on from where it began ([`ChunkMark`]):
    /// raw pixels or PNG bytes written since are rolled back, in place and at
    /// no cost, and a PNG the chunk's bytes refused is a PNG again, since
    /// kitty never read them. An inflate cannot roll back, and its upload is
    /// dropped with the chunk, where kitty keeps it: the second stated
    /// difference, beside where the reads leave kitty after the cap.
    fn too_long(&mut self, n: usize, store: &mut ImageStore, scratch: &mut [u8]) -> bool {
        if self.chunk == Chunk::Closed {
            return true;
        }
        self.body = self.body.saturating_add(n);
        if self.body <= KITTY_BODY_MAX {
            return false;
        }
        self.counts.too_long += 1;
        match (self.mark, self.load) {
            (
                ChunkMark::Raw {
                    image,
                    at,
                    carry,
                    held,
                    alpha,
                    quiet,
                },
                Load::Raw { image: now, .. },
            ) if now == image => {
                self.load = Load::Raw {
                    image,
                    at,
                    carry,
                    held,
                    alpha,
                };
                self.start.quiet = quiet;
            }
            (ChunkMark::Png { len, quiet }, Load::Png { .. } | Load::Refused(_)) => {
                self.load = Load::Png { len };
                self.start.quiet = quiet;
            }
            (ChunkMark::Refused, Load::Refused(_)) => {}
            _ => {
                self.abort(store);
                self.settle(store, scratch);
            }
        }
        self.counts.excess = self.excess_at;
        self.b64 = KittyBase64::NEW;
        self.chunk = Chunk::Closed;
        true
    }

    /// One body byte after the `G`. The `;` that ends the control data
    /// decides the command and is [`KittyByte::Separator`].
    #[inline]
    pub fn put(&mut self, b: u8, store: &mut ImageStore, scratch: &mut [u8]) -> KittyByte {
        self.feed.runs += 1;
        if self.too_long(1, store, scratch) {
            return KittyByte::Taken;
        }
        self.take(b, store, scratch)
    }

    /// **A run of body bytes, as [`KittyStream::put`] on each would take
    /// them**: the control data a byte at a time up to its `;`, then the
    /// payload through [`KittyBase64::feed`] in one call per [`RUN_OCTETS`] of
    /// output. Returns [`KittyByte::Separator`] if the run held the `;`;
    /// whatever it decided is in [`KittyStream::replies`] after it, and the
    /// payload past it queues none.
    ///
    /// **The caller hands only bytes its parser puts in the body**, as it
    /// would to `put`: the run is the body, already free of the controls
    /// that end, cancel or are ignored in it.
    pub fn put_run(&mut self, run: &[u8], store: &mut ImageStore, scratch: &mut [u8]) -> KittyByte {
        self.feed.runs += 1;
        if self.too_long(run.len(), store, scratch) {
            return KittyByte::Taken;
        }
        let mut said = KittyByte::Taken;
        let mut at = 0;
        while let (Chunk::Control, Some(&b)) = (self.chunk, run.get(at)) {
            if self.take(b, store, scratch) == KittyByte::Separator {
                said = KittyByte::Separator;
            }
            at += 1;
        }
        if self.chunk == Chunk::Payload {
            self.payload_run(&run[at..], store, scratch);
        }
        said
    }

    /// **Payload bytes in bulk**, the octets of each [`KittyBase64::feed`]
    /// to [`KittyStream::decoded`] as the byte path hands it each quantum's:
    /// the data, or a file medium's path. A byte that is no base64 stops the
    /// chunk's data there, in bulk as a byte at a time, and the rest of the
    /// run is read and dropped; past a refusal by the octets nothing more is
    /// decoded, as on the byte path.
    fn payload_run(&mut self, mut run: &[u8], store: &mut ImageStore, scratch: &mut [u8]) {
        let mut out = [0u8; RUN_OCTETS];
        while !run.is_empty() {
            if matches!(self.load, Load::Idle | Load::Refused(_)) {
                return;
            }
            let fed = self.b64.feed(run, &mut out);
            self.decoded(&out[..fed.wrote], store, scratch);
            run = &run[fed.read..];
        }
    }

    /// One body byte, [`KittyStream::put`]'s work.
    #[inline]
    fn take(&mut self, b: u8, store: &mut ImageStore, scratch: &mut [u8]) -> KittyByte {
        match self.chunk {
            Chunk::Payload => self.payload(b, store, scratch),
            Chunk::Control if b == b';' => {
                self.control_end(store, scratch);
                return KittyByte::Separator;
            }
            Chunk::Control => {
                if self.ctrl_len < KITTY_CONTROL_MAX {
                    self.ctrl[self.ctrl_len] = b;
                }
                self.ctrl_len = self.ctrl_len.saturating_add(1);
            }
            Chunk::Closed => {}
        }
        KittyByte::Taken
    }

    /// **The chunk's terminator**: a command with no payload is parsed now,
    /// the last chunk of a transmission finishes it, and a command that is
    /// not a transmission runs. Returns how far the cursor moves.
    pub fn end(
        &mut self,
        at: &KittyAt,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Option<KittyMove> {
        let moved = self.end_chunk(at, store, scratch);
        self.settle(store, scratch);
        moved
    }

    /// **Give the scratch's pages back once no PNG holds it**: none
    /// loading, none decoding. Every way a PNG's use ends -- decoded,
    /// refused, aborted, failed -- passes a terminator, a step or a
    /// cancelling `APC G`, and each settles.
    fn settle(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        let loading = matches!(self.load, Load::Png { .. } | Load::PngZ { .. });
        let decoding = matches!(self.deferred, Deferred::Png { .. });
        if self.scratch_hi > 0 && !loading && !decoding {
            self.give_back(store, scratch);
        }
    }

    /// The scratch a PNG wrote, to the store's release.
    fn give_back(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        let hi = self.scratch_hi.min(scratch.len());
        self.scratch_hi = 0;
        if hi > 0 {
            store.release_span(&mut scratch[..hi]);
        }
    }

    /// [`KittyStream::end`]'s work.
    fn end_chunk(
        &mut self,
        at: &KittyAt,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Option<KittyMove> {
        if self.chunk == Chunk::Control {
            self.control_end(store, scratch);
        }
        if self.chunk == Chunk::Closed {
            return None;
        }
        self.chunk = Chunk::Closed;
        // Each chunk's data decodes on its own, as kitty decodes each
        // command's payload (`KittyBase64`): a short final quantum's octets
        // now, and the next chunk begins afresh.
        let tail = self.b64.end();
        if !matches!(self.load, Load::Idle | Load::Refused(_)) {
            self.decoded(tail.as_slice(), store, scratch);
        }
        let this = self.this_chunk();
        if self.continues == Continues::Yes {
            if this.act() == KittyAction::Delete && this.action.is_some() {
                // The delete aborted the upload at its control data; it
                // runs as a command of its own.
                return self.command(this, at, store, scratch);
            }
            return match this.more {
                KittyMore::More => None,
                KittyMore::Last => self.finish(at, store, scratch),
            };
        }
        match this.act() {
            KittyAction::Transmit
            | KittyAction::TransmitPut
            | KittyAction::Query
            | KittyAction::Frame => {
                // A file medium is one chunk: its `m` chunks nothing.
                match (this.more, self.start.medium) {
                    (KittyMore::More, KittyMedium::Direct) => None,
                    _ => self.finish(at, store, scratch),
                }
            }
            _ => self.command(this, at, store, scratch),
        }
    }

    /// The open chunk's own keys.
    fn this_chunk(&self) -> KittyCommand {
        KittyCommand::parse(&self.ctrl[..self.ctrl_len.min(KITTY_CONTROL_MAX)])
    }

    /// **The control data is complete**: decide whether this chunk
    /// continues a transmission or begins a command, and for a new
    /// transmission reserve where its payload goes.
    fn control_end(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        let mut this = self.this_chunk();
        if self.ctrl_len > KITTY_CONTROL_MAX {
            this.fail(KittyError::CONTROL);
        }
        self.chunk = Chunk::Payload;
        if self.load != Load::Idle {
            // A continuation: only `m` and `q` count, but a delete aborts.
            if this.act() == KittyAction::Delete && this.action.is_some() {
                self.abort(store);
                self.continues = Continues::Yes;
                return;
            }
            self.continues = Continues::Yes;
            if this.quiet != KittyQuiet::Loud {
                self.start.quiet = this.quiet;
            }
            return;
        }
        self.continues = Continues::No;
        // A new command: a decode still pending finishes first, so its reply
        // leaves before this one's.
        self.drain(store, scratch);
        match this.act() {
            KittyAction::Transmit | KittyAction::TransmitPut | KittyAction::Query => {
                self.transmit(this, store, scratch)
            }
            KittyAction::Frame => self.transmit_frame(this, store, scratch),
            _ => {}
        }
    }

    /// Begin a transmission: check what it declares, and reserve.
    fn transmit(&mut self, mut cmd: KittyCommand, store: &mut ImageStore, scratch: &mut [u8]) {
        self.b64 = KittyBase64::NEW;
        self.stage_len = 0;
        self.z = Inflate::NEW;
        let query = cmd.act() == KittyAction::Query;
        // An image nothing can name and nothing shows goes when the next
        // transmission begins, as kitty trims it (`add_trim_predicate`). A
        // query leaves the store as it found it, so it trims nothing.
        if !query {
            store.kitty_free_unplaced(|id, _| id.is_none());
        }
        // A file medium no embedder serves is refused here, before anything
        // is reserved; a served one is checked and reserved as `t=d` is,
        // its payload being the path.
        self.path_len = 0;
        if cmd.error.is_none() && cmd.medium != KittyMedium::Direct && self.files.is_none() {
            cmd.error = Some(KittyError::FILE);
        }
        if cmd.error.is_none() && !query {
            // "When re-transmitting image data for a specific id, the
            // existing image and all its placements must be deleted."
            if let Some(old) = store.kitty_image(cmd.id) {
                store.free(old);
            }
            // Its virtual placements go with it, by the id: an image freed
            // before, by a failed decode, leaves them naming no live image.
            if !cmd.id.is_none() {
                let id = cmd.id;
                self.drop_virtuals(store, KittyFree::Placements, |v| v.id == id);
            }
            if cmd.id.is_none() && !cmd.number.is_none() {
                cmd.id = self.fresh_id(store);
            }
        }
        let owner = match query {
            true => ImageOwner::Kitty(KittyId::NONE, KittyNumber::NONE),
            false => ImageOwner::Kitty(cmd.id, cmd.number),
        };
        self.start = cmd;
        if let Some(e) = cmd.error {
            self.load = Load::Refused(e);
            return;
        }
        self.load = self.load_for(cmd, owner, ImageId::NONE, store, scratch);
    }

    /// **Begin an animation frame's data** (`a=f`, "Transferring animation
    /// frame data"): "exactly the same as for transferring image data",
    /// into an image of its own with no name, sparing the image it is a
    /// frame of when space is made. Naming no image it is refused and
    /// answered nothing, as kitty answers nothing; naming one that does not
    /// exist, ENOENT at the last chunk.
    fn transmit_frame(
        &mut self,
        mut cmd: KittyCommand,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) {
        self.b64 = KittyBase64::NEW;
        self.stage_len = 0;
        self.z = Inflate::NEW;
        self.path_len = 0;
        if cmd.error.is_none() && cmd.medium != KittyMedium::Direct && self.files.is_none() {
            cmd.error = Some(KittyError::FILE);
        }
        if cmd.error.is_none() && cmd.id.is_none() && cmd.number.is_none() {
            cmd.error = Some(KittyError::FRAME_NAMELESS);
        }
        let root = frame_root(&cmd, store);
        if cmd.error.is_none() && root.is_none() {
            cmd.error = Some(KittyError::NO_ANIMATED);
        }
        // The frame's number, resolved before anything loads, as kitty
        // resolves it (`handle_animation_frame_load_command()`): every
        // reply after the image is found carries it in `r`, an error's
        // too; past the last, or none, it is the next.
        if let Some(count) = root.and_then(|r| store.kitty_frames(r)).map(|(n, _, _)| n) {
            if cmd.rows == 0 || cmd.rows > count + 1 {
                cmd.rows = count + 1;
            }
        }
        self.start = cmd;
        self.load = match (cmd.error, root) {
            (Some(e), _) => Load::Refused(e),
            (None, None) => Load::Refused(KittyError::NO_ANIMATED),
            (None, Some(root)) => {
                let nameless = ImageOwner::Kitty(KittyId::NONE, KittyNumber::NONE);
                self.load_for(cmd, nameless, root, store, scratch)
            }
        };
    }

    /// Where a transmission's payload goes: the scratch for a PNG, a span
    /// reserved for `owner` for raw pixels, deleting for space anything but
    /// `spare`.
    fn load_for(
        &mut self,
        cmd: KittyCommand,
        owner: ImageOwner,
        spare: ImageId,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Load {
        match (cmd.format, cmd.compression) {
            (KittyFormat::Png, KittyCompression::None) => Load::Png { len: 0 },
            (KittyFormat::Png, KittyCompression::Zlib) => {
                // "if you use both PNG and compression, then you must provide
                // the S key with the size of the PNG data".
                let need = usize::try_from(cmd.size).unwrap_or(usize::MAX);
                match need {
                    0 => Load::Refused(KittyError::NO_DATA),
                    n if n > scratch.len() => Load::Refused(KittyError::PNG_SIZE),
                    n => Load::PngZ { need: n },
                }
            }
            (_, compression) => {
                match self.reserve(cmd.width, cmd.height, owner, &cmd, spare, store) {
                    Err(e) => Load::Refused(e),
                    Ok(image) => match compression {
                        KittyCompression::None => Load::Raw {
                            image,
                            at: 0,
                            carry: [0; 4],
                            held: 0,
                            alpha: ImageAlpha::OpaqueOrClear,
                        },
                        KittyCompression::Zlib => Load::RawZ { image },
                    },
                }
            }
        }
    }

    /// **Reserve a `width` x `height` image**, every declared size checked
    /// against what the store was given before anything is reserved: a
    /// side past 65535 or an area past the whole arena is refused and
    /// counted without looking for a gap. A new image that does not fit
    /// deletes older images until it does, those without placements first
    /// ([`ImageStore::reserve_evicting`]), and the virtual placements of
    /// an image it deleted go with it ([`KittyStream::drop_dead_virtuals`]).
    /// A query decodes in the store's
    /// probe when it fits there, whatever the arena holds, and otherwise in
    /// a gap of the arena, deleting nothing
    /// ([`KITTY_PROBE`](crate::KITTY_PROBE)).
    fn reserve(
        &mut self,
        width: u32,
        height: u32,
        owner: ImageOwner,
        cmd: &KittyCommand,
        spare: ImageId,
        store: &mut ImageStore,
    ) -> Result<ImageId, KittyError> {
        let act = cmd.act();
        if width == 0 || height == 0 {
            store.refuse(ImageRefusal::Empty);
            return Err(KittyError::ZERO_SIZE);
        }
        let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
            store.refuse(ImageRefusal::TooLarge);
            return Err(KittyError::TOO_LARGE);
        };
        let (w, h) = (ImagePx::of(w), ImagePx::of(h));
        if act == KittyAction::Query {
            if let Some(probe) = store.reserve_probe(w, h) {
                return Ok(probe);
            }
        }
        if w.index() * h.index() > store.capacity() {
            store.refuse(ImageRefusal::Arena(w, h));
            return Err(KittyError::TOO_LARGE);
        }
        // A query stores nothing the client can name or show, so it
        // deletes nothing for space; and nothing goes that the command
        // needs: the image a frame is added to, and the parent a put names
        // with `P`, with what their placements hang under.
        let parent = match cmd.parent {
            0 => ImageId::NONE,
            p => store.kitty_image(KittyId::of(p)).unwrap_or(ImageId::NONE),
        };
        let evicted = store.images_evicted;
        let got = match act {
            KittyAction::Query => store.reserve(w, h, owner),
            _ => store.reserve_keeping(w, h, owner, [spare, parent]),
        };
        if store.images_evicted != evicted {
            self.drop_dead_virtuals(store);
        }
        got.ok_or(KittyError::NO_SPACE)
    }

    /// **The virtual placements of an image the quota deleted go too**,
    /// and every relative placement under them: their hold went with the
    /// image, and a placeholder naming its id shows nothing, as after a
    /// delete of the image. Kept, a stale entry would take a slot of the
    /// table, and would name the image's slot when a later image with its
    /// id took it.
    #[cold]
    #[inline(never)]
    fn drop_dead_virtuals(&mut self, store: &mut ImageStore) {
        for k in 0..KITTY_VIRTUALS {
            let v = self.virtuals[k];
            if v.image.is_none() || KittyStream::live(&v, store) {
                continue;
            }
            store.kitty_orphan_virtual(v.serial);
            self.virtuals[k] = Virtual::EMPTY;
            self.virtuals_live -= 1;
        }
    }

    /// An id no live image has, for an image sent with a number alone:
    /// "the terminal will reply with the id of the newly created image".
    fn fresh_id(&mut self, store: &ImageStore) -> KittyId {
        loop {
            let id = KittyId::of(self.next_id);
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if store.kitty_image(id).is_none() {
                return id;
            }
        }
    }

    /// One payload byte.
    #[inline]
    fn payload(&mut self, b: u8, store: &mut ImageStore, scratch: &mut [u8]) {
        if matches!(self.load, Load::Idle | Load::Refused(_)) {
            return;
        }
        let o = self.b64.push(b);
        self.decoded(o.as_slice(), store, scratch);
    }

    /// Decoded payload octets: the data, or a file medium's path, a byte
    /// at a time. A `copy_from_slice` here changed the code LTO made of
    /// `View::put_ascii_run`, a function this does not call, and `cat`'s
    /// instructions with it: 0.7%, on `ingest2`.
    #[inline]
    fn decoded(&mut self, bytes: &[u8], store: &mut ImageStore, scratch: &mut [u8]) {
        match self.start.medium {
            KittyMedium::Direct => self.octets(bytes, store, scratch),
            _ => {
                for &b in bytes {
                    match self.path.get_mut(self.path_len) {
                        Some(d) => *d = b,
                        None => return self.refuse_load(KittyError::PATH, store),
                    }
                    self.path_len += 1;
                }
            }
        }
    }

    /// **Read a file medium's data through the embedder**, the bytes going
    /// where a direct payload's go. The most the transmission can use is
    /// decided first: `s * v` pixels of raw data, read to that and no
    /// further; the scratch for a PNG; and the zlib bound of either inflated
    /// size for `o=z`, past which the read is refused before it starts. A
    /// PNG through zlib keeps `S` as "the size of the PNG data", which sizes
    /// the inflate as it does for `t=d`, and reads the object to its end:
    /// the two sentences about `S` meet there, and the compressed size is
    /// not the PNG's.
    fn read_file(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        let (limit, past, size) = match self.load {
            Load::Raw { .. } => (self.raw_bytes(), KittyPastLimit::Stop, self.start.size),
            Load::RawZ { .. } => (
                zlib_bound(self.raw_bytes()),
                KittyPastLimit::Refuse,
                self.start.size,
            ),
            Load::Png { .. } => (scratch.len(), KittyPastLimit::Refuse, self.start.size),
            Load::PngZ { need } => (zlib_bound(need), KittyPastLimit::Refuse, 0),
            Load::Idle | Load::Refused(_) => return,
        };
        let path = self.path;
        let req = KittyFileRequest {
            medium: self.start.medium,
            path: &path[..self.path_len],
            offset: self.start.offset,
            size,
            limit,
            past,
        };
        // Taken for the read, whose sink needs the stream, and put back
        // with nothing between that can return.
        let Some(files) = self.files.take() else {
            return self.refuse_load(KittyError::FILE, store);
        };
        let read = files.read(&req, &mut |chunk| {
            self.octets(chunk, store, scratch);
            match self.load {
                Load::Refused(_) => KittyFlow::Stop,
                _ => KittyFlow::More,
            }
        });
        self.files = Some(files);
        if read.is_err() && !matches!(self.load, Load::Refused(_)) {
            self.refuse_load(KittyError::FILE, store);
        }
    }

    /// Decoded octets, to where the load puts them.
    fn octets(&mut self, bytes: &[u8], store: &mut ImageStore, scratch: &mut [u8]) {
        match self.load {
            Load::Raw {
                image,
                mut at,
                mut carry,
                mut held,
                mut alpha,
            } => {
                let bpp = self.start.format.bytes_per_pixel();
                let Some(px) = store.decoding_mut(image) else {
                    return self.refuse_load(KittyError::NO_SPACE, store);
                };
                for &x in bytes {
                    carry[held] = x;
                    held += 1;
                    if held < bpp {
                        continue;
                    }
                    held = 0;
                    let p = match bpp {
                        3 => Pixel::opaque(chan(carry[0]), chan(carry[1]), chan(carry[2])),
                        _ => Pixel::straight(
                            chan(carry[3]),
                            chan(carry[0]),
                            chan(carry[1]),
                            chan(carry[2]),
                        ),
                    };
                    match px.get_mut(at) {
                        Some(slot) => {
                            *slot = p;
                            alpha = alpha.with(p);
                            at += 1;
                        }
                        None => self.counts.excess += bpp,
                    }
                }
                self.load = Load::Raw {
                    image,
                    at,
                    carry,
                    held,
                    alpha,
                };
            }
            Load::RawZ { .. } | Load::PngZ { .. } => {
                for &x in bytes {
                    if self.stage_len == STAGE {
                        self.flush(store, scratch);
                    }
                    self.stage[self.stage_len] = x;
                    self.stage_len += 1;
                }
            }
            Load::Png { mut len } => {
                let reach = len.saturating_add(bytes.len()).min(scratch.len());
                self.scratch_hi = self.scratch_hi.max(reach);
                for &x in bytes {
                    match scratch.get_mut(len) {
                        Some(slot) => *slot = x,
                        None => return self.refuse_load(KittyError::TOO_MUCH, store),
                    }
                    len += 1;
                    if len == 24 {
                        if let Err(e) = header_fits(&scratch[..24], self.start.act(), store) {
                            return self.refuse_load(e, store);
                        }
                    }
                }
                self.load = Load::Png { len };
            }
            Load::Idle | Load::Refused(_) => {}
        }
    }

    /// The staged octets into the inflate.
    fn flush(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        let n = self.stage_len;
        self.stage_len = 0;
        let fed = match self.load {
            Load::RawZ { image } => {
                let need = self.raw_bytes();
                let Some(px) = store.decoding_mut(image) else {
                    return self.refuse_load(KittyError::NO_SPACE, store);
                };
                self.z
                    .feed(&self.stage[..n], &mut px.as_mut_bytes()[..need])
            }
            Load::PngZ { need } => {
                let before = self.z.written();
                let fed = self.z.feed(&self.stage[..n], &mut scratch[..need]);
                self.scratch_hi = self.scratch_hi.max(self.z.written().min(need));
                if before < 24 && self.z.written() >= 24 {
                    if let Err(e) = header_fits(&scratch[..24], self.start.act(), store) {
                        return self.refuse_load(e, store);
                    }
                }
                fed
            }
            _ => return,
        };
        if fed.is_err() {
            self.refuse_load(KittyError::INFLATE, store);
        }
    }

    /// The inflated length of a raw `o=z` image: `s * v` pixels of its
    /// format's bytes.
    fn raw_bytes(&self) -> usize {
        let px = usize::try_from(self.start.width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(self.start.height).unwrap_or(0));
        px.saturating_mul(self.start.format.bytes_per_pixel())
    }

    /// Refuse the load in flight: its reserved image goes, and the chunks
    /// left are consumed to the last, which replies.
    fn refuse_load(&mut self, e: KittyError, store: &mut ImageStore) {
        if let Load::Raw { image, .. } | Load::RawZ { image } = self.load {
            store.free_decoding(image);
        }
        self.load = Load::Refused(e);
    }

    /// Abort the load in flight with no reply: a delete or a cancelled
    /// chunk.
    fn abort(&mut self, store: &mut ImageStore) {
        if let Load::Raw { image, .. } | Load::RawZ { image } = self.load {
            store.free_decoding(image);
        }
        self.load = Load::Idle;
    }

    /// **The last chunk**: the base64's final quantum, the inflate's end,
    /// and the size the data came to. A raw image is complete here; a PNG,
    /// and inflated bytes, are reserved, placed and left to
    /// [`KittyStream::step`].
    fn finish(
        &mut self,
        at: &KittyAt,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Option<KittyMove> {
        let mut cmd = self.start;
        // Marked on the copy every later reply is made from -- `done`,
        // `png_ready`, and the owed command a deferred decode answers.
        if cmd.medium != KittyMedium::Direct && !matches!(self.load, Load::Idle | Load::Refused(_))
        {
            cmd.read = KittyFileRead::Asked;
            self.read_file(store, scratch);
        }
        if matches!(self.load, Load::RawZ { .. } | Load::PngZ { .. }) {
            self.flush(store, scratch);
            if matches!(self.load, Load::RawZ { .. } | Load::PngZ { .. })
                && self.z.finish().is_err()
            {
                self.refuse_load(KittyError::INFLATE, store);
            }
        }
        let load = self.load;
        self.load = Load::Idle;
        self.counts.commands += 1;
        match load {
            Load::Idle => None,
            Load::Refused(e) => {
                self.reply(&cmd, Err(e));
                None
            }
            Load::Raw {
                image,
                at: n,
                held,
                alpha,
                ..
            } => {
                let total = store
                    .size_of(image)
                    .map_or(0, |(w, h)| w.index() * h.index());
                if n < total {
                    // An image the file's bytes leave short is the
                    // single error, which `reply` makes of this, as
                    // kitty's `ABRT_INSUFFICIENT_DATA` makes it for a file;
                    // a direct one is kitty's ENODATA with the bytes it
                    // came to, the pixel in progress included.
                    store.free(image);
                    let bpp = cmd.format.bytes_per_pixel();
                    self.short = Shortfall::Bytes {
                        have: n * bpp + held,
                        need: total * bpp,
                    };
                    self.reply(&cmd, Err(KittyError::NO_DATA));
                    return None;
                }
                store.complete(image, alpha);
                // An RGB root frame keeps no alpha a later frame could
                // give it, as kitty's three-byte frame keeps none.
                if cmd.format == KittyFormat::Rgb {
                    store.mark_rgb(image);
                }
                self.done(image, cmd, at, store)
            }
            Load::RawZ { image } => {
                let left = store
                    .size_of(image)
                    .map_or(0, |(w, h)| w.index() * h.index());
                self.defer(
                    Deferred::Convert {
                        image,
                        format: cmd.format,
                        left,
                        alpha: ImageAlpha::OpaqueOrClear,
                    },
                    cmd,
                    at,
                    store,
                    scratch,
                )
            }
            Load::Png { len } => self.png_ready(len, cmd, at, store, scratch),
            Load::PngZ { need } => self.png_ready(need, cmd, at, store, scratch),
        }
    }

    /// A PNG's bytes are all in the scratch: read its header, reserve, and
    /// defer the decode. **A file medium's is decoded now, and everything
    /// in the decode that can refuse runs before the reserve**, in the
    /// scratch alone ([`Png::reconstruct`]): the reserve deletes older
    /// images for the space, and an image that does not decode is never
    /// added, so nothing is deleted "to make space for the new one"
    /// (`[[source: kitty-graphics-rst]]`, "Image persistence and storage
    /// quotas"), as kitty decodes before it adds and applies its quota
    /// after (`handle_add_command`, `[[source: kitty-graphics-c]]`). A
    /// missing file deletes nothing either, and the one file error is all
    /// either answers.
    fn png_ready(
        &mut self,
        len: usize,
        cmd: KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Option<KittyMove> {
        let header = Png::parse(&scratch[..len]).and_then(|png| {
            let raw = png.raw_len().ok_or(PngError::Buffer)?;
            Ok((png.size(), png.steps()?, raw))
        });
        let (size, steps, raw) = match header {
            Ok(h) => h,
            Err(_) => {
                self.reply(&cmd, Err(KittyError::BAD_PNG));
                return None;
            }
        };
        if len.saturating_add(raw) > scratch.len() {
            self.reply(&cmd, Err(KittyError::TOO_MUCH));
            return None;
        }
        // After a file read a refused placement keeps no image, as a missing
        // file keeps none, so it must delete none for space either: decided
        // before the reserve, which may.
        if cmd.read == KittyFileRead::Asked && cmd.act() == KittyAction::TransmitPut {
            if let Some(e) = self.placement_refusal(&cmd, at, store) {
                self.reply(&cmd, Err(e));
                return None;
            }
        }
        let now = cmd.read == KittyFileRead::Asked;
        if now {
            // The scanlines are made in `[len, len + raw)`.
            self.scratch_hi = self.scratch_hi.max(len + raw);
            let (data, rest) = scratch.split_at_mut(len);
            if Png::reparse(data)
                .and_then(|p| p.reconstruct(rest))
                .is_err()
            {
                self.reply(&cmd, Err(KittyError::BAD_PNG));
                return None;
            }
        }
        let owner = match cmd.act() {
            KittyAction::Query | KittyAction::Frame => {
                ImageOwner::Kitty(KittyId::NONE, KittyNumber::NONE)
            }
            _ => ImageOwner::Kitty(cmd.id, cmd.number),
        };
        let spare = match cmd.act() {
            KittyAction::Frame => frame_root(&cmd, store).unwrap_or(ImageId::NONE),
            _ => ImageId::NONE,
        };
        let image = match self.reserve(size.width, size.height, owner, &cmd, spare, store) {
            Ok(i) => i,
            Err(e) => {
                self.reply(&cmd, Err(e));
                return None;
            }
        };
        if now {
            // What is left cannot refuse: the pixels, then the conversion
            // a stepped decode ends in.
            let (data, rest) = scratch.split_at_mut(len);
            let made = Png::reparse(data).and_then(|p| {
                let px = store.decoding_mut(image).ok_or(PngError::Buffer)?;
                p.write_rgba(rest, px.as_mut_bytes())
            });
            if made.is_err() {
                store.free_decoding(image);
                self.reply(&cmd, Err(KittyError::BAD_PNG));
                return None;
            }
            let left = store
                .size_of(image)
                .map_or(0, |(w, h)| w.index() * h.index());
            let d = Deferred::Convert {
                image,
                format: KittyFormat::Png,
                left,
                alpha: ImageAlpha::OpaqueOrClear,
            };
            return self.defer(d, cmd, at, store, scratch);
        }
        self.png = Some(steps);
        // The decode makes its scanlines in `[len, len + raw)`.
        self.scratch_hi = self.scratch_hi.max(len + raw);
        self.defer(Deferred::Png { image, len }, cmd, at, store, scratch)
    }

    /// **The refusal [`KittyStream::place`] would make of `cmd` for an
    /// image not yet reserved**, in its order, counted as it counts them:
    /// a relative placement, the stream's virtual table with no slot empty
    /// or naming a freed image, the store's placement table full. Exact
    /// only for a new image, which no placement of the same id can make
    /// room for; an `a=p` goes through `place` alone.
    fn placement_refusal(
        &mut self,
        cmd: &KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
    ) -> Option<KittyError> {
        if let Some(e) = unplaceable(cmd) {
            return Some(e);
        }
        if cmd.parent != 0 {
            if let Err(e) = self.parent_of(cmd, at, store, None) {
                return Some(e);
            }
        }
        if cmd.virtual_place != 0 {
            let room = cmd.id.is_none()
                || self
                    .virtuals
                    .iter()
                    .any(|v| v.image.is_none() || !KittyStream::live(v, store));
            if room {
                return None;
            }
            self.counts.virtuals_refused += 1;
            return Some(KittyError::VIRTUALS_FULL);
        }
        if store.placement_free() {
            return None;
        }
        store.placements_refused += 1;
        Some(KittyError::PLACEMENTS_FULL)
    }

    /// Hand a decode to [`KittyStream::step`]: placed now, and the cursor
    /// moved now, its pixels shown when they are made. A query is decoded
    /// to the end here, and so is a file medium's image, then placed and
    /// answered as a raw one is ([`KittyStream::done`]): a decode failing
    /// later would move the cursor for a picture never shown and answer
    /// after the bytes that follow, telling the client the file exists
    /// where a missing one answers at once and moves nothing.
    fn defer(
        &mut self,
        d: Deferred,
        cmd: KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Option<KittyMove> {
        self.deferred = d;
        self.owed = cmd;
        let image = match d {
            Deferred::Png { image, .. } | Deferred::Convert { image, .. } => image,
            Deferred::Idle => return None,
        };
        if cmd.act() == KittyAction::Query {
            self.drain(store, scratch);
            return None;
        }
        if cmd.read == KittyFileRead::Asked {
            self.drain(store, scratch);
            // A failed decode has freed its image and answered.
            return match store.image(image) {
                Some(_) => self.done(image, cmd, at, store),
                None => None,
            };
        }
        if cmd.act() != KittyAction::TransmitPut {
            return None;
        }
        // The reply waits for the decode, so a placement refused now is
        // kept in the owed command's `error`, which a command reaching here
        // has empty, for the reply to carry.
        match self.place(image, &cmd, at, store) {
            Ok(moved) => moved,
            Err(e) => {
                self.owed.error = Some(e);
                None
            }
        }
    }

    /// An image stored whole: freed again for a query, placed for `a=T`,
    /// and answered.
    fn done(
        &mut self,
        image: ImageId,
        cmd: KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
    ) -> Option<KittyMove> {
        if cmd.act() == KittyAction::Query {
            store.free(image);
            self.reply(&cmd, Ok(()));
            return None;
        }
        if cmd.act() == KittyAction::Frame {
            self.frame_done(image, cmd, store);
            return None;
        }
        // The placement first, so the reply is its outcome: an `OK` for a
        // placement not made would tell the client its picture is up.
        let placed = match cmd.act() {
            KittyAction::TransmitPut => self.place(image, &cmd, at, store),
            _ => Ok(None),
        };
        // After a file read the refusal is the single error, and the image
        // goes with it, as a missing file leaves none: an image kept would
        // answer the next `a=p` where a missing file's answers ENOENT.
        if placed.is_err() && cmd.read == KittyFileRead::Asked {
            store.free(image);
        } else {
            self.counts.images += 1;
        }
        self.reply(&cmd, placed.map(|_| ()));
        placed.ok().flatten()
    }

    /// Finish any deferred decode now.
    fn drain(&mut self, store: &mut ImageStore, scratch: &mut [u8]) {
        while self.pending() == DecodeProgress::More {
            self.step(DecodeBudget::ALL, store, scratch);
        }
    }

    /// **Advance the deferred decode** by about `budget` bytes of work; the
    /// image shows, and its reply is owed, when it completes. A decode that
    /// fails frees its image, placements and all, and replies with the error.
    pub fn step(
        &mut self,
        budget: DecodeBudget,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> DecodeProgress {
        let cmd = self.owed;
        match self.deferred {
            Deferred::Idle => {}
            Deferred::Png { image, .. } | Deferred::Convert { image, .. }
                if store.decoding_mut(image).is_none() =>
            {
                // **Cleared before it was whole** -- `ED 2` over its
                // placement, or its last placement expiring -- and its slot
                // perhaps another image's by now: nothing is freed, and the
                // command is answered ENOENT, the image it named being gone.
                self.deferred = Deferred::Idle;
                self.png = None;
                self.reply(&cmd, Err(KittyError::CLEARED));
            }
            Deferred::Png { image, len } => {
                let (data, rest) = scratch.split_at_mut(len);
                let png = &mut self.png;
                let result = Png::reparse(data).and_then(|p| {
                    let px = store.decoding_mut(image).ok_or(PngError::Buffer)?;
                    let steps = png.as_mut().ok_or(PngError::Buffer)?;
                    p.step(steps, rest, px.as_mut_bytes(), budget.0)
                });
                match result {
                    Ok(PngProgress::More) => {}
                    Ok(PngProgress::Done) => {
                        self.png = None;
                        let left = store
                            .size_of(image)
                            .map_or(0, |(w, h)| w.index() * h.index());
                        self.deferred = Deferred::Convert {
                            image,
                            format: KittyFormat::Png,
                            left,
                            alpha: ImageAlpha::OpaqueOrClear,
                        };
                    }
                    Err(_) => self.fail_deferred(image, &cmd, store, KittyError::BAD_PNG),
                }
            }
            Deferred::Convert {
                image,
                format,
                left,
                alpha,
            } => {
                let Some(px) = store.decoding_mut(image) else {
                    return self.pending();
                };
                let n = px.len();
                let k = left.min((budget.0 / 4).max(1));
                let alpha = convert(px, format, n - left, k, alpha);
                let left = left - k;
                match left {
                    0 => {
                        self.deferred = Deferred::Idle;
                        store.complete(image, alpha);
                        // An RGB root frame keeps no alpha a later frame could
                        // give it, as kitty's three-byte frame keeps none.
                        if cmd.format == KittyFormat::Rgb {
                            store.mark_rgb(image);
                        }
                        // What `defer` kept of the placement it tried.
                        let placed = cmd.error.map_or(Ok(()), Err);
                        match (cmd.act(), cmd.read) {
                            (KittyAction::Query, _) => {
                                store.free(image);
                                self.reply(&cmd, placed);
                            }
                            (KittyAction::Frame, KittyFileRead::Unasked) => {
                                self.frame_done(image, cmd, store);
                            }
                            // `defer`, draining, places and answers it.
                            (_, KittyFileRead::Asked) => {}
                            _ => {
                                self.counts.images += 1;
                                self.reply(&cmd, placed);
                            }
                        }
                    }
                    _ => {
                        self.deferred = Deferred::Convert {
                            image,
                            format,
                            left,
                            alpha,
                        }
                    }
                }
            }
        }
        self.settle(store, scratch);
        self.pending()
    }

    /// A deferred decode failed: the image goes, with its placement.
    fn fail_deferred(
        &mut self,
        image: ImageId,
        cmd: &KittyCommand,
        store: &mut ImageStore,
        e: KittyError,
    ) {
        self.deferred = Deferred::Idle;
        self.png = None;
        store.free_decoding(image);
        self.reply(cmd, Err(e));
    }

    /// **Place `image` at the cursor** with `cmd`'s geometry, and say how
    /// far the cursor moves, or why nothing was placed -- the reply the
    /// command owes. A relative placement is refused (EINVAL), a full
    /// table refuses (ENOSPC), and either moves nothing; a Unicode
    /// placeholder's virtual one is made in the stream's table and moves
    /// nothing.
    fn place(
        &mut self,
        image: ImageId,
        cmd: &KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
    ) -> Result<Option<KittyMove>, KittyError> {
        if let Some(e) = unplaceable(cmd) {
            return Err(e);
        }
        if cmd.virtual_place != 0 {
            self.place_virtual(image, cmd, store)?;
            return Ok(None);
        }
        let (iw, ih) = store.size_of(image).ok_or(KittyError::NO_IMAGE)?;
        let mut place = geometry(cmd, iw, ih, at);
        let mut anchor = (at.screen, at.top.plus(at.row), at.col);
        if cmd.parent != 0 {
            let replacing = store.kitty_slot(image, place.id);
            let parent = self.parent_of(cmd, at, store, replacing)?;
            place.parent = parent;
            // Under a virtual parent the anchor is the drawing's to find;
            // the cursor's cell is kept for what it is tested at.
            if let Some((screen, line, col, oc, ol)) = store.kitty_anchor(parent) {
                anchor = (screen, line, col);
                place.over_cols = oc;
                place.over_rows = ol;
            }
        }
        let (screen, line, col) = anchor;
        let pid = store.place_kitty(image, screen, line, col, ZIndex::of(cmd.z), place);
        if pid.is_none() {
            return Err(KittyError::PLACEMENTS_FULL);
        }
        // "the cursor must not move after a relative position, regardless
        // of the value of the C key".
        if cmd.parent != 0 {
            return Ok(None);
        }
        Ok(match cmd.cursor {
            KittyCursor::Stay => None,
            KittyCursor::Move => Some(KittyMove {
                rows: place.rows,
                cols: place.cols,
            }),
        })
    }

    /// **The parent `cmd`'s `P` and `Q` name**, checked in kitty's order
    /// (`[[source: kitty-graphics-c]]` `handle_put_command`): no image
    /// with the id `P`, then one with no placement on this screen or
    /// virtual, `Q` given or not, then none with the placement id `Q`,
    /// is ENOPARENT, each with kitty's text less the ids it prints; with
    /// no `Q`, the image's oldest placement, on screen or virtual. A
    /// placement named as its own parent is EINVAL, and a chain that
    /// would loop or run past
    /// [`KITTY_PARENT_DEPTH`](crate::KITTY_PARENT_DEPTH) parents ECYCLE
    /// or ETOODEEP. `replacing` is the slot of the placement this put
    /// replaces, if it replaces one.
    fn parent_of(
        &self,
        cmd: &KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
        replacing: Option<crate::image::PlacementId>,
    ) -> Result<KittyParent, KittyError> {
        let pid = KittyId::of(cmd.parent);
        let Some(img) = store.kitty_image(pid) else {
            return Err(KittyError::NO_PARENT_IMAGE);
        };
        let q = KittyPlacement::of(cmd.parent_placement);
        let screen = store.kitty_parent_slot(img, q, at.screen);
        let virt = self
            .virtuals
            .iter()
            .filter(|v| {
                v.id == pid && (q.is_none() || v.placement == q) && KittyStream::live(v, store)
            })
            .min_by_key(|v| v.serial);
        let base = KittyParent {
            h: cmd.parent_x,
            v: cmd.parent_y,
            ..KittyParent::NONE
        };
        let on_screen = |slot, serial| KittyParent {
            slot,
            serial,
            on: ParentOn::Screen,
            ..base
        };
        let on_virtual = |v: &Virtual| KittyParent {
            serial: v.serial,
            on: ParentOn::Virtual,
            ..base
        };
        let parent = match (screen, virt) {
            (Some((slot, serial)), Some(v)) if serial < v.serial => on_screen(slot, serial),
            (_, Some(v)) => on_virtual(v),
            (Some((slot, serial)), None) => on_screen(slot, serial),
            // kitty asks whether the image has a placement before it
            // looks for `Q`'s, so an image with none is that, `Q` or not.
            (None, None) => {
                let placed = store
                    .kitty_parent_slot(img, KittyPlacement::NONE, at.screen)
                    .is_some()
                    || self
                        .virtuals
                        .iter()
                        .any(|v| v.id == pid && KittyStream::live(v, store));
                return Err(match placed {
                    false => KittyError::PARENT_UNPLACED,
                    true => KittyError::NO_PARENT,
                });
            }
        };
        if parent.on == ParentOn::Screen && replacing == Some(parent.slot) {
            return Err(KittyError::SELF_PARENT);
        }
        match store.kitty_link_check(parent, replacing) {
            Ok(()) => Ok(parent),
            Err(LinkRefusal::Cycle) => Err(KittyError::CYCLE),
            Err(LinkRefusal::TooDeep) => Err(KittyError::TOO_DEEP),
        }
    }

    /// **A command that is not a transmission**: put, delete, or one of the
    /// animation actions this step does not build.
    fn command(
        &mut self,
        cmd: KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
        scratch: &mut [u8],
    ) -> Option<KittyMove> {
        self.counts.commands += 1;
        if let Some(e) = cmd.error {
            self.reply(&cmd, Err(e));
            return None;
        }
        match cmd.act() {
            KittyAction::Put => {
                let image = match cmd.id.is_none() {
                    false => store.kitty_image(cmd.id),
                    true => store.kitty_newest(cmd.number),
                };
                // A command with neither names nothing, and kitty answers it
                // with nothing ("Put graphics command without image id or
                // number").
                if cmd.id.is_none() && cmd.number.is_none() {
                    self.counts.refused += 1;
                    return None;
                }
                let Some(image) = image.filter(|i| store.image(*i).is_some()) else {
                    self.reply(&cmd, Err(KittyError::NO_IMAGE));
                    return None;
                };
                let mut answered = cmd;
                if let Some((id, _)) = store.kitty_names(image) {
                    answered.id = id;
                }
                let placed = self.place(image, &answered, at, store);
                self.reply(&answered, placed.map(|_| ()));
                placed.ok().flatten()
            }
            KittyAction::Delete => {
                self.delete(&cmd, at, store, scratch);
                None
            }
            KittyAction::Animate | KittyAction::Compose => {
                // Naming no image, kitty answers nothing ("Animation
                // command ... without image id or number").
                if cmd.id.is_none() && cmd.number.is_none() {
                    self.counts.refused += 1;
                    return None;
                }
                let Some(root) = frame_root(&cmd, store) else {
                    self.reply(&cmd, Err(KittyError::NO_ANIMATED));
                    return None;
                };
                match cmd.act() {
                    // Answered only with an error, as kitty answers it
                    // (`handle_animation_control_command()` sets no reply).
                    KittyAction::Animate => store.kitty_animation(
                        root,
                        FrameControl {
                            frame: cmd.rows,
                            gap: cmd.z,
                            current: cmd.cols,
                            state: cmd.width,
                            loops: cmd.height,
                        },
                    ),
                    _ => {
                        let blend = match cmd.compose {
                            0 => FrameBlend::Blend,
                            _ => FrameBlend::Replace,
                        };
                        let done = store.kitty_compose(
                            root,
                            FrameCompose {
                                source: cmd.rows,
                                dest: cmd.cols,
                                w: cmd.w,
                                h: cmd.h,
                                sx: cmd.cell_x,
                                sy: cmd.cell_y,
                                dx: cmd.x,
                                dy: cmd.y,
                                blend,
                            },
                        );
                        self.reply(&cmd, done.map_err(frame_error));
                    }
                }
                None
            }
            _ => {
                self.counts.ignored += 1;
                None
            }
        }
    }

    /// **An `a=f` frame's data is whole**: composed into its image's frame
    /// ([`ImageStore::kitty_frame`]) and freed, and answered with the
    /// frame's number in `r`, as kitty's `finish_command_response` adds it
    /// to an animation command's reply -- on an error the number the frame
    /// would have had.
    fn frame_done(&mut self, data: ImageId, cmd: KittyCommand, store: &mut ImageStore) {
        let mut answered = cmd;
        let result = match frame_root(&cmd, store) {
            None => Err(KittyError::NO_ANIMATED),
            Some(root) => {
                let count = store.kitty_frames(root).map_or(1, |(n, _, _)| n);
                if cmd.rows == 0 || cmd.rows > count + 1 {
                    answered.rows = count + 1;
                }
                let load = FrameLoad {
                    edit: cmd.rows,
                    base: cmd.cols,
                    x: cmd.x,
                    y: cmd.y,
                    replace: match cmd.cell_x {
                        1 => FrameBlend::Replace,
                        _ => FrameBlend::Blend,
                    },
                    background: cmd.cell_y,
                    gap: cmd.z,
                    depth: match cmd.format {
                        KittyFormat::Rgb => FrameDepth::Rgb,
                        _ => FrameDepth::Rgba,
                    },
                };
                let evicted = store.images_evicted;
                let made = store.kitty_frame(root, data, load);
                if store.images_evicted != evicted {
                    self.drop_dead_virtuals(store);
                }
                match made {
                    Ok(n) => {
                        answered.rows = n.raw();
                        self.counts.frames += 1;
                        Ok(())
                    }
                    Err(e) => Err(frame_error(e)),
                }
            }
        };
        store.free(data);
        self.reply(&answered, result);
    }

    /// **`a=d`**, every specifier the "Deleting images" table lists but the
    /// frame deletes, each as kitty's `handle_delete_command` reads it
    /// (`[[source: kitty-graphics-c]]`): a placement goes when it matches,
    /// and an image it leaves with none goes when the letter is uppercase or
    /// no id names it.
    fn delete(
        &mut self,
        cmd: &KittyCommand,
        at: &KittyAt,
        store: &mut ImageStore,
        _scratch: &mut [u8],
    ) {
        let (what, free) = cmd.delete;
        let screen = at.screen;
        let pid = cmd.placement;
        // "Virtual placements can be deleted by a deletion command only when
        // the d key is equal to i, I, r, R, n or N" -- first, so an image
        // they alone held is freed by an uppercase letter as one with no
        // placement is.
        match what {
            KittyDelete::Id => {
                let id = cmd.id;
                self.drop_virtuals(store, free, |v| {
                    !id.is_none() && v.id == id && (pid.is_none() || v.placement == pid)
                });
            }
            KittyDelete::Number => {
                if let Some(img) = store.kitty_newest(cmd.number) {
                    self.drop_virtuals(store, free, |v| {
                        v.image == img && (pid.is_none() || v.placement == pid)
                    });
                }
            }
            KittyDelete::Range => {
                let (lo, hi) = (cmd.x, cmd.y);
                self.drop_virtuals(store, free, |v| {
                    !v.id.is_none() && lo <= v.id.raw() && v.id.raw() <= hi
                });
            }
            _ => {}
        }
        // An `I`, `N` or `R` naming an image no placement shows frees it too.
        if free == KittyFree::Data && pid.is_none() {
            match what {
                KittyDelete::Id => {
                    let id = cmd.id;
                    store.kitty_free_unplaced(|i, _| !id.is_none() && i == id);
                }
                KittyDelete::Number => {
                    if let Some(img) = store.kitty_newest(cmd.number) {
                        if store.kitty_names(img).is_some() {
                            let names = store.kitty_names(img);
                            store.kitty_free_unplaced(|i, n| Some((i, n)) == names);
                        }
                    }
                }
                KittyDelete::Range => {
                    let (lo, hi) = (cmd.x, cmd.y);
                    store
                        .kitty_free_unplaced(|i, _| !i.is_none() && lo <= i.raw() && i.raw() <= hi);
                }
                _ => {}
            }
        }
        // A cell named 1-based, from the window's top-left.
        let cell = |x: u32, y: u32| -> Option<(LineIdx, usize)> {
            let row = usize::try_from(y.checked_sub(1)?).ok()?;
            let col = usize::try_from(x.checked_sub(1)?).ok()?;
            Some((at.top.plus(Rows::of_count(row)), col))
        };
        let covers_line = |p: &PlacementSlot, line: LineIdx| -> bool {
            let rows = p.kitty.shown().index();
            p.line <= line && line < p.line.plus(Rows::of_count(rows))
        };
        let covers_col = |p: &PlacementSlot, col: usize| -> bool {
            let cols = p.kitty.cols.index().max(1);
            p.col.index() <= col && col < p.col.index() + cols
        };
        let window_end = at.top.plus(at.rows);
        // Where placeholders put a virtual placement, by its serial: a
        // relative placement under one is tested where it is drawn, and
        // where none is found it is drawn nowhere and intersects nothing.
        let virtuals = &self.virtuals;
        let drawn = |serial: PlaceSerial| {
            virtuals
                .iter()
                .find(|v| !v.image.is_none() && v.serial == serial)
                .and_then(|v| v.drawn)
        };
        match what {
            KittyDelete::Visible => {
                store.kitty_delete_drawn(free, drawn, |p, _, _| {
                    let rows = p.kitty.shown().index();
                    p.screen == screen
                        && p.line < window_end
                        && p.line.plus(Rows::of_count(rows)) > at.top
                });
            }
            // With a placement id, the one placement the pair names, found
            // by it: an id names one live image, a transmission replacing
            // the image it named.
            KittyDelete::Id if !pid.is_none() => {
                if let Some(img) = store.kitty_image(cmd.id) {
                    store.kitty_delete_placement(free, img, pid);
                }
            }
            KittyDelete::Id => {
                let id = cmd.id;
                store.kitty_delete(free, |_, i, _| !id.is_none() && i == id);
            }
            KittyDelete::Number => {
                let Some(img) = store.kitty_newest(cmd.number) else {
                    return;
                };
                match pid.is_none() {
                    true => {
                        store.kitty_delete(free, |p, _, _| p.image == img);
                    }
                    false => {
                        store.kitty_delete_placement(free, img, pid);
                    }
                }
            }
            // "All placements that intersect with the current cursor
            // position": any display column of the cursor's cell.
            KittyDelete::Cursor => {
                let line = at.top.plus(at.row);
                let cols = at.col.index()..at.col.index() + at.span.index();
                store.kitty_delete_drawn(free, drawn, |p, _, _| {
                    p.screen == screen
                        && covers_line(p, line)
                        && cols.clone().any(|c| covers_col(p, c))
                });
            }
            KittyDelete::Cell | KittyDelete::CellZ => {
                let Some((line, col)) = cell(cmd.x, cmd.y) else {
                    return;
                };
                let z = (what == KittyDelete::CellZ).then_some(ZIndex::of(cmd.z));
                store.kitty_delete_drawn(free, drawn, |p, _, _| {
                    p.screen == screen
                        && covers_line(p, line)
                        && covers_col(p, col)
                        && z.is_none_or(|z| p.z == z)
                });
            }
            KittyDelete::Range => {
                let (lo, hi) = (cmd.x, cmd.y);
                store.kitty_delete(free, |_, i, _| {
                    !i.is_none() && lo <= i.raw() && i.raw() <= hi
                });
            }
            KittyDelete::Column => {
                let Some((_, col)) = cell(cmd.x, 1) else {
                    return;
                };
                store.kitty_delete_drawn(free, drawn, |p, _, _| {
                    p.screen == screen && covers_col(p, col)
                });
            }
            KittyDelete::Row => {
                let Some((line, _)) = cell(1, cmd.y) else {
                    return;
                };
                store.kitty_delete_drawn(free, drawn, |p, _, _| {
                    p.screen == screen && covers_line(p, line)
                });
            }
            KittyDelete::Z => {
                let z = ZIndex::of(cmd.z);
                store.kitty_delete(free, |p, _, _| p.z == z);
            }
            // "Delete animation frames": the frame `r`, kitty's
            // `handle_delete_frame_command()`; an image of one frame goes
            // whole by `d=F`. Naming no image, nothing, as kitty.
            KittyDelete::Frames => {
                let Some(root) = frame_root(cmd, store) else {
                    return;
                };
                let whole = match free {
                    KittyFree::Data => FrameDeleted::Image,
                    KittyFree::Placements => FrameDeleted::Nothing,
                };
                if store.kitty_delete_frame(root, cmd.rows, whole) == FrameDeleted::Image {
                    let id = store.kitty_names(root).map(|(i, _)| i);
                    store.free(root);
                    if let Some(id) = id.filter(|i| !i.is_none()) {
                        self.drop_virtuals(store, KittyFree::Placements, |v| v.id == id);
                    }
                }
            }
        }
    }

    /// Whether virtual placement `v` still names a live image by its id: an
    /// image freed under it (a failed decode) leaves it naming none, and a
    /// slot reused since names another id.
    fn live(v: &Virtual, store: &ImageStore) -> bool {
        !v.image.is_none() && store.kitty_names(v.image).is_some_and(|(i, _)| i == v.id)
    }

    /// **Make a virtual placement of `image`** (`U=1`): replacing the one of
    /// the same image and non-zero placement id, "If you send two
    /// placements with the same image id and placement id the second one
    /// will replace the first". An image no id names cannot be shown by a
    /// placeholder, and is not given one; a full table refuses (ENOSPC).
    /// Both are counted. The nameless one is answered nothing: only `a=T`
    /// with neither `i` nor `I` makes an image no id names (an `I` alone
    /// is given one), and the text replies only to a command that named
    /// an image ("If it does so, the terminal emulator will reply").
    fn place_virtual(
        &mut self,
        image: ImageId,
        cmd: &KittyCommand,
        store: &mut ImageStore,
    ) -> Result<(), KittyError> {
        let (id, _) = store.kitty_names(image).ok_or(KittyError::NO_IMAGE)?;
        if id.is_none() {
            debug_assert!(cmd.id.is_none() && cmd.number.is_none());
            self.counts.virtuals_refused += 1;
            self.counts.refused += 1;
            return Ok(());
        }
        // What names no live image is gone, without a release: its image
        // went with every reference it had.
        for v in self.virtuals.iter_mut() {
            if !v.image.is_none() && !KittyStream::live(v, store) {
                store.kitty_orphan_virtual(v.serial);
                *v = Virtual::EMPTY;
                self.virtuals_live -= 1;
            }
        }
        if !store.hold(image) {
            return Err(KittyError::NO_IMAGE);
        }
        let pid = cmd.placement;
        // A replacement keeps the serial, and so the relative placements
        // under it: "When the parent placement moves, the relative
        // placement moves along with it".
        let mut serial = None;
        if !pid.is_none() {
            for v in self.virtuals.iter_mut() {
                if !v.image.is_none() && v.image == image && v.placement == pid {
                    serial = Some(v.serial);
                    store.release(v.image, KittyFree::Placements);
                    *v = Virtual::EMPTY;
                    self.virtuals_live -= 1;
                }
            }
        }
        let count = |n: u32| usize::try_from(n).unwrap_or(usize::MAX);
        let Some(slot) = self.virtuals.iter_mut().find(|v| v.image.is_none()) else {
            store.release(image, KittyFree::Placements);
            self.counts.virtuals_refused += 1;
            return Err(KittyError::VIRTUALS_FULL);
        };
        let serial = match serial {
            Some(s) => s,
            None => store.take_serial(),
        };
        *slot = Virtual {
            image,
            id,
            placement: pid,
            cols: Cols(count(cmd.cols)),
            rows: Rows::of_count(count(cmd.rows)),
            z: ZIndex::of(cmd.z),
            serial,
            drawn: None,
        };
        self.virtuals_live += 1;
        Ok(())
    }

    /// Delete the live virtual placements `hit` picks, releasing each
    /// one's reference with `free`'s rule, and the ones naming no live
    /// image with nothing to release.
    fn drop_virtuals(
        &mut self,
        store: &mut ImageStore,
        free: KittyFree,
        hit: impl Fn(&Virtual) -> bool,
    ) {
        for v in self.virtuals.iter_mut() {
            if v.image.is_none() || !hit(v) {
                continue;
            }
            if KittyStream::live(v, store) {
                store.release(v.image, free);
            }
            store.kitty_orphan_virtual(v.serial);
            *v = Virtual::EMPTY;
            self.virtuals_live -= 1;
        }
    }

    /// Whether any virtual placement is held.
    pub fn has_virtuals(&self) -> bool {
        self.virtuals_live > 0
    }

    /// **Whether the chunk ending deletes by a place on the screen** -- a
    /// cell, a row, a column, the cursor's cell or the visible screen --
    /// while a relative placement may hang under a virtual one: the one
    /// command that needs where placeholders draw such a placement
    /// ([`KittyStream::mark_drawn`]). One parse of the control data, asked
    /// only while a virtual placement is held.
    pub fn wants_drawn(&self, store: &ImageStore) -> bool {
        if self.virtuals_live == 0 || !store.kitty_relatives() {
            return false;
        }
        let c = self.this_chunk();
        c.act() == KittyAction::Delete
            && matches!(
                c.delete.0,
                KittyDelete::Visible
                    | KittyDelete::Cursor
                    | KittyDelete::Cell
                    | KittyDelete::CellZ
                    | KittyDelete::Column
                    | KittyDelete::Row
            )
    }

    /// Forget where placeholders were found, before they are looked for.
    pub fn clear_drawn(&mut self) {
        for v in self.virtuals.iter_mut() {
            v.drawn = None;
        }
    }

    /// **A placeholder run of image `image` and placement `placement` is
    /// at `line` and display column `col`** of the live screen: the
    /// virtual placement it shows, picked as [`KittyStream::virtual_of`]
    /// picks it, keeps the least line and the least column, where "The x
    /// position is the minimum of all the placeholder x positions and the
    /// y position is the minimum of all the placeholder y positions"
    /// (`[[source: kitty-graphics]]`, "Relative placements") puts the
    /// placements under it.
    pub fn mark_drawn(
        &mut self,
        image: KittyId,
        placement: KittyPlacement,
        (line, col): (LineIdx, Col),
        store: &ImageStore,
    ) {
        if image.is_none() {
            return;
        }
        let Some(v) = self.virtuals.iter_mut().find(|v| {
            v.id == image
                && (placement.is_none() || v.placement == placement)
                && KittyStream::live(v, store)
        }) else {
            return;
        };
        v.drawn = Some(match v.drawn {
            None => (line, col),
            Some((l, c)) => (l.min(line), c.min(col)),
        });
    }

    /// **The virtual placement a placeholder run of image `image` and
    /// placement `placement` shows**: the one with that placement id, or
    /// with none the first in the table ("the terminal may choose any
    /// virtual placement of the given image"), its image live and whole.
    /// Its geometry is the whole image fitted into its cells at `at`'s cell
    /// size, aspect kept and centred; a count not given is the image's own
    /// size in cells, rounded up (kitty's `grman_put_cell_image`).
    pub fn virtual_of(
        &self,
        image: KittyId,
        placement: KittyPlacement,
        at: &KittyAt,
        store: &ImageStore,
    ) -> Option<KittyVirtual> {
        if image.is_none() {
            return None;
        }
        let v = self.virtuals.iter().find(|v| {
            v.id == image
                && (placement.is_none() || v.placement == placement)
                && KittyStream::live(v, store)
        })?;
        let (iw, ih) = store.size_of(v.image)?;
        let cells = |given: usize, px: ImagePx, cell: ImagePx| match (given, cell.raw()) {
            (0, 0) => 0,
            (0, c) => px.index().div_ceil(usize::from(c)),
            (n, _) => n,
        };
        let place = KittyPlace {
            id: v.placement,
            cols: Cols(cells(v.cols.index(), iw, at.cell_w)),
            rows: Rows::of_count(cells(v.rows.index(), ih, at.cell_h)),
            fit: KittyFit::Box,
            ..KittyPlace::NONE
        };
        Some(KittyVirtual {
            image: v.image,
            place,
            z: v.z,
            serial: v.serial,
        })
    }

    /// **`ED 2` clears the images on screen** as it clears the text: "The
    /// clear screen escape code (usually <ESC>[2J) should also clear all
    /// images" ("Interaction with other terminal actions"), freeing what it
    /// leaves unplaced as kitty's `grman_clear` does. **And on the
    /// alternate screen those below it**, which a reverse scroll pushed
    /// there and a line feed at the bottom would bring back over the
    /// cleared screen: kitty's `clear_filter_func` takes every placement
    /// not wholly above the screen (`[[source: kitty-graphics-c]]`). The
    /// primary keeps the rows under its window, their text and pictures,
    /// which `ED 2` does not erase and a taller window shows again, so a
    /// picture there stays with its row. One wholly above the screen, in
    /// the primary's history, stays, as kitty's does.
    pub fn clear_screen(&mut self, at: &KittyAt, store: &mut ImageStore) {
        let screen = at.screen;
        let end = match screen {
            ImageScreen::Primary => Some(at.top.plus(at.rows)),
            ImageScreen::Alternate => None,
        };
        store.kitty_delete(KittyFree::Data, |p, _, _| {
            let rows = p.kitty.shown().index();
            p.screen == screen
                && end.is_none_or(|e| p.line < e)
                && p.line.plus(Rows::of_count(rows)) > at.top
        });
    }

    /// Owe the reply to `cmd`: `OK`, or the error. Sent only for a command
    /// that named an image, and as its `q` allows.
    fn reply(&mut self, cmd: &KittyCommand, result: Result<(), KittyError>) {
        let short = core::mem::replace(&mut self.short, Shortfall::Unsaid);
        let result = match cmd.read {
            KittyFileRead::Unasked => result,
            KittyFileRead::Asked => result.map_err(|_| KittyError::FILE),
        };
        if result.is_err() {
            self.counts.refused += 1;
        }
        if cmd.id.is_none() && cmd.number.is_none() {
            return;
        }
        let quiet = match (cmd.quiet, result) {
            (KittyQuiet::Loud, _) => false,
            (KittyQuiet::ErrorsOnly, r) => r.is_ok(),
            (KittyQuiet::Silent, _) => true,
        };
        if quiet {
            return;
        }
        let mut r = ReplyBuf::new();
        // The body of the text's shape, "`<ESC>_Gi=<id>;OK<ESC>\\`": the core
        // frames it in the controls S7C1T/S8C1T chose.
        r.put(b"G");
        let mut first = Sep::First;
        // "`,r=`" an animation command's frame, kitty's
        // `finish_command_response`.
        let frame = match cmd.act() {
            KittyAction::Frame | KittyAction::Animate => cmd.rows,
            _ => 0,
        };
        for (key, n) in [
            (b'i', cmd.id.raw()),
            (b'I', cmd.number.raw()),
            (b'p', cmd.placement.raw()),
            (b'r', frame),
        ] {
            if n == 0 {
                continue;
            }
            if first == Sep::Later {
                r.put(b",");
            }
            first = Sep::Later;
            r.put(&[key, b'=']);
            r.num(n);
        }
        r.put(b";");
        match result {
            Ok(()) => r.put(b"OK"),
            Err(e) => {
                r.put(e.code.as_bytes());
                r.put(b":");
                r.put(e.text.as_bytes());
                if let (KittyError::NO_DATA, Shortfall::Bytes { have, need }) = (e, short) {
                    r.put(b": ");
                    r.num(u32::try_from(have).unwrap_or(u32::MAX));
                    r.put(b" < ");
                    r.num(u32::try_from(need).unwrap_or(u32::MAX));
                }
            }
        }
        let b = &r.buf[..r.len];
        if self.out_len + 1 + b.len() <= OUTBOX {
            // `ReplyBuf` holds at most `REPLY_MAX` bytes, which fits a byte.
            self.outbox[self.out_len] = u8::try_from(b.len()).unwrap_or(u8::MAX);
            for (d, s) in self.outbox[self.out_len + 1..].iter_mut().zip(b) {
                *d = *s;
            }
            self.out_len += 1 + b.len();
        } else {
            self.counts.replies_dropped += 1;
        }
    }
}

/// The replies a stream owes, each a body ([`KittyStream::replies`]).
pub struct KittyReplies<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for KittyReplies<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let (&n, rest) = self.rest.split_first()?;
        let (body, rest) = rest.split_at(usize::from(n).min(rest.len()));
        self.rest = rest;
        Some(body)
    }
}

/// Whether a separator goes before the next key of a reply.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sep {
    First,
    Later,
}

/// One reply being written.
struct ReplyBuf {
    buf: [u8; REPLY_MAX],
    len: usize,
}

impl ReplyBuf {
    fn new() -> ReplyBuf {
        ReplyBuf {
            buf: [0; REPLY_MAX],
            len: 0,
        }
    }

    fn put(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if let Some(d) = self.buf.get_mut(self.len) {
                *d = b;
                self.len += 1;
            }
        }
    }

    fn num(&mut self, n: u32) {
        let mut digits = [0u8; 10];
        let mut k = digits.len();
        let mut v = n;
        loop {
            k -= 1;
            digits[k] = b'0' + u8::try_from(v % 10).unwrap_or(0);
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.put(&digits[k..]);
    }
}

/// Why a placement `cmd` asks for is not made: a relative one (`P`), a
/// later step, and a virtual one made relative with it, which "the
/// terminal must respond with the EINVAL error code" ("Relative
/// placements"). **The image an animation command names**: by `i`, or
/// the newest with the number `I`, whole.
fn frame_root(cmd: &KittyCommand, store: &ImageStore) -> Option<ImageId> {
    let image = match cmd.id.is_none() {
        false => store.kitty_image(cmd.id),
        true if !cmd.number.is_none() => store.kitty_newest(cmd.number),
        true => None,
    };
    image.filter(|i| store.image(*i).is_some())
}

/// A frame command's refusal in kitty's code.
fn frame_error(e: FrameRefusal) -> KittyError {
    match e {
        FrameRefusal::Wide => KittyError::FRAME_WIDE,
        FrameRefusal::Tall => KittyError::FRAME_TALL,
        FrameRefusal::NoBase => KittyError::NO_BASE,
        FrameRefusal::NoSpace => KittyError::FRAME_SPACE,
        FrameRefusal::NoSource => KittyError::NO_SOURCE,
        FrameRefusal::NoDest => KittyError::NO_DEST,
        FrameRefusal::DestOut => KittyError::DEST_OUT,
        FrameRefusal::SourceOut => KittyError::SOURCE_OUT,
        FrameRefusal::Overlap => KittyError::OVERLAP,
    }
}

/// **A placement no state can make**: a virtual one with a parent,
/// "Virtual placements created for Unicode placeholder based images cannot
/// also be relative placements ... the terminal must respond with the
/// EINVAL error code" (`[[source: kitty-graphics]]`, "Relative
/// placements").
fn unplaceable(cmd: &KittyCommand) -> Option<KittyError> {
    match (cmd.parent, cmd.virtual_place) {
        (0, _) | (_, 0) => None,
        _ => Some(KittyError::VIRTUAL_RELATIVE),
    }
}

fn chan(v: u8) -> Chan {
    Chan::from_u8(v)
}

/// **The PNG's declared size, against the store, before anything is
/// reserved**: the signature, `IHDR`'s width and height from its first 24
/// bytes, a side past 65535 or an area past the whole arena refused and
/// counted -- past the arena and the query probe, for a query. Malformed
/// bytes pass here and are refused at the last chunk.
fn header_fits(head: &[u8], act: KittyAction, store: &mut ImageStore) -> Result<(), KittyError> {
    if head.get(..8) != Some(b"\x89PNG\r\n\x1a\n") || head.get(12..16) != Some(b"IHDR") {
        return Ok(());
    }
    let be = |at: usize| -> u32 {
        let mut b = [0u8; 4];
        for (d, s) in b.iter_mut().zip(&head[at..at + 4]) {
            *d = *s;
        }
        u32::from_be_bytes(b)
    };
    let (w, h) = (be(16), be(20));
    let (Ok(w), Ok(h)) = (u16::try_from(w), u16::try_from(h)) else {
        store.refuse(ImageRefusal::TooLarge);
        return Err(KittyError::TOO_LARGE);
    };
    let (w, h) = (ImagePx::of(w), ImagePx::of(h));
    let area = w.index() * h.index();
    let probe = match act {
        KittyAction::Query => store.probe_capacity(),
        _ => 0,
    };
    if area > store.capacity() && area > probe {
        store.refuse(ImageRefusal::Arena(w, h));
        return Err(KittyError::TOO_LARGE);
    }
    Ok(())
}

/// **Turn `k` pixels of a span's bytes into pixels**, from pixel `from` in
/// the order that keeps every source byte unread until it is: `f=24`'s
/// three bytes a pixel from the last pixel down (pixel `i` writes bytes
/// `4i..4i+4`, above every source byte of a lower pixel), so `from` counts
/// down from the end; RGBA from the first up. Returns the alpha mark with
/// these pixels folded in.
fn convert(
    px: &mut [Pixel],
    format: KittyFormat,
    from: usize,
    k: usize,
    mut alpha: ImageAlpha,
) -> ImageAlpha {
    let n = px.len();
    match format {
        KittyFormat::Rgb => {
            let bytes = px.as_mut_bytes();
            for done in from..from + k {
                let i = n - 1 - done;
                let (r, g, b) = (bytes[3 * i], bytes[3 * i + 1], bytes[3 * i + 2]);
                let p = Pixel::opaque(chan(r), chan(g), chan(b));
                for (d, s) in bytes[4 * i..4 * i + 4]
                    .iter_mut()
                    .zip(p.bits().to_ne_bytes())
                {
                    *d = s;
                }
            }
            alpha
        }
        KittyFormat::Rgba | KittyFormat::Png => {
            for p in &mut px[from..from + k] {
                *p = p.from_memory_rgba();
                alpha = alpha.with(*p);
            }
            alpha
        }
    }
}

/// **A placement's geometry**: the source rectangle, the cell offset, and
/// the cells it covers -- `c` and `r` as given, and a count not given the
/// cells the drawn image reaches ([`KittyPlace::draw`]), so the cursor
/// moves "by the number of cols in the image placement rectangle and ...
/// rows" it is drawn in (`[[source: kitty-graphics]]`, "Controlling
/// displayed image layout"). Kitty derives a count from the box plus the
/// offset (`[[source: kitty-graphics-c]]` `update_dest_rect`), one row
/// more than the drawn image reaches when `X` widens it, its practice and
/// not followed. With the cell size unknown, an absent count is zero and
/// moves nothing.
fn geometry(cmd: &KittyCommand, iw: ImagePx, ih: ImagePx, at: &KittyAt) -> KittyPlace {
    let px = |n: u32| ImagePx::of(u16::try_from(n).unwrap_or(u16::MAX));
    let cx = cmd.cell_x.min(u32::from(at.cell_w.raw().saturating_sub(1)));
    let cy = cmd.cell_y.min(u32::from(at.cell_h.raw().saturating_sub(1)));
    let count = |n: u32| usize::try_from(n).unwrap_or(usize::MAX);
    let fit = match (cmd.cols, cmd.rows) {
        (0, 0) => KittyFit::Natural,
        (_, 0) => KittyFit::Cols,
        (0, _) => KittyFit::Rows,
        _ => KittyFit::Box,
    };
    let mut place = KittyPlace {
        id: match cmd.id.is_none() {
            // "If you specify a placement id for an image that does not have
            // an id ... it will be ignored".
            true => KittyPlacement::NONE,
            false => cmd.placement,
        },
        src_x: px(cmd.x),
        src_y: px(cmd.y),
        src_w: px(cmd.w),
        src_h: px(cmd.h),
        cell_x: px(cx),
        cell_y: px(cy),
        cols: Cols(count(cmd.cols)),
        rows: Rows::of_count(count(cmd.rows)),
        fit,
        cut_top: Rows::ZERO,
        cut_bottom: Rows::ZERO,
        ..KittyPlace::NONE
    };
    let drawn = place.draw(iw, ih, at.cell_w, at.cell_h);
    // The source as drawn, clipped to the image, so a delete and a reader
    // see the rectangle and not the request.
    place.src_x = px(drawn.src.x);
    place.src_y = px(drawn.src.y);
    place.src_w = px(drawn.src.w);
    place.src_h = px(drawn.src.h);
    let reach = |off: u32, len: u32, cell: ImagePx| match cell.raw() {
        0 => 0,
        c => count(
            (u64::from(off) + u64::from(len))
                .div_ceil(u64::from(c))
                .try_into()
                .unwrap_or(u32::MAX),
        ),
    };
    let (d, s) = (drawn.dest, drawn.src);
    match fit {
        KittyFit::Natural => {
            place.cols = Cols(reach(cx, s.w, at.cell_w));
            place.rows = Rows::of_count(reach(cy, s.h, at.cell_h));
        }
        KittyFit::Cols => place.rows = Rows::of_count(reach(d.y, d.h, at.cell_h)),
        KittyFit::Rows => place.cols = Cols(reach(d.x, d.w, at.cell_w)),
        KittyFit::Box => {}
    }
    place
}
