use crate::*;

/// An OSC (Operating System Command) payload, resolved.
///
/// `[[source: ctlseqs]]` § "Set Text Parameters" gives the form as
/// `OSC Ps ; Pt BEL` or `OSC Ps ; Pt ST`, and both terminators are accepted:
/// "XTerm accepts either BEL or ST for terminating OSC sequences, and when
/// returning information, uses the same terminator used in a query."
///
/// Everything here is attacker-controlled: a window title is whatever the
/// remote end of an ssh session cared to send. Nothing in this type owns a
/// growable buffer, and the payload the core keeps is bounded and truncated
/// rather than rejected -- a title that is too long should be shortened, not
/// dropped, and must never be a reason to allocate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc<'a> {
    /// `OSC 0` — "Change Icon Name and Window Title to Pt."
    SetIconAndTitle(&'a [u8]),
    /// `OSC 1` — "Change Icon Name to Pt."
    SetIcon(&'a [u8]),
    /// `OSC 2` — "Change Window Title to Pt."
    SetTitle(&'a [u8]),
    /// `OSC 4 ; c ; spec` — "Change Color Number c to the color specified by
    /// spec." Only the first pair is taken; see [`Osc::parse`].
    SetColor { index: Chan, spec: XColor<'a> },
    /// `OSC 104` — reset the palette. `None` resets every entry, which is what
    /// an omitted parameter means.
    ResetColor(Option<Chan>),
    /// `OSC 10 ; spec` / `OSC 11 ; spec` -- the default foreground and
    /// background. `[[source: ctlseqs]]`: "Change VT100 text foreground
    /// color to Pt", and `?` in place of a spec is a query "with a control
    /// sequence of the same form". Only the first spec is taken, as with
    /// `OSC 4`: xterm lets one sequence carry several and answer each, and a
    /// fixed reply buffer does not.
    SetDynamic {
        which: DynamicColor,
        spec: XColor<'a>,
    },
    /// `OSC 110` / `OSC 111` -- reset the default foreground or background
    /// to what the embedder's theme says.
    ResetDynamic(DynamicColor),
    /// `OSC 8 ; params ; URI` — a hyperlink.
    ///
    /// An empty URI *closes* the current link rather than opening an empty one,
    /// which is how the [explicit hyperlink spec][osc8] ends a run.
    ///
    /// [osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
    Hyperlink { id: &'a [u8], uri: &'a [u8] },
    /// `OSC 133 ; <kind> ...` -- FinalTerm's semantic prompt marks, as
    /// every shell that ships integration emits them.
    ///
    /// **What baud wants them for is not what everyone else wants them
    /// for.** The usual uses are navigation and selection; here the
    /// load-bearing one is [`Semantic::PromptStart`], because it is the
    /// boundary between the rows an application will redraw and the rows
    /// nobody will. A resize may re-cut the second and must not re-cut
    /// the first: a shell redraws its prompt by counting the rows it
    /// occupies and erasing that many, so changing the count behind its
    /// back leaves the top of the old prompt on screen. See
    /// `docs/research/notes/m-resize-and-reflow.md`.
    Semantic(Semantic),
    /// Recognised as an OSC and not acted on.
    Unsupported,
}

/// Which dynamic colour an `OSC 10`-family sequence names. Three of xterm's
/// ten: `12` (the cursor) is stored and read now that a renderer draws a
/// cursor colour -- the deferral this enum once carried ("no renderer here
/// reads a cursor colour yet") expired when `baud-render` landed, which is
/// the day `docs/vt-coverage.md` named for it. `13`-`19` remain out: they are
/// pointer, Tektronix and highlight colours this terminal has no object for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DynamicColor {
    Foreground,
    Background,
    Cursor,
}

impl DynamicColor {
    /// The `Ps` that sets this colour; the reset is `100 + Ps`.
    pub fn code(self) -> u16 {
        match self {
            DynamicColor::Foreground => 10,
            DynamicColor::Background => 11,
            DynamicColor::Cursor => 12,
        }
    }
    pub fn index(self) -> usize {
        match self {
            DynamicColor::Foreground => 0,
            DynamicColor::Background => 1,
            DynamicColor::Cursor => 2,
        }
    }
}

/// Whether the screen is shown in reverse video: `?5`, DECSCNM -- and the
/// one spelling of what that does to the colours, which the renderer reads
/// to draw and the core reads to set, query and reset.
///
/// **Every cell draws with its foreground and background exchanged**, an
/// explicit `SGR` colour included: DEC STD 070 defines the mode as
/// reversing "the definitions of the foreground and background colors"
/// (`[[source: dec-std-070]]` § 5.6, page 5-94, PDF 349), and its Color Text
/// Extension says it "will affect which attribute bits from display memory
/// are used to render the text foreground and background color. It does not
/// affect the current rendition" (§ 5.6.3.2, page 5-107, PDF 362). The
/// owner decided this reading on 2026-10-13 over xterm's `ReverseVideo`,
/// which swaps the defaults and palette entries 0/7 and 8/15 and leaves
/// every other colour where it was (util.c 3269-3307, the file
/// `[[source: xterm-scroll]]` pins). A cell's own `SGR 7` exchanges again
/// ([`Video::drawn`]), and no palette entry moves, so an `OSC 4 ; i` or
/// `OSC 104 ; i` names the entry drawn wherever `i` is named.
///
/// A default cell drawn exchanged shows the stored background as its text
/// and the stored foreground as its page, so the colour shown in a role is
/// the other role's stored colour ([`Video::role`]): what an `OSC 10`,
/// `11`, `110` or `111` sent under `?5` acts on (note V, V17), and what the
/// margin, the scrollbar and a fade take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Video {
    #[default]
    Normal,
    Reverse,
}

impl Video {
    /// A cell's foreground and background as drawn under this video and
    /// the cell's own `attrs`: exchanged when exactly one of the screen's
    /// `?5` and the cell's `SGR 7` reverses them, so reverse inside reverse
    /// video draws the pair as stored. The one test of which side is the
    /// page: the colours, the contrast floor's words, the opaque-background
    /// test and the picture finder all take it.
    pub fn drawn<T>(self, attrs: Attrs, fg: T, bg: T) -> (T, T) {
        match (self, attrs.has(Attrs::REVERSE)) {
            (Video::Normal, false) | (Video::Reverse, true) => (fg, bg),
            (Video::Normal, true) | (Video::Reverse, false) => (bg, fg),
        }
    }

    /// The stored dynamic colour a role names under this video: the
    /// foreground and the background trade places, and the cursor colour is
    /// its own. Its own inverse.
    pub fn role(self, which: DynamicColor) -> DynamicColor {
        match (self, which) {
            (Video::Reverse, DynamicColor::Foreground) => DynamicColor::Background,
            (Video::Reverse, DynamicColor::Background) => DynamicColor::Foreground,
            (_, w) => w,
        }
    }

    /// The cursor colour after a change of video, given the text and the
    /// page shown *before* it: a colour equal to the text becomes the page,
    /// which the change then shows as the text. xterm's `ReverseVideo` does
    /// exactly that before its swap (`[[source: xterm-scroll]]` util.c
    /// 3293-3295), and `ReverseOldColors` to the copy `OSC 12 ; ?` answers
    /// from (`[[source: xterm-misc]]` misc.c 6418-6431), so the cursor keeps
    /// the text's colour instead of vanishing into the page. `OSC 12` is
    /// xterm's, so xterm's rule governs it under the exchange too, and its
    /// reason holds there: a default cell exchanged draws the old text
    /// colour as its page, which a cursor left in that colour would vanish
    /// into.
    ///
    /// A cursor colour nobody set ([`ColorWord::DEFAULT`]) is drawn in the
    /// text colour already, and stays unset; so does one whose page is not
    /// known, which is a core no embedder told its colours.
    pub fn cursor_across(cursor: ColorWord, text: ColorWord, page: ColorWord) -> ColorWord {
        if !cursor.is_default() && cursor == text && !page.is_default() {
            page
        } else {
            cursor
        }
    }
}

/// The mark an `OSC 133` carries.
///
/// The four FinalTerm letters, plus the `P` form iTerm2 and the shells
/// that follow it emit for a prompt with a kind attached (`OSC 133 ; P ;
/// k=i`). `P` is a prompt start like `A` is, and reading it as anything
/// else is how a terminal ends up thinking a shell sends no marks at
/// all -- the owner's own `zsh` sends `P` and not `A`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Semantic {
    /// `A`, or `P` with a kind: a prompt line begins here, and the kind
    /// says which. Only [`PromptKind::Initial`] begins a *block*.
    PromptStart(PromptKind),
    /// `B`: the prompt has ended and what follows is what the user typed.
    CommandStart,
    /// `C`: the command's output begins here.
    OutputStart,
    /// `D`: the command is done.
    ///
    /// The exit status `D` may carry is **not** kept. Nothing here acts
    /// on it, and `docs/vt-coverage.md`'s rule is that a value recorded
    /// and never read is worse than one not parsed -- it reads as a
    /// capability. It goes in beside whatever first needs it.
    CommandDone,
    /// A mark this terminal does not act on, kept so it is counted rather
    /// than silently dropped.
    Other,
}

