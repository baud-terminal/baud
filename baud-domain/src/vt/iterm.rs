//! **iTerm2's `OSC 1337 ; key [= value]` messages**: every key iTerm2 knows,
//! in a table compiled here and looked up by an index built at compile
//! time, and the values of the keys baud acts on read to their documented
//! form (`[[source: iterm2-escape-codes]]`).
//!
//! **The key is the bytes before the first `=`**, and the value the bytes
//! after it, or none: iTerm2's `keyValuePairInToken` splits there, "key:
//! Sequence of characters not = or ^G" (`[[source:
//! iterm2-964a75d-xtermparser]]`). The table's rows are every key the pinned
//! pages name and every key iTerm2's `executeXtermSetKvp` compares
//! (`[[source: iterm2-964a75d-vt100terminal]]`), listed in `iterm_keys.rs`
//! by `scripts/gen-iterm2-keys.py`; `every_iterm2_key_has_its_row` fails
//! when a key there has no row here, or a row names a key neither lists.
//!
//! **Each row says what baud does with its key** ([`ItermDoes`]): acts on
//! it, refuses it because acting would be unsafe, has not built it, has no
//! counterpart of the iTerm2 feature it drives, or ignores it as iTerm2
//! does. Everything but the first is counted by the core, a key at a time.

use crate::*;

/// One of iTerm2's `OSC 1337` keys, in [`ROWS`]' order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItermKey {
    CursorShape,
    ShellIntegrationVersion,
    RemoteHost,
    SetMark,
    StealFocus,
    ClearScrollback,
    CurrentDir,
    SetProfile,
    AddNote,
    AddAnnotation,
    AddHiddenNote,
    AddHiddenAnnotation,
    HighlightCursorLine,
    ClearCapturedOutput,
    CopyToClipboard,
    File,
    MultipartFile,
    Block,
    UpdateBlock,
    Button,
    FilePart,
    FileEnd,
    Copy,
    RequestUpload,
    BeginFile,
    EndFile,
    EndCopy,
    RequestAttention,
    SetBackgroundImageFile,
    SetProfileProperty,
    SetBadgeFormat,
    SetUserVar,
    ReportCellSize,
    SetWindowFrame,
    ReportWindowFrame,
    ReportScreenFrames,
    UnicodeVersion,
    SetColors,
    SetKeyLabel,
    PushKeyLabels,
    PopKeyLabels,
    Disinter,
    ReportVariable,
    Custom,
    Notification,
    Capabilities,
    Env,
    It2ssh,
    SendConductor,
    EndSsh,
    OpenUrl,
    Wrap,
    Debug,
    ExecFailed,
}

/// Why acting on a key would be unsafe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItermUnsafe {
    /// It brings the window to the front over whatever the user is in.
    StealsFocus,
    /// It switches or rewrites the settings the user chose.
    RewritesSettings,
    /// It hands files from the user's machine to the program that asked,
    /// a remote one over ssh included, through a chooser the program opens:
    /// baud opens none for a program, as it writes no file a program sends.
    UploadsFiles,
}

/// What baud does with a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItermDoes {
    /// Acted on, to the documentation.
    Acts,
    /// Refused, because acting on it is unsafe.
    Refuses(ItermUnsafe),
    /// It has a meaning in baud, and is not built yet: its row in
    /// `docs/vt-coverage.md` says so.
    Unbuilt,
    /// It drives a feature of iTerm2's own that baud has no counterpart of:
    /// its Python scripts, its ssh integration, buried sessions, captured
    /// output, its upgrade prompt.
    NoCounterpart,
    /// iTerm2 itself does nothing with it: "Deprecated and unsupported code
    /// BeginFile received. Use File instead."
    Retired,
}

use ItermDoes::{Acts, NoCounterpart, Refuses, Retired, Unbuilt};
use ItermUnsafe::{RewritesSettings, StealsFocus, UploadsFiles};

/// How many keys iTerm2 knows.
pub const ITERM_KEYS: usize = 54;

