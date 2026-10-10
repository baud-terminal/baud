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
/// XParseColor." Names are deliberately *not* resolved -- that needs the X11
/// `rgb.txt` table, which baud has no citable copy of, and inventing the
/// mapping would be guessing at a protocol. A name parses to
/// [`XColor::Unresolved`] and is ignored rather than approximated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XColor<'a> {
    Rgb(Chan, Chan, Chan),
    /// `?` — a query. The reply must have the same shape as the setter.
    Query,
    Unresolved(&'a [u8]),
}

impl<'a> XColor<'a> {
    /// `rgb:R/G/B` with 1-4 hex digits per channel, or `#RGB` / `#RRGGBB` /
    /// `#RRRGGGBBB` / `#RRRRGGGGBBBB`.
    ///
    /// Channels are scaled to 8 bits by taking the *high* bits, which is what
    /// XParseColor does: `rgb:f/f/f` is white, not rgb(15,15,15).
    pub fn parse(s: &'a [u8]) -> XColor<'a> {
        if s == b"?" {
            return XColor::Query;
        }
        if let Some(rest) = strip(s, b"rgb:") {
            let mut it = Split::new(rest, b'/');
            if let (Some(r), Some(g), Some(b), None) = (it.next(), it.next(), it.next(), it.next())
            {
                if let (Some(r), Some(g), Some(b)) = (hex_chan(r), hex_chan(g), hex_chan(b)) {
                    return XColor::Rgb(r, g, b);
                }
            }
            return XColor::Unresolved(s);
        }
        if let Some(rest) = strip(s, b"#") {
            let n = rest.len();
            if n % 3 == 0 && (3..=12).contains(&n) {
                let w = n / 3;
                if let (Some(r), Some(g), Some(b)) = (
                    hex_chan(&rest[..w]),
                    hex_chan(&rest[w..2 * w]),
                    hex_chan(&rest[2 * w..]),
                ) {
                    return XColor::Rgb(r, g, b);
                }
            }
        }
        XColor::Unresolved(s)
    }
}

fn strip<'a>(s: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    (s.len() >= p.len() && &s[..p.len()] == p).then(|| &s[p.len()..])
}

/// One to four hex digits, scaled to eight bits from the top.
fn hex_chan(s: &[u8]) -> Option<Chan> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | d as u32;
    }
    // Scale so the widest digit run maps its maximum to 255: `f` -> 255,
    // `ffff` -> 255. Left-align then take the top byte.
    let bits = s.len() * 4;
    let max = (1u32 << bits) - 1;
    Some(Chan::from_u8(((v * 255 + max / 2) / max) as u8))
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