/// Which line of a prompt an `A` or `P` mark opens: the `k=` option.
///
/// A shell with a multi-line prompt marks *each* line -- kitty's own
/// zsh integration sends `OSC 133 ; A ; k=s` "just before starting to
/// draw the PS2 prompt", and fish marks its continuation lines the same
/// way -- so a terminal that took every mark as the start of a new block
/// would measure a block from its last line. Ghostty's parser accepts
/// exactly these four values; the default when `k=` is absent is the
/// initial line, which is what a bare `A` has always meant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PromptKind {
    /// `k=i`, or no `k=` at all: the first line of a prompt.
    #[default]
    Initial,
    /// `k=s`: a secondary prompt (`PS2`), drawn on a later line of the
    /// same block.
    Secondary,
    /// `k=r`: a right prompt, on the same line as the one before it.
    Right,
    /// `k=c`: a continuation line of the primary prompt.
    Continuation,
}

impl PromptKind {
    /// The kind named by the `key=value` fields after the mark letter,
    /// separated by `;`. Only `k=` is read; `cl=`, `aid=`, `redraw=` and
    /// `click_events=` pass through unread until something acts on them.
    fn of(fields: &mut Split<'_>) -> PromptKind {
        for kv in fields {
            if let Some(v) = strip(kv, b"k=") {
                return match v {
                    b"s" => PromptKind::Secondary,
                    b"r" => PromptKind::Right,
                    b"c" => PromptKind::Continuation,
                    _ => PromptKind::Initial,
                };
            }
        }
        PromptKind::Initial
    }
}