/// The table: each key's spelling, the key, and what baud does with it. Row
/// `i` is key `i`, which the index below checks as it is built.
const ROWS: [(&[u8], ItermKey, ItermDoes); ITERM_KEYS] = [
    (b"CursorShape", ItermKey::CursorShape, Acts),
    (
        b"ShellIntegrationVersion",
        ItermKey::ShellIntegrationVersion,
        NoCounterpart,
    ),
    (b"RemoteHost", ItermKey::RemoteHost, Acts),
    (b"SetMark", ItermKey::SetMark, Acts),
    (b"StealFocus", ItermKey::StealFocus, Refuses(StealsFocus)),
    (b"ClearScrollback", ItermKey::ClearScrollback, Acts),
    (b"CurrentDir", ItermKey::CurrentDir, Acts),
    (
        b"SetProfile",
        ItermKey::SetProfile,
        Refuses(RewritesSettings),
    ),
    (b"AddNote", ItermKey::AddNote, Unbuilt),
    (b"AddAnnotation", ItermKey::AddAnnotation, Unbuilt),
    (b"AddHiddenNote", ItermKey::AddHiddenNote, Unbuilt),
    (
        b"AddHiddenAnnotation",
        ItermKey::AddHiddenAnnotation,
        Unbuilt,
    ),
    (
        b"HighlightCursorLine",
        ItermKey::HighlightCursorLine,
        Unbuilt,
    ),
    (
        b"ClearCapturedOutput",
        ItermKey::ClearCapturedOutput,
        NoCounterpart,
    ),
    (b"CopyToClipboard", ItermKey::CopyToClipboard, Unbuilt),
    (b"File", ItermKey::File, Unbuilt),
    (b"MultipartFile", ItermKey::MultipartFile, Unbuilt),
    (b"Block", ItermKey::Block, Unbuilt),
    (b"UpdateBlock", ItermKey::UpdateBlock, Unbuilt),
    (b"Button", ItermKey::Button, Unbuilt),
    (b"FilePart", ItermKey::FilePart, Unbuilt),
    (b"FileEnd", ItermKey::FileEnd, Unbuilt),
    (b"Copy", ItermKey::Copy, Acts),
    (
        b"RequestUpload",
        ItermKey::RequestUpload,
        Refuses(UploadsFiles),
    ),
    (b"BeginFile", ItermKey::BeginFile, Retired),
    (b"EndFile", ItermKey::EndFile, Retired),
    (b"EndCopy", ItermKey::EndCopy, Unbuilt),
    (b"RequestAttention", ItermKey::RequestAttention, Acts),
    (
        b"SetBackgroundImageFile",
        ItermKey::SetBackgroundImageFile,
        Unbuilt,
    ),
    (
        b"SetProfileProperty",
        ItermKey::SetProfileProperty,
        Refuses(RewritesSettings),
    ),
    (b"SetBadgeFormat", ItermKey::SetBadgeFormat, Unbuilt),
    (b"SetUserVar", ItermKey::SetUserVar, Acts),
    (b"ReportCellSize", ItermKey::ReportCellSize, Acts),
    (b"SetWindowFrame", ItermKey::SetWindowFrame, Unbuilt),
    (b"ReportWindowFrame", ItermKey::ReportWindowFrame, Unbuilt),
    (b"ReportScreenFrames", ItermKey::ReportScreenFrames, Unbuilt),
    (b"UnicodeVersion", ItermKey::UnicodeVersion, Unbuilt),
    (b"SetColors", ItermKey::SetColors, Acts),
    (b"SetKeyLabel", ItermKey::SetKeyLabel, Unbuilt),
    (b"PushKeyLabels", ItermKey::PushKeyLabels, Unbuilt),
    (b"PopKeyLabels", ItermKey::PopKeyLabels, Unbuilt),
    (b"Disinter", ItermKey::Disinter, NoCounterpart),
    (b"ReportVariable", ItermKey::ReportVariable, Acts),
    (b"Custom", ItermKey::Custom, NoCounterpart),
    (b"Notification", ItermKey::Notification, Acts),
    (b"Capabilities", ItermKey::Capabilities, Unbuilt),
    (b"Env", ItermKey::Env, NoCounterpart),
    (b"it2ssh", ItermKey::It2ssh, NoCounterpart),
    (b"SendConductor", ItermKey::SendConductor, NoCounterpart),
    (b"EndSSH", ItermKey::EndSsh, NoCounterpart),
    (b"OpenURL", ItermKey::OpenUrl, Unbuilt),
    (b"wrap", ItermKey::Wrap, NoCounterpart),
    (b"Debug", ItermKey::Debug, NoCounterpart),
    (b"ExecFailed", ItermKey::ExecFailed, NoCounterpart),
];

impl ItermKey {
    /// Every key, in the table's order.
    pub const ALL: [ItermKey; ITERM_KEYS] = {
        let mut out = [ItermKey::CursorShape; ITERM_KEYS];
        let mut i = 0;
        while i < ITERM_KEYS {
            out[i] = ROWS[i].1;
            i += 1;
        }
        out
    };

    /// The key spelled `name`, exactly (iTerm2 compares with
    /// `isEqualToString:`, so case matters): one hash and one compare.
    pub fn of(name: &[u8]) -> Option<ItermKey> {
        KEY_INDEX.find(&KEY_NAMES, name).map(|i| ROWS[i].1)
    }

    /// Its row: the table is in key order.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The spelling iTerm2 compares.
    pub fn name(self) -> &'static [u8] {
        ROWS[self.index()].0
    }

    /// What baud does with it.
    pub fn does(self) -> ItermDoes {
        ROWS[self.index()].2
    }
}

const KEY_NAMES: [&[u8]; ITERM_KEYS] = {
    let mut out: [&[u8]; ITERM_KEYS] = [b""; ITERM_KEYS];
    let mut i = 0;
    while i < ITERM_KEYS {
        assert!(ROWS[i].1 as usize == i, "a row out of key order");
        out[i] = ROWS[i].0;
        i += 1;
    }
    out
};

/// A multiplier under which no two keys share a bucket, searched for by
/// `scripts/gen-iterm2-keys.py --search`: a key is one hash and one compare.
const KEY_INDEX: NameIndex<128> = NameIndex::build(&KEY_NAMES, 0xad79_0546_471e_f041);
const _: () = assert!(KEY_INDEX.probes == 0);

// ---------------------------------------------------------------- the index

/// FNV-1a over a name's bytes.
const fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

const fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// **An open-addressed index over a list of names, built at compile time**:
/// `S` slots, a power of two, each the name's position plus one or 0, the
/// bucket from the high bits of a multiplicative hash of its FNV-1a, as
/// `StyleId::bucket` takes them, by a multiplier searched for so that no two
/// names share one. `probes` is the longest walk past a bucket any name
/// needs, held at zero by an assert beside each index, so a list that
/// collides fails to compile rather than slows quietly.
pub(crate) struct NameIndex<const S: usize> {
    slot: [u8; S],
    mul: u64,
    probes: usize,
}

impl<const S: usize> NameIndex<S> {
    const fn bucket(name: &[u8], mul: u64) -> usize {
        (fnv(name).wrapping_mul(mul) >> (64 - S.trailing_zeros())) as usize
    }

    pub(crate) const fn build(names: &[&[u8]], mul: u64) -> NameIndex<S> {
        assert!(S.is_power_of_two() && names.len() < S && names.len() < 255);
        let mut slot = [0u8; S];
        let mut probes = 0;
        let mut r = 0;
        while r < names.len() {
            let mut i = Self::bucket(names[r], mul);
            let mut n = 0;
            while slot[i] != 0 {
                assert!(!same(names[slot[i] as usize - 1], names[r]), "a name twice");
                i = (i + 1) & (S - 1);
                n += 1;
            }
            slot[i] = (r + 1) as u8;
            if n > probes {
                probes = n;
            }
            r += 1;
        }
        NameIndex { slot, mul, probes }
    }

