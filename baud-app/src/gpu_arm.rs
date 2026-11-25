//! **The Linux driver's GPU arm**: frames drawn on the device the compositor
//! names (`baud_present_vulkan::present`) and shown as dmabufs, with the CPU
//! arm -- `render_at` into shared memory -- under it for every frame the GPU
//! arm does not draw.
//!
//! It starts at the first frame the window has a size and the compositor's
//! formats for. `BAUD_GPU=off` keeps it off. Any failure hands that frame to
//! the CPU arm, and `gpu_fallback.rs` decides what follows: a lost device is
//! replaced once a process, a size whose targets ran out of memory waits for
//! a smaller one, and anything else turns the arm off for the window's life.
//! Each reason is logged ([`Off`]); a start names the device as it reports
//! itself (`baud_present_vulkan::reported`). The arm it leaves is torn down
//! once a CPU frame has been committed over it, each `wl_buffer` and
//! timeline before the memory and the device ([`teardown`]), and logged with
//! how long that took. A frame the arm hands back is the CPU arm's
//! ([`Arm::Cpu`]); one it owes, with both buffers still the compositor's,
//! draws nothing and waits for a release ([`Arm::Owed`]). Each frame is
//! counted by which it was, and the counts are written on one line as the
//! window ends ([`Tally`]).

use crate::gpu_fallback::{Due, Failure, Fallback, Next, Restarts, Size, smaller};
use baud_core::{Seal, Term};
use baud_present_vulkan::choose::{Refusal, SyncObj, choose};
use baud_present_vulkan::damage::{TARGETS, TargetIx};
use baud_present_vulkan::frame::{Failed, Gpu, Kind};
use baud_present_vulkan::present::{Cpu, Frame, Inputs, Presenter, Remade};
use baud_present_vulkan::reported::Reported;
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::dmabuf::{Answer, Dmabuf, Imported, Params, Points, Timeline};
use baud_present_wayland::{DmabufShown, Holds, Window};
use baud_render::Renderer;
use baud_wayland::{ObjectId, Sender};
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Instant;

/// Why the GPU arm left.
#[derive(Debug)]
pub enum Off {
    /// `BAUD_GPU=off`.
    Env,
    /// No Vulkan loader.
    NoVulkan,
    /// No device or format the compositor takes.
    Refused(Refusal),
    /// The device failed making the targets for a size.
    Fitting(Failed),
    /// The device failed.
    Failed(Failed),
    /// The compositor refused a target, or could not be asked for one.
    NotImported,
}

impl Off {
    /// **The failure the fallback weighs**, at the window's `size`; none
    /// for `BAUD_GPU=off`, which is not one.
    fn failure(&self, size: Option<Size>) -> Option<Failure> {
        let device = |e: &Failed, oom: Failure| match e.kind() {
            Kind::Lost => Failure::Lost,
            Kind::OutOfMemory => oom,
            Kind::Other => Failure::Other(*e),
        };
        Some(match self {
            Off::Env => return None,
            Off::NoVulkan => Failure::NoLoader,
            Off::Refused(_) | Off::NotImported => Failure::Refused(size),
            Off::Fitting(e) => match size {
                Some(s) => device(e, Failure::OutOfMemoryFitting(s)),
                None => device(e, Failure::OutOfMemory),
            },
            Off::Failed(e) => device(e, Failure::OutOfMemory),
        })
    }
}

impl core::fmt::Display for Off {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Off::Env => f.write_str("BAUD_GPU=off"),
            Off::NoVulkan => f.write_str("no Vulkan loader"),
            Off::Refused(r) => write!(f, "the compositor's device or formats: {r:?}"),
            Off::Fitting(e) => write!(f, "the device failed making the targets: {e:?}"),
            Off::Failed(e) => write!(f, "the device failed: {e:?}"),
            Off::NotImported => f.write_str("the compositor refused a target"),
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
    /// The CPU arm draws it, for this reason.
    Cpu(Why),
}

/// **Why a frame is drawn on the CPU arm.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The arm waits for the window's first size and formats.
    Waiting,
    /// The arm is off: `BAUD_GPU=off`, it could not start, or the device
    /// failed and the arm is off or waits to try again ([`Off`]).
    Off,
    /// A slide is composed, which only the CPU arm does.
    Slide,
    /// The window has no buffer size.
    Sizeless,
    /// A cell pipeline is still building.
    Building,
    /// An image is on screen.
    Images,
    /// A trail no stage of the device's path draws.
    Trail,
    /// The target drawn has no buffer the compositor imported, or a
    /// target's buffer is still asked for.
    Unimported,
    /// The compositor did not take the buffer.
    Refused,
}

