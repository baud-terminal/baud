//! **Failures forced on the device, and the targets alive**, for the
//! crate's own tests: the `test-hooks` feature, which baud's own build
//! leaves off (`xtask/tests/test_hooks.rs`). The `n`-th fallible device
//! call from now fails with the code given -- a target's allocation, a
//! submission, a step of the cell path's memory, a buffer's memory,
//! binding or mapping, an image's memory, binding or view, the sheets'
//! sampler -- as a lost device or memory running out would, and a count of
//! the targets made and not yet dropped says whether a failure left one
//! behind; a second count, the `Gpu`'s tally, says the same of the other
//! device objects the crate makes.

use crate::frame::Failed;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

/// How many fallible device calls succeed before the forced one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calls(pub u32);

/// How many are made and not yet dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Live(pub usize);

/// How many frames' ends the kernel took as their target's write fence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fenced(pub u64);

/// Whether a wake at a release point is made as the kernel answers, or
/// refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Wakes {
    #[default]
    Asked,
    Refused,
}

/// What a frame's end exports as: the descriptor the implementation made,
/// or `-1`, as an implementation may answer for a semaphore already
/// signalled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Export {
    #[default]
    AsMade,
    Signalled,
}

#[derive(Default)]
struct State {
    fail: Option<(Calls, Failed)>,
    live: Live,
    /// Landings held: the end handed out for the next frame, and the side
    /// whose write turns it readable.
    landing: Option<(Option<OwnedFd>, UnixStream)>,
    /// What targets made from now answer whether they take sync files.
    sync_files: Option<crate::sys::SyncFiles>,
    /// The frames' ends the kernel took as their target's write fence.
    fenced: Fenced,
    /// Whether wakes at a release point are refused.
    wakes: Wakes,
    /// What frames' ends export as.
    export: Export,
}

/// The forced failure waiting, and the targets alive.
#[derive(Default)]
pub(crate) struct Hooks(Mutex<State>);

impl Hooks {
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub(crate) fn fail_at(&self, n: Calls, code: Failed) {
        self.with(|s| s.fail = Some((n, code)));
    }

    /// **A fallible device call**: the forced failure when its turn has
    /// come, once.
    pub(crate) fn call(&self) -> Result<(), Failed> {
        self.with(|s| match s.fail {
            Some((Calls(0), code)) => {
                s.fail = None;
                Err(code)
            }
            Some((Calls(n), code)) => {
                s.fail = Some((Calls(n - 1), code));
                Ok(())
            }
            None => Ok(()),
        })
    }

    pub(crate) fn made(&self) {
        self.with(|s| s.live.0 += 1);
    }

    pub(crate) fn dropped(&self) {
        self.with(|s| s.live.0 = s.live.0.saturating_sub(1));
    }

    pub(crate) fn live(&self) -> Live {
        self.with(|s| s.live)
    }

    /// **Landings held from now**: a connected pair, the end handed out
    /// for the next frame and the side the release writes.
    pub(crate) fn hold_landings(&self) {
        let Ok((end, release)) = UnixStream::pair() else {
            return;
        };
        self.with(|s| s.landing = Some((Some(end.into()), release)));
    }

    /// Whether landings are held.
    pub(crate) fn landing_held(&self) -> crate::vk::Has {
        self.with(|s| match s.landing {
            Some(_) => crate::vk::Has::Yes,
            None => crate::vk::Has::No,
        })
    }

    /// The held landing's end, for the frame in flight, once.
    pub(crate) fn landing_fd(&self) -> Option<OwnedFd> {
        self.with(|s| s.landing.as_mut().and_then(|(end, _)| end.take()))
    }

    /// **Targets made from now refuse sync files**, as a dmabuf whose
    /// exporter keeps no fences does.
    pub(crate) fn refuse_sync_files(&self) {
        self.with(|s| s.sync_files = Some(crate::sys::SyncFiles::Refused));
    }

    /// What a target answers whether it takes sync files: refused while
    /// [`Hooks::refuse_sync_files`] holds, else `answer`.
    pub(crate) fn sync_files(&self, answer: crate::sys::SyncFiles) -> crate::sys::SyncFiles {
        self.with(|s| s.sync_files.unwrap_or(answer))
    }

    /// **Wakes at a release point refused from now**, as a kernel without
    /// the request refuses them.
    pub(crate) fn refuse_wakes(&self) {
        self.with(|s| s.wakes = Wakes::Refused);
    }

    /// Whether wakes at a release point are refused.
    pub(crate) fn wakes(&self) -> Wakes {
        self.with(|s| s.wakes)
    }

    /// **Frames' ends export as `-1` from now**, their descriptors closed.
    pub(crate) fn signal_exports(&self) {
        self.with(|s| s.export = Export::Signalled);
    }

    /// What frames' ends export as.
    pub(crate) fn export(&self) -> Export {
        self.with(|s| s.export)
    }

    /// A frame's end taken as its target's write fence.
    pub(crate) fn fenced(&self) {
        self.with(|s| s.fenced.0 += 1);
    }

    /// The frames' ends taken as their target's write fence so far.
    pub(crate) fn fences(&self) -> Fenced {
        self.with(|s| s.fenced)
    }

    /// **The held landings released**: their end turns readable.
    pub(crate) fn release_landings(&self) {
        if let Some((_, mut release)) = self.with(|s| s.landing.take()) {
            let _ = release.write_all(&[1]);
        }
    }
}
