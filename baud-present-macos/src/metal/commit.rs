//! **Every command buffer is made, committed and heard from here**, and
//! nowhere else in the crate (`tests/commit_gates.rs` reads the source for
//! it). A buffer is an [`Encoded`] from the moment it is made, which
//! encodes and has no other way to commit than [`Encoded::commit`]; that
//! registers the one completion handler before `commit()`, as the commit
//! page requires, and consumes the buffer, so it is committed once and no
//! handler is added after.
//!
//! **The handler reads the status on every completion**: frames waited
//! for, frames notified, and the warm-up pass nothing waits for. A buffer
//! the GPU aborted is `MTLCommandBufferStatusError`, "aborted due to an
//! error during execution" (`MTLCommandBuffer.h`), and its target holds
//! whatever it held, so a frame is a [`Completion::Done`] only when the
//! status read is `Completed`; anything else is a [`Completion::Failed`]
//! naming the error's code and text, counted per code ([`Faults`]). The
//! handler runs on both outcomes: the `addCompletedHandler:` page calls the
//! handler "a good place to check the status property", and "if the status
//! is equal to error, you can investigate further by checking the error".
//!
//! **The frame in flight is counted out in the handler, once, on every
//! arm**, after the counts and before the answer is sent: a waiter that
//! has its answer, and a draw on the main thread that sees no frame in
//! flight, both find the counts already taken.
//!
//! **Every wait has a deadline** ([`Deadline`]). A command buffer has no
//! timed wait and no cancel (`MTLCommandBuffer.h`: `waitUntilCompleted`
//! takes no timeout), so a frame waited for blocks on its handler's answer
//! for at most its bound and is then [`Waited::Overdue`], still committed;
//! a frame notified carries a [`Flying`] the window schedules its own wakes
//! with, one at [`Deadline::FRAME`] and one at [`Deadline::GIVE_UP`] after
//! its commit, each counting only if that frame is still unanswered. A
//! frame counted overdue and answered after is counted late, once, by its
//! handler: the frame's own state word ([`Mark`]) is moved from in flight
//! to overdue by the bound, or to answered by the handler, whichever comes
//! first, and the other reads what the first left.
//!
//! **The fault seam** (`gpu-fault`, dev-dependencies only): a test may
//! replace what the handler reports with a failure ([`Inject::Fail`]), or
//! hold the whole completion -- the count-out, the answer and the notified
//! closure -- until it releases it ([`Inject::Hold`]), which is what a
//! frame the GPU never finishes looks like to every consumer. The GPU
//! still runs every frame; only the report is replaced.

use crate::unwind::{SiteName, guard};
use baud_domain::HostTime;
use baud_render::Tally;
use block2::RcBlock;
use core::fmt;
use core::ptr::NonNull;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSError;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandBufferError, MTLCommandBufferErrorDomain,
    MTLCommandBufferStatus, MTLCommandQueue, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

type CommandBuffer = Retained<ProtocolObject<dyn MTLCommandBuffer>>;

/// A completed frame's landing, from the stamps Metal puts on the command
/// buffer: its own start-to-end, and its end as a host time.
///
/// **Made only from a status read**: the handler builds one when the
/// buffer's status is `Completed`, and nowhere else, so a `Landing` is
/// proof the frame drew. Test builds alone may stamp one by hand
/// (`Landing::stamped`), for the journal's own tests.
///
/// `Copy` plain data: it crosses from the GPU's thread to whatever the
/// journal hands it on to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Landing {
    gpu: Duration,
    ended: HostTime,
}

impl Landing {
    /// The frame's own start-to-end on the GPU, `GPUEndTime - GPUStartTime`
    /// -- what the frame report prints as its GPU time. Zero for a stamp
    /// pair that is not ordered, never a negative.
    pub fn gpu(&self) -> Duration {
        self.gpu
    }

    /// When the GPU finished it, on the host clock. `GPUEndTime` is "the
    /// host time in seconds that GPU finishes executing this command
    /// buffer" (`MTLCommandBuffer.h`), so as a [`HostTime`] it is on the
    /// same clock as the key event that asked for the frame and the
    /// display-link target it is shown at. [`HostTime::ZERO`] when the
    /// stamp is not a time: the header says the property "returns zero if
    /// CPU has not received completion notification", which a completion
    /// handler never observes, so a zero here is the conversion refusing
    /// rather than Metal.
    pub fn ended(&self) -> HostTime {
        self.ended
    }

    /// A landing stamped by hand, for a test of what consumes one. Test
    /// builds only: every other `Landing` comes from a status read.
    #[cfg(any(test, feature = "gpu-fault"))]
    pub fn stamped(gpu: Duration, ended: HostTime) -> Landing {
        Landing { gpu, ended }
    }

    /// The stamps of a buffer whose status read `Completed`. Both are
    /// `CFTimeInterval` seconds of the host time (`MTLCommandBuffer.h`).
    fn of(cmd: &ProtocolObject<dyn MTLCommandBuffer>) -> Landing {
        let (start, end) = (cmd.GPUStartTime(), cmd.GPUEndTime());
        Landing {
            gpu: Duration::try_from_secs_f64((end - start).max(0.0)).unwrap_or(Duration::ZERO),
            ended: HostTime::of_secs(end).unwrap_or(HostTime::ZERO),
        }
    }
}