impl Why {
    /// Every reason, in the order the exit line writes them.
    pub const ALL: [Why; 9] = [
        Why::Waiting,
        Why::Off,
        Why::Slide,
        Why::Sizeless,
        Why::Building,
        Why::Images,
        Why::Trail,
        Why::Unimported,
        Why::Refused,
    ];

    /// The reason's word on the exit line.
    pub fn word(self) -> &'static str {
        match self {
            Why::Waiting => "waiting",
            Why::Off => "off",
            Why::Slide => "slide",
            Why::Sizeless => "sizeless",
            Why::Building => "building",
            Why::Images => "images",
            Why::Trail => "trail",
            Why::Unimported => "unimported",
            Why::Refused => "refused",
        }
    }

    /// The presenter's reason, as the arm counts it.
    fn of(cpu: Cpu) -> Why {
        match cpu {
            Cpu::Building => Why::Building,
            Cpu::Images => Why::Images,
            Cpu::Trail => Why::Trail,
        }
    }
}

/// A count of frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frames(pub u64);

/// **Every frame by what became of it**, for the line written at exit:
/// shown on the device, drawn on the CPU arm by why ([`Why`]), or owed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub shown: Frames,
    pub cpu: Cpus,
    pub owed: Frames,
}

/// The frames drawn on the CPU arm, one count a reason ([`Why`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cpus {
    pub waiting: Frames,
    pub off: Frames,
    pub slide: Frames,
    pub sizeless: Frames,
    pub building: Frames,
    pub images: Frames,
    pub trail: Frames,
    pub unimported: Frames,
    pub refused: Frames,
}

impl Cpus {
    /// The count for `why`.
    fn of(&mut self, why: Why) -> &mut Frames {
        match why {
            Why::Waiting => &mut self.waiting,
            Why::Off => &mut self.off,
            Why::Slide => &mut self.slide,
            Why::Sizeless => &mut self.sizeless,
            Why::Building => &mut self.building,
            Why::Images => &mut self.images,
            Why::Trail => &mut self.trail,
            Why::Unimported => &mut self.unimported,
            Why::Refused => &mut self.refused,
        }
    }

    /// Every CPU frame, whatever its reason.
    fn total(mut self) -> Frames {
        Frames(Why::ALL.iter().map(|w| self.of(*w).0).sum())
    }
}

impl Tally {
    /// One frame more, by what became of it.
    pub fn count(&mut self, arm: Arm) {
        let n = match arm {
            Arm::Shown => &mut self.shown,
            Arm::Cpu(why) => self.cpu.of(why),
            Arm::Owed => &mut self.owed,
        };
        n.0 += 1;
    }
}

/// The exit line, one line in a fixed format: `gpu exit: <shown> frames
/// shown on the device, <cpu> drawn on the CPU (<word> <count>, ...),
/// <owed> owed`, the CPU frames' reasons in [`Why::ALL`]'s order, each count
/// a plain decimal.
impl core::fmt::Display for Tally {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut cpu = self.cpu;
        write!(
            f,
            "gpu exit: {} frames shown on the device, {} drawn on the CPU (",
            self.shown.0,
            cpu.total().0
        )?;
        for (i, why) in Why::ALL.into_iter().enumerate() {
            let sep = if i == 0 { "" } else { ", " };
            write!(f, "{sep}{} {}", why.word(), cpu.of(why).0)?;
        }
        write!(f, "), {} owed", self.owed.0)
    }
}

/// The arm while it runs: the device, the presenter, and each target's
/// buffer and timeline as the compositor knows them, or its buffer still
/// asked for.
struct On {
    gpu: Gpu,
    presenter: Presenter,
    asked: [Option<Params>; TARGETS],
    buffers: [Option<ObjectId>; TARGETS],
    timelines: [Option<Timeline>; TARGETS],
}

/// When a GPU arm that failed tries again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Retry {
    /// At a size smaller than this one.
    AtSmaller(Size),
    /// With a new device, at the fallback's deadline.
    Restart,
}