/// An X11 colour specification, as `XParseColor` accepts it.
///
/// `[[source: ctlseqs]]`: "The spec can be a name or RGB specification as per
/// XParseColor." XParseColor reads the `#` forms itself and sends anything
/// else Xcms does not take to the X server (`[[source: libx11-parsecol-b8d5a70]]`
/// 114-139), whose `OsLookupColor` compares with `strncasecmp` and an exact
/// length (`[[source: xserver-oscolor-98aed9d]]` 852-884): **case is ignored
/// and nothing else**, so `lime green` and `LimeGreen` are colours, each
/// spelled in the table, and `lime  green` is not. The table is X.Org's
/// rgb.txt (`[[source: xorg-rgb-0d2caec]]`), 782 names, generated into
/// `baud-domain/src/vt/x_colour_names.rs` and found by two hashes and one
/// comparison. `rgbi:` and the CIE and TekHVC forms are Xcms's, converted to
/// sRGB (`baud-domain/src/vt/xcms.rs`). What none of these reads -- a name
/// the server lacks, a malformed spec -- parses to [`XColor::Unresolved`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XColor<'a> {
    Rgb(Chan, Chan, Chan),
    /// `?` — a query. The reply must have the same shape as the setter.
    Query,
    Unresolved(&'a [u8]),
}

