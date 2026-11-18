//! **The Linux driver's GPU arm**: frames drawn on the device the compositor
//! names (`baud_present_vulkan::present`) and shown as dmabufs, with the CPU
//! arm -- `render_at` into shared memory -- under it for every frame the GPU
//! arm does not draw.
//!
//! It starts at the first frame the window has a size and the compositor's
//! formats for, and stays off for the window's life when it cannot:
//! `BAUD_GPU=off`, no Vulkan loader, no device or format the compositor
//! takes, or a failure on the device. Each reason is logged once
//! ([`Off`]). A frame the arm hands back is the CPU arm's
//! ([`Arm::Cpu`]); one it owes, with both buffers still the compositor's,
//! draws nothing and waits for a release ([`Arm::Owed`]).

use baud_core::{Seal, Term};
use baud_present_vulkan::choose::{Refusal, SyncObj, choose};
use baud_present_vulkan::damage::{TARGETS, TargetIx};
use baud_present_vulkan::frame::{Failed, Gpu};
use baud_present_vulkan::present::{Frame, Inputs, Presenter, Remade};
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::dmabuf::{Dmabuf, Imported, Points, Timeline};
use baud_present_wayland::{DmabufShown, Holds, Window};
use baud_render::Renderer;
use baud_wayland::{ObjectId, Sender};
use std::os::fd::{AsFd, BorrowedFd};

/// Why the GPU arm is off for the window's life.
#[derive(Debug)]
pub enum Off {
    /// `BAUD_GPU=off`.
    Env,
    /// No Vulkan loader.
    NoVulkan,
    /// No device or format the compositor takes.
    Refused(Refusal),
    /// The device failed.
    Failed(Failed),
    /// The compositor did not import a target.
    NotImported,
}

impl core::fmt::Display for Off {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Off::Env => f.write_str("BAUD_GPU=off"),
            Off::NoVulkan => f.write_str("no Vulkan loader"),
            Off::Refused(r) => write!(f, "the compositor's device or formats: {r:?}"),
            Off::Failed(e) => write!(f, "the device failed: {e:?}"),
            Off::NotImported => f.write_str("the compositor imported no target"),
        }
    }
}

/// What became of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    /// Drawn and handed to the compositor.
    Shown,
    /// Nothing drawn: both buffers are the compositor's.
    Owed,
    /// The CPU arm draws it.
    Cpu,
}

/// The arm while it runs: the device, the presenter, and each target's
/// buffer and timeline as the compositor knows them.
struct On {
    gpu: Gpu,
    presenter: Presenter,
    buffers: [Option<ObjectId>; TARGETS],
    timelines: [Option<Timeline>; TARGETS],
}

enum State {
    /// Not started: no size or formats yet.
    Waiting,
    /// Off for the window's life, its reason logged.
    Off,
    On(Box<On>),
}

/// Which arm drew the frame before.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    Gpu,
    Cpu,
}

pub struct GpuArm {
    state: State,
    last: Last,
}

impl GpuArm {
    /// The arm, off when `env` -- `BAUD_GPU` -- is `off`.
    pub fn new(env: Option<&str>) -> GpuArm {
        GpuArm {
            state: match env {
                Some("off") => {
                    crate::info!("gpu: off, frames drawn on the CPU: {}", Off::Env);
                    State::Off
                }
                _ => State::Waiting,
            },
            last: Last::Cpu,
        }
    }

    /// **Started on the compositor's device**, once the window has a size
    /// and formats; any failure turns it off, logged.
    fn start(&mut self, window: &Window, term: &Term<'_>) {
        if !matches!(self.state, State::Waiting) {
            return;
        }
        let (Some(offered), Some(_)) = (window.buffer_formats(), window.buffer_size()) else {
            return;
        };
        let started = (|| -> Result<On, Off> {
            let vulkan = Vulkan::open().map_err(|_| Off::NoVulkan)?;
            let candidates = vulkan.candidates();
            let only: Vec<_> = candidates.iter().map(|(_, c)| c.clone()).collect();
            let sync = match window.syncobj_offered() {
                true => SyncObj::Offered,
                false => SyncObj::Absent,
            };
            let choice = choose(&only, offered, sync).map_err(Off::Refused)?;
            let physical = candidates[choice.device.0].0;
            let gpu = Gpu::new(vulkan, physical, choice).map_err(Off::Failed)?;
            let presenter = Presenter::new(&gpu, term).map_err(Off::Failed)?;
            Ok(On {
                gpu,
                presenter,
                buffers: [None; TARGETS],
                timelines: [None; TARGETS],
            })
        })();
        self.state = match started {
            Ok(on) => {
                crate::info!("gpu: frames drawn on the device the compositor names");
                State::On(Box::new(on))
            }
            Err(why) => {
                crate::info!("gpu: off, frames drawn on the CPU: {why}");
                State::Off
            }
        };
    }

