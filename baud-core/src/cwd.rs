//! **`OSC 7`: the directory a shell says it is in**, kept for the embedder
//! (`Term::working_directory`). The URI is read by
//! [`baud_domain::DirectoryReport`], on RFC 8089's grammar; what is here is
//! the keeping.
//!
//! **Every report replaces the last, and one that cannot be read leaves
//! none.** A shell sends its directory at each prompt; one the core cannot
//! read -- off the grammar, another scheme, a path decoding to a NUL, or a
//! body past `OSC_MAX` cut short -- still says the shell moved, so the
//! directory it names last is no longer known. Kept, the old one would be
//! shown as where the shell is after it left. Each such report is counted
//! (`Term::directories_refused`) and as unsupported.
//!
//! **A host other than this machine is kept as named, not judged here.**
//! RFC 8089 § 3 makes a file URI local when its host "resolves to the
//! machine from which the URI is being interpreted" (`[[source: rfc8089]]`
//! 236-240), and § 5 warns that "Treating a non-local file URI as local ...
//! can result in security problems" (287-289): which machine that is, the
//! embedder knows and a no_std core does not. [`FileHost::Local`] is the
//! two forms the RFC itself calls local, no host and `localhost`.
//!
//! `RIS` forgets it, with the host `OSC 1337 ; RemoteHost` named: a
//! terminal made operational has no directory reported
//! (`[[source: ecma48]]` 8.3.105), and a shell reports it again at its next
//! prompt (Jev's lean, RESTORES, 0.80/0.86). `DECSTR` keeps both.

use crate::logging::Body;
use crate::paint::Paint;
use baud_domain::*;

/// A count of the `OSC 7` reports refused, as a reading to compare: off
/// RFC 8089's grammar, of another scheme, decoding to a NUL, or cut at
/// `OSC_MAX`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirectoriesRefused(u32);

impl DirectoriesRefused {
    pub(crate) const FIRST: DirectoriesRefused = DirectoriesRefused(0);

    fn next(self) -> DirectoriesRefused {
        DirectoriesRefused(self.0.wrapping_add(1))
    }
}

/// The shell's directory as its last `OSC 7` gave it: the host it named
/// and the path, percent-decoded. The path is bytes, as a file system
/// stores a name; RFC 8089 § 4 asks for UTF-8 before the encoding, and a
/// shell that sent something else is read as it sent it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorkingDirectory<'t> {
    pub host: FileHost<'t>,
    pub path: &'t [u8],
}

/// What [`crate::storage::StrBuf`]'s `cwd` bytes hold: nothing, or a host
/// name's bytes (none for [`FileHost::Local`]) followed by the path's.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kept {
    Nothing,
    Local(ByteLen),
    Named { host: ByteLen, path: ByteLen },
}

/// The kept directory and the refusals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cwd {
    pub(crate) kept: Kept,
    pub(crate) refused: DirectoriesRefused,
}

impl Cwd {
    pub(crate) const NONE: Cwd = Cwd {
        kept: Kept::Nothing,
        refused: DirectoriesRefused::FIRST,
    };

    /// The directory, read out of the bytes `kept` describes.
    pub(crate) fn read<'t>(&self, bytes: &'t [u8]) -> Option<WorkingDirectory<'t>> {
        match self.kept {
            Kept::Nothing => None,
            Kept::Local(path) => Some(WorkingDirectory {
                host: FileHost::Local,
                path: &bytes[..path.index()],
            }),
            Kept::Named { host, path } => {
                let (h, rest) = bytes.split_at(host.index());
                Some(WorkingDirectory {
                    host: FileHost::Named(h),
                    path: &rest[..path.index()],
                })
            }
        }
    }
}

impl Paint<'_, '_> {
    /// An `OSC 7` read: kept, or refused and counted, and either way it
    /// replaces the directory known before it.
    pub(crate) fn report_directory(&mut self, report: DirectoryReport<'_>, body: Body) {
        let kept = match (report, body) {
            (DirectoryReport::File(uri), Body::Whole) => keep(self.strs.cwd, uri),
            _ => None,
        };
        match kept {
            Some(k) => self.strs.cwd_at.kept = k,
            None => self.refuse_directory(),
        }
    }

    /// A report that cannot be read: it replaces the directory known
    /// before it, leaving none, and is counted.
    pub(crate) fn refuse_directory(&mut self) {
        self.strs.cwd_at.refused = self.strs.cwd_at.refused.next();
        self.unsupported_total += 1;
        self.strs.cwd_at.kept = Kept::Nothing;
    }

    /// The directory is no longer known, which is no refusal: an `OSC 1337 ;
    /// RemoteHost` moved the shell to a host whose directory has not been
    /// said.
    pub(crate) fn forget_directory(&mut self) {
        self.strs.cwd_at.kept = Kept::Nothing;
    }
}

/// `host` (none: this machine) and `path` written into `out` as sent, as
/// `OSC 1337 ; CurrentDir` gives them: `None` when the two do not fit.
pub(crate) fn keep_as_sent(out: &mut [u8], host: Option<&[u8]>, path: &[u8]) -> Option<Kept> {
    let h = host.map_or(0, <[u8]>::len);
    let all = out.get_mut(..h + path.len())?;
    let (dst, rest) = all.split_at_mut(h);
    dst.copy_from_slice(host.unwrap_or(b""));
    rest.copy_from_slice(path);
    let path = ByteLen::from_index(path.len());
    Some(match host {
        None => Kept::Local(path),
        Some(_) => Kept::Named {
            host: ByteLen::from_index(h),
            path,
        },
    })
}

/// Write `uri`'s host and decoded path into `out`, and say where they are;
/// `None` when the path decodes to a NUL or does not fit, which a body
/// within `OSC_MAX` always does.
fn keep(out: &mut [u8], uri: FileUri<'_>) -> Option<Kept> {
    match uri.host {
        FileHost::Local => {
            let n = ByteLen::from_index(uri.decode_path(out)?.len());
            Some(Kept::Local(n))
        }
        FileHost::Named(h) => {
            let (dst, rest) = out.split_at_mut_checked(h.len())?;
            dst.copy_from_slice(h);
            let n = ByteLen::from_index(uri.decode_path(rest)?.len());
            Some(Kept::Named {
                host: ByteLen::from_index(h.len()),
                path: n,
            })
        }
    }
}