/// **How long a frame may be in flight before something is done about
/// it.** No `Default` and no maximum: every wait names its bound, and the
/// bounds are these.
///
/// **Where they come from.** The slowest commit-to-handler time recorded
/// ([`Faults::slowest`]) over three runs of the whole event rig, each run
/// alone on the GPU beside a CPU stressor, was 7.5, 12.4 and 28.8 ms: 28.8
/// ms is the slowest any frame took. A bound that fires on a loaded but
/// healthy machine is non-determinism, which is a defect here, so each is
/// a stated multiple of that slowest. **Any slowest recorded over 30 ms is
/// a finding to measure again, never a reason to raise a bound quietly**:
/// the multiples are of 28.8 ms, and a slower frame changes what they are
/// multiples of. The rig prints each row's slowest, and the cells suite
/// each test's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline(Duration);

impl Deadline {
    /// **300 ms, about 10x the slowest**: a window's frame still in flight
    /// this long after its commit is overdue -- counted once and said once.
    /// That is all it does: the window holds every draw `Busy` as before,
    /// since the frame's set and targets may still be read.
    pub const FRAME: Deadline = Deadline(Duration::from_millis(300));
    /// **3 s, about 100x the slowest**: a window's frame still in flight
    /// this long after its commit makes the window give up the GPU for the
    /// rest of the session ([`GpuUse::GaveUp`]), never to return to it.
    /// Counted once and said once; the window still holds every draw
    /// `Busy` until the frame lands, since drawing on the CPU path instead
    /// is not built.
    pub const GIVE_UP: Deadline = Deadline(Duration::from_secs(3));
    /// **3 s, about 100x the slowest**: a test's or a bench's wait on one
    /// frame ([`crate::Done::Within`]) or on the GPU going idle
    /// ([`crate::Gpu::idle_within`]). Past it the wait returns what it
    /// found, and the caller fails naming it.
    pub const WAIT: Deadline = Deadline(Duration::from_secs(3));
    /// **300 ms, about 10x the slowest**: the first frame at launch, waited
    /// for before the window opens. Past it the window opens with that
    /// frame drawn on the CPU, never with a surface nothing drew. A launch
    /// past it alone never gives up the GPU: only [`Deadline::GIVE_UP`]
    /// does, counted from the same commit.
    pub const LAUNCH: Deadline = Deadline(Duration::from_millis(300));

    /// The bound as a length of time.
    pub fn get(self) -> Duration {
        self.0
    }

    /// A bound of the test's own, for a wait the fault seam holds past it.
    /// Test builds only: every other wait names one of the constants.
    #[cfg(feature = "gpu-fault")]
    pub const fn injected(d: Duration) -> Deadline {
        Deadline(d)
    }
}

/// **A wait that reached its bound** with the frame still unanswered: how
/// long it waited, and the buffer's status then. The buffer stays
/// committed, since Metal has no cancel, and its answer, when it comes, is
/// counted late.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overdue {
    pub after: Duration,
    pub status: StatusWord,
}

/// **The GPU still busy at a wait's bound** ([`crate::Gpu::idle_within`]):
/// how long it waited, and every count the handlers had taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StillBusy {
    pub after: Duration,
    pub faults: Box<Faults>,
}

impl fmt::Display for StillBusy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a frame still in flight after {:?}; {}",
            self.after, self.faults
        )
    }
}

/// **Whether a window still draws on the GPU**: live, or given up for the
/// rest of the session, and why. Never back from `GaveUp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuUse {
    Live,
    GaveUp(GaveUp),
}

/// Why a window gave up the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaveUp {
    /// A frame still in flight at [`Deadline::GIVE_UP`].
    Stuck,
}

/// **Which commit a frame is**, numbered from one in the order a `Gpu`
/// committed them: what its deadline wakes carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitId(pub u64);

/// What a frame's state word ([`Mark`]) holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Committed, its handler has not answered, and no bound has come.
    InFlight,
    /// A bound came first: counted overdue.
    Overdue,
    /// The handler answered.
    Answered,
}

/// A [`State`] as the atomic word holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Word(u8);

impl From<State> for Word {
    fn from(s: State) -> Word {
        Word(match s {
            State::InFlight => 0,
            State::Overdue => 1,
            State::Answered => 2,
        })
    }
}

impl From<Word> for State {
    fn from(w: Word) -> State {
        match w.0 {
            0 => State::InFlight,
            1 => State::Overdue,
            _ => State::Answered,
        }
    }
}

/// **One committed frame's state**, shared by its handler, its waiter and
/// its deadline wakes: which commit, when, and where it is. A bound moves
/// it from in flight to overdue, the handler from either to answered; each
/// move is one atomic step, so a frame is counted overdue at most once,
/// and late only if it was.
struct Mark {
    id: CommitId,
    at: Instant,
    state: AtomicU8,
}