    /// **The frame drawn on the GPU arm and shown**, or owed, or the CPU
    /// arm's. `slide` says a slide is composed this frame, which only the
    /// CPU arm does.
    pub fn frame(
        &mut self,
        (window, s): (&mut Window, &mut Sender<'_>),
        renderer: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        seal: Seal,
        inputs: &Inputs<'_>,
        slide: crate::effects::Composed,
    ) -> Arm {
        self.start(window, term);
        let arm = match (&mut self.state, slide) {
            (State::On(on), crate::effects::Composed::No) => {
                match on.frame((window, s), renderer, term, seal, inputs) {
                    Ok(arm) => arm,
                    Err(why) => {
                        crate::warn!("gpu: off, frames drawn on the CPU: {why}");
                        if let State::On(on) = core::mem::replace(&mut self.state, State::Off) {
                            on.presenter.destroy(&on.gpu);
                        }
                        Arm::Cpu
                    }
                }
            }
            _ => Arm::Cpu,
        };
        match arm {
            Arm::Shown => self.last = Last::Gpu,
            Arm::Cpu => {
                if let State::On(on) = &mut self.state {
                    on.presenter.drawn_elsewhere();
                }
            }
            Arm::Owed => {}
        }
        arm
    }

    /// **Whether the CPU arm's own buffer is stale**: the frame before was
    /// the GPU arm's, so the CPU arm repaints whole. Read once a CPU frame.
    pub fn cpu_owes_whole(&mut self) -> bool {
        let was = self.last == Last::Gpu;
        self.last = Last::Cpu;
        was
    }

    /// **What the last frame the GPU arm drew cost**, from its target in hand
    /// to its fence; `None` while the arm is not on.
    pub fn cost(&self) -> Option<std::time::Duration> {
        match &self.state {
            State::On(on) => Some(on.presenter.cost()),
            _ => None,
        }
    }

    /// The descriptor a finished pipeline build makes readable.
    pub fn ready_fd(&self) -> Option<BorrowedFd<'_>> {
        match &self.state {
            State::On(on) => on.presenter.ready_fd(),
            _ => None,
        }
    }

    /// **A build finished**: whether a frame is owed.
    pub fn woken(&mut self) -> bool {
        match &mut self.state {
            State::On(on) => on.presenter.finished() == baud_present_vulkan::pipeline::Built::Yes,
            _ => false,
        }
    }
}

impl On {
    fn frame(
        &mut self,
        (window, s): (&mut Window, &mut Sender<'_>),
        renderer: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        seal: Seal,
        inputs: &Inputs<'_>,
    ) -> Result<Arm, Off> {
        let Some(size) = window.buffer_size() else {
            return Ok(Arm::Cpu);
        };
        if self.presenter.fit(&self.gpu, size).map_err(Off::Failed)? == Remade::Yes {
            self.import(window, s)?;
        }
        // The buffers the compositor released since.
        for (k, b) in self.buffers.iter().enumerate() {
            if let Some(b) = b {
                if window.holds(*b) == Holds::No {
                    self.presenter.released(TargetIx(k));
                }
            }
        }
        let k = match self
            .presenter
            .frame(&self.gpu, renderer, term, seal, inputs)
            .map_err(Off::Failed)?
        {
            Frame::Drew(k) => k,
            Frame::Owed => return Ok(Arm::Owed),
            Frame::Cpu(_) => return Ok(Arm::Cpu),
        };
        let points = match (
            self.presenter.points(&self.gpu, k).map_err(Off::Failed)?,
            self.timelines[k.0],
        ) {
            (Some((acquire, release)), Some(tl)) => Some(Points {
                acquire: (tl, acquire),
                release: (tl, release),
            }),
            _ => None,
        };
        let Some(buffer) = self.buffers[k.0] else {
            return Ok(Arm::Cpu);
        };
        let shown = window.present_dmabuf(s, buffer, size, points, self.presenter.damage());
        self.presenter.shown(k, shown);
        Ok(match shown {
            DmabufShown::Yes => Arm::Shown,
            _ => Arm::Cpu,
        })
    }

    /// **Each new target made a buffer**, and its timeline imported where
    /// frames carry points; the old targets' buffers and timelines
    /// destroyed first.
    fn import(&mut self, window: &mut Window, s: &mut Sender<'_>) -> Result<(), Off> {
        for b in self.buffers.iter_mut().filter_map(Option::take) {
            window.drop_buffer(s, b);
        }
        for tl in self.timelines.iter_mut().filter_map(Option::take) {
            baud_present_wayland::dmabuf::destroy_timeline(s, tl);
        }
        for k in 0..TARGETS {
            let Some((t, tl)) = self.presenter.target(TargetIx(k)) else {
                return Err(Off::NotImported);
            };
            let planes = [t.plane()];
            let d = Dmabuf {
                width: t.width,
                height: t.height,
                format: t.format,
                planes: &planes,
            };
            self.buffers[k] = match window.import(s, &d) {
                Imported::Buffer(b) => Some(b),
                _ => return Err(Off::NotImported),
            };
            self.timelines[k] = match tl {
                Some(tl) => Some(
                    window
                        .import_timeline(s, tl.fd.as_fd())
                        .ok_or(Off::NotImported)?,
                ),
                None => None,
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **`BAUD_GPU=off` keeps the arm off from the start**, and nothing
    /// else does: unset or any other value, it waits to start.
    #[test]
    fn baud_gpu_off_is_off_from_the_start() {
        assert!(matches!(GpuArm::new(Some("off")).state, State::Off));
        assert!(matches!(GpuArm::new(None).state, State::Waiting));
        assert!(matches!(GpuArm::new(Some("on")).state, State::Waiting));
    }
}