impl<'a> XColor<'a> {
    /// `rgb:R/G/B` with 1-4 hex digits per channel, `#RGB` / `#RRGGBB` /
    /// `#RRRGGGBBB` / `#RRRRGGGGBBBB`, `rgbi:`, `CIEXYZ:`, `CIEuvY:`,
    /// `CIExyY:`, `CIELab:`, `CIELuv:`, `TekHVC:`, or an X11 colour name.
    ///
    /// Each form is read to XParseColor's sixteen bits a channel first, as
    /// the Xlib manual's "Color Strings" defines them
    /// (`[[source: xlib-ch06-b8d5a70]]`): "Color strings are
    /// case-insensitive", so `RGB:FF/00/00` is red; `rgb:` digits are scaled,
    /// as Xcms scales them, truncating (`[[source: libx11-lrgb-b8d5a70]]`
    /// 1286-1375); and a `#` form's digits "represent the most significant
    /// bits of the value", so `#3a7` is `#3000a0007000`. Xcms lower-cases the
    /// spec before it matches a prefix (`[[source: libx11-cmscolnm-b8d5a70]]`
    /// 214-235). The channel kept is the value's high byte, as xterm computes
    /// it on TrueColor (`[[source: xterm-misc-9489b20]]` 2725-2733) and as the
    /// X resources reader draws it (decided there 2026-10-17); a 24-bit
    /// Xvfb gives the same pixel for every spec held in
    /// `[[source: xparsecolor-obs-rgb-out]]`: `#fff` is `f0f0f0`.
    pub fn parse(s: &'a [u8]) -> XColor<'a> {
        if s == b"?" {
            return XColor::Query;
        }
        let wide = match s.split_first() {
            Some((b'#', digits)) => sharp(digits),
            _ => match s.iter().position(|&b| b == b':') {
                Some(colon) if s[..colon].eq_ignore_ascii_case(b"rgb") => device(&s[colon + 1..]),
                Some(colon) => match super::xcms::Space::of(&s[..colon]) {
                    Some(space) => super::xcms::resolve(space, &s[colon + 1..]),
                    // A prefix no colour space has goes to the server as a
                    // name, as Xcms's refusal does, and no name holds a
                    // colon.
                    None => return XColor::named(s),
                },
                None => return XColor::named(s),
            },
        };
        match wide {
            Some([r, g, b]) => XColor::Rgb(high(r), high(g), high(b)),
            None => XColor::Unresolved(s),
        }
    }

    fn named(s: &'a [u8]) -> XColor<'a> {
        use super::x_colour_names::{POOL, SEEDS, SLOTS};
        match crate::colour_name::find(&SEEDS, &SLOTS, POOL, s) {
            Some([r, g, b]) => XColor::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
            None => XColor::Unresolved(s),
        }
    }
}

fn strip<'a>(s: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    (s.len() >= p.len() && &s[..p.len()] == p).then(|| &s[p.len()..])
}

/// The eight bits a 24-bit TrueColor visual keeps of a sixteen-bit value:
/// its high byte.
fn high(v: u16) -> Chan {
    Chan::from_u8(v.to_be_bytes()[0])
}

/// One to four hex digits, case ignored, and how many there were.
fn hex(s: &[u8]) -> Option<(u32, u32)> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        v = (v << 4) | char::from(b).to_digit(16)?;
    }
    Some((v, u32::try_from(s.len()).ok()? * 4))
}

/// `#` and three runs of equal width: each run is the high bits of its
/// channel, the rest zero.
fn sharp(digits: &[u8]) -> Option<[u16; 3]> {
    let w = digits.len() / 3;
    if !matches!(digits.len(), 3 | 6 | 9 | 12) {
        return None;
    }
    let mut out = [0u16; 3];
    for (c, run) in out.iter_mut().zip(digits.chunks(w)) {
        let (v, bits) = hex(run)?;
        *c = u16::try_from(v << (16 - bits)).ok()?;
    }
    Some(out)
}

/// `R/G/B` after `rgb:`, one to four digits each, scaled to sixteen bits as
/// Xcms scales them: `v * 0xffff / (2^bits - 1)`, truncated.
fn device(fields: &[u8]) -> Option<[u16; 3]> {
    let mut it = Split::new(fields, b'/');
    let mut out = [0u16; 3];
    for c in &mut out {
        let (v, bits) = hex(it.next()?)?;
        *c = u16::try_from(v * 0xffff / ((1 << bits) - 1)).ok()?;
    }
    it.next().is_none().then_some(out)
}

/// Split a byte slice on a separator without allocating.
struct Split<'a> {
    rest: Option<&'a [u8]>,
    sep: StreamByte,
}

impl<'a> Split<'a> {
    fn new(s: &'a [u8], sep: u8) -> Split<'a> {
        Split {
            rest: Some(s),
            sep: StreamByte::from_stream(sep),
        }
    }
}

impl<'a> Iterator for Split<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let cur = self.rest?;
        match cur
            .iter()
            .position(|&b| StreamByte::from_stream(b) == self.sep)
        {
            Some(i) => {
                self.rest = Some(&cur[i + 1..]);
                Some(&cur[..i])
            }
            None => {
                self.rest = None;
                Some(cur)
            }
        }
    }
}