impl Mark {
    /// The bound has come: in flight becomes overdue. Whether this call
    /// was the one that moved it, which is the one that counts it.
    fn overdue(&self) -> bool {
        self.state
            .compare_exchange(
                Word::from(State::InFlight).0,
                Word::from(State::Overdue).0,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// The handler has answered: what the state was before.
    fn answer(&self) -> State {
        State::from(Word(
            self.state
                .swap(Word::from(State::Answered).0, Ordering::AcqRel),
        ))
    }

    fn now(&self) -> State {
        State::from(Word(self.state.load(Ordering::Acquire)))
    }
}

/// **A frame committed, as its deadline wakes carry it**: which commit it
/// is, and the counts its bounds feed. Every frame a draw commits hands one
/// back ([`crate::GpuFrame::flying`]); the window schedules a main-queue
/// wake at each of its bounds with it, which counts only if this frame --
/// not whichever is in flight then -- is still unanswered.
#[derive(Clone)]
pub struct Flying {
    mark: Arc<Mark>,
    counters: Arc<Counters>,
}

/// What a deadline wake found of its frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Came {
    /// The frame was answered before the bound: nothing counted.
    Answered,
    /// Woken before the bound: nothing counted.
    Early,
    /// Still unanswered at [`Deadline::FRAME`]: counted overdue, by this
    /// wake or by a waiter before it.
    Overdue,
    /// Still unanswered at [`Deadline::GIVE_UP`]: the GPU given up.
    GaveUp,
}

impl fmt::Debug for Flying {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Flying({:?}, {:?})", self.mark.id, self.mark.now())
    }
}

impl Flying {
    /// Which commit this is.
    pub fn id(&self) -> CommitId {
        self.mark.id
    }

    /// When `bound` comes for this frame: its commit, plus the bound.
    pub fn due(&self, bound: Deadline) -> Instant {
        self.mark.at + bound.0
    }

    /// **Its [`Deadline::FRAME`] wake, at `now`**: counted overdue, once,
    /// if this frame is still in flight and the bound has come.
    pub fn frame_due(&self, now: Instant) -> Came {
        if now < self.due(Deadline::FRAME) {
            return Came::Early;
        }
        if self.mark.overdue() {
            self.counters.overdue.fetch_add(1, Ordering::Relaxed);
        }
        match self.mark.now() {
            State::Answered => Came::Answered,
            State::InFlight | State::Overdue => Came::Overdue,
        }
    }

    /// **Its [`Deadline::GIVE_UP`] wake, at `now`**: if this frame is still
    /// unanswered, counted overdue if nothing counted it yet, and the GPU
    /// given up -- counted the once a `Gpu`'s life it happens. A frame
    /// whose handler answers between the state read here and the give-up's
    /// step still gives the GPU up: that frame came 3 s late, and the
    /// give-up holds for the session, as every give-up does.
    pub fn give_up_due(&self, now: Instant) -> Came {
        if now < self.due(Deadline::GIVE_UP) {
            return Came::Early;
        }
        if self.mark.overdue() {
            self.counters.overdue.fetch_add(1, Ordering::Relaxed);
        }
        if self.mark.now() == State::Answered {
            return Came::Answered;
        }
        let _ = self
            .counters
            .gave_up
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        Came::GaveUp
    }
}

/// **Proof that a frame waited for drew**: made only from a waited
/// frame's [`Completion::Done`], and what [`crate::Gpu::read`] takes, so a
/// read-back of a frame that failed, or was never waited for, does not
/// compile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Landed(Landing);

impl Landed {
    /// The landing it was made from.
    pub fn landing(&self) -> Landing {
        self.0
    }
}

/// **What a draw's wait found**: the frame drawn, with the proof a
/// read-back takes; failed, with why; still unanswered at the wait's
/// bound; or not waited for, when the draw asked to be told instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Waited {
    #[default]
    No,
    Landed(Landed),
    Failed(Fault),
    Overdue(Overdue),
}

impl Waited {
    /// What a waited frame's completion proves.
    pub(crate) fn of(c: Completion) -> Waited {
        match c {
            Completion::Done(l) => Waited::Landed(Landed(l)),
            Completion::Failed(f) => Waited::Failed(f),
        }
    }

    /// The proof that the frame drew, or what was found instead.
    pub fn landed(&self) -> Result<&Landed, &Waited> {
        match self {
            Waited::Landed(l) => Ok(l),
            other => Err(other),
        }
    }
}

impl fmt::Display for Waited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Waited::No => f.write_str("not waited for"),
            Waited::Landed(_) => f.write_str("landed"),
            Waited::Failed(fault) => write!(f, "failed, {fault}"),
            Waited::Overdue(o) => {
                write!(f, "unanswered after {:?}, status {}", o.after, o.status.0)
            }
        }
    }
}

/// What the GPU reported for one command buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completion {
    /// The status read `Completed`: "finished executing successfully".
    Done(Landing),
    /// Anything else: aborted with an error, or an answer the handler
    /// should never see. The target holds undefined content.
    Failed(Fault),
}

/// Why a buffer did not complete, and what Metal said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub cause: Cause,
    /// The error's own description, with the encoders' execution states
    /// where the build asks for them; or why there is no error to read.
    pub text: Box<str>,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.cause, self.text)
    }
}

/// What a failed completion was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// Status `Error`, with the error's code.
    Error(FaultCode),
    /// A completion whose status was neither `Completed` nor `Error`, which
    /// the handler should never see: counted, and treated as a failure.
    Odd(StatusWord),
    /// The handler was let go without answering a frame waited for: the
    /// fault seam dropped a held completion. Not a status read, so it is
    /// not counted; the waiter is told.
    Unanswered,
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::Error(c) => write!(f, "{c:?} (MTLCommandBufferError {})", c.code().0),
            Cause::Odd(s) => write!(f, "status {} at completion", s.0),
            Cause::Unanswered => f.write_str("no answer"),
        }
    }
}