enum State {
    /// Not started: no size or formats yet.
    Waiting,
    /// Failed, the CPU arm drawing until it tries again.
    Down(Retry),
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

/// An arm that failed, kept until a CPU frame is committed over it, and
/// what its log line says.
struct Leaving {
    on: Box<On>,
    said: String,
}

pub struct GpuArm {
    state: State,
    last: Last,
    tally: Tally,
    fallback: Fallback,
    leaving: Option<Leaving>,
}

/// One step of tearing an arm down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// A buffer asked for and not yet answered, forgotten.
    Ask(TargetIx),
    Buffer(TargetIx),
    Timeline(TargetIx),
    /// The presenter's targets, memory, sheets and pipelines.
    Presenter,
    Device,
}

/// **The order an arm is torn down in**: each buffer still asked for, each
/// `wl_buffer` and timeline the compositor knows, then the presenter's
/// memory, then the device -- the objects made from a thing before the
/// thing.
fn teardown<A, B, L>(
    asked: &[Option<A>; TARGETS],
    buffers: &[Option<B>; TARGETS],
    timelines: &[Option<L>; TARGETS],
) -> Vec<Step> {
    let mut steps: Vec<Step> = (0..TARGETS)
        .filter(|&k| asked[k].is_some())
        .map(|k| Step::Ask(TargetIx(k)))
        .collect();
    steps.extend(
        (0..TARGETS)
            .filter(|&k| buffers[k].is_some())
            .map(|k| Step::Buffer(TargetIx(k))),
    );
    steps.extend(
        (0..TARGETS)
            .filter(|&k| timelines[k].is_some())
            .map(|k| Step::Timeline(TargetIx(k))),
    );
    steps.extend([Step::Presenter, Step::Device]);
    steps
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
            tally: Tally::default(),
            fallback: Fallback::new(Restarts::process()),
            leaving: None,
        }
    }

    /// **Started on the compositor's device**, once the window has a size
    /// and formats, and again after a failure when the fallback says.
    fn start(&mut self, window: &Window, term: &Term<'_>) {
        if self.leaving.is_none() {
            let again = match self.state {
                State::Down(Retry::AtSmaller(failed)) => {
                    window.buffer_size().is_some_and(|s| smaller(s, failed))
                }
                State::Down(Retry::Restart) => self.fallback.due(Instant::now()) == Due::Restart,
                _ => false,
            };
            if again {
                crate::info!("gpu: trying the GPU arm again");
                self.state = State::Waiting;
            }
        }
        if !matches!(self.state, State::Waiting) {
            return;
        }
        let (Some(offered), Some(_)) = (window.buffer_formats(), window.buffer_size()) else {
            return;
        };
        let started = (|| -> Result<(On, Reported), Off> {
            let vulkan = Vulkan::open().map_err(|_| Off::NoVulkan)?;
            let candidates = vulkan.candidates();
            let only: Vec<_> = candidates.iter().map(|(_, c)| c.clone()).collect();
            let sync = match window.syncobj_offered() {
                true => SyncObj::Offered,
                false => SyncObj::Absent,
            };
            let choice = choose(&only, offered, sync).map_err(Off::Refused)?;
            let physical = candidates[choice.device.0].0;
            let reported = vulkan.reported(physical);
            let gpu = Gpu::new(vulkan, physical, choice).map_err(Off::Failed)?;
            let presenter = Presenter::new(&gpu, term).map_err(Off::Failed)?;
            Ok((
                On {
                    gpu,
                    presenter,
                    asked: [None; TARGETS],
                    buffers: [None; TARGETS],
                    timelines: [None; TARGETS],
                },
                reported,
            ))
        })();
        match started {
            Ok((on, reported)) => {
                crate::info!("gpu: frames drawn on the device the compositor names: {reported}");
                self.state = State::On(Box::new(on));
            }
            Err(why) => {
                let said = self.down(&why, window.buffer_size());
                crate::info!("{said}");
            }
        }
    }

    /// **The arm down after `why`**: the fallback told, the state it says,
    /// and the line to log.
    fn down(&mut self, why: &Off, size: Option<Size>) -> String {
        let Some(failure) = why.failure(size) else {
            self.state = State::Off;
            return format!("gpu: off, frames drawn on the CPU: {why}");
        };
        let (state, then) = match self.fallback.failed(failure, Instant::now()) {
            Next::Off => (State::Off, "off for the window's life"),
            Next::AtSmaller(s) => (
                State::Down(Retry::AtSmaller(s)),
                "tried again at a smaller size",
            ),
            Next::RestartAt(_) => (State::Down(Retry::Restart), "a new device in a second"),
        };
        self.state = state;
        format!("gpu: frames drawn on the CPU: {why}; {then}")
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
                        // The arm taken out before `down` sets what follows,
                        // and kept until a CPU frame is committed over it.
                        let on = match core::mem::replace(&mut self.state, State::Off) {
                            State::On(on) => Some(on),
                            _ => None,
                        };
                        let said = self.down(&why, window.buffer_size());
                        if let Some(on) = on {
                            self.leaving = Some(Leaving { on, said });
                        }
                        Arm::Cpu(Why::Off)
                    }
                }
            }
            _ => Arm::Cpu(self.cpu_why(slide)),
        };
        match arm {
            Arm::Shown => self.last = Last::Gpu,
            Arm::Cpu(_) => {
                if let State::On(on) = &mut self.state {
                    on.presenter.drawn_elsewhere();
                }
            }
            Arm::Owed => {}
        }
        self.tally.count(arm);
        arm
    }

    /// Why a frame the arm does not draw itself is the CPU arm's: it waits
    /// to start, it is off or down until it tries again, or a slide is
    /// composed.
    fn cpu_why(&self, slide: crate::effects::Composed) -> Why {
        match (&self.state, slide) {
            (State::Waiting, _) => Why::Waiting,
            (State::Off | State::Down(_), _) => Why::Off,
            (State::On(_), _) => Why::Slide,
        }
    }

    /// **The line written as the window ends**: every frame since it opened
    /// by what became of it ([`Tally`]), at the level the start line is.
    pub fn exit(&self) {
        crate::info!("{}", self.tally);
    }

    /// **A CPU frame committed**: an arm that failed is torn down now that
    /// the surface no longer shows its buffers, and logged with how long it
    /// took.
    pub fn committed(&mut self, window: &mut Window, s: &mut Sender<'_>) {
        let Some(Leaving { on, said }) = self.leaving.take() else {
            return;
        };
        let started = Instant::now();
        on.leave(window, s);
        crate::warn!("{said}; torn down in {:?}", started.elapsed());
    }

    /// **When the loop must wake for a new device**, while one waits.
    pub fn deadline(&self) -> Option<Instant> {
        self.fallback.deadline()
    }

    /// Each failure counted, and each restart.
    pub fn counts(&self) -> crate::gpu_fallback::Counts {
        self.fallback.counts()
    }

    /// **Whether the CPU arm's own buffer is stale**: the frame before was
    /// the GPU arm's, so the CPU arm repaints whole. Read once a CPU frame.
    pub fn cpu_owes_whole(&mut self) -> bool {
        let was = self.last == Last::Gpu;
        self.last = Last::Cpu;
        was
    }

    /// **The screen as it stands made ready to draw** off `term`'s window
    /// `back` lines up under `seal`, so the first frame's build is not
    /// cold: through the cell path while the arm is on
    /// (`Presenter::prepare`), which warms the CPU path's caches too, else
    /// through the CPU path's (`Renderer::prepare`).
    pub fn prepare(
        &mut self,
        renderer: &mut baud_render::Renderer<'_, '_>,
        term: &baud_core::Term<'_>,
        seal: baud_domain::Seal,
        back: baud_domain::Rows,
    ) {
        match &mut self.state {
            State::On(on) => on.presenter.prepare(renderer, term, seal, back),
            _ => renderer.prepare(term, seal, back),
        }
    }

    /// **What the last frame the GPU arm drew cost**, from its target in hand
    /// to its fence, and whether its build drew into the atlas; `None` while
    /// the arm is not on.
    pub fn cost(&self) -> Option<(std::time::Duration, baud_render::Cold)> {
        match &self.state {
            State::On(on) => Some((on.presenter.cost(), on.presenter.cold())),
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
            return Ok(Arm::Cpu(Why::Sizeless));
        };
        if self.presenter.fit(&self.gpu, size).map_err(Off::Fitting)? == Remade::Yes {
            self.import(window, s)?;
        }
        // The compositor's answers to the buffers asked for: a refusal turns
        // the arm off, and until both are made the frames are the CPU's.
        for k in 0..TARGETS {
            if let Some(p) = self.asked[k] {
                match window.answer(p) {
                    Answer::Buffer(b) => {
                        self.buffers[k] = Some(b);
                        self.asked[k] = None;
                    }
                    Answer::Refused => return Err(Off::NotImported),
                    Answer::Waiting => {}
                }
            }
        }
        if self.buffers.iter().any(Option::is_none) {
            return Ok(Arm::Cpu(Why::Unimported));
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
            Frame::Cpu(cpu) => return Ok(Arm::Cpu(Why::of(cpu))),
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
            return Ok(Arm::Cpu(Why::Unimported));
        };
        let shown = window.present_dmabuf(s, buffer, size, points, self.presenter.damage());
        self.presenter.shown(k, shown);
        Ok(match shown {
            DmabufShown::Yes => Arm::Shown,
            _ => Arm::Cpu(Why::Refused),
        })
    }

    /// **A buffer asked for each new target**, and its timeline imported
    /// where frames carry points; the old targets' buffers, asks and
    /// timelines given up first.
    fn import(&mut self, window: &mut Window, s: &mut Sender<'_>) -> Result<(), Off> {
        for p in self.asked.iter_mut().filter_map(Option::take) {
            window.forget(s, p);
        }
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
            self.asked[k] = match window.import(s, &d) {
                Imported::Asked(p) => Some(p),
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

    /// **The arm torn down** in [`teardown`]'s order.
    fn leave(self, window: &mut Window, s: &mut Sender<'_>) {
        let On {
            gpu,
            presenter,
            asked,
            buffers,
            timelines,
        } = self;
        let (mut presenter, mut gpu) = (Some(presenter), Some(gpu));
        for step in teardown(&asked, &buffers, &timelines) {
            match step {
                Step::Ask(k) => {
                    if let Some(p) = asked[k.0] {
                        window.forget(s, p);
                    }
                }
                Step::Buffer(k) => {
                    if let Some(b) = buffers[k.0] {
                        window.drop_buffer(s, b);
                    }
                }
                Step::Timeline(k) => {
                    if let Some(tl) = timelines[k.0] {
                        baud_present_wayland::dmabuf::destroy_timeline(s, tl);
                    }
                }
                Step::Presenter => {
                    if let (Some(p), Some(g)) = (presenter.take(), gpu.as_ref()) {
                        p.destroy(g);
                    }
                }
                Step::Device => drop(gpu.take()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The exit counts tally each frame by its arm**, every CPU frame
    /// by why, and the line writes them in its fixed format.
    #[test]
    fn the_exit_line_tallies_each_frame_by_its_arm() {
        let mut t = Tally::default();
        assert_eq!(
            t.to_string(),
            "gpu exit: 0 frames shown on the device, 0 drawn on the CPU (waiting 0, off 0, \
             slide 0, sizeless 0, building 0, images 0, trail 0, unimported 0, refused 0), \
             0 owed"
        );
        for arm in [
            Arm::Cpu(Why::Waiting),
            Arm::Cpu(Why::Waiting),
            Arm::Shown,
            Arm::Cpu(Why::Building),
            Arm::Shown,
            Arm::Owed,
            Arm::Cpu(Why::Building),
            Arm::Cpu(Why::Building),
            Arm::Shown,
            Arm::Cpu(Why::Images),
            Arm::Cpu(Why::Trail),
            Arm::Cpu(Why::Slide),
            Arm::Cpu(Why::Sizeless),
            Arm::Cpu(Why::Unimported),
            Arm::Cpu(Why::Refused),
            Arm::Cpu(Why::Off),
            Arm::Shown,
        ] {
            t.count(arm);
        }
        assert_eq!(
            t.to_string(),
            "gpu exit: 4 frames shown on the device, 12 drawn on the CPU (waiting 2, off 1, \
             slide 1, sizeless 1, building 3, images 1, trail 1, unimported 1, refused 1), \
             1 owed"
        );
    }

    /// **Every reason a CPU frame has is counted under its own word**: one
    /// frame of each reason moves its word's count alone.
    #[test]
    fn each_reason_is_counted_under_its_own_word() {
        for why in Why::ALL {
            let mut t = Tally::default();
            t.count(Arm::Cpu(why));
            let line = t.to_string();
            let ones: Vec<&str> = Why::ALL
                .iter()
                .filter(|w| line.contains(&format!("{} 1", w.word())))
                .map(|w| w.word())
                .collect();
            assert_eq!(ones, [why.word()], "{line}");
        }
    }

    /// **An arm off from the start counts every frame on the CPU as off**,
    /// none on the device, and its exit line does not read as a `gpu:`
    /// line.
    #[test]
    fn an_arm_off_counts_every_frame_on_the_cpu() {
        let mut arm = GpuArm::new(Some("off"));
        for _ in 0..3 {
            let why = arm.cpu_why(crate::effects::Composed::No);
            arm.tally.count(Arm::Cpu(why));
        }
        let line = arm.tally.to_string();
        assert_eq!(
            line,
            "gpu exit: 0 frames shown on the device, 3 drawn on the CPU (waiting 0, off 3, \
             slide 0, sizeless 0, building 0, images 0, trail 0, unimported 0, refused 0), \
             0 owed"
        );
        assert!(!line.starts_with("gpu:"));
        let waiting = GpuArm::new(None);
        assert_eq!(waiting.cpu_why(crate::effects::Composed::No), Why::Waiting);
    }

    /// **`BAUD_GPU=off` keeps the arm off from the start**, and nothing
    /// else does: unset or any other value, it waits to start.
    #[test]
    fn baud_gpu_off_is_off_from_the_start() {
        assert!(matches!(GpuArm::new(Some("off")).state, State::Off));
        assert!(matches!(GpuArm::new(None).state, State::Waiting));
        assert!(matches!(GpuArm::new(Some("on")).state, State::Waiting));
    }

    /// **An arm is torn down from the outside in**: every buffer still asked
    /// for, every buffer and timeline the compositor knows, then the presenter's memory, then
    /// the device, and nothing named twice.
    #[test]
    fn an_arm_is_torn_down_buffers_and_timelines_before_memory_and_device() {
        let both = [Some(()), Some(())];
        assert_eq!(
            teardown(&both, &both, &both),
            [
                Step::Ask(TargetIx(0)),
                Step::Ask(TargetIx(1)),
                Step::Buffer(TargetIx(0)),
                Step::Buffer(TargetIx(1)),
                Step::Timeline(TargetIx(0)),
                Step::Timeline(TargetIx(1)),
                Step::Presenter,
                Step::Device,
            ]
        );
        let none: [Option<()>; TARGETS] = [None, None];
        assert_eq!(
            teardown(&none, &[Some(()), None], &none),
            [Step::Buffer(TargetIx(0)), Step::Presenter, Step::Device],
            "only what the compositor knows"
        );
    }

    /// **An arm that failed is torn down after the CPU frame over it is
    /// committed**: in the driver, the one call to `committed` follows the
    /// CPU arm's draw into the window's buffer and sits in the arm it takes
    /// when the frame was presented, so the surface never shows a buffer
    /// already destroyed.
    #[test]
    fn the_driver_tears_an_arm_down_after_the_cpu_frame_is_committed() {
        let src = include_str!("linux.rs");
        assert_eq!(
            src.matches(".committed(&mut self.window").count(),
            1,
            "one call"
        );
        let present = src
            .find("self.draw_in_place((seal, back))")
            .expect("the CPU arm's draw");
        let committed = src.find(".committed(&mut self.window").expect("the call");
        assert!(committed > present, "after the present");
        let between = &src[present..committed];
        assert!(
            between.contains("== Presented::Yes") && !between.contains("\n        }\n"),
            "inside the arm the committed frame takes"
        );
    }

    /// **A lost device turns into a restart, memory running out for the
    /// targets into a wait for a smaller size, and `BAUD_GPU=off` into no
    /// failure at all**: what each reason the arm leaves for is weighed as.
    #[test]
    fn each_reason_the_arm_leaves_for_is_weighed_by_its_kind() {
        let px = (baud_present_wayland::Px(800), baud_present_wayland::Px(600));
        let size = Some(px);
        let lost = Failed::LOST;
        let oom = Failed::OUT_OF_DEVICE_MEMORY;
        let host = Failed::OUT_OF_HOST_MEMORY;
        let init = Failed::NoMemoryType;
        assert_eq!(Off::Env.failure(size), None);
        assert_eq!(Off::NoVulkan.failure(size), Some(Failure::NoLoader));
        assert_eq!(Off::NotImported.failure(size), Some(Failure::Refused(size)));
        assert_eq!(Off::Failed(lost).failure(size), Some(Failure::Lost));
        assert_eq!(Off::Fitting(lost).failure(size), Some(Failure::Lost));
        assert_eq!(
            Off::Fitting(oom).failure(size),
            Some(Failure::OutOfMemoryFitting(px))
        );
        assert_eq!(
            Off::Fitting(host).failure(size),
            Some(Failure::OutOfMemoryFitting(px))
        );
        assert_eq!(Off::Failed(oom).failure(size), Some(Failure::OutOfMemory));
        assert_eq!(Off::Failed(init).failure(size), Some(Failure::Other(init)));
        assert_eq!(
            Off::Failed(Failed::Spirv).failure(size),
            Some(Failure::Other(Failed::Spirv))
        );
    }
}