    pub(crate) fn find(&self, names: &[&[u8]], name: &[u8]) -> Option<usize> {
        let mut i = Self::bucket(name, self.mul);
        for _ in 0..=self.probes {
            let s = self.slot[i] as usize;
            if s == 0 {
                return None;
            }
            if names[s - 1] == name {
                return Some(s - 1);
            }
            i = (i + 1) & (S - 1);
        }
        None
    }
}

// ---------------------------------------------------------------- messages

/// An `OSC 1337` body, as far as baud reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc1337<'a> {
    /// `RequestAttention`: "yes to request attention by bouncing the dock
    /// icon indefinitely, once to bounce it a single time, or no to cancel
    /// a previous request".
    Attention(Attention),
    /// `RequestAttention=fireworks`: "fireworks explode at the cursor's
    /// location", a drawn effect not built, counted.
    Fireworks,
    /// `CursorShape=N`, "where [N]=0, 1, or 2": the shape, its blink kept
    /// (Jev, KEEP_BLINK, 1.00/1.00). A value that is not a number is 0, as
    /// iTerm2 reads it with `intValue` ("Bogusly, non-numbers are treated
    /// as 0"; Jev, BLOCK, 0.92/0.98).
    CursorShape(CursorShape),
    /// `ClearScrollback`: "To erase the scrollback history".
    ClearScrollback,
    /// `CurrentDir=dir`: the directory, as sent.
    CurrentDir(&'a [u8]),
    /// `RemoteHost=user@host`: the host, the bytes after the last `@` (all
    /// of them with no `@`), as iTerm2's `setRemoteHostFromString` splits it
    /// "because Windows UPN format includes an @ in the user name".
    RemoteHost(&'a [u8]),
    /// `ReportCellSize`.
    ReportCellSize,
    /// `SetMark`: "record a location and then jump back to it later", the
    /// line [`MarkAt`] names.
    SetMark(MarkAt),
    /// `ReportVariable=base64`: the encoded name.
    ReportVariable(&'a [u8]),
    /// `SetUserVar=name=base64`, or a bare `name`: the variable
    /// `user.<name>` set to the decoded value, or unset. `value` is the
    /// base64 as sent, `None` for a bare name, which iTerm2's
    /// `UserVariableAssignment` reads as "unset it" (`[[source:
    /// iterm2-964a75d-user-variable]]`).
    SetUserVar {
        name: &'a [u8],
        value: Option<&'a [u8]>,
    },
    /// `Notification=key=base64;...`: a notification, as iTerm2's source
    /// posts one "rich" ([`NoticePairs`]). The pages do not name the key.
    Notification(NoticePairs<'a>),
    /// `RequestUpload=format=tgz`, which baud refuses: with `tgz` or no
    /// format the program waits for an answer; with another iTerm2 sends
    /// none.
    RequestUpload(UploadFormat),
    /// `SetColors=key=value[,key=value...]`: [`ColorSettings`].
    SetColors(ColorSettings<'a>),
    /// `Copy=:base64`, whole: the base64 after the `:`. The core streams a
    /// `Copy=:` body into the clipboard as it arrives, so it reaches here
    /// only from a test or an embedder without the core.
    Copy(&'a [u8]),
    /// `Copy=2;uid`: iTerm2's segmented copy begins, its pieces to come
    /// under `uid` ([`CopyHeader`]).
    CopyBegin(&'a [u8]),
    /// `Copy=3;uid:base64`: a piece of it. The core streams one whose `uid`
    /// is the open copy's.
    CopySegment { uid: &'a [u8], data: &'a [u8] },
    /// `Copy=4`: the segmented copy ends and is copied.
    CopyEnd,
    /// `Copy=2` or `Copy=3` with no `uid`: iTerm2 abandons the copy.
    CopyAbandon,
    /// A key baud acts on with a value off its form: `CursorShape` outside 0
    /// to 2 (iTerm2 does nothing), `RequestAttention` with a value the text
    /// does not define, `Copy` without its `:`.
    Malformed(ItermKey),
    /// A key baud does not act on: [`ItermKey::does`] says why.
    Ignored(ItermKey),
    /// A key iTerm2 does not know.
    Unknown,
}

impl<'a> Osc1337<'a> {
    /// The body after `1337;`.
    pub fn parse(pt: &'a [u8]) -> Osc1337<'a> {
        let (name, value) = match pt.iter().position(|&b| b == b'=') {
            Some(i) => (&pt[..i], &pt[i + 1..]),
            None => (pt, &pt[pt.len()..]),
        };
        let Some(key) = ItermKey::of(name) else {
            return Osc1337::Unknown;
        };
        if key == ItermKey::RequestUpload {
            return Osc1337::RequestUpload(UploadFormat::of(value));
        }
        if key.does() != Acts {
            return Osc1337::Ignored(key);
        }
        match key {
            ItermKey::RequestAttention => match value {
                b"yes" => Osc1337::Attention(Attention::Yes),
                b"once" => Osc1337::Attention(Attention::Once),
                b"no" => Osc1337::Attention(Attention::No),
                b"fireworks" => Osc1337::Fireworks,
                _ => Osc1337::Malformed(key),
            },
            ItermKey::CursorShape => match int_value(value) {
                0 => Osc1337::CursorShape(CursorShape::Block),
                1 => Osc1337::CursorShape(CursorShape::Bar),
                2 => Osc1337::CursorShape(CursorShape::Underline),
                _ => Osc1337::Malformed(key),
            },
            ItermKey::ClearScrollback => Osc1337::ClearScrollback,
            ItermKey::CurrentDir => Osc1337::CurrentDir(value),
            ItermKey::RemoteHost => {
                Osc1337::RemoteHost(match value.iter().rposition(|&b| b == b'@') {
                    Some(i) => &value[i + 1..],
                    None => value,
                })
            }
            ItermKey::ReportCellSize => Osc1337::ReportCellSize,
            // iTerm2's source: `saveScrollPosition` records "the entire
            // region of visible lines"; anything else, "implicitly
            // saveCursorLine" (`terminalSaveScrollPositionWithArgument:`).
            ItermKey::SetMark => Osc1337::SetMark(match value {
                b"saveScrollPosition" => MarkAt::WindowTop,
                _ => MarkAt::CursorLine,
            }),
            ItermKey::ReportVariable => Osc1337::ReportVariable(value),
            ItermKey::SetColors => Osc1337::SetColors(ColorSettings(value)),
            ItermKey::Notification => Osc1337::Notification(NoticePairs(value)),
            // "name=base64 to set a user variable or just name to unset it";
            // a name with a `.` is refused, "which is not allowed".
            ItermKey::SetUserVar => {
                let (name, value) = match value.iter().position(|&b| b == b'=') {
                    Some(i) => (&value[..i], Some(&value[i + 1..])),
                    None => (value, None),
                };
                match name.contains(&b'.') {
                    true => Osc1337::Malformed(key),
                    false => Osc1337::SetUserVar { name, value },
                }
            }
            ItermKey::Copy => {
                let (header, data) = match value.iter().position(|&b| b == b':') {
                    Some(i) => (&value[..i], Some(&value[i + 1..])),
                    None => (value, None),
                };
                match (CopyHeader::parse(header), data) {
                    (CopyHeader::Whole, Some(data)) => Osc1337::Copy(data),
                    (CopyHeader::Begin(uid), _) => Osc1337::CopyBegin(uid),
                    (CopyHeader::Segment(uid), data) => Osc1337::CopySegment {
                        uid,
                        data: data.unwrap_or(b""),
                    },
                    (CopyHeader::End, _) => Osc1337::CopyEnd,
                    (CopyHeader::Abandon, _) => Osc1337::CopyAbandon,
                    (CopyHeader::Whole | CopyHeader::Malformed, _) => Osc1337::Malformed(key),
                }
            }
            _ => Osc1337::Ignored(key),
        }
    }
}

/// **What a `RequestUpload` asks for**, as iTerm2's source reads its
/// `;`-separated `key=value` arguments (`requestUploadWithFormat:`,
/// `[[source: iterm2-964a75d-ptysession]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UploadFormat {
    /// `format=tgz`, or no `format`, which iTerm2 takes as `tgz`: the
    /// program waits for `ok` or for the cancel.
    Tgz,
    /// Any other format: iTerm2 shows a warning and sends nothing.
    Other,
}

impl UploadFormat {
    fn of(value: &[u8]) -> UploadFormat {
        let format = value
            .split(|&b| b == b';')
            .filter_map(|part| part.strip_prefix(b"format="))
            .next_back();
        match format {
            None | Some(b"tgz") => UploadFormat::Tgz,
            Some(_) => UploadFormat::Other,
        }
    }
}

/// What a `Notification` names: iTerm2's source decodes each value of its
/// `key=base64` pairs, split on `;`, and posts `title`, `subtitle` and
/// `message` -- the body, empty when absent -- and an `image`
/// (`screenPostUserNotification:rich:`, `[[source:
/// iterm2-964a75d-ptysession]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticePart {
    Title,
    Subtitle,
    Message,
    Image,
    /// A key iTerm2 does not read.
    Other,
}

/// `Notification`'s value: its `key=base64` pairs, in order, each with the
/// part its key names and the base64 as sent; a part with no `=` is a key
/// with an empty value, as iTerm2's split makes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoticePairs<'a>(&'a [u8]);

impl<'a> NoticePairs<'a> {
    /// Each pair: the part and its base64.
    pub fn pairs(self) -> impl Iterator<Item = (NoticePart, &'a [u8])> {
        self.0
            .split(|&b| b == b';')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = match p.iter().position(|&b| b == b'=') {
                    Some(i) => (&p[..i], &p[i + 1..]),
                    None => (p, &p[p.len()..]),
                };
                let part = match k {
                    b"title" => NoticePart::Title,
                    b"subtitle" => NoticePart::Subtitle,
                    b"message" => NoticePart::Message,
                    b"image" => NoticePart::Image,
                    _ => NoticePart::Other,
                };
                (part, v)
            })
    }
}

/// **The line a `SetMark` marks**, as iTerm2's source reads its argument
/// (`[[source: iterm2-964a75d-screen-terminal-delegate]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkAt {
    /// The cursor's line: no argument, or any but `saveScrollPosition`.
    CursorLine,
    /// `saveScrollPosition`: the line at the window's top, so a jump, which
    /// puts a mark at the window's top, shows the region as it was (Jev,
    /// WINDOW_TOP, 0.90/0.94).
    WindowTop,
}

/// **What a `Copy=` asks, from the value before its `:`**, as iTerm2's
/// `executeXtermSetKvp` reads it (`[[source: iterm2-964a75d-vt100terminal]]`):
/// the first `;`-part's `intValue` is the mode, an empty value mode 1.
/// Mode 1 is the page's whole copy, "OSC 1337 ; Copy=:[base64] ST"; 2, 3
/// and 4 are the segmented copy iTerm2's `it2copy` sends inside tmux -- `2;
/// uid` to begin, `3;uid:base64` a piece, `4` to end -- which the pages do
/// not name and which is built because a program sends it (Jev's lean,
/// BUILD_SEGMENTS, 0.80/0.78 asked again, under the owner's "we need to
/// support such protocols fully, always!").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyHeader<'a> {
    Whole,
    Begin(&'a [u8]),
    Segment(&'a [u8]),
    End,
    /// Mode 2 or 3 without its `uid`: iTerm2 leaves copy mode.
    Abandon,
    /// Any other mode, which iTerm2's `switch` passes over.
    Malformed,
}

impl<'a> CopyHeader<'a> {
    pub fn parse(header: &'a [u8]) -> CopyHeader<'a> {
        let mut parts = header.split(|&b| b == b';');
        let first = parts.next().unwrap_or(b"");
        let uid = parts.next();
        let mode = match (first.is_empty(), uid) {
            (true, None) => 1,
            _ => int_value(first),
        };
        match (mode, uid) {
            (1, _) => CopyHeader::Whole,
            (2, Some(u)) => CopyHeader::Begin(u),
            (3, Some(u)) => CopyHeader::Segment(u),
            (2 | 3, None) => CopyHeader::Abandon,
            (4, _) => CopyHeader::End,
            _ => CopyHeader::Malformed,
        }
    }
}

/// `NSString`'s `intValue`, as iTerm2 reads `CursorShape`'s value: leading
/// spaces skipped, a sign, the digits up to the first that is not one, 0
/// for none, saturated past `i32`.
fn int_value(v: &[u8]) -> i64 {
    let mut i = 0;
    while i < v.len() && v[i] == b' ' {
        i += 1;
    }
    let neg = match v.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut n: i64 = 0;
    while i < v.len() && v[i].is_ascii_digit() {
        n = (n * 10 + i64::from(v[i] - b'0')).min(i64::from(i32::MAX) + 1);
        i += 1;
    }
    match neg {
        true => -n,
        false => n,
    }
}

// ---------------------------------------------------------------- SetColors

/// A colour `SetColors` names: "fg bg bold link selbg selfg curbg curfg
/// underline tab" and the sixteen ANSI names, `preset`, and the two more
/// iTerm2's source takes, `match` and `badge`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorTarget {
    Foreground,
    Background,
    Bold,
    Link,
    Match,
    Selection,
    SelectedText,
    Cursor,
    CursorText,
    Underline,
    Badge,
    Tab,
    Preset,
    /// `black` to `br_white`: palette entry 0 to 15.
    Ansi(Chan),
}

const COLOR_ROWS: [(&[u8], ColorTarget); 29] = [
    (b"fg", ColorTarget::Foreground),
    (b"bg", ColorTarget::Background),
    (b"bold", ColorTarget::Bold),
    (b"link", ColorTarget::Link),
    (b"match", ColorTarget::Match),
    (b"selbg", ColorTarget::Selection),
    (b"selfg", ColorTarget::SelectedText),
    (b"curbg", ColorTarget::Cursor),
    (b"curfg", ColorTarget::CursorText),
    (b"underline", ColorTarget::Underline),
    (b"badge", ColorTarget::Badge),
    (b"tab", ColorTarget::Tab),
    (b"preset", ColorTarget::Preset),
    (b"black", ColorTarget::Ansi(Chan(0))),
    (b"red", ColorTarget::Ansi(Chan(1))),
    (b"green", ColorTarget::Ansi(Chan(2))),
    (b"yellow", ColorTarget::Ansi(Chan(3))),
    (b"blue", ColorTarget::Ansi(Chan(4))),
    (b"magenta", ColorTarget::Ansi(Chan(5))),
    (b"cyan", ColorTarget::Ansi(Chan(6))),
    (b"white", ColorTarget::Ansi(Chan(7))),
    (b"br_black", ColorTarget::Ansi(Chan(8))),
    (b"br_red", ColorTarget::Ansi(Chan(9))),
    (b"br_green", ColorTarget::Ansi(Chan(10))),
    (b"br_yellow", ColorTarget::Ansi(Chan(11))),
    (b"br_blue", ColorTarget::Ansi(Chan(12))),
    (b"br_magenta", ColorTarget::Ansi(Chan(13))),
    (b"br_cyan", ColorTarget::Ansi(Chan(14))),
    (b"br_white", ColorTarget::Ansi(Chan(15))),
];

const COLOR_NAMES: [&[u8]; 29] = {
    let mut out: [&[u8]; 29] = [b""; 29];
    let mut i = 0;
    while i < 29 {
        out[i] = COLOR_ROWS[i].0;
        i += 1;
    }
    out
};

/// Searched for as [`KEY_INDEX`]'s is.
const COLOR_INDEX: NameIndex<64> = NameIndex::build(&COLOR_NAMES, 0x2adb_c858_5cc4_8531);
const _: () = assert!(COLOR_INDEX.probes == 0);

impl ColorTarget {
    /// The colour `name` names, exactly.
    pub fn of(name: &[u8]) -> Option<ColorTarget> {
        COLOR_INDEX
            .find(&COLOR_NAMES, name)
            .map(|i| COLOR_ROWS[i].1)
    }
}

/// A colour `SetColors` gives, in the space it names: "srgb (the standard
/// sRGB color space), rgb (the device-specific color space), p3"; sRGB with
/// none, as iTerm2's source takes it, and with a space none of the three,
/// which the page says a space "should be one of" and iTerm2's source reads
/// in its default space (Jev's lean, DEFAULT_SPACE, 0.62/0.78 and 0.06/0.48
/// asked again). `rgb` is macOS's Generic RGB, what iTerm2's source makes
/// of it (`[NSColorSpace genericRGBColorSpace]`), converted as a theme
/// file's `Calibrated` is (Jev's lean, CALIBRATED, 0.97/0.89).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorValue<'a> {
    /// Three or six hex digits, in a space.
    Rgb([u8; 3], themefile::gamut::Space),
    /// `tab=default`: "removes the tab color".
    Default,
    /// `preset=name`.
    Named(&'a [u8]),
    /// `i:N`, iTerm2's source's binding of a colour to palette entry N,
    /// which tracks the entry from then on; baud has no binding, and counts
    /// it (Jev's lean, COUNTED, 0.90/0.78 and 0.56/0.62 asked again).
    Binding(&'a [u8]),
    /// Neither three nor six hex digits.
    Malformed,
}

/// One `key=value` of a `SetColors`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSetting<'a> {
    Set(ColorTarget, ColorValue<'a>),
    /// A key no colour has.
    UnknownName(&'a [u8]),
    /// No `=`, nothing before it or nothing after it, which iTerm2 skips.
    Malformed,
}

/// `SetColors`' value: "key=value" pairs, split on `,` and each applied in
/// turn, as iTerm2's source splits them (Jev, COMMA_PAIRS, 0.98/1.00).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorSettings<'a>(&'a [u8]);

impl<'a> IntoIterator for ColorSettings<'a> {
    type Item = ColorSetting<'a>;
    type IntoIter = core::iter::Map<
        core::slice::Split<'a, u8, fn(&u8) -> bool>,
        fn(&'a [u8]) -> ColorSetting<'a>,
    >;
    fn into_iter(self) -> Self::IntoIter {
        let comma: fn(&u8) -> bool = |&b| b == b',';
        let read: fn(&'a [u8]) -> ColorSetting<'a> = color_setting;
        self.0.split(comma).map(read)
    }
}

fn color_setting(part: &[u8]) -> ColorSetting<'_> {
    let Some(eq) = part.iter().position(|&b| b == b'=') else {
        return ColorSetting::Malformed;
    };
    if eq == 0 || eq + 1 == part.len() {
        return ColorSetting::Malformed;
    }
    let (name, value) = (&part[..eq], &part[eq + 1..]);
    let Some(target) = ColorTarget::of(name) else {
        return ColorSetting::UnknownName(name);
    };
    ColorSetting::Set(target, color_value(target, value))
}

fn color_value(target: ColorTarget, v: &[u8]) -> ColorValue<'_> {
    use themefile::gamut::Space;
    if target == ColorTarget::Preset {
        return ColorValue::Named(v);
    }
    if target == ColorTarget::Tab && v == b"default" {
        return ColorValue::Default;
    }
    if let Some(n) = v.strip_prefix(b"i:") {
        return ColorValue::Binding(n);
    }
    let (space, hex) = match v.iter().position(|&b| b == b':') {
        Some(c) if c != 0 && c + 1 != v.len() => {
            let space = match &v[..c] {
                b"srgb" => Space::Srgb,
                b"rgb" => Space::Calibrated,
                b"p3" => Space::DisplayP3,
                _ => Space::Srgb,
            };
            (space, &v[c + 1..])
        }
        _ => (Space::Srgb, v),
    };
    let digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut d = [0u8; 6];
    for (i, &b) in hex.iter().enumerate().take(6) {
        match digit(b) {
            Some(x) => d[i] = x,
            None => return ColorValue::Malformed,
        }
    }
    match hex.len() {
        6 => ColorValue::Rgb(
            [d[0] << 4 | d[1], d[2] << 4 | d[3], d[4] << 4 | d[5]],
            space,
        ),
        3 => ColorValue::Rgb([d[0] * 17, d[1] * 17, d[2] * 17], space),
        _ => ColorValue::Malformed,
    }
}

// ---------------------------------------------------------------- variables

/// The longest name a user variable keeps, in bytes: a longer one is
/// refused and counted. iTerm2 names no limit; 64 holds every name its
/// shell integration sets.
pub const USER_VAR_NAME_MAX: usize = 64;

/// The longest value a user variable keeps, in bytes: the most an `OSC`
/// body of 512 bytes can carry once its base64 is decoded, so a whole
/// `SetUserVar` always fits.
pub const USER_VAR_VALUE_MAX: usize = 384;

/// How many user variables a terminal keeps; the next new name is refused
/// and counted. A table, not an eviction: a variable a program set and
/// later reads back must not have been dropped for another's.
pub const USER_VARS: usize = 16;

/// Whether a slot of the table holds a variable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VarSlot {
    Free,
    Taken,
}

/// **One user variable**, `user.<name>`, as `SetUserVar` sets it: the name
/// without its `user.`, and the value decoded.
#[derive(Clone, Copy)]
pub struct UserVar {
    slot: VarSlot,
    name: [u8; USER_VAR_NAME_MAX],
    name_len: ByteLen,
    value: [u8; USER_VAR_VALUE_MAX],
    value_len: ByteLen,
}

impl UserVar {
    /// A free slot.
    pub const FREE: UserVar = UserVar {
        slot: VarSlot::Free,
        name: [0; USER_VAR_NAME_MAX],
        name_len: ByteLen::ZERO,
        value: [0; USER_VAR_VALUE_MAX],
        value_len: ByteLen::ZERO,
    };

    /// The name, without `user.`.
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len.index()]
    }

    /// The value, decoded.
    pub fn value(&self) -> &[u8] {
        &self.value[..self.value_len.index()]
    }
}

/// Why a `SetUserVar` was not kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserVarRefusal {
    /// The name passes [`USER_VAR_NAME_MAX`].
    NameTooLong,
    /// Every slot holds another name.
    Full,
}

/// **The user variables, over a slice the embedder lends**: one slot a
/// variable, found by its name. At most [`USER_VARS`] slots, read only by an
/// `OSC 1337` and an embedder, so a walk over them is never on the per-byte
/// path.
pub struct UserVars<'b>(pub &'b mut [UserVar]);