/// An `MTLCommandBufferStatus` as read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusWord(pub u64);

/// An `MTLCommandBufferError` number as an error carries it, or another
/// domain's code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorNumber(pub u64);

/// `MTLCommandBufferError`'s codes (`MTLCommandBuffer.h`), and any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultCode {
    Internal,
    /// "took too long ... interrupted and aborted".
    Timeout,
    /// "an unserviceable GPU page fault".
    PageFault,
    /// "responsible for too many timeouts or hangs": permanent.
    AccessRevoked,
    NotPermitted,
    OutOfMemory,
    /// "most commonly caused when the caller deletes a resource before
    /// executing a command buffer that refers to it".
    InvalidResource,
    Memoryless,
    DeviceRemoved,
    StackOverflow,
    /// A code the header does not name, or an error from another domain.
    Unknown(ErrorNumber),
}

impl FaultCode {
    /// The codes the header names, in its order.
    pub const NAMED: [FaultCode; 10] = [
        FaultCode::Internal,
        FaultCode::Timeout,
        FaultCode::PageFault,
        FaultCode::AccessRevoked,
        FaultCode::NotPermitted,
        FaultCode::OutOfMemory,
        FaultCode::InvalidResource,
        FaultCode::Memoryless,
        FaultCode::DeviceRemoved,
        FaultCode::StackOverflow,
    ];

    /// How many counters there are: one a named code, and one for every
    /// other.
    pub const COUNT: usize = FaultCode::NAMED.len() + 1;

    /// The header's number for it.
    pub fn code(self) -> ErrorNumber {
        ErrorNumber(match self {
            FaultCode::Internal => 1,
            FaultCode::Timeout => 2,
            FaultCode::PageFault => 3,
            FaultCode::AccessRevoked => 4,
            FaultCode::NotPermitted => 7,
            FaultCode::OutOfMemory => 8,
            FaultCode::InvalidResource => 9,
            FaultCode::Memoryless => 10,
            FaultCode::DeviceRemoved => 11,
            FaultCode::StackOverflow => 12,
            FaultCode::Unknown(n) => n.0,
        })
    }

    /// The code an `MTLCommandBufferError` number names.
    fn of(n: ErrorNumber) -> FaultCode {
        FaultCode::NAMED
            .into_iter()
            .find(|c| c.code() == n)
            .unwrap_or(FaultCode::Unknown(n))
    }

    /// Its counter of `all`, one a code in [`FaultCode::NAMED`]'s order and
    /// the last for every other.
    pub fn pick<T>(self, all: &[T; FaultCode::COUNT]) -> &T {
        let at = FaultCode::NAMED
            .iter()
            .position(|c| *c == self)
            .unwrap_or(FaultCode::NAMED.len());
        &all[at]
    }
}

/// What the handlers have counted, taken together for a report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// Frames whose status read `Error`, one counter a code
    /// ([`FaultCode::pick`]).
    pub failed: [Tally; FaultCode::COUNT],
    /// Frames that completed with neither `Completed` nor `Error`.
    pub odd_status: Tally,
    /// The warm-up pass, failed either way. Nothing waits for it, so this
    /// is the only place its failure shows.
    pub warm_failed: Tally,
    /// The longest a frame took from just before its `commit()` to its
    /// handler: the time a bound on the wait has to allow.
    pub slowest: Duration,
    /// Frames still unanswered at their bound: a waiter's, or the
    /// window's [`Deadline::FRAME`] wake's.
    pub overdue: Tally,
    /// Frames counted overdue and answered after: each counted once, by
    /// its handler.
    pub late: Tally,
    /// The GPU given up ([`Deadline::GIVE_UP`]): one at most.
    pub gave_up: Tally,
}

impl Faults {
    /// Whether the window still draws on the GPU.
    pub fn gpu_use(&self) -> GpuUse {
        match self.gave_up == Tally::ZERO {
            true => GpuUse::Live,
            false => GpuUse::GaveUp(GaveUp::Stuck),
        }
    }

    /// The frames that failed with `code`.
    pub fn of(&self, code: FaultCode) -> Tally {
        *code.pick(&self.failed)
    }

    /// Every frame that did not complete: each code's, and the odd ones.
    pub fn frames_failed(&self) -> Tally {
        let mut all = self.odd_status;
        for n in self.failed {
            all.add(n);
        }
        all
    }
}

impl fmt::Display for Faults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "gpu: {} failed, {} odd, {} warm failed, {} overdue, {} late, {} gave up, slowest {:?}",
            self.frames_failed().value(),
            self.odd_status.value(),
            self.warm_failed.value(),
            self.overdue.value(),
            self.late.value(),
            self.gave_up.value(),
            self.slowest
        )?;
        for code in FaultCode::NAMED {
            let n = self.of(code);
            if n != Tally::ZERO {
                write!(f, ", {code:?} {}", n.value())?;
            }
        }
        let other = self.of(FaultCode::Unknown(ErrorNumber(0)));
        if other != Tally::ZERO {
            write!(f, ", other codes {}", other.value())?;
        }
        Ok(())
    }
}

