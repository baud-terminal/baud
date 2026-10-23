//! **iTerm2's `OSC 1337` messages, acted on** (`[[source:
//! iterm2-escape-codes]]`): the key is read by [`baud_domain::Osc1337`]
//! through its compiled table, and what is here is what each key baud acts
//! on does -- the cursor's shape, the history erased, the directory and its
//! host, the cell size in points, the variable report, the palette -- and
//! the count, a key at a time, of every message not acted on.
//!
//! **A `Copy=:` body is streamed**, as an `OSC 52` body is: `keep_pending`
//! routes the bytes after `1337;Copy=:` into the clipboard areas
//! (`clip.rs`), so a copy longer than an `OSC` body is kept whole.

use crate::logging::Body;
use crate::paint::Paint;
use crate::storage::OSC_MAX;
use baud_domain::*;

/// The longest host name kept from a `RemoteHost`: 255 octets, RFC 1035
/// § 2.3.4's limit on a name, which is longer than any host name written
/// as text.
pub(crate) const HOST_MAX: usize = 255;

/// The messages not acted on, a key at a time, and the ones naming a key
/// iTerm2 does not know ([`crate::Term::iterm_counts`]): each also counts
/// in `strings_ignored` and `unsupported_total`, but a `ReportCellSize`
/// asked before its facts are told, which counts in `unsupported_param`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItermCounts {
    by_key: [Messages; ITERM_KEYS],
    /// Messages whose key iTerm2 does not know.
    pub unknown: Messages,
    /// Expressions in the badge formats kept that baud does not evaluate,
    /// each drawn as nothing: every one but a user variable's,
    /// `\(user.NAME)`.
    pub badge_unevaluated: Messages,
}

impl ItermCounts {
    pub(crate) const NONE: ItermCounts = ItermCounts {
        by_key: [Messages::NONE; ITERM_KEYS],
        unknown: Messages::NONE,
        badge_unevaluated: Messages::NONE,
    };

    /// The messages with `key` not acted on: a key baud does not act on
    /// ([`ItermKey::does`]), or one it does with a value off its form.
    pub fn of(&self, key: ItermKey) -> Messages {
        self.by_key[key.index()]
    }

    /// The messages refused because acting on them is unsafe
    /// ([`ItermDoes::Refuses`]).
    pub fn refused(&self) -> Messages {
        ItermKey::ALL
            .iter()
            .filter(|k| matches!(k.does(), ItermDoes::Refuses(_)))
            .fold(Messages::NONE, |n, &k| n.plus(self.of(k)))
    }
}

/// How deep `UnicodeVersion=push` goes: a push past it is refused and
/// counted. iTerm2's stack has no bound; sixteen holds a shell, an editor and
/// the programs nested in them.
pub(crate) const UNICODE_STACK: usize = 16;

/// The longest label a push keeps, in bytes; a longer one is refused.
pub(crate) const UNICODE_LABEL_MAX: usize = 32;

/// One entry of the `UnicodeVersion` stack: its label and the version.
#[derive(Clone, Copy)]
pub(crate) struct Pushed {
    label: [u8; UNICODE_LABEL_MAX],
    len: ByteLen,
    widths: UnicodeWidths,
}

/// **The longest badge format kept**, decoded: what the longest `OSC`
/// body baud reads, `OSC_MAX` bytes of base64, decodes to.
pub(crate) const BADGE_MAX: usize = OSC_MAX / 4 * 3;

/// The badge format a program set, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Badge {
    /// None since power-on: the embedder's own, iTerm2's profile's.
    Unset,
    /// Its bytes, the first of [`Iterm::badge`], UTF-8.
    Set(ByteLen),
}

/// The host the last `RemoteHost` named.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Host {
    /// None named, or an empty one: this machine.
    Local,
    /// Its bytes, the first of [`Iterm::host`].
    Named(ByteLen),
    /// Longer than [`HOST_MAX`], or cut at `OSC_MAX`: the directories that
    /// follow are refused.
    Unreadable,
}