/// The value of `user.<name>` in `vars`, if a program set it.
pub fn user_var<'a>(vars: &'a [UserVar], name: &[u8]) -> Option<&'a [u8]> {
    vars.iter()
        .find(|v| v.slot == VarSlot::Taken && v.name() == name)
        .map(UserVar::value)
}

impl UserVars<'_> {
    fn find(&self, name: &[u8]) -> Option<usize> {
        self.0
            .iter()
            .position(|v| v.slot == VarSlot::Taken && v.name() == name)
    }

    /// Set `user.<name>` to `value`, which [`USER_VAR_VALUE_MAX`] already
    /// bounds; a name already set keeps its slot.
    pub fn set(&mut self, name: &[u8], value: &[u8]) -> Result<(), UserVarRefusal> {
        if name.len() > USER_VAR_NAME_MAX {
            return Err(UserVarRefusal::NameTooLong);
        }
        let at = match self.find(name) {
            Some(i) => i,
            None => self
                .0
                .iter()
                .position(|v| v.slot == VarSlot::Free)
                .ok_or(UserVarRefusal::Full)?,
        };
        let v = &mut self.0[at];
        let n = value.len().min(USER_VAR_VALUE_MAX);
        v.slot = VarSlot::Taken;
        v.name[..name.len()].copy_from_slice(name);
        v.name_len = ByteLen::from_index(name.len());
        v.value[..n].copy_from_slice(&value[..n]);
        v.value_len = ByteLen::from_index(n);
        Ok(())
    }

    /// Unset `user.<name>`: nothing if it is not set.
    pub fn unset(&mut self, name: &[u8]) {
        if let Some(i) = self.find(name) {
            self.0[i] = UserVar::FREE;
        }
    }
}