/// The handlers' counters. Each is a `Relaxed` add, as the journal's are:
/// they publish nothing, and the count-out that follows them is the
/// release a reader acquires.
#[derive(Default)]
struct Counters {
    failed: [AtomicU64; FaultCode::COUNT],
    odd_status: AtomicU64,
    warm_failed: AtomicU64,
    slowest: AtomicU64,
    overdue: AtomicU64,
    late: AtomicU64,
    gave_up: AtomicU64,
}

impl Counters {
    fn count(&self, kind: Kind, completion: &Completion, took: Duration) {
        match (kind, completion) {
            (Kind::Warm, Completion::Failed(_)) => {
                self.warm_failed.fetch_add(1, Ordering::Relaxed);
            }
            (Kind::Warm, Completion::Done(_)) => {}
            (Kind::Frame, done) => {
                self.slowest.fetch_max(
                    u64::try_from(took.as_nanos()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
                match done {
                    Completion::Done(_) => {}
                    Completion::Failed(f) => match f.cause {
                        Cause::Error(code) => {
                            code.pick(&self.failed).fetch_add(1, Ordering::Relaxed);
                        }
                        Cause::Odd(_) => {
                            self.odd_status.fetch_add(1, Ordering::Relaxed);
                        }
                        Cause::Unanswered => {}
                    },
                }
            }
        }
    }

    fn read(&self) -> Faults {
        let tally = |a: &AtomicU64| {
            Tally::of(usize::try_from(a.load(Ordering::Relaxed)).unwrap_or(usize::MAX))
        };
        Faults {
            failed: core::array::from_fn(|i| tally(&self.failed[i])),
            odd_status: tally(&self.odd_status),
            warm_failed: tally(&self.warm_failed),
            slowest: Duration::from_nanos(self.slowest.load(Ordering::Relaxed)),
            overdue: tally(&self.overdue),
            late: tally(&self.late),
            gave_up: tally(&self.gave_up),
        }
    }
}

/// Which buffer a completion is: a frame, counted in flight, or the
/// warm-up pass, which reads no memory a frame writes and is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Frame,
    Warm,
}

/// A closure a late answer runs, on the GPU's completion thread: what a
/// frame waited for and given up on at its bound wakes the window with when
/// it does land.
pub type Late = Arc<dyn Fn() + Send + Sync>;

/// What [`Encoded::commit`] does when the buffer completes.
pub(crate) enum Then {
    /// Answers [`Committed::wait`], which waits for at most the bound; an
    /// answer after it runs the closure, if there is one.
    Within(Deadline, Option<Late>),
    /// Runs this, on the GPU's completion thread, under
    /// [`crate::unwind::guard`] so a panic in it cannot unwind into Metal.
    Notify(Arc<dyn Fn(Completion) + Send + Sync>),
    /// Nothing waits and nothing is told: the warm-up pass. Still counted.
    Discard,
}

/// Where a completion's answer goes: [`Then`] with the channel made.
#[derive(Clone)]
enum Answer {
    Wait(SyncSender<Completion>, Option<Late>),
    Notify(Arc<dyn Fn(Completion) + Send + Sync>),
    Discard,
}

/// The frames in flight and what their handlers counted, shared with
/// every handler: a [`crate::Gpu`]'s.
pub(crate) struct Flight {
    in_flight: Arc<AtomicUsize>,
    counters: Arc<Counters>,
    /// The commits made, for the next one's [`CommitId`].
    commits: AtomicU64,
    #[cfg(feature = "gpu-fault")]
    seam: Arc<Seam>,
}

impl Flight {
    pub(crate) fn new() -> Flight {
        #[cfg(feature = "gpu-fault")]
        let seam = Arc::new(Seam::default());
        #[cfg(all(feature = "gpu-fault", feature = "effect-probe"))]
        rig::made(&seam);
        Flight {
            in_flight: Arc::new(AtomicUsize::new(0)),
            counters: Arc::new(Counters::default()),
            commits: AtomicU64::new(0),
            #[cfg(feature = "gpu-fault")]
            seam,
        }
    }

    /// Whether a frame is committed and its handler has not counted it
    /// out.
    pub(crate) fn busy(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) != 0
    }

    /// What the handlers have counted.
    pub(crate) fn faults(&self) -> Faults {
        self.counters.read()
    }

    /// **Spins until no frame is in flight, for at most `bound`**, for a
    /// bench or a test that must not write what a frame reads. The one
    /// loop on a frame in flight; past the bound it says so with every
    /// count, for the caller to fail on.
    pub(crate) fn idle_within(&self, bound: Deadline) -> Result<(), StillBusy> {
        let start = Instant::now();
        loop {
            if !self.busy() {
                return Ok(());
            }
            let after = start.elapsed();
            if after >= bound.0 {
                return Err(StillBusy {
                    after,
                    faults: Box::new(self.faults()),
                });
            }
            std::hint::spin_loop();
        }
    }

    /// A command buffer from `queue`, to encode. Test and probe builds ask
    /// Metal for each encoder's execution state on an error, which the
    /// header warns "may increase CPU, GPU, and/or memory overhead", so a
    /// shipped build does not until that is measured; the state goes into
    /// the [`Fault`]'s text.
    pub(crate) fn encoded(&self, queue: &ProtocolObject<dyn MTLCommandQueue>) -> Option<Encoded> {
        #[cfg(any(feature = "gpu-fault", feature = "effect-probe"))]
        let cmd = {
            let d = objc2_metal::MTLCommandBufferDescriptor::new();
            // The plain `commandBuffer` form's: the buffer keeps what it
            // reads alive until it completes. The header names no default.
            d.setRetainedReferences(true);
            d.setErrorOptions(objc2_metal::MTLCommandBufferErrorOption::EncoderExecutionStatus);
            queue.commandBufferWithDescriptor(&d)?
        };
        #[cfg(not(any(feature = "gpu-fault", feature = "effect-probe")))]
        let cmd = queue.commandBuffer()?;
        Some(Encoded { cmd })
    }
}