impl<'a> Osc<'a> {
    /// Resolve an OSC payload -- the bytes between the introducer and the
    /// string terminator, with neither included.
    ///
    /// `OSC 4` may carry "any number of c/spec pairs"; baud takes the first and
    /// ignores the rest, which is a deliberate under-implementation rather than
    /// a silent one: applications emit one pair per sequence in practice, and
    /// handling n pairs means n replies from a fixed reply buffer.
    pub fn parse(payload: &'a [u8]) -> Osc<'a> {
        let mut it = Split::new(payload, b';');
        let Some(ps) = it.next() else {
            return Osc::Unsupported;
        };
        let rest = match payload.len() > ps.len() {
            true => &payload[ps.len() + 1..],
            false => &payload[payload.len()..],
        };
        match ps {
            b"0" => Osc::SetIconAndTitle(rest),
            b"1" => Osc::SetIcon(rest),
            b"2" => Osc::SetTitle(rest),
            b"4" => {
                let mut p = Split::new(rest, b';');
                match (p.next(), p.next()) {
                    (Some(c), Some(spec)) => match dec_chan(c) {
                        Some(index) => Osc::SetColor {
                            index,
                            spec: XColor::parse(spec),
                        },
                        None => Osc::Unsupported,
                    },
                    _ => Osc::Unsupported,
                }
            }
            b"10" | b"11" | b"12" => {
                let which = match ps {
                    b"10" => DynamicColor::Foreground,
                    b"11" => DynamicColor::Background,
                    _ => DynamicColor::Cursor,
                };
                let mut p = Split::new(rest, b';');
                match p.next() {
                    Some(spec) if !spec.is_empty() => Osc::SetDynamic {
                        which,
                        spec: XColor::parse(spec),
                    },
                    _ => Osc::Unsupported,
                }
            }
            b"110" => Osc::ResetDynamic(DynamicColor::Foreground),
            b"111" => Osc::ResetDynamic(DynamicColor::Background),
            b"112" => Osc::ResetDynamic(DynamicColor::Cursor),
            b"104" => {
                if rest.is_empty() {
                    Osc::ResetColor(None)
                } else {
                    match dec_chan(rest) {
                        Some(c) => Osc::ResetColor(Some(c)),
                        None => Osc::Unsupported,
                    }
                }
            }
            b"8" => {
                // `OSC 8 ; params ; URI`. The params field carries `id=...`
                // and is allowed to be empty; the URI is everything after the
                // second semicolon, and may itself contain semicolons.
                let mut p = Split::new(rest, b';');
                let params = p.next().unwrap_or(b"");
                let uri = if rest.len() > params.len() {
                    &rest[params.len() + 1..]
                } else {
                    b""
                };
                Osc::Hyperlink {
                    id: link_id(params),
                    uri,
                }
            }
            b"133" => {
                let mut p = Split::new(rest, b';');
                match p.next() {
                    Some(b"A") | Some(b"P") => {
                        Osc::Semantic(Semantic::PromptStart(PromptKind::of(&mut p)))
                    }
                    Some(b"B") => Osc::Semantic(Semantic::CommandStart),
                    Some(b"C") => Osc::Semantic(Semantic::OutputStart),
                    Some(b"D") => Osc::Semantic(Semantic::CommandDone),
                    _ => Osc::Semantic(Semantic::Other),
                }
            }
            _ => Osc::Unsupported,
        }
    }
}

/// The `id=` value out of an `OSC 8` parameter field, if there is one.
///
/// Parameters are `key=value` pairs separated by `:`. Only `id` is defined by
/// the spec; anything else is ignored rather than guessed at.
fn link_id(params: &[u8]) -> &[u8] {
    let mut it = Split::new(params, b':');
    for kv in &mut it {
        if let Some(v) = strip(kv, b"id=") {
            return v;
        }
    }
    b""
}

/// A decimal palette index, 0-255.
fn dec_chan(s: &[u8]) -> Option<Chan> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as u32;
    }
    (v <= 255).then(|| Chan::from_u8(v as u8))
}