// ---------------------------------------------------------------- numbers

/// A count of messages, as the core keeps one a key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Messages(usize);

impl Messages {
    pub const NONE: Messages = Messages(0);

    pub const fn of(n: usize) -> Messages {
        Messages(n)
    }

    pub fn index(self) -> usize {
        self.0
    }

    /// One more, saturating.
    pub fn next(self) -> Messages {
        Messages(self.0.saturating_add(1))
    }

    pub fn plus(self, o: Messages) -> Messages {
        Messages(self.0.saturating_add(o.0))
    }
}

/// The backing scale: device pixels a point, in hundredths, as the embedder
/// tells it (2.0 on a Retina display is 200).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackingScale(u16);

impl BackingScale {
    /// None told.
    pub const UNTOLD: BackingScale = BackingScale(0);

    /// `scale` pixels a point, to the nearest hundredth; `UNTOLD` for one
    /// that is not finite and positive.
    pub fn of(scale: f64) -> BackingScale {
        match scale.is_finite() && scale > 0.0 && scale < 600.0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            true => BackingScale((scale * 100.0 + 0.5) as u16),
            false => BackingScale::UNTOLD,
        }
    }

    pub fn hundredths(self) -> u16 {
        self.0
    }
}

/// A number in hundredths as iTerm2 writes a point size: `%0.2f`, and then
/// `stringByCompactingFloatingPointString` -- trailing zeros trimmed, one
/// kept after the point (`[[source: iterm2-964a75d-nsstringiterm]]`), so
/// 1750 is `17.5`, 800 is `8.0` and 233 is `2.33` (Jev, COMPACTED,
/// 0.98/1.00).
pub fn compact_hundredths(h: u32, out: &mut [u8; 16]) -> &[u8] {
    let mut digits = [0u8; 10];
    let mut n = h / 100;
    let mut k = 0;
    loop {
        digits[k] = b'0' + (n % 10) as u8;
        k += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let mut len = 0;
    while k > 0 {
        k -= 1;
        out[len] = digits[k];
        len += 1;
    }
    out[len] = b'.';
    out[len + 1] = b'0' + ((h / 10) % 10) as u8;
    out[len + 2] = b'0' + (h % 10) as u8;
    len += 3;
    if out[len - 1] == b'0' {
        len -= 1;
    }
    &out[..len]
}

/// `px / scale` points, in hundredths, as `%0.2f` rounds it: to the
/// nearest, a tie to the even hundredth.
pub fn points_hundredths(px: u32, scale: BackingScale) -> Option<u32> {
    let s = u64::from(scale.0);
    if s == 0 {
        return None;
    }
    let num = u64::from(px) * 10_000;
    let (q, r) = (num / s, num % s);
    let up = match (2 * r).cmp(&s) {
        core::cmp::Ordering::Greater => true,
        core::cmp::Ordering::Less => false,
        core::cmp::Ordering::Equal => q % 2 == 1,
    };
    u32::try_from(q + u64::from(up)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every key the pinned pages name and every key iTerm2's source
    /// compares has its row, and no row names a key neither lists**:
    /// `iterm_keys.rs`, generated from both by `scripts/gen-iterm2-keys.py`,
    /// against the table, through the index a message is dispatched by.
    #[test]
    fn every_iterm2_key_has_its_row() {
        use super::super::iterm_keys::{DOC_KEYS, SOURCE_KEYS};
        for k in DOC_KEYS.iter().chain(SOURCE_KEYS.iter()) {
            let key = ItermKey::of(k)
                .unwrap_or_else(|| panic!("{} has no row", core::str::from_utf8(k).unwrap()));
            assert_eq!(key.name(), *k);
        }
        for key in ItermKey::ALL {
            assert!(
                DOC_KEYS.contains(&key.name()) || SOURCE_KEYS.contains(&key.name()),
                "{:?} is in neither list",
                key
            );
        }
        assert_eq!(SOURCE_KEYS.len(), ITERM_KEYS, "the source names every key");
    }

    /// A key is compared exactly, as `isEqualToString:` compares it.
    #[test]
    fn a_key_is_its_exact_spelling() {
        assert_eq!(ItermKey::of(b"SetMark"), Some(ItermKey::SetMark));
        assert_eq!(ItermKey::of(b"setmark"), None);
        assert_eq!(ItermKey::of(b"SetMar"), None);
        assert_eq!(ItermKey::of(b""), None);
        assert_eq!(ItermKey::of(b"it2ssh"), Some(ItermKey::It2ssh));
    }

    #[test]
    fn osc1337_reads_each_acted_key() {
        assert_eq!(
            Osc1337::parse(b"RequestAttention=yes"),
            Osc1337::Attention(Attention::Yes)
        );
        assert_eq!(
            Osc1337::parse(b"RequestAttention=fireworks"),
            Osc1337::Fireworks
        );
        assert_eq!(
            Osc1337::parse(b"RequestAttention=maybe"),
            Osc1337::Malformed(ItermKey::RequestAttention)
        );
        assert_eq!(
            Osc1337::parse(b"CursorShape=1"),
            Osc1337::CursorShape(CursorShape::Bar)
        );
        assert_eq!(
            Osc1337::parse(b"CursorShape=2"),
            Osc1337::CursorShape(CursorShape::Underline)
        );
        assert_eq!(
            Osc1337::parse(b"CursorShape=bar"),
            Osc1337::CursorShape(CursorShape::Block)
        );
        assert_eq!(
            Osc1337::parse(b"CursorShape=3"),
            Osc1337::Malformed(ItermKey::CursorShape)
        );
        assert_eq!(
            Osc1337::parse(b"CursorShape=-1"),
            Osc1337::Malformed(ItermKey::CursorShape)
        );
        assert_eq!(
            Osc1337::parse(b"RemoteHost=me@corp@box.example"),
            Osc1337::RemoteHost(b"box.example")
        );
        assert_eq!(
            Osc1337::parse(b"RemoteHost=box"),
            Osc1337::RemoteHost(b"box")
        );
        assert_eq!(Osc1337::parse(b"Copy=:aGk="), Osc1337::Copy(b"aGk="));
        assert_eq!(Osc1337::parse(b"Copy=1:aGk="), Osc1337::Copy(b"aGk="));
        assert_eq!(Osc1337::parse(b"Copy=2;77"), Osc1337::CopyBegin(b"77"));
        assert_eq!(
            Osc1337::parse(b"Copy=3;77:aGk="),
            Osc1337::CopySegment {
                uid: b"77",
                data: b"aGk="
            }
        );
        assert_eq!(Osc1337::parse(b"Copy=4;77"), Osc1337::CopyEnd);
        assert_eq!(Osc1337::parse(b"Copy=3"), Osc1337::CopyAbandon);
        assert_eq!(Osc1337::parse(b"Copy="), Osc1337::Malformed(ItermKey::Copy));
        assert_eq!(
            Osc1337::parse(b"Copy=9:x"),
            Osc1337::Malformed(ItermKey::Copy)
        );
        assert_eq!(
            Osc1337::parse(b"SetMark"),
            Osc1337::SetMark(MarkAt::CursorLine)
        );
        assert_eq!(
            Osc1337::parse(b"SetMark=saveScrollPosition"),
            Osc1337::SetMark(MarkAt::WindowTop)
        );
        assert_eq!(
            Osc1337::parse(b"SetMark=saveCursorLine"),
            Osc1337::SetMark(MarkAt::CursorLine)
        );
        assert_eq!(
            Osc1337::parse(b"SetUserVar=gitBranch=bWFpbg=="),
            Osc1337::SetUserVar {
                name: b"gitBranch",
                value: Some(b"bWFpbg==")
            }
        );
        assert_eq!(
            Osc1337::parse(b"SetUserVar=gitBranch"),
            Osc1337::SetUserVar {
                name: b"gitBranch",
                value: None
            }
        );
        assert_eq!(
            Osc1337::parse(b"SetUserVar=a.b=eA=="),
            Osc1337::Malformed(ItermKey::SetUserVar)
        );
        assert_eq!(
            Osc1337::parse(b"RequestUpload=format=tgz;version=eA=="),
            Osc1337::RequestUpload(UploadFormat::Tgz)
        );
        assert_eq!(
            Osc1337::parse(b"RequestUpload"),
            Osc1337::RequestUpload(UploadFormat::Tgz)
        );
        assert_eq!(
            Osc1337::parse(b"RequestUpload=format=zip"),
            Osc1337::RequestUpload(UploadFormat::Other)
        );
        let Osc1337::Notification(n) =
            Osc1337::parse(b"Notification=title=aGk=;message=eA==;x;z=1")
        else {
            panic!("a notification");
        };
        let parts: [(NoticePart, &[u8]); 4] = {
            let mut it = n.pairs();
            core::array::from_fn(|_| it.next().unwrap())
        };
        assert_eq!(
            parts,
            [
                (NoticePart::Title, &b"aGk="[..]),
                (NoticePart::Message, &b"eA=="[..]),
                (NoticePart::Other, &b""[..]),
                (NoticePart::Other, &b"1"[..]),
            ]
        );
        assert_eq!(Osc1337::parse(b"Nonsense=1"), Osc1337::Unknown);
    }

    #[test]
    fn set_colors_reads_names_spaces_and_digits() {
        use themefile::gamut::Space;
        let all: [ColorSetting<'_>; 8] = {
            let mut it = ColorSettings(
                b"fg=f0f0f0,br_red=p3:fff,bg=rgb:123,=1,curfg,nope=fff,tab=default,red=xyz:fff",
            )
            .into_iter();
            core::array::from_fn(|_| it.next().unwrap())
        };
        assert_eq!(
            all[0],
            ColorSetting::Set(
                ColorTarget::Foreground,
                ColorValue::Rgb([0xf0; 3], Space::Srgb)
            )
        );
        assert_eq!(
            all[1],
            ColorSetting::Set(
                ColorTarget::Ansi(Chan(9)),
                ColorValue::Rgb([0xff; 3], Space::DisplayP3)
            )
        );
        assert_eq!(
            all[2],
            ColorSetting::Set(
                ColorTarget::Background,
                ColorValue::Rgb([0x11, 0x22, 0x33], Space::Calibrated)
            )
        );
        assert_eq!(all[3], ColorSetting::Malformed);
        assert_eq!(all[4], ColorSetting::Malformed);
        assert_eq!(all[5], ColorSetting::UnknownName(b"nope"));
        assert_eq!(
            all[6],
            ColorSetting::Set(ColorTarget::Tab, ColorValue::Default)
        );
        assert_eq!(
            all[7],
            ColorSetting::Set(
                ColorTarget::Ansi(Chan(1)),
                ColorValue::Rgb([0xff; 3], Space::Srgb)
            )
        );
        assert_eq!(
            color_value(ColorTarget::Bold, b"ffff"),
            ColorValue::Malformed
        );
        assert_eq!(
            color_value(ColorTarget::Bold, b"ggg"),
            ColorValue::Malformed
        );
    }

    #[test]
    fn a_point_size_is_written_as_iterm2_writes_it() {
        let mut b = [0u8; 16];
        assert_eq!(compact_hundredths(1750, &mut b), b"17.5");
        assert_eq!(compact_hundredths(800, &mut b), b"8.0");
        assert_eq!(compact_hundredths(233, &mut b), b"2.33");
        assert_eq!(compact_hundredths(5, &mut b), b"0.05");
        assert_eq!(compact_hundredths(12340, &mut b), b"123.4");
        let s2 = BackingScale::of(2.0);
        assert_eq!(points_hundredths(17, s2), Some(850));
        assert_eq!(points_hundredths(17, BackingScale::of(3.0)), Some(567));
        assert_eq!(points_hundredths(16, BackingScale::of(1.0)), Some(1600));
        assert_eq!(points_hundredths(1, BackingScale::UNTOLD), None);
    }
}