/// A command buffer being encoded: what encoding needs, and one way to
/// commit.
pub(crate) struct Encoded {
    cmd: CommandBuffer,
}

impl Encoded {
    /// A render pass over `pass`.
    pub(crate) fn render(
        &self,
        pass: &MTLRenderPassDescriptor,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>> {
        self.cmd.renderCommandEncoderWithDescriptor(pass)
    }

    /// A blit pass.
    pub(crate) fn blit(&self) -> Option<Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>> {
        self.cmd.blitCommandEncoder()
    }

    /// **Commits the buffer with the status-reading handler**, counting a
    /// frame in flight until the handler counts it out. The only
    /// `commit()` in the crate. A frame's [`Mark`] is made here, before the
    /// commit, and shared with the handler, the waiter and the window's
    /// deadline wakes ([`Committed::flying`]).
    pub(crate) fn commit(self, flight: &Flight, then: Then) -> Committed {
        let kind = match then {
            Then::Discard => Kind::Warm,
            Then::Within(..) | Then::Notify(_) => Kind::Frame,
        };
        let (answer, rx) = match then {
            Then::Within(bound, late) => {
                let (tx, rx) = sync_channel(1);
                (Answer::Wait(tx, late), Some((rx, bound)))
            }
            Then::Notify(f) => (Answer::Notify(f), None),
            Then::Discard => (Answer::Discard, None),
        };
        let at = Instant::now();
        let (flying, mark) = match kind {
            Kind::Frame => {
                flight.in_flight.fetch_add(1, Ordering::AcqRel);
                let id = CommitId(flight.commits.fetch_add(1, Ordering::Relaxed) + 1);
                let mark = Arc::new(Mark {
                    id,
                    at,
                    state: AtomicU8::new(Word::from(State::InFlight).0),
                });
                (
                    Some(Arc::clone(&flight.in_flight)),
                    Some(Flying {
                        mark,
                        counters: Arc::clone(&flight.counters),
                    }),
                )
            }
            Kind::Warm => (None, None),
        };
        let counters = Arc::clone(&flight.counters);
        #[cfg(feature = "gpu-fault")]
        let (seam, inject) = (Arc::clone(&flight.seam), flight.seam.inject());
        let marked = mark.as_ref().map(|f| Arc::clone(&f.mark));
        let block = RcBlock::new(move |done: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
            let took = at.elapsed();
            // SAFETY: Metal hands the handler the command buffer that
            // completed, valid for the call.
            let completion = read(unsafe { done.as_ref() });
            #[cfg(feature = "gpu-fault")]
            let completion = inject.report(completion);
            let deliver = {
                let (counters, flying, answer, marked) = (
                    Arc::clone(&counters),
                    flying.clone(),
                    answer.clone(),
                    marked.clone(),
                );
                #[cfg(feature = "gpu-fault")]
                let seam = Arc::clone(&seam);
                move || {
                    counters.count(kind, &completion, took);
                    // Answered: late if a bound came first, counted before
                    // the count-out, as every count is.
                    let late = marked.is_some_and(|m| m.answer() == State::Overdue);
                    if late {
                        counters.late.fetch_add(1, Ordering::Relaxed);
                    }
                    if let Some(f) = &flying {
                        f.fetch_sub(1, Ordering::AcqRel);
                    }
                    match answer {
                        // A waiter that has gone takes nothing; one gone at
                        // its bound is told the frame landed after all.
                        Answer::Wait(tx, then_late) => {
                            let _ = tx.try_send(completion);
                            if let (true, Some(then)) = (late, then_late) {
                                guard(SiteName("a late frame's GPU completion"), (), || then());
                            }
                        }
                        Answer::Notify(then) => {
                            guard(SiteName("a frame's GPU completion"), (), || {
                                then(completion)
                            })
                        }
                        Answer::Discard => {}
                    }
                    #[cfg(feature = "gpu-fault")]
                    seam.delivered();
                }
            };
            #[cfg(feature = "gpu-fault")]
            if inject == Inject::Hold {
                seam.hold(Box::new(deliver));
                return;
            }
            deliver();
        });
        // SAFETY: the block is a valid `MTLCommandBufferHandler` for the
        // length of the call; Metal copies it, so the `RcBlock` may drop
        // after.
        unsafe { self.cmd.addCompletedHandler(RcBlock::as_ptr(&block)) };
        self.cmd.commit();
        Committed {
            rx,
            flying: mark,
            cmd: self.cmd,
        }
    }
}

/// A committed buffer: its frame's [`Flying`], and the answer a
/// [`Then::Within`] commit is waited for through.
pub(crate) struct Committed {
    rx: Option<(Receiver<Completion>, Deadline)>,
    flying: Option<Flying>,
    cmd: CommandBuffer,
}

