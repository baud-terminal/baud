//! Logging (XTLOGGING): `CSI ? 46 h`, "Start logging (XTLOGGING), xterm",
//! `CSI ? 46 l`, "Stop logging", and `OSC 46 ; Pt`, "Change Log File to Pt.
//! The parameter Pt sets the logFile resource" (`[[source: ctlseqs]]`).
//!
//! **The core keeps the mode and the name, and writes nothing**: the file
//! is the embedder's, which turns the feature on ([`Logging`]) and writes
//! the bytes of each read the core parsed while the mode was set. So the
//! log holds the host's bytes as xterm's does, from the byte after `?46 h`
//! up to and including the `?46 l` (`StartLog` takes `logstart =
//! VTbuffer->next`, and `CloseLog` flushes to it, `[[source:
//! xterm-misc]]`), **a read stops at each change** -- the parser goes to
//! `Node::Held`, as at a page-width request -- so every part of a read the
//! embedder feeds was parsed under one state, read before it was fed.
//!
//! As xterm: `OSC 46` only renames, so a log already open keeps its file
//! until the next start; `?` and an empty `Pt` change nothing and are
//! counted; a name beginning `|`, which xterm runs as a command only when
//! built with `ALLOWLOGFILEEXEC`, is refused at the start and counted, as
//! a program on the line, a remote one included, must not run commands;
//! `RIS` and `DECSTR` leave the mode and the name. A name `OSC_MAX` cut
//! is a different file, refused and counted, as a clipped link is.

use baud_domain::{ByteLen, LogFile, LogMode, Logging, ModeChange, ModeState};

use crate::paint::Paint;
use crate::storage::OSC_MAX;

/// A count of the logging requests refused, as a reading to compare: a
/// `?46` or an `OSC 46` while the embedder has logging off, a start naming
/// a command, and an `OSC 46` that names no file (`?`, empty, or cut).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogsRefused(u32);

/// Whether an `OSC` body was kept whole or reached `OSC_MAX`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Body {
    Whole,
    Cut,
}

/// The setting, the mode and the name `OSC 46` last gave.
#[derive(Clone, Copy)]
pub(crate) struct Logger {
    pub(crate) setting: Logging,
    pub(crate) mode: LogMode,
    name: [u8; OSC_MAX],
    len: ByteLen,
    pub(crate) refused: LogsRefused,
}

impl Logger {
    pub(crate) const NONE: Logger = Logger {
        setting: Logging::Off,
        mode: LogMode::Stopped,
        name: [0; OSC_MAX],
        len: ByteLen::ZERO,
        refused: LogsRefused(0),
    };

    /// The name `OSC 46` gave, `None` before one: the embedder's default.
    pub(crate) fn name(&self) -> Option<&[u8]> {
        match self.len.index() {
            0 => None,
            n => Some(&self.name[..n]),
        }
    }

    pub(crate) fn refuse(&mut self) {
        self.refused = LogsRefused(self.refused.0.wrapping_add(1));
    }

    /// What `DECRQM ?46` answers: `4` while the embedder has logging off,
    /// as no program can set it, else whether a log is started.
    pub(crate) fn state(&self) -> ModeState {
        match (self.setting, self.mode) {
            (Logging::Off, _) => ModeState::PermanentlyReset,
            (Logging::On, LogMode::Started) => ModeState::Set,
            (Logging::On, LogMode::Stopped) => ModeState::Reset,
        }
    }
}

impl Paint<'_, '_> {
    /// `OSC 46 ; Pt`: the name the next start opens, unless the body
    /// reached `OSC_MAX` and the name may be cut.
    pub(crate) fn log_file(&mut self, f: LogFile<'_>, body: Body) {
        let log = &mut self.strs.log;
        match (log.setting, f, body) {
            (Logging::On, LogFile::Name(name), Body::Whole) => {
                log.name[..name.len()].copy_from_slice(name);
                log.len = ByteLen::from_index(name.len());
            }
            _ => log.refuse(),
        }
    }

    /// `?46`: start or stop, the read stopping after the change so the
    /// embedder starts or stops between two bytes of it.
    pub(crate) fn set_logging_mode(&mut self, change: ModeChange, node: &mut crate::graph::Node) {
        let log = &mut self.strs.log;
        let next = match (log.setting, change) {
            (Logging::Off, _) => return log.refuse(),
            (Logging::On, ModeChange::Set) if log.name().is_some_and(|n| n[0] == b'|') => {
                return log.refuse();
            }
            (Logging::On, ModeChange::Set) => LogMode::Started,
            (Logging::On, ModeChange::Reset) => LogMode::Stopped,
        };
        if next != log.mode {
            log.mode = next;
            self.hold_read(node);
        }
    }
}