/// What the messages leave between them, in [`crate::storage::StrBuf`]:
/// the host, the backing scale the embedder told and the counts.
#[derive(Clone, Copy)]
pub(crate) struct Iterm {
    host: [u8; HOST_MAX],
    host_at: Host,
    badge: [u8; BADGE_MAX],
    badge_at: Badge,
    /// The badge's colour as `SetColors=badge=` set it, opaque as iTerm2
    /// makes it, or `None` for the embedder's.
    pub(crate) badge_colour: Option<ColorWord>,
    pub(crate) scale: BackingScale,
    /// `UnicodeVersion`'s stack, its entries from the front, the first
    /// `None` its top. `RIS` keeps it, as iTerm2's reset does (Jev, KEPT,
    /// 1.00/1.00).
    pushed: [Option<Pushed>; UNICODE_STACK],
    pub(crate) counts: ItermCounts,
}

impl Iterm {
    pub(crate) const NEW: Iterm = Iterm {
        host: [0; HOST_MAX],
        host_at: Host::Local,
        badge: [0; BADGE_MAX],
        badge_at: Badge::Unset,
        badge_colour: None,
        scale: BackingScale::UNTOLD,
        pushed: [None; UNICODE_STACK],
        counts: ItermCounts::NONE,
    };

    /// The badge format a program set, `None` while none has.
    pub(crate) fn badge(&self) -> Option<&str> {
        match self.badge_at {
            Badge::Unset => None,
            Badge::Set(n) => core::str::from_utf8(&self.badge[..n.index()]).ok(),
        }
    }
}