impl Committed {
    /// The frame, as its deadline wakes carry it; none for the warm-up
    /// pass, which is no frame.
    pub(crate) fn flying(&self) -> Option<Flying> {
        self.flying.clone()
    }

    /// **Blocks until the handler answers, for at most the commit's
    /// bound** ([`Then::Within`]), from the moment it was committed. Past
    /// it the frame is counted overdue -- by this wait, unless the answer
    /// came in the same instant -- and is [`Waited::Overdue`], still
    /// committed; its answer, when it comes, is counted late. A commit
    /// that asked for no answer, or a handler let go without one, is a
    /// [`Cause::Unanswered`] failure rather than a hang.
    pub(crate) fn wait(self) -> Waited {
        let unanswered = || {
            Waited::Failed(Fault {
                cause: Cause::Unanswered,
                text: "the completion handler answered nothing".into(),
            })
        };
        let Some((rx, bound)) = self.rx else {
            return unanswered();
        };
        let Some(flying) = self.flying else {
            return unanswered();
        };
        let left = flying.due(bound).saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(c) => Waited::of(c),
            Err(RecvTimeoutError::Disconnected) => unanswered(),
            Err(RecvTimeoutError::Timeout) => {
                if flying.mark.overdue() {
                    flying.counters.overdue.fetch_add(1, Ordering::Relaxed);
                    return Waited::Overdue(Overdue {
                        after: flying.mark.at.elapsed(),
                        status: StatusWord(u64::try_from(self.cmd.status().0).unwrap_or(u64::MAX)),
                    });
                }
                // Answered as the bound came: the handler moved the state
                // before it sends, so the answer is a send away.
                rx.recv_timeout(bound.0)
                    .map_or_else(|_| unanswered(), Waited::of)
            }
        }
    }
}

/// What a completed buffer says: its status, and the error or the stamps.
fn read(cmd: &ProtocolObject<dyn MTLCommandBuffer>) -> Completion {
    let status = cmd.status();
    if status == MTLCommandBufferStatus::Completed {
        return Completion::Done(Landing::of(cmd));
    }
    let error = cmd.error();
    let cause = match status {
        MTLCommandBufferStatus::Error => Cause::Error(match &error {
            Some(e) => code_of(e),
            None => FaultCode::Unknown(ErrorNumber(
                u64::try_from(MTLCommandBufferError::None.0).unwrap_or(0),
            )),
        }),
        other => Cause::Odd(StatusWord(u64::try_from(other.0).unwrap_or(u64::MAX))),
    };
    let text = match &error {
        Some(e) => describe(e),
        None => "no error was set".to_string(),
    };
    Completion::Failed(Fault {
        cause,
        text: text.into_boxed_str(),
    })
}

/// The `MTLCommandBufferError` an error names: its code in Metal's
/// domain, or an unknown code from any other.
fn code_of(e: &NSError) -> FaultCode {
    // SAFETY: an immutable `NSString` the framework exports.
    let ours = &*e.domain() == unsafe { MTLCommandBufferErrorDomain };
    match (ours, u64::try_from(e.code())) {
        (true, Ok(n)) => FaultCode::of(ErrorNumber(n)),
        (_, Ok(n)) => FaultCode::Unknown(ErrorNumber(n)),
        (_, Err(_)) => FaultCode::Unknown(ErrorNumber(u64::MAX)),
    }
}

/// An error's description, with the encoders' execution states where the
/// build asked for them.
fn describe(e: &NSError) -> String {
    let text = format!(
        "{} ({}, {})",
        e.localizedDescription(),
        e.domain(),
        e.code()
    );
    match encoders(e) {
        Some(states) => format!("{text}; encoders: {states}"),
        None => text,
    }
}

/// The encoders' execution states an error carries, which test and probe
/// builds ask for ([`Flight::encoded`]).
#[cfg(any(feature = "gpu-fault", feature = "effect-probe"))]
fn encoders(e: &NSError) -> Option<String> {
    // SAFETY: an immutable `NSString` the framework exports.
    let key = unsafe { objc2_metal::MTLCommandBufferEncoderInfoErrorKey };
    let info = e.userInfo().objectForKey(key)?;
    // SAFETY: every object answers `description` with an `NSString`,
    // returned autoreleased, which `Retained` takes.
    let said: Retained<objc2_foundation::NSString> =
        unsafe { objc2::msg_send![&*info, description] };
    Some(said.to_string())
}

/// A shipped build does not ask for them.
#[cfg(not(any(feature = "gpu-fault", feature = "effect-probe")))]
fn encoders(_: &NSError) -> Option<String> {
    None
}

/// **What the fault seam replaces**, from the next commit on.
#[cfg(feature = "gpu-fault")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inject {
    /// The real report.
    Pass,
    /// A failure with this cause in place of a `Completed` status; a real
    /// failure stays as it was.
    Fail(Cause),
    /// The whole completion -- the counts, the count-out, the answer and
    /// the notified closure -- held until [`crate::Gpu::release`], as a
    /// frame the GPU never finishes looks to every consumer. A waited
    /// frame held is never answered until then.
    Hold,
}

#[cfg(feature = "gpu-fault")]
impl Inject {
    fn report(self, real: Completion) -> Completion {
        match (self, real) {
            (Inject::Fail(cause), Completion::Done(_)) => Completion::Failed(Fault {
                cause,
                text: "injected by the fault seam".into(),
            }),
            (_, real) => real,
        }
    }
}

