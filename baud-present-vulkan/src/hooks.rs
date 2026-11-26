//! **Failures forced on the device, and the targets alive**, for the
//! crate's own tests: the `test-hooks` feature, which baud's own build
//! leaves off (`xtask/tests/test_hooks.rs`). The `n`-th fallible device
//! call from now fails with the code given -- a target's allocation, a
//! submission -- as a lost device or memory running out would, and a count
//! of the targets made and not yet dropped says whether a failure left one
//! behind.

use crate::frame::Failed;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

/// How many fallible device calls succeed before the forced one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calls(pub u32);

/// How many targets are made and not yet dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Live(pub usize);

#[derive(Default)]
struct State {
    fail: Option<(Calls, Failed)>,
    live: Live,
    /// Landings held: the end handed out for the next frame, and the side
    /// whose write turns it readable.
    landing: Option<(Option<OwnedFd>, UnixStream)>,
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

    /// **The held landings released**: their end turns readable.
    pub(crate) fn release_landings(&self) {
        if let Some((_, mut release)) = self.with(|s| s.landing.take()) {
            let _ = release.write_all(&[1]);
        }
    }
}