impl Paint<'_, '_> {
    /// `OSC 1337`: each key baud acts on, and every other one counted.
    pub(crate) fn osc1337(&mut self, o: Osc1337<'_>, body: Body, term: StringTerm) {
        match o {
            Osc1337::Attention(a) => {
                self.strs.attention = Some(a);
                self.strs.attention_requests = self.strs.attention_requests.next();
            }
            // "0: Block 1: Vertical bar 2: Underline", the blink kept, as
            // iTerm2's `terminalSetCursorType` changes the type alone (Jev,
            // KEEP_BLINK, 1.00/1.00).
            Osc1337::CursorShape(shape) => {
                let blink = match self.cold.cursor_style.blinks() {
                    true => CursorBlink::Blinking,
                    false => CursorBlink::Steady,
                };
                self.cold.cursor_style = CursorStyle::of(shape, blink);
            }
            // "To erase the scrollback history": `CSI 3 J`'s erasure. iTerm2's
            // source clears the screen as well (`clearBufferSavingPrompt:`,
            // `[[source: iterm2-964a75d-screen-terminal-delegate]]`), a
            // departure from the text Jev leaned to below 0.9 (0.50/0.48,
            // 0.46/0.54 asked again), so the text is followed.
            // "This shows or hides the cursor guide": the embedder draws it
            // (`Term::cursor_guide`), over its own setting until `RIS`.
            Osc1337::CursorGuide(g) => self.cold.cursor_guide = Some(g),
            Osc1337::ClearScrollback => self.erase_saved_lines(),
            Osc1337::CurrentDir(dir) => self.current_dir(dir, body),
            Osc1337::RemoteHost(host) => self.remote_host(host, body),
            Osc1337::ReportCellSize => self.report_cell_size(term),
            Osc1337::Capabilities => self.capabilities(),
            Osc1337::SetMark(at) => self.set_mark(at),
            Osc1337::UnicodeVersion(op) => self.unicode_version(op),
            Osc1337::Notification(pairs) => self.notification(pairs),
            Osc1337::RequestUpload(_) => self.request_upload(),
            Osc1337::ReportVariable(name) => self.report_variable(name, term),
            Osc1337::BadgeFormat(b64) => self.set_badge_format(b64, body),
            Osc1337::SetColors(list) => self.set_colors(list),
            Osc1337::SetUserVar { name, value } => self.set_user_var(name, value, body),
            Osc1337::Copy(data) => self.copy_whole(data),
            Osc1337::CopyBegin(uid) => self.segment_begin(uid),
            Osc1337::CopySegment { uid, data } => self.segment_whole(uid, data),
            Osc1337::CopyEnd => self.segment_end(),
            Osc1337::CopyAbandon => self.segment_abandon(),
            Osc1337::File { .. }
            | Osc1337::MultipartFile(_)
            | Osc1337::FilePart(_)
            | Osc1337::FileEnd => self.file_message(o),
            Osc1337::Fireworks => self.not_acted(ItermKey::RequestAttention),
            Osc1337::Malformed(key) | Osc1337::Ignored(key) => self.not_acted(key),
            Osc1337::Unknown => {
                self.strs.iterm.counts.unknown = self.strs.iterm.counts.unknown.next();
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
    }

    /// A message with `key` not acted on, counted under it.
    pub(crate) fn not_acted(&mut self, key: ItermKey) {
        let n = &mut self.strs.iterm.counts.by_key[key.index()];
        *n = n.next();
        self.strings_ignored += 1;
        self.unsupported_total += 1;
    }

    /// **`CurrentDir=dir`: the directory, as sent, on the host the last
    /// `RemoteHost` named** -- "a combination of RemoteHost and CurrentDir"
    /// is what `OSC 7` is the synonym of (Jev, LAST_REMOTE_HOST,
    /// 0.96/0.98). An empty one, or one cut at `OSC_MAX`, leaves no
    /// directory and is counted with `OSC 7`'s refusals (Jev's lean,
    /// LEAVES_NONE, 0.88/0.92): iTerm2 asks the process then, which the
    /// core cannot see.
    fn current_dir(&mut self, dir: &[u8], body: Body) {
        let len = match (self.strs.iterm.host_at, body) {
            (Host::Unreadable, _) | (_, Body::Cut) => return self.refuse_directory(),
            _ if dir.is_empty() => return self.refuse_directory(),
            (Host::Local, Body::Whole) => None,
            (Host::Named(n), Body::Whole) => Some(n.index()),
        };
        let host = len.map(|n| &self.strs.iterm.host[..n]);
        match crate::cwd::keep_as_sent(self.strs.cwd, host, dir) {
            Some(k) => self.strs.cwd_at.kept = k,
            None => self.refuse_directory(),
        }
    }

    /// **`RemoteHost=user@host`: the host the directories that follow are
    /// on**, its control characters dropped as iTerm2's
    /// `setRemoteHostFromString` drops them; none, or an empty one, is this
    /// machine. Alone it leaves the directory unknown until the next
    /// `CurrentDir` (Jev, LAST_REMOTE_HOST). One past [`HOST_MAX`] or cut at
    /// `OSC_MAX` is unreadable, and the directories that follow it are
    /// refused until another names a host.
    fn remote_host(&mut self, host: &[u8], body: Body) {
        let it = &mut self.strs.iterm;
        let mut n = 0;
        let mut fits = body == Body::Whole;
        for &b in host.iter().filter(|&&b| b >= 0x20 && b != 0x7f) {
            if n == HOST_MAX {
                fits = false;
                break;
            }
            it.host[n] = b;
            n += 1;
        }
        it.host_at = match (fits, n) {
            (false, _) => Host::Unreadable,
            (true, 0) => Host::Local,
            (true, n) => Host::Named(ByteLen::from_index(n)),
        };
        self.forget_directory();
    }

    /// **`ReportCellSize`: "OSC 1337 ; ReportCellSize=[height];[width];[scale]
    /// ST"**, the cell in points -- its device pixels over the backing
    /// scale -- and the scale, each as iTerm2 writes them (Jev, COMPACTED,
    /// 0.98/1.00), ended as the query was: by the page's own notation "ST
    /// means either BEL ... or ESC \\" (Jev's lean, QUERY_TERMINATOR,
    /// 0.80/0.68). Silence while the cell or the scale is untold, counted
    /// as an XTWINOPS report with its fact untold is (`unsupported_param`)
    /// and under the key: a number baud does not know would be a wrong
    /// answer.
    fn report_cell_size(&mut self, term: StringTerm) {
        let scale = self.strs.iterm.scale;
        let (w, h) = (
            u32::from(self.cell_width.raw()),
            u32::from(self.cell_height.raw()),
        );
        let (Some(w), Some(h), true) = (
            points_hundredths(w, scale).filter(|_| w != 0),
            points_hundredths(h, scale).filter(|_| h != 0),
            scale != BackingScale::UNTOLD,
        ) else {
            // As an XTWINOPS report whose fact is untold: silence, counted
            // as a parameter not honoured, and under the key.
            let n = &mut self.strs.iterm.counts.by_key[ItermKey::ReportCellSize.index()];
            *n = n.next();
            self.unsupported_param += 1;
            return;
        };
        let (mut a, mut b, mut c) = ([0u8; 16], [0u8; 16], [0u8; 16]);
        let s = u32::from(scale.hundredths());
        let parts: [&[u8]; 7] = [
            b"\x1b]1337;ReportCellSize=",
            compact_hundredths(h, &mut a),
            b";",
            compact_hundredths(w, &mut b),
            b";",
            compact_hundredths(s, &mut c),
            terminator(term),
        ];
        self.push_reply(&parts);
    }

    /// **`ReportVariable=base64`: answered in iTerm2's shape**, `ESC ] 1337
    /// ; ReportVariable=base64(value)` (`[[source:
    /// iterm2-964a75d-vt100output]]`), every name with the empty value, the
    /// answer iTerm2 gives a variable the user has not allowed, a `user.`
    /// variable a program set included: it is kept for the embedder alone
    /// (`Term::user_var`), since the answer goes to whatever reads the
    /// terminal then (Jev's lean, EMPTY_AS_DENIED, 0.84/0.62 and 0.70/0.84
    /// asked again with the variables built). Ended as the query was
    /// (Jev's lean, QUERY_TERMINATOR). A name that is not base64 of UTF-8
    /// gets no reply, as iTerm2 sends none, and is counted.
    fn report_variable(&mut self, name: &[u8], term: StringTerm) {
        let mut out = [0u8; OSC_MAX];
        let Some(n) = decode_whole(name, &mut out).map(ByteLen::index) else {
            return self.not_acted(ItermKey::ReportVariable);
        };
        if core::str::from_utf8(&out[..n]).is_err() {
            return self.not_acted(ItermKey::ReportVariable);
        }
        self.push_reply(&[b"\x1b]1337;ReportVariable=", terminator(term)]);
    }

    /// **`SetUserVar=name=base64`: `user.<name>` set to the decoded value,
    /// or unset** by a bare name or a value that is not base64 of UTF-8, as
    /// iTerm2's `UserVariableAssignment` reads a value that does not decode
    /// as none (`[[source: iterm2-964a75d-user-variable]]`). A message cut
    /// at `OSC_MAX`, a name past [`USER_VAR_NAME_MAX`] and a new name with
    /// every slot taken are refused and counted: a cut value is a wrong one.
    fn set_user_var(&mut self, name: &[u8], value: Option<&[u8]>, body: Body) {
        if body == Body::Cut {
            return self.not_acted(ItermKey::SetUserVar);
        }
        let mut out = [0u8; OSC_MAX];
        let decoded = value
            .and_then(|v| decode_whole(v, &mut out))
            .map(ByteLen::index)
            .filter(|&n| core::str::from_utf8(&out[..n]).is_ok());
        let mut vars = UserVars(self.strs.user_vars);
        let kept = match decoded {
            Some(n) => vars.set(name, &out[..n]),
            None => {
                vars.unset(name);
                Ok(())
            }
        };
        if kept.is_err() {
            self.not_acted(ItermKey::SetUserVar);
        }
    }

    /// **`Notification`: posted as an `OSC 9` notification is, under the
    /// same rules** -- the user's setting, the window's focus, the desk's
    /// bounds -- its `title` and `message`, each decoded from base64, as
    /// iTerm2's source posts it "rich" (`[[source:
    /// iterm2-964a75d-ptysession]]`); a value that does not decode is none,
    /// as there. baud's notification has no subtitle, so a `subtitle` is
    /// dropped and counted (Jev's lean, DROPPED_COUNTED, 0.88/0.88), and so
    /// is an `image`, which names a picture for the system to load and
    /// which baud would have to read from wherever the program says.
    fn notification(&mut self, pairs: NoticePairs<'_>) {
        let (mut title, mut body) = ([0u8; OSC_MAX], [0u8; OSC_MAX]);
        let (mut tl, mut bl) = (0, 0);
        for (part, coded) in pairs.pairs() {
            let mut out = [0u8; OSC_MAX];
            let n = decode_whole(coded, &mut out)
                .map(ByteLen::index)
                .filter(|&n| core::str::from_utf8(&out[..n]).is_ok());
            match (part, n) {
                (NoticePart::Title, Some(n)) => {
                    title[..n].copy_from_slice(&out[..n]);
                    tl = n;
                }
                (NoticePart::Message, Some(n)) => {
                    body[..n].copy_from_slice(&out[..n]);
                    bl = n;
                }
                (NoticePart::Subtitle | NoticePart::Image, Some(n)) if n > 0 => {
                    self.unsupported_total += 1;
                }
                _ => {}
            }
        }
        let end = self.strs.notices.simple(
            self.strs.notice_area,
            NoticeFrom::Iterm,
            &title[..tl],
            &body[..bl],
        );
        self.notice_outcome(end);
    }

    /// **`RequestUpload`: refused with silence**: baud opens no chooser for
    /// a program to take the user's files through. The page has the
    /// terminal "respond with a status of ok or abort followed by a newline"
    /// (`[[source: iterm2-escape-codes]]`), and iTerm2's own source names the
    /// risk in that -- "The protocol calls to send \"abort\\n\" but this
    /// introduces a security risk because reports must not contain
    /// newlines" -- and sends Control-C instead (`[[source:
    /// iterm2-964a75d-ptysession]]`), which is no status the page names. An
    /// answer that is unsafe is not sent and one of another shape is never
    /// sent, so neither goes, for any format. Counted under its key, among
    /// the refusals.
    fn request_upload(&mut self) {
        self.not_acted(ItermKey::RequestUpload);
    }

    /// **`UnicodeVersion`: which Unicode version's widths the next character
    /// is measured by** -- "8 or 9", 0 the default, `push` and `pop` with an
    /// optional label (`[[source: iterm2-escape-codes]]`). Nine is the
    /// current tables; eight Unicode 8's (Jev's lean, BUILD_UNICODE8,
    /// 0.40/0.36 and 0.76/0.82 asked again). A push keeps the version and
    /// its label; a pop takes entries off until one has its label, or the
    /// top one with none, and restores that one's version, as iTerm2's
    /// `popUnicodeVersion:` does -- a label no entry has empties the stack
    /// and changes nothing. Cells already written keep their widths. A
    /// number iTerm2 does not take, a push past the stack and a label past
    /// its bound are counted under the key.
    fn unicode_version(&mut self, op: UnicodeOp<'_>) {
        let it = &mut self.strs.iterm;
        let depth = it
            .pushed
            .iter()
            .position(Option::is_none)
            .unwrap_or(UNICODE_STACK);
        let set = match op {
            UnicodeOp::Set(v) => Some(v),
            UnicodeOp::Default => Some(UnicodeWidths::Current),
            UnicodeOp::Push(label) => {
                if depth == UNICODE_STACK || label.len() > UNICODE_LABEL_MAX {
                    return self.not_acted(ItermKey::UnicodeVersion);
                }
                let mut e = Pushed {
                    label: [0; UNICODE_LABEL_MAX],
                    len: ByteLen::from_index(label.len()),
                    widths: self.cold.widths.widths(),
                };
                e.label[..label.len()].copy_from_slice(label);
                it.pushed[depth] = Some(e);
                None
            }
            UnicodeOp::Pop(label) => {
                let mut found = None;
                for slot in it.pushed[..depth].iter_mut().rev() {
                    let Some(e) = slot.take() else { continue };
                    if label.is_empty() || &e.label[..e.len.index()] == label {
                        found = Some(e.widths);
                        break;
                    }
                }
                found
            }
            UnicodeOp::Malformed => return self.not_acted(ItermKey::UnicodeVersion),
        };
        if let Some(v) = set {
            self.cold.widths = WidthPolicy::of(self.cold.widths.ambiguous(), v);
        }
    }

    /// **`Capabilities`: "OSC 1337 ; Capabilities = {FeatureString} ST"**
    /// (`[[source: iterm2-feature-reporting]]`), each feature claimed only
    /// where baud does it now, ended with `ESC \`: that page defines `ST`
    /// as "a synonym for `ESC 0x5c`", where the escape-codes page's notation
    /// lets "ST" be BEL as well (`[[source: iterm2-escape-codes]]`) and
    /// iTerm2's source ends this report with BEL; `ESC \` is the one ending
    /// both pages allow. Both 24-bit forms, the mouse modes with
    /// `?1006`, DECSCUSR 1 to 6 and 0 for the configured style, title
    /// stacks and titles, bracketed paste, focus reports, strikethrough and
    /// overline drawn, synchronised output, hyperlinks and progress always;
    /// `UNICODE_BASIC`, which "Indicates if UTF-8 is accepted", and the
    /// Unicode version while UTF-8 is the input, neither under ISO 8859-1,
    /// whose 0 is "No unicode support is claimed"; DECSLRM at Level 4 alone,
    /// where it is executed (Jev, LEVEL_GATED, 0.98/0.96); clipboard writes
    /// while `OSC 52` writes reach a clipboard; notifications while the
    /// embedder delivers them; Sixel while the Primary DA claims it;
    /// ambiguous-wide and the Unicode version as they are set. `FILE` is
    /// not claimed: it says "including those using lossy compression", and
    /// baud draws PNG alone and refuses downloads (Jev, NOT_CLAIMED,
    /// 0.90/0.92).
    fn capabilities(&mut self) {
        let clip =
            self.strs.window_asks.clipboard == Osc52Writes::On && !self.strs.clip_area.is_empty();
        // `24BIT` both forms (1 | 2), `DECSCUSR` 1-4, 5-6 and 0 (1 | 2 | 4),
        // `TITLES` stacks and setting (1 | 2).
        let utf8 = self.cold.encoding == InputEncoding::Utf8;
        let version = match utf8 {
            true => self.cold.widths.widths().version(),
            false => 0,
        };
        let f = Features::of(3, 7, version, 3)
            .with(Feature::ClipboardWritable, Claim::of(clip))
            .with(
                Feature::Decslrm,
                Claim::of(self.cold.level == ConformanceLevel::Four),
            )
            .with(Feature::Mouse, Claim::Yes)
            .with(Feature::UnicodeBasic, Claim::of(utf8))
            .with(
                Feature::AmbiguousWide,
                Claim::of(self.cold.widths.ambiguous() == AmbiguousWidth::Wide),
            )
            .with(Feature::BracketedPaste, Claim::Yes)
            .with(Feature::FocusReporting, Claim::Yes)
            .with(Feature::Strikethrough, Claim::Yes)
            .with(Feature::Overline, Claim::Yes)
            .with(Feature::Sync, Claim::Yes)
            .with(Feature::Hyperlinks, Claim::Yes)
            .with(
                Feature::Notifications,
                Claim::of(
                    self.strs.notices.support().is_some() && !self.strs.notice_area.is_empty(),
                ),
            )
            .with(
                Feature::Sixel,
                Claim::of(self.sixel_claim() == SixelClaim::Yes),
            )
            .with(Feature::Progress, Claim::Yes);
        let mut out = [0u8; 64];
        let codes = f.encode(&mut out);
        let mut buf = [0u8; 64];
        let n = codes.len();
        buf[..n].copy_from_slice(codes);
        self.push_reply(&[b"\x1b]1337;Capabilities=", &buf[..n], b"\x1b\\"]);
    }

    /// **`SetMark`: a line to jump back to**, "record a location and then
    /// jump back to it later" (`[[source: iterm2-escape-codes]]`), in the
    /// prompt log Cmd-Up and Cmd-Down already walk: the cursor's line, or
    /// with `saveScrollPosition` the window's top line. On the alternate
    /// screen, which keeps no history and where a jump does nothing, it is
    /// counted under its key (Jev, COUNTED, 0.96/0.96).
    fn set_mark(&mut self, at: MarkAt) {
        if self.cold.screen != crate::term::Screen::Normal {
            return self.not_acted(ItermKey::SetMark);
        }
        let line = match at {
            MarkAt::CursorLine => self.book.top.plus(Rows::of_count(self.cy.index())),
            MarkAt::WindowTop => self
                .book
                .top
                .minus(self.book.window_back())
                .unwrap_or(LineIdx::FIRST),
        };
        self.strs.prompts.mark(line);
    }

    /// **`SetBadgeFormat=base64`: the badge's format**, kept for the embedder
    /// to evaluate and draw (`Term::badge_format`), as iTerm2's
    /// `screenSetBadgeFormat` keeps it (`[[source:
    /// iterm2-964a75d-ptysession]]`): decoded as UTF-8, and left as it was,
    /// and counted under the key, when it is not base64 of UTF-8 ("Badge is
    /// not properly base64 encoded"), when it calls a function ("For
    /// security reasons, this is not allowed and the badge was not
    /// updated"), or when its body was cut at `OSC_MAX`. An empty one
    /// clears the badge. `RIS` keeps it, as iTerm2's reset does. Each
    /// expression in it but a user variable's is counted
    /// (`ItermCounts::badge_unevaluated`): the embedder draws those as
    /// nothing (Jev's lean, LITERAL_AND_USER).
    fn set_badge_format(&mut self, b64: &[u8], body: Body) {
        let mut out = [0u8; BADGE_MAX];
        let mut n = 0;
        let mut b = Base64::NEW;
        let mut fits = body == Body::Whole;
        let mut keep = |o: Octets, n: &mut usize, fits: &mut bool| {
            for &x in o.as_slice() {
                match out.get_mut(*n) {
                    Some(slot) => *slot = x,
                    None => *fits = false,
                }
                *n += 1;
            }
        };
        for &x in b64 {
            match b.push(x) {
                Base64Step::Octets(o) => keep(o, &mut n, &mut fits),
                Base64Step::Need => {}
                Base64Step::Invalid => return self.not_acted(ItermKey::SetBadgeFormat),
            }
        }
        match b.finish() {
            Ok(tail) => keep(tail, &mut n, &mut fits),
            Err(_) => return self.not_acted(ItermKey::SetBadgeFormat),
        }
        let Some(text) = out.get(..n).filter(|_| fits) else {
            return self.not_acted(ItermKey::SetBadgeFormat);
        };
        match core::str::from_utf8(text) {
            Ok(s) if !calls_a_function(s) => {}
            _ => return self.not_acted(ItermKey::SetBadgeFormat),
        }
        let it = &mut self.strs.iterm;
        it.badge[..n].copy_from_slice(text);
        it.badge_at = Badge::Set(ByteLen::from_index(n));
        let mut unevaluated = it.counts.badge_unevaluated;
        if let Some(s) = it.badge() {
            interpolated_parts(s, |p| {
                if let Interpolated::Expression(e) = p {
                    if user_variable(e).is_none() {
                        unevaluated = unevaluated.next();
                    }
                }
            });
        }
        self.strs.iterm.counts.badge_unevaluated = unevaluated;
    }

    /// **`SetColors`: each `key=value` sets the colour baud keeps for that
    /// role**, where `OSC 10`, `11`, `12`, `17` and `19`, `OSC 5 ; 0` and
    /// `OSC 4` set it -- the text, the page, the cursor, the selection and
    /// its text, bold text and the sixteen ANSI colours -- a P3 or Generic
    /// RGB value converted to sRGB as a theme file's is. `curfg` sets the
    /// text under a block cursor, as `OSC 21 ; cursor_text` does. `badge`
    /// sets the badge's, opaque, as iTerm2's source makes a hex colour.
    /// `link`, `underline`, `tab`, `preset` and the source's `match` name
    /// colours baud keeps none of, and each such pair, and each off
    /// the form, is counted as an `OSC 4` pair that cannot be honoured is.
    fn set_colors(&mut self, list: ColorSettings<'_>) {
        for setting in list {
            let (target, rgb, space) = match setting {
                ColorSetting::Set(t, ColorValue::Rgb(rgb, space)) => (t, rgb, space),
                _ => {
                    self.unsupported_total += 1;
                    continue;
                }
            };
            let c = rgb.map(|v| f64::from(v) / 255.0);
            let ([r, g, b], _) = themefile::gamut::to_srgb(space, c);
            let w = ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
            let dynamic = |which| Some(which);
            let slot = match target {
                ColorTarget::Foreground => dynamic(DynamicColor::Foreground),
                ColorTarget::Background => dynamic(DynamicColor::Background),
                ColorTarget::Cursor => dynamic(DynamicColor::Cursor),
                ColorTarget::Selection => dynamic(DynamicColor::HighlightBg),
                ColorTarget::SelectedText => dynamic(DynamicColor::HighlightFg),
                ColorTarget::Bold => {
                    self.set_special(SpecialColor::Bold, w);
                    None
                }
                ColorTarget::Ansi(c) => {
                    self.set_palette(c, w);
                    None
                }
                ColorTarget::CursorText => {
                    self.strs.palette[crate::storage::CURSOR_TEXT] = w;
                    None
                }
                ColorTarget::Badge => {
                    self.strs.iterm.badge_colour = Some(w);
                    None
                }
                ColorTarget::Link
                | ColorTarget::Match
                | ColorTarget::Underline
                | ColorTarget::Tab
                | ColorTarget::Preset => {
                    self.unsupported_total += 1;
                    None
                }
            };
            if let Some(which) = slot {
                let role = self.cold.video().role(which);
                self.set_dynamic(role, w);
            }
        }
    }

    /// A `Copy=:` that reached the dispatcher whole -- the core streams one
    /// as it arrives, so only a body handed to `Osc::parse` by another
    /// caller -- through the same clipboard path.
    fn copy_whole(&mut self, data: &[u8]) {
        let kind = self.cold.str_kind;
        self.copy_start();
        for &b in data {
            self.clip_put(StreamByte::from_stream(b));
        }
        self.clip_end();
        self.cold.str_kind = kind;
    }
}

/// `data`, base64, decoded whole into `out`: its length, or `None` for data
/// that is not base64 or does not fit.
fn decode_whole(data: &[u8], out: &mut [u8]) -> Option<ByteLen> {
    let mut n = 0;
    let mut b64 = Base64::NEW;
    let mut keep = |o: Octets, n: &mut usize| -> Option<()> {
        for &x in o.as_slice() {
            *out.get_mut(*n)? = x;
            *n += 1;
        }
        Some(())
    };
    for &b in data {
        match b64.push(b) {
            Base64Step::Octets(o) => keep(o, &mut n)?,
            Base64Step::Need => {}
            Base64Step::Invalid => return None,
        }
    }
    keep(b64.finish().ok()?, &mut n)?;
    Some(ByteLen::from_index(n))
}

fn terminator(term: StringTerm) -> &'static [u8] {
    match term {
        StringTerm::Bel => b"\x07",
        StringTerm::St => b"\x1b\\",
    }
}