/// The seam's state: what to inject, the completions held, and how many
/// were delivered, under one lock a test may wait on.
#[cfg(feature = "gpu-fault")]
struct Seam {
    state: std::sync::Mutex<SeamState>,
    changed: std::sync::Condvar,
}

#[cfg(feature = "gpu-fault")]
struct SeamState {
    inject: Inject,
    held: Vec<Box<dyn FnOnce() + Send>>,
    delivered: Tally,
}

#[cfg(feature = "gpu-fault")]
impl Default for Seam {
    fn default() -> Seam {
        Seam {
            state: std::sync::Mutex::new(SeamState {
                inject: Inject::Pass,
                held: Vec::new(),
                delivered: Tally::ZERO,
            }),
            changed: std::sync::Condvar::new(),
        }
    }
}

#[cfg(feature = "gpu-fault")]
impl Seam {
    fn lock(&self) -> std::sync::MutexGuard<'_, SeamState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn inject(&self) -> Inject {
        self.lock().inject
    }

    fn hold(&self, deliver: Box<dyn FnOnce() + Send>) {
        self.lock().held.push(deliver);
        self.changed.notify_all();
    }

    fn delivered(&self) {
        self.lock().delivered.bump();
        self.changed.notify_all();
    }

    fn held(&self) -> Tally {
        Tally::of(self.lock().held.len())
    }

    /// Delivers every held completion on this thread, in the order held.
    fn release(&self) -> Tally {
        let held = core::mem::take(&mut self.lock().held);
        let n = Tally::of(held.len());
        for deliver in held {
            deliver();
        }
        n
    }
}

/// **The event rig's doors to the fault seam** (`effect-probe` with
/// `gpu-fault`): the rig's child has no `Gpu` to inject through -- the
/// window's is made on another thread and owned by its presenter -- so the
/// last seam made is kept here, and what the rig arms applies to it and to
/// every one made after.
#[cfg(all(feature = "gpu-fault", feature = "effect-probe"))]
pub(crate) mod rig {
    use super::{Inject, Seam};
    use baud_render::Tally;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    /// What every seam made from now on reports.
    static ARMED: Mutex<Inject> = Mutex::new(Inject::Pass);
    /// The last seam made: the window's.
    static LAST: Mutex<Option<Arc<Seam>>> = Mutex::new(None);

    fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A seam made: it reports what is armed, and is the one the doors
    /// reach.
    pub(super) fn made(seam: &Arc<Seam>) {
        seam.lock().inject = *lock(&ARMED);
        *lock(&LAST) = Some(Arc::clone(seam));
    }

    /// What the completions of frames committed from now on report, on
    /// the last seam made and every one after.
    pub(crate) fn arm(inject: Inject) {
        *lock(&ARMED) = inject;
        if let Some(s) = lock(&LAST).as_ref() {
            s.lock().inject = inject;
        }
    }

    /// The completions the last seam made holds.
    pub(crate) fn held() -> Tally {
        lock(&LAST).as_ref().map_or(Tally::ZERO, |s| s.held())
    }

    /// Delivers what the last seam made holds, on this thread.
    pub(crate) fn release() -> Tally {
        let seam = lock(&LAST).clone();
        seam.map_or(Tally::ZERO, |s| s.release())
    }
}

#[cfg(feature = "gpu-fault")]
impl Flight {
    pub(crate) fn set_inject(&self, inject: Inject) {
        self.seam.lock().inject = inject;
    }

    pub(crate) fn held(&self) -> Tally {
        self.seam.held()
    }

    /// Delivers every held completion on this thread, in the order held.
    pub(crate) fn release(&self) -> Tally {
        self.seam.release()
    }

    /// Waits up to `within` until `n` completions are held; how many are.
    pub(crate) fn await_held(&self, n: Tally, within: Duration) -> Tally {
        let s = self.seam.lock();
        let (s, _) = self
            .seam
            .changed
            .wait_timeout_while(s, within, |s| Tally::of(s.held.len()).value() < n.value())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Tally::of(s.held.len())
    }

    /// Waits up to `within` until `n` completions were delivered; how many
    /// were.
    pub(crate) fn await_delivered(&self, n: Tally, within: Duration) -> Tally {
        let s = self.seam.lock();
        let (s, _) = self
            .seam
            .changed
            .wait_timeout_while(s, within, |s| s.delivered.value() < n.value())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.delivered
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorNumber, FaultCode};

    /// **Every code round-trips through its number and has a counter of
    /// its own**, and every code the header does not name shares the last.
    #[test]
    fn every_code_has_its_own_counter() {
        let slots: [usize; FaultCode::COUNT] = core::array::from_fn(|i| i);
        let mut seen = [false; FaultCode::COUNT];
        for c in FaultCode::NAMED {
            assert_eq!(FaultCode::of(c.code()), c, "{c:?} round-trips");
            seen[*c.pick(&slots)] = true;
        }
        for n in [5, 99] {
            let c = FaultCode::Unknown(ErrorNumber(n));
            assert_eq!(FaultCode::of(c.code()), c, "{c:?} round-trips");
            assert_eq!(*c.pick(&slots), FaultCode::NAMED.len(), "{c:?}");
        }
        assert!(
            seen[..FaultCode::NAMED.len()].iter().all(|s| *s),
            "{seen:?}"
        );
    }
}
