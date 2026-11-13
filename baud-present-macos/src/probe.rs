//! **The effect probe**, built only under the test-only `effect-probe`
//! feature: where each effect a driver asks of the OS actually runs.
//!
//! Under the feature an effect's body -- a link opened or revealed, a
//! command started, a pasteboard written -- is this module's [`record`]
//! instead of the OS call, so a test opens nothing, starts nothing and
//! writes no pasteboard. [`record`] notes the effect's kind, whether it
//! runs inside a view's `frame` or another of its callbacks or in a turn
//! of its own ([`Where`]), and whether a responder call is on the stack
//! ([`Responder`]). Then it makes the call the reveal crash's stack shows
//! under the services handler, `-[NSApplication
//! nextEventMatchingMask:untilDate:inMode:dequeue:]` in the default mode,
//! with a `wake:` of its own queued first, until that wake is delivered:
//! inside a responder call that re-enters the view, which is the crash.
//!
//! [`Guarded`](crate::unwind::Guarded) keeps [`Where`] and [`Responder`]
//! for every responder method and counts each `wake` it delivers.

use core::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use macos_liquid_glass::drawable::{DrawableView, Waker};
use macos_liquid_glass::objc2::MainThreadMarker;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{Message, msg_send, sel};
use macos_liquid_glass::objc2_app_kit::{NSApplication, NSEventMask};
use macos_liquid_glass::objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint};
use macos_liquid_glass::objc2_quartz_core::CADisplayLink;

/// Where a call runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    /// Inside the view's `frame`.
    InFrame,
    /// Inside another of the view's responder methods.
    InCallback,
    /// In a turn of its own, no responder method on the stack.
    InTurn,
}

/// Whether a responder method is on the stack: the view's responder is
/// borrowed for each one's whole length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Responder {
    Free,
    Held,
}

/// What an effect was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectKind {
    /// A link opened (`link::workspace`).
    Open,
    /// A file revealed in Finder (`link::workspace`).
    Reveal,
    /// A `[link.*]` command started.
    Command,
    /// The pasteboard written (`clipboard::write_text`).
    Copy,
    /// The pasteboard emptied (`clipboard::clear`).
    Clear,
    /// The find pasteboard written (`clipboard::write_find`).
    FindText,
    /// Words to VoiceOver (`announce::announce`).
    Announce,
    /// Permission to notify asked (`notify::Center`).
    Authorize,
    /// A notification posted.
    Notice,
    /// A notification removed.
    Unnotice,
    /// The Dock tile's bar shown or taken down.
    DockBar,
    /// The Dock icon asked to bounce (`notify::attention`).
    Attention,
    /// That request cancelled (`notify::cancel_attention`).
    Unattention,
    /// The system beep (`bell::beep`).
    Beep,
    /// A window raised, no focus taken (`window::order_front`).
    Front,
    /// A window raised and made key (`window::make_key_and_order_front`).
    KeyFront,
    /// The application activated (`window::activate`).
    Activate,
    /// The application's Dock icon set (`window::set_icon`).
    Icon,
    /// A window restored from the Dock (`window::deminiaturize`).
    Deminiaturize,
    /// A window put in the Dock (`window::miniaturize`).
    Miniaturize,
    /// A window put behind the others (`window::order_back`).
    Back,
    /// A window into full screen or out of it
    /// (`window::toggle_full_screen`).
    FullScreen,
    /// A window's frame set (`window::set_frame`).
    Frame,
    /// A new window opened, running a program (`window::open_running`).
    OpenWindow,
}

/// Each kind's word in a [`Record::line`], one table both ways.
const KINDS: [(EffectKind, &str); 24] = [
    (EffectKind::Open, "open"),
    (EffectKind::Reveal, "reveal"),
    (EffectKind::Command, "command"),
    (EffectKind::Copy, "copy"),
    (EffectKind::Clear, "clear"),
    (EffectKind::FindText, "find-text"),
    (EffectKind::Announce, "announce"),
    (EffectKind::Authorize, "authorize"),
    (EffectKind::Notice, "notice"),
    (EffectKind::Unnotice, "unnotice"),
    (EffectKind::DockBar, "dock-bar"),
    (EffectKind::Attention, "attention"),
    (EffectKind::Unattention, "unattention"),
    (EffectKind::Beep, "beep"),
    (EffectKind::Front, "front"),
    (EffectKind::KeyFront, "key-front"),
    (EffectKind::Activate, "activate"),
    (EffectKind::Icon, "icon"),
    (EffectKind::Deminiaturize, "deminiaturize"),
    (EffectKind::Miniaturize, "miniaturize"),
    (EffectKind::Back, "back"),
    (EffectKind::FullScreen, "full-screen"),
    (EffectKind::Frame, "frame"),
    (EffectKind::OpenWindow, "open-window"),
];

impl EffectKind {
    /// The kind's word.
    pub fn word(self) -> &'static str {
        KINDS
            .iter()
            .find(|(k, _)| *k == self)
            .map_or("?", |(_, w)| w)
    }

    /// The kind a word names.
    pub fn of(word: &str) -> Option<EffectKind> {
        KINDS.iter().find(|(_, w)| *w == word).map(|(k, _)| *k)
    }
}

/// What the nested call met: the wake queued before it, delivered inside
/// it, or the bound reached with none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nested {
    Delivered,
    Never,
}

/// One effect, as it ran.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    pub kind: EffectKind,
    pub at: Where,
    pub responder: Responder,
    pub nested: Nested,
}

impl Record {
    /// The record as one line of text, for a child process to print and
    /// its parent to read back with [`Record::parse`].
    pub fn line(&self) -> String {
        let kind = self.kind.word();
        let at = match self.at {
            Where::InFrame => "in-frame",
            Where::InCallback => "in-callback",
            Where::InTurn => "in-turn",
        };
        let responder = match self.responder {
            Responder::Free => "free",
            Responder::Held => "held",
        };
        let nested = match self.nested {
            Nested::Delivered => "delivered",
            Nested::Never => "never",
        };
        format!("effect {kind} {at} {responder} {nested}")
    }

    /// What [`Record::line`] wrote, or none for any other line.
    pub fn parse(line: &str) -> Option<Record> {
        let mut w = line.split(' ');
        if w.next()? != "effect" {
            return None;
        }
        let kind = EffectKind::of(w.next()?)?;
        let at = match w.next()? {
            "in-frame" => Where::InFrame,
            "in-callback" => Where::InCallback,
            "in-turn" => Where::InTurn,
            _ => return None,
        };
        let responder = match w.next()? {
            "free" => Responder::Free,
            "held" => Responder::Held,
            _ => return None,
        };
        let nested = match w.next()? {
            "delivered" => Nested::Delivered,
            "never" => Nested::Never,
            _ => return None,
        };
        w.next().is_none().then_some(Record {
            kind,
            at,
            responder,
            nested,
        })
    }
}

/// The wakes [`Guarded`](crate::unwind::Guarded) has delivered: compared,
/// never read as a number.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Wakes(u64);

impl Wakes {
    /// None delivered.
    pub const NONE: Wakes = Wakes(0);

    /// One more.
    #[must_use]
    pub fn next(self) -> Wakes {
        Wakes(self.0 + 1)
    }
}

/// Whether a turn's door (`turn::soon`, `turn::later`, `turn::after`)
/// hands over inline: the rig's controls, which put the effect back where
/// it was before the fix.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inline {
    /// On a main-queue turn of its own, as shipped.
    No,
    /// Inside the callback that asked: the click's answer, on `wake`.
    InCallback,
    /// Parked, and run by the next `frame` (`run_parked`): the crash's own
    /// shape, the reveal made inside the frame.
    InFrame,
    /// `turn::after` alone inside the callback that asked, its delay not
    /// waited, the other doors on turns of their own: a notification's
    /// expiry made where it was before it had a turn.
    AfterInCallback,
}

/// The bound on the nested call: reaching it is a failure, never a wait.
const BOUND: Duration = Duration::from_secs(2);

thread_local! {
    static AT: Cell<(Where, Responder)> = const { Cell::new((Where::InTurn, Responder::Free)) };
    static WAKES: Cell<Wakes> = const { Cell::new(Wakes::NONE) };
    static WAKER: Cell<Option<Waker>> = const { Cell::new(None) };
    static LOG: RefCell<Vec<Record>> = const { RefCell::new(Vec::new()) };
    static INLINE: Cell<Inline> = const { Cell::new(Inline::No) };
    static PARKED: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    static KEY: Cell<Key> = const { Cell::new(Key::No) };
    /// Whether the printed paths' worker owes a turn (`PathsDue`).
    static PATHS_DUE: Cell<PathsDue> = const { Cell::new(PathsDue::Idle) };
    /// Whether the activity lights ask for a step (`LampsDue`).
    static LAMPS_DUE: Cell<LampsDue> = const { Cell::new(LampsDue::Idle) };
    /// The activity lights lit at their last step, by their letters (`lit`).
    static LIT: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    /// The printed paths asked about (`paths_asked`).
    static PATHS_ASKED: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The text of each printed path asked about, in order
    /// (`paths_asked_texts`).
    static PATHS_ASKED_TEXTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The batches the floor held (`paths_held`).
    static PATHS_HELD: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The batches sent to the printed paths' worker (`paths_batches`).
    static PATHS_BATCHES: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The batches by why each was due (`paths_by`).
    static PATHS_BY: Cell<PathsBy> = const { Cell::new(PathsBy::NONE) };
    /// The link lines at rest at the last settle (`lines_marked`).
    static LINES_MARKED: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// Whether a frame is owed and not yet drawn (`FrameOwed`).
    static FRAME_OWED: Cell<FrameOwed> = const { Cell::new(FrameOwed::No) };
    /// Whether the launch splash owes frames (`Splash`).
    static SPLASH: Cell<Splash> = const { Cell::new(Splash::Still) };
    /// Whether baud believes its window is on screen (`Shown`).
    static SHOWN: Cell<Shown> = const { Cell::new(Shown::No) };
    /// The launch splash's starts (`greeted`).
    static GREETED: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The notices whose `w` passed and the core was told so (`expired`).
    static EXPIRED: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The turns that did notice work, news owed (`notice_turned`).
    static NOTICE_TURNS: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The window's key and occlusion state read for a post's plan
    /// (`notice_state_read`).
    static NOTICE_STATE_READS: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The lights' places read for their hover text (`lights_read`).
    static LIGHTS_READS: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The lights' look made from the theme (`look_made`).
    static LOOKS_MADE: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The title strip's ends read from the window (`edges_read`).
    static EDGES_READS: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The focus baud last set (`Focused`).
    static FOCUSED: Cell<Focused> = const { Cell::new(Focused::Unset) };
    /// What the last read found under the pointer (`Previewed`).
    static PREVIEWED: Cell<Previewed> = const { Cell::new(Previewed::Unset) };
    /// The reads of the link under the pointer (`HoverReads`).
    static HOVER_READS: Cell<HoverReads> = const { Cell::new(HoverReads(0)) };
    /// The turns that fed the core (`FedTurns`).
    static FED_TURNS: Cell<FedTurns> = const { Cell::new(FedTurns(0)) };
    /// The clock baud-app's frame gate reads the last commit's age by
    /// (`GateClock`).
    static GATE_CLOCK: Cell<GateClock> = const { Cell::new(GateClock::Live) };
    /// The hands of this machine's names to the links (`hand_names`).
    static HANDS: Cell<Taken> = const { Cell::new(Taken(0)) };
    /// The followed settings' changes taken: scroller, flashing, animated.
    static FOLLOWED: Cell<[Taken; 3]> = const { Cell::new([Taken(0); 3]) };
    /// What baud reads of the display options (`Access`).
    static ACCESS: Cell<Access> = const { Cell::new(Access::System) };
    /// The changes of the motion preference and the display settings taken.
    static TAKEN: Cell<(Taken, Taken)> = const { Cell::new((Taken(0), Taken(0))) };
    /// The state the display link's door last sent (`display_link.rs`),
    /// and how many times it sent one.
    static LINK: Cell<(crate::display_link::LinkState, Sends)> =
        const { Cell::new((crate::display_link::LinkState::Pause, Sends(0))) };
    /// The polls a window's last settle found still the tick's, by their
    /// rows' names (baud-app's `frames::Holder`).
    static HELD: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    /// The target the frame rig gives its next tick, as a real link would
    /// (`set_target`).
    static TARGET: Cell<Option<f64>> = const { Cell::new(None) };
    /// The frames the window had drawn at its last settle.
    static DRAWN: Cell<Option<baud_render::Tally>> = const { Cell::new(None) };
    /// What the GPU's completions had counted at the window's last settle.
    static GPU: Cell<crate::metal::Faults> = Cell::new(crate::metal::Faults::default());
    /// What the window opened with.
    static OPENED: Cell<Opened> = const { Cell::new(Opened::Unset) };
    /// The surface a frame drawn on the CPU in the GPU's place was shown on.
    static CPU_SHOWN: Cell<Option<crate::redraw::SurfaceIx>> = const { Cell::new(None) };
    /// The GPU frames committed drawn whole.
    static WHOLE: Cell<baud_render::Tally> = const { Cell::new(baud_render::Tally::ZERO) };
    /// The GPU frames committed.
    static COMMITTED: Cell<baud_render::Tally> = const { Cell::new(baud_render::Tally::ZERO) };
    /// What last set the layer's contents: a frame drawn on the CPU, or a
    /// GPU surface handed over.
    static LAYER: Cell<Layer> = const { Cell::new(Layer::Unset) };
    /// The frames the window had drawn when it gave the GPU up, if it did.
    static SWITCHED: Cell<Option<baud_render::Tally>> = const { Cell::new(None) };
    /// What the window's last settle found owed, as it spells it.
    static OWED: RefCell<String> = const { RefCell::new(String::new()) };
    /// Whether the polls still the tick's hold the link (`Lift`).
    static LIFT: Cell<Lift> = const { Cell::new(Lift::Kept) };
    /// How many display links a run made: one per run while the door
    /// starts a new link each time, one in all once it resumes the one.
    static MADE: Cell<Made> = const { Cell::new(Made(0)) };
    /// How many of the rig's ticks reached the view's `tick:`.
    static DELIVERED: Cell<Delivered> = const { Cell::new(Delivered(0)) };
    /// How many palettes the window's appearance applied.
    static STYLED: Cell<Styled> = const { Cell::new(Styled(0)) };
    /// The half of its blink the caret was last seen in, and how many
    /// times that half has changed (`caret_half`).
    static CARET: Cell<(Option<CaretHalf>, Flips)> = const { Cell::new((None, Flips(0))) };
}

/// Hold `run` for the next frame ([`Inline::InFrame`]).
pub fn park(run: Box<dyn FnOnce()>) {
    PARKED.with_borrow_mut(|p| p.push(run));
}

/// Run what [`park`] held: called by the Driver's `frame`, under the probe
/// only. Taken first, so an effect that parks another leaves it a frame.
pub fn run_parked() {
    let parked = PARKED.with_borrow_mut(core::mem::take);
    for run in parked {
        run();
    }
}

/// **Whether baud believes its window is key**: the event rig's focus
/// door. The rig's window is never made key, so without it every left
/// press is the click that brings the window forward and a link's
/// Cmd-click is never armed. It changes what baud believes, read by
/// `poll_focus` alone (baud-app's source check), and never AppKit's key
/// state, which the rig checks false after every iteration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// What AppKit says, alone.
    No,
    /// Key, whatever AppKit says.
    Yes,
}

/// Set whether baud believes its window is key.
pub fn set_key(key: Key) {
    KEY.with(|k| k.set(key));
}

/// Whether baud believes its window is key.
pub fn key() -> Key {
    KEY.with(Cell::get)
}

/// **Whether the printed paths' worker owes a turn**: a batch due or
/// out (baud-app's `Probes::deadline`); nothing owed but a wake it
/// scheduled still to come, which turns once and changes nothing -- the
/// hang's, for a batch answered since it was sent; or nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathsDue {
    Idle,
    Waking,
    Owed,
}

/// Set whether the printed paths' worker owes a turn.
pub fn set_paths_due(due: PathsDue) {
    PATHS_DUE.with(|d| d.set(due));
}

/// Whether the printed paths' worker owes a turn: the frame rig's settle
/// waits for the setup's batch and its answer.
pub fn paths_due() -> PathsDue {
    PATHS_DUE.with(Cell::get)
}

/// **Whether a frame is owed and not yet drawn**, as the window's last
/// settle found it (baud-app's `Pending::Dirty`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameOwed {
    No,
    Yes,
}

/// Set whether a frame is owed, at a settle.
pub fn set_frame_owed(owed: FrameOwed) {
    FRAME_OWED.with(|o| o.set(owed));
}

/// Whether the last settle found a frame owed: the frame rig's release
/// waits for none.
pub fn frame_owed() -> FrameOwed {
    FRAME_OWED.with(Cell::get)
}

/// **Whether the launch splash owes frames**, as the window's last settle
/// found it: playing or fading, or waiting, held or gone (baud-app's
/// `Splash::owes`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Splash {
    Still,
    Moving,
}

/// Set whether the splash owes frames, at a settle.
pub fn set_splash(splash: Splash) {
    SPLASH.with(|s| s.set(splash));
}

/// Whether the last settle found the splash owing frames: the frame rig
/// plays a started splash to its end before it types.
pub fn splash() -> Splash {
    SPLASH.with(Cell::get)
}

/// **Whether the activity lights ask for a step**: a light to change at
/// once or a hold or dwell still to end (baud-app's `Deadline::Lights`), or
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LampsDue {
    Idle,
    Owed,
}

/// Set whether the activity lights ask for a step.
pub fn set_lamps_due(due: LampsDue) {
    LAMPS_DUE.with(|d| d.set(due));
}

/// Whether the activity lights ask for a step: the frame rig's settle
/// waits for what the setup lit to go dark.
pub fn lamps_due() -> LampsDue {
    LAMPS_DUE.with(Cell::get)
}

/// Set the activity lights lit at a step, by their letters.
pub fn set_lit(letters: Vec<&'static str>) {
    LIT.with(|l| *l.borrow_mut() = letters);
}

/// **The activity lights lit at their last step**, by their letters: the
/// frame rig prints them.
pub fn lit() -> Vec<&'static str> {
    LIT.with(|l| l.borrow().clone())
}

/// Set the printed paths asked about so far, the worker's own count.
pub fn set_paths_asked(asked: Taken) {
    PATHS_ASKED.with(|b| b.set(asked));
}

/// **The printed paths asked about so far**, in every batch: the frame rig
/// counts those an act's output owed. A batch that names only the groups
/// a read saw asks about none.
pub fn paths_asked() -> Taken {
    PATHS_ASKED.with(Cell::get)
}

/// **Set the text of every printed path asked about so far**, in the order
/// asked: the worker's log, which only grows, so a list as long as the one
/// kept is the one kept and nothing is copied.
pub fn set_paths_asked_texts(texts: &[Vec<u8>]) {
    PATHS_ASKED_TEXTS.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() != texts.len() {
            *t = texts
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect();
        }
    });
}

/// **The text of every printed path asked about so far**, in the order
/// asked: the frame rig prints those an act's output asked about, so a
/// row can say which path its batch asked about, not only how many.
pub fn paths_asked_texts() -> Vec<String> {
    PATHS_ASKED_TEXTS.with(|t| t.borrow().clone())
}

/// Set the batches that asked about a path the floor held, and the batches
/// sent, the worker's own counts.
pub fn set_paths_batches(held: Taken, batches: Taken) {
    PATHS_HELD.with(|h| h.set(held));
    PATHS_BATCHES.with(|b| b.set(batches));
}

/// **The batches that asked about a path later than their own deadline**,
/// held by the floor after the last that did (baud-app's `By::Floor`): the
/// frame rig's listing row reads none.
pub fn paths_held() -> Taken {
    PATHS_HELD.with(Cell::get)
}

/// The batches sent to the printed paths' worker so far, of every kind.
pub fn paths_batches() -> Taken {
    PATHS_BATCHES.with(Cell::get)
}

/// **The screen batches sent to the printed paths' worker, by why each was
/// due** (baud-app's `By`), and the batches the floor held whose own
/// deadline the window's shell's `D` set: the worker's own counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathsBy {
    /// The window's shell's `D`.
    pub ended: Taken,
    /// The quiet after the last read.
    pub quiet: Taken,
    /// The ceiling after the path was met.
    pub ceiling: Taken,
    /// The floor after the last batch that asked about a path.
    pub floor: Taken,
    /// Held by the floor, a `D` having made them due.
    pub held_granted: Taken,
}

impl PathsBy {
    /// None sent.
    pub const NONE: PathsBy = PathsBy {
        ended: Taken(0),
        quiet: Taken(0),
        ceiling: Taken(0),
        floor: Taken(0),
        held_granted: Taken(0),
    };

    /// The batches sent since `earlier` was taken.
    pub fn since(self, earlier: PathsBy) -> PathsBy {
        let d = |a: Taken, b: Taken| Taken(a.0.saturating_sub(b.0));
        PathsBy {
            ended: d(self.ended, earlier.ended),
            quiet: d(self.quiet, earlier.quiet),
            ceiling: d(self.ceiling, earlier.ceiling),
            floor: d(self.floor, earlier.floor),
            held_granted: d(self.held_granted, earlier.held_granted),
        }
    }
}

/// Set the batches by why each was due, the worker's own counts.
pub fn set_paths_by(by: PathsBy) {
    PATHS_BY.with(|b| b.set(by));
}

/// **The batches sent so far, by why each was due**: the frame rig counts
/// those after an act's release, and a listing that ended on the shell's
/// `D` is asked about by it alone.
pub fn paths_by() -> PathsBy {
    PATHS_BY.with(Cell::get)
}

/// Set the link lines at rest, at a settle.
pub fn set_lines_marked(lines: Taken) {
    LINES_MARKED.with(|l| l.set(lines));
}

/// **The link lines at rest as the last settle found them**, each a run of
/// one row: the frame rig's listing row ends once its names are drawn.
pub fn lines_marked() -> Taken {
    LINES_MARKED.with(Cell::get)
}

/// **Whether baud believes its window is on screen**: what AppKit says,
/// or shown whatever it says -- the frame rig's window, which is never
/// ordered front, told it is shown (`rig_call::RigWindow::Shown`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    /// What AppKit says, alone.
    No,
    /// On screen, whatever AppKit says.
    Yes,
}

/// Set whether baud believes its window is on screen.
pub fn set_shown(shown: Shown) {
    SHOWN.with(|s| s.set(shown));
}

/// Whether baud believes its window is on screen.
pub fn shown() -> Shown {
    SHOWN.with(Cell::get)
}

/// **The launch splash started**, its window shown (baud-app's
/// `splash_check`), counted for the frame rig.
pub fn greeted() {
    GREETED.with(|g| g.set(Taken(g.get().0.saturating_add(1))));
}

/// How many times the launch splash started.
pub fn greetings() -> Taken {
    GREETED.with(Cell::get)
}

/// **A turn on which notices expired**: their `w` passed, and the window's
/// core was told they went (baud-app's `Delivery::mac_expired`).
pub fn expired() {
    EXPIRED.with(|e| e.set(Taken(e.get().0.saturating_add(1))));
}

/// How many turns notices expired on.
pub fn expirations() -> Taken {
    EXPIRED.with(Cell::get)
}

/// **What answered the last held `p=alive`**: none yet; Notification
/// Center's list, on the turn its wake brought (baud-app's
/// `Delivery::mac_listed`); or the read's bound (`reads::NOTICE_WAIT`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AliveAnswered {
    Unanswered,
    List,
    Bound,
}

thread_local! {
    static ALIVE_ANSWERED: Cell<AliveAnswered> = const { Cell::new(AliveAnswered::Unanswered) };
}

/// Set what answered the last held `p=alive`.
pub fn set_alive_answered(a: AliveAnswered) {
    ALIVE_ANSWERED.with(|c| c.set(a));
}

/// What answered the last held `p=alive`.
pub fn alive_answered() -> AliveAnswered {
    ALIVE_ANSWERED.with(Cell::get)
}

/// **A turn did notice work**: news was owed it (baud-app's
/// `notice_turn`, past its owed flag).
pub fn notice_turned() {
    NOTICE_TURNS.with(|n| n.set(Taken(n.get().0.saturating_add(1))));
}

/// How many turns did notice work.
pub fn notice_turns() -> Taken {
    NOTICE_TURNS.with(Cell::get)
}

/// **The window's state read for a notice's plan**: its key status and
/// occlusion, which baud-app's `deliver_notices` reads only when a read
/// posted a notification.
pub fn notice_state_read() {
    NOTICE_STATE_READS.with(|n| n.set(Taken(n.get().0.saturating_add(1))));
}

/// How many times the window's state was read for a notice's plan.
pub fn notice_state_reads() -> Taken {
    NOTICE_STATE_READS.with(Cell::get)
}

/// **The lights' places read for their hover text**: each shown light's
/// frame on the screen, brought into the terminal's view, which baud-app's
/// `lights_at` reads only where the lights may have moved.
pub fn lights_read() {
    LIGHTS_READS.with(|n| n.set(Taken(n.get().0.saturating_add(1))));
}

/// How many times the lights' places were read for their hover text.
pub fn lights_reads() -> Taken {
    LIGHTS_READS.with(Cell::get)
}

/// **The lights' look made from the theme**, which baud-app keeps and
/// makes again only where the theme, the lights' colours or the glow
/// changed.
pub fn look_made() {
    LOOKS_MADE.with(|n| n.set(Taken(n.get().0.saturating_add(1))));
}

/// How many times the lights' look was made.
pub fn looks_made() -> Taken {
    LOOKS_MADE.with(Cell::get)
}

/// **The title strip's ends read from the window**: the traffic lights'
/// frames and the window's width, which baud-app keeps and reads again
/// only after a resize or a change of backing.
pub fn edges_read() {
    EDGES_READS.with(|n| n.set(Taken(n.get().0.saturating_add(1))));
}

/// How many times the title strip's ends were read.
pub fn edges_reads() -> Taken {
    EDGES_READS.with(Cell::get)
}

/// **The focus baud last set**, as the frame rig prints it: none before
/// the first, and then what each `set_focus` was given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focused {
    /// None set yet.
    Unset,
    /// The window's keyboard gained.
    Gained,
    /// The window's keyboard lost.
    Lost,
}

/// Record the focus baud set.
pub fn focused(f: Focused) {
    FOCUSED.with(|c| c.set(f));
}

/// The focus baud last set.
pub fn focus() -> Focused {
    FOCUSED.with(Cell::get)
}

/// **What baud reads of the accessibility display options**: the frame
/// rig's door. What the system says, or what the rig set in its place --
/// Reduce Motion and Reduce Transparency each on or off, whatever this Mac
/// is set to -- so a row's start and its change are the rig's on every
/// machine, which cannot be changed in System Settings by a test. Read in
/// baud-app's two readers alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    /// What the system says, alone.
    System,
    /// These, in place of what the system says.
    Set {
        motion: Turned,
        transparency: Turned,
    },
}

/// A display option the rig set on or off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turned {
    Off,
    On,
}

impl Access {
    /// Reduce Motion as baud reads it, the system saying `system`.
    pub fn motion(self, system: Turned) -> Turned {
        match self {
            Access::System => system,
            Access::Set { motion, .. } => motion,
        }
    }

    /// Reduce Transparency as baud reads it, the system saying `system`.
    pub fn transparency(self, system: Turned) -> Turned {
        match self {
            Access::System => system,
            Access::Set { transparency, .. } => transparency,
        }
    }
}

/// Set what baud reads of the display options.
pub fn set_access(a: Access) {
    ACCESS.with(|c| c.set(a));
}

/// What baud reads of the display options.
pub fn access() -> Access {
    ACCESS.with(Cell::get)
}

/// A display option whose change baud took.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayOption {
    /// The motion preference (`poll_motion`).
    Motion,
    /// Reduce Transparency or Increase Contrast (`poll_display`).
    Display,
}

/// How many changes of an option baud took.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Taken(pub u32);

/// **A change of `o` taken**, counted for the frame rig.
pub fn took(o: DisplayOption) {
    TAKEN.with(|t| {
        let (m, d) = t.get();
        t.set(match o {
            DisplayOption::Motion => (Taken(m.0.saturating_add(1)), d),
            DisplayOption::Display => (m, Taken(d.0.saturating_add(1))),
        });
    });
}

/// The changes of `o` taken.
pub fn taken(o: DisplayOption) -> Taken {
    let (m, d) = TAKEN.with(Cell::get);
    match o {
        DisplayOption::Motion => m,
        DisplayOption::Display => d,
    }
}

/// **What the last read of the link under a resting pointer found for
/// the next frame**, for the frame rig: no read yet, a link's preview, or
/// none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Previewed {
    Unset,
    Link,
    Nothing,
}

/// Record what a read found under the pointer.
pub fn previewed(p: Previewed) {
    PREVIEWED.with(|c| c.set(p));
}

/// What the last read found under the pointer.
pub fn preview() -> Previewed {
    PREVIEWED.with(Cell::get)
}

/// **The clock baud-app's frame gate reads the last commit's age by**
/// (its `since_commit`, and nothing else): the instant now, or one the
/// frame rig froze -- so a turn that feeds the core draws no frame for
/// the time since the last, a frame interval of which owes one (D14),
/// whatever the machine's pace. Frozen at an instant, a commit after it
/// is no time ago -- every commit, frozen at one before the app was made;
/// advanced ([`advance_gate`]), every commit is that much older. No other clock reads it: the caret's, a synchronised update's
/// bound, the pace's deadlines and the splash keep theirs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateClock {
    Live,
    Frozen(std::time::Instant),
}

/// The frame gate's clock stopped at `at`.
pub fn freeze_gate(at: std::time::Instant) {
    GATE_CLOCK.with(|c| c.set(GateClock::Frozen(at)));
}

/// The frame gate's clock live again.
pub fn unfreeze_gate() {
    GATE_CLOCK.with(|c| c.set(GateClock::Live));
}

/// The frozen frame gate's clock moved on by `d`; a live one is left as it
/// is.
pub fn advance_gate(d: std::time::Duration) {
    GATE_CLOCK.with(|c| {
        if let GateClock::Frozen(at) = c.get() {
            c.set(GateClock::Frozen(at + d));
        }
    });
}

/// The frame gate's clock.
pub fn gate_clock() -> GateClock {
    GATE_CLOCK.with(Cell::get)
}

/// How many times baud-app read the link under a resting pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HoverReads(pub u32);

/// **A read of the link under the pointer** (baud-app's `read_hover_as`),
/// counted for the frame rig: what output that moves nothing the last read
/// went over must not add to.
pub fn hover_read() {
    HOVER_READS.with(|r| r.set(HoverReads(r.get().0.saturating_add(1))));
}

/// The reads of the link under the pointer.
pub fn hover_reads() -> HoverReads {
    HOVER_READS.with(Cell::get)
}

/// How many of baud-app's turns fed the core.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FedTurns(pub u32);

/// **A turn that fed the core** (baud-app's pump), counted for the frame
/// rig: what reads of the hover made per feed, rather than per frame,
/// would equal.
pub fn fed_turn() {
    FED_TURNS.with(|r| r.set(FedTurns(r.get().0.saturating_add(1))));
}

/// The turns that fed the core.
pub fn fed_turns() -> FedTurns {
    FED_TURNS.with(Cell::get)
}

/// **What the notification center answers a "which are delivered" with,
/// in a probe build**: what it sent and has not removed, or nothing, ever:
/// the frame rig's Notification Center that never answers, so a held
/// `p=alive` is answered at its bound (baud-app's `reads::NOTICE_WAIT`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeList {
    Answers,
    Stalled,
}

thread_local! {
    static NOTICE_LIST: Cell<NoticeList> = const { Cell::new(NoticeList::Answers) };
}

/// Set what the center answers a "which are delivered" with.
pub fn set_notice_list(l: NoticeList) {
    NOTICE_LIST.with(|c| c.set(l));
}

/// What the center answers a "which are delivered" with.
pub fn notice_list() -> NoticeList {
    NOTICE_LIST.with(Cell::get)
}

/// **What `link::host_name` answers in a probe build**: the system's
/// `NSProcessInfo.hostName`, or the frame rig's stalled reverse lookup,
/// which holds the window's read of every name (baud-app's `Hosts`) until
/// the rig answers it with a name of its own. Shared across threads: the
/// read runs on a thread of its own.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HostName {
    System,
    Stalled,
    Answered(String),
}

static HOST_NAME: std::sync::Mutex<HostName> = std::sync::Mutex::new(HostName::System);
static HOST_NAME_SET: std::sync::Condvar = std::sync::Condvar::new();

/// Set what `link::host_name` answers, waking a read it holds.
pub fn set_host_name(h: HostName) {
    *HOST_NAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = h;
    HOST_NAME_SET.notify_all();
}

/// The name `link::host_name` answers, `None` for the system's: held
/// while the lookup is stalled.
pub fn host_name() -> Option<String> {
    let held = HOST_NAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let held = HOST_NAME_SET
        .wait_while(held, |h| *h == HostName::Stalled)
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match &*held {
        HostName::Answered(name) => Some(name.clone()),
        HostName::System | HostName::Stalled => None,
    }
}

/// **This machine's names handed to the links** by baud-app
/// (`hand_host_names`), counted for the frame rig.
pub fn hand_names() {
    HANDS.with(|n| n.set(Taken(n.get().0.saturating_add(1))));
}

/// How many times baud-app handed this machine's names to the links.
pub fn hands() -> Taken {
    HANDS.with(Cell::get)
}

/// A setting the window follows, taken (baud-app's `follow_settings`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Followed {
    /// "Show scroll bars".
    Scroller,
    /// Dim Flashing Lights.
    Flashing,
    /// Animated Images.
    Animated,
}

/// **A followed setting's change taken** by baud-app, counted for the
/// frame rig: on the turn its notification woke, not on a tick.
pub fn follow(s: Followed) {
    FOLLOWED.with(|f| {
        let [scroller, flashing, animated] = f.get();
        let one = |t: Taken| Taken(t.0.saturating_add(1));
        f.set(match s {
            Followed::Scroller => [one(scroller), flashing, animated],
            Followed::Flashing => [scroller, one(flashing), animated],
            Followed::Animated => [scroller, flashing, one(animated)],
        });
    });
}

/// How many times `s`'s change was taken.
pub fn followed(s: Followed) -> Taken {
    let [scroller, flashing, animated] = FOLLOWED.with(Cell::get);
    match s {
        Followed::Scroller => scroller,
        Followed::Flashing => flashing,
        Followed::Animated => animated,
    }
}

/// Set whether a turn's door hands over inline.
pub fn set_inline(inline: Inline) {
    INLINE.with(|i| i.set(inline));
}

/// Whether a turn's door hands over inline.
pub fn inline() -> Inline {
    INLINE.with(Cell::get)
}

/// A responder method entered at `at`, left when this drops.
pub struct Entered((Where, Responder));

impl Drop for Entered {
    fn drop(&mut self) {
        AT.with(|a| a.set(self.0));
    }
}

/// Enter a responder method: `InFrame` for `frame`, `InCallback` for the
/// rest.
pub fn enter(at: Where) -> Entered {
    Entered(AT.with(|a| a.replace((at, Responder::Held))))
}

/// Where the main thread is now.
pub fn now() -> (Where, Responder) {
    AT.with(Cell::get)
}

/// A `wake` delivered to the responder.
pub fn woke() {
    WAKES.with(|w| w.set(w.get().next()));
}

/// The wakes delivered so far.
pub fn wakes() -> Wakes {
    WAKES.with(Cell::get)
}

/// The view whose `wake:` [`record`] queues before its nested call.
pub fn set_waker(waker: Waker) {
    WAKER.with(|w| w.set(Some(waker)));
}

/// Every effect recorded since the last take, in order.
pub fn take() -> Vec<Record> {
    LOG.with(|l| core::mem::take(&mut *l.borrow_mut()))
}

/// **The effect `kind`, recorded where it runs**, then the crash's nested
/// call: a `wake:` posted from another thread and joined, so it is
/// certainly queued, and `nextEventMatchingMask:` in the default mode,
/// which runs the main run loop, until the responder has had it or
/// [`BOUND`] has passed.
pub fn record(kind: EffectKind) {
    let (at, responder) = now();
    let before = wakes();
    if let Some(waker) = WAKER.with(Cell::get) {
        let _ = std::thread::spawn(move || waker.wake()).join();
    }
    // Inside a responder call the window crate holds the wake until the
    // call returns, so the wait there is one nested call -- which runs the
    // queued perform, ready since the join -- and not the bound.
    let bound = match responder {
        Responder::Held => Duration::ZERO,
        Responder::Free => BOUND,
    };
    let nested = match MainThreadMarker::new() {
        None => Nested::Never,
        Some(mtm) => {
            let app = NSApplication::sharedApplication(mtm);
            let start = Instant::now();
            let mut first = true;
            loop {
                if wakes() > before {
                    break Nested::Delivered;
                }
                if !first && start.elapsed() >= bound {
                    break Nested::Never;
                }
                first = false;
                let until = NSDate::dateWithTimeIntervalSinceNow(0.01);
                // SAFETY: the default mode is a framework constant; not
                // dequeued, so no event is taken from the app.
                let _ = unsafe {
                    app.nextEventMatchingMask_untilDate_inMode_dequeue(
                        NSEventMask::Any,
                        Some(&until),
                        NSDefaultRunLoopMode,
                        false,
                    )
                };
            }
        }
    };
    LOG.with(|l| {
        l.borrow_mut().push(Record {
            kind,
            at,
            responder,
            nested,
        });
    });
}

/// How many times the display link's door sent a state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sends(pub u32);

/// How many of the rig's ticks reached the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivered(pub u32);

/// **Which half of its blink the caret is in**, as the window's tick
/// reads its clock: shown, hidden, or not blinking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaretHalf {
    Shown,
    Hidden,
    Steady,
}

/// How many times the caret has changed half: its flips, on its own clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flips(pub u32);

/// **Whether the polls still the tick's hold the display link**: kept, as
/// in every real build, or lifted, the frame rig's ceiling arm
/// (`rig_call::RigHolders`). Only a probe build has the lift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lift {
    Kept,
    Lifted,
}

/// The lift the frame rig asked for.
pub fn set_lift(lift: Lift) {
    LIFT.with(|l| l.set(lift));
}

/// Whether the polls still the tick's hold the link.
pub fn lift() -> Lift {
    LIFT.with(Cell::get)
}

/// How many display links the door's runs made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Made(pub u32);

/// The links the door's runs made.
pub fn made() -> Made {
    MADE.with(Cell::get)
}

/// **The display link's door, recorded** (`display_link::ViewLink`):
/// under the probe it sends nothing to AppKit, and the rig's
/// [`Ticker`] delivers only while the state is
/// [`LinkState::Run`](crate::display_link::LinkState::Run). A run that
/// would make a new link counts it.
pub fn linked(to: crate::display_link::LinkState, making: crate::display_link::Making) {
    if to == crate::display_link::LinkState::Run && making == crate::display_link::Making::New {
        MADE.with(|m| m.set(Made(m.get().0.saturating_add(1))));
    }
    LINK.with(|l| {
        let (_, Sends(n)) = l.get();
        l.set((to, Sends(n.saturating_add(1))));
    });
}

/// The state the door last sent, and how many it has sent.
pub fn link() -> (crate::display_link::LinkState, Sends) {
    LINK.with(Cell::get)
}

/// **The polls still the tick's**, as the last settle found them.
pub fn held(rows: impl Iterator<Item = &'static str>) {
    HELD.with_borrow_mut(|h| {
        h.clear();
        h.extend(rows);
    });
}

/// **The vsync the frame rig's next tick is due at**, on the display
/// link's clock: the [`Ticker`]'s link is never on a run loop, so its own
/// `targetTimestamp` is zero, and a frame that read it would see the
/// clock stand still.
pub fn set_target(secs: f64) {
    TARGET.with(|t| t.set(Some(secs)));
}

/// The target the frame rig set, if it set one.
pub fn target() -> Option<f64> {
    TARGET.with(Cell::get)
}

/// **The frames the window had drawn**, as its last settle counted them.
pub fn drawn(frames: baud_render::Tally) {
    DRAWN.with(|d| d.set(Some(frames)));
}

/// The frames drawn at the last settle.
pub fn frames() -> baud_render::Tally {
    DRAWN.with(Cell::get).unwrap_or_default()
}

/// **What the GPU's completions had counted**, as the window's last settle
/// found them: the frames that failed and the slowest, for the frame rig
/// to print.
pub fn gpu(faults: crate::metal::Faults) {
    GPU.with(|g| g.set(faults));
}

/// The GPU's counts at the last settle.
pub fn gpu_faults() -> crate::metal::Faults {
    GPU.with(Cell::get)
}

/// **What the window's first frame was drawn with**, for the frame rig: the
/// GPU, the CPU in its place ([`crate::Presenter::present_on_cpu`]), or
/// nothing, a window that would open with a surface nothing drew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opened {
    Unset,
    Gpu,
    Cpu,
    Undrawn,
}

/// Whether a GPU frame was drawn whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Whole {
    No,
    Yes,
}

/// The window's first frame, as it opened with it.
pub fn opened(with: Opened) {
    OPENED.with(|o| o.set(with));
}

/// What the window opened with.
pub fn opening() -> Opened {
    OPENED.with(Cell::get)
}

/// **The surface a frame drawn on the CPU in the GPU's place was shown
/// on** ([`crate::Presenter::present_on_cpu`]), for the frame rig to read
/// back.
pub fn cpu_shown(at: crate::redraw::SurfaceIx) {
    CPU_SHOWN.with(|s| s.set(Some(at)));
}

/// The surface the last frame drawn on the CPU in the GPU's place was
/// shown on, if one was.
pub fn cpu_shown_on() -> Option<crate::redraw::SurfaceIx> {
    CPU_SHOWN.with(Cell::get)
}

/// A GPU frame committed: counted, and whether it was drawn whole, counted
/// if it was.
pub fn gpu_drew(whole: Whole) {
    COMMITTED.with(|c| c.set(c.get().inc()));
    if whole == Whole::Yes {
        WHOLE.with(|w| w.set(w.get().inc()));
    }
}

/// **The GPU frames committed**, for the frame rig: a frame owed by a
/// failure is committed after it.
pub fn gpu_committed() -> baud_render::Tally {
    COMMITTED.with(Cell::get)
}

/// **The GPU frames committed drawn whole**, for the frame rig: a frame
/// after one that failed or landed late is.
pub fn gpu_whole() -> baud_render::Tally {
    WHOLE.with(Cell::get)
}

/// **What last set the layer's contents**, for the frame rig: a frame
/// drawn on the CPU ([`crate::Presenter::present_changed`],
/// [`crate::Presenter::present_on_cpu`]), or a GPU surface (the launch's,
/// set where it drew, and the view's `presented` for every other).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Unset,
    Cpu,
    Gpu,
}

/// The layer's contents set from `by`.
pub fn layer(by: Layer) {
    LAYER.with(|l| l.set(by));
}

/// What last set the layer's contents.
pub fn layer_by() -> Layer {
    LAYER.with(Cell::get)
}

/// **The window gave the GPU up** with `frames` drawn, its first frame on
/// the CPU still to come.
pub fn switched(frames: baud_render::Tally) {
    SWITCHED.with(|s| s.set(Some(frames)));
}

/// The frames drawn when the window gave the GPU up, if it did.
pub fn switched_at() -> Option<baud_render::Tally> {
    SWITCHED.with(Cell::get)
}

/// **How many `Gpu`s this process made**, for the frame rig: one, the
/// window's, unless something made the device again.
pub fn gpu_made() -> baud_render::Tally {
    #[cfg(feature = "gpu-fault")]
    return crate::metal::rig_made();
    #[cfg(not(feature = "gpu-fault"))]
    baud_render::Tally::ZERO
}

/// **What the frame rig asks of the GPU's fault seam** (`BAUD_RIG_GPU`):
/// every completion reported as it was; held until released; reported
/// failed (`Internal`) from the next commit on, for ever; the next so many
/// frames reported failed (`Internal`), and then none; or the next frame
/// reported as from a device that revoked this client's access
/// (`AccessRevoked`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuSeam {
    Pass,
    Hold,
    Fail,
    FailNext(baud_render::Tally),
    Revoked,
}

/// Whether the seam took what was asked: it exists only in a build with
/// `gpu-fault`, which a row that needs it must fail without rather than
/// pass with nothing injected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seamed {
    Armed,
    NotBuilt,
}

/// **Arms the fault seam**: the window's GPU, and every one made after,
/// reports what `seam` says from its next commit on.
pub fn gpu_seam(seam: GpuSeam) -> Seamed {
    #[cfg(feature = "gpu-fault")]
    {
        use crate::metal::{Cause, FaultCode, Inject};
        crate::metal::rig_arm(match seam {
            GpuSeam::Pass => Inject::Pass,
            GpuSeam::Hold => Inject::Hold,
            GpuSeam::Fail => Inject::Fail(Cause::Error(FaultCode::Internal)),
            GpuSeam::FailNext(n) => Inject::FailNext(Cause::Error(FaultCode::Internal), n),
            GpuSeam::Revoked => Inject::FailNext(
                Cause::Error(FaultCode::AccessRevoked),
                baud_render::Tally::of(1),
            ),
        });
        Seamed::Armed
    }
    #[cfg(not(feature = "gpu-fault"))]
    {
        let _ = seam;
        Seamed::NotBuilt
    }
}

/// The completions the seam holds.
pub fn gpu_held() -> baud_render::Tally {
    #[cfg(feature = "gpu-fault")]
    return crate::metal::rig_held();
    #[cfg(not(feature = "gpu-fault"))]
    baud_render::Tally::ZERO
}

/// **Delivers every completion the seam holds**, on this thread, as their
/// handlers would have: how many.
pub fn gpu_release() -> baud_render::Tally {
    #[cfg(feature = "gpu-fault")]
    return crate::metal::rig_release();
    #[cfg(not(feature = "gpu-fault"))]
    baud_render::Tally::ZERO
}

/// **What the last settle found owed**, for the frame rig to print.
pub fn owed(what: String) {
    OWED.with_borrow_mut(|o| *o = what);
}

/// What the last settle found owed.
pub fn owes() -> String {
    OWED.with_borrow(Clone::clone)
}

/// The polls the last settle found still the tick's.
pub fn holders() -> Vec<&'static str> {
    HELD.with_borrow(Clone::clone)
}

/// How many palettes a window's appearance applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Styled(pub u32);

/// **A palette applied for the window's appearance** (baud-app's
/// `apply_style`), counted for the frame rig.
pub fn restyled() {
    STYLED.with(|s| s.set(Styled(s.get().0.saturating_add(1))));
}

/// The palettes applied.
pub fn styled() -> Styled {
    STYLED.with(Cell::get)
}

/// How many of the rig's ticks have reached the view.
pub fn delivered() -> Delivered {
    DELIVERED.with(Cell::get)
}

/// **The half the caret is in at a tick**, as the window read it from the
/// caret's clock: a change from the half last seen is a flip.
pub fn caret_half(half: CaretHalf) {
    CARET.with(|c| {
        let (seen, Flips(n)) = c.get();
        let n = match seen {
            Some(was) if was != half => n.saturating_add(1),
            _ => n,
        };
        c.set((Some(half), Flips(n)));
    });
}

/// The caret's flips seen so far.
pub fn flips() -> Flips {
    CARET.with(|c| c.get().1)
}

/// One pass of the main run loop in the default mode, for at most
/// `seconds`, returning after the first source it handles.
pub fn pass(seconds: f64) {
    // SAFETY: the default mode is a framework constant.
    let mode = unsafe { macos_liquid_glass::objc2_core_foundation::kCFRunLoopDefaultMode };
    macos_liquid_glass::objc2_core_foundation::CFRunLoop::run_in_mode(mode, seconds, true);
}

/// **The view's frame, driven by hand**: a `CADisplayLink` targeting the
/// view's `tick:`, never added to a run loop, so it fires only when
/// [`Ticker::tick`] sends it -- the display link's own code path, with no
/// vsync to wait on and no window on screen.
pub struct Ticker {
    view: Retained<DrawableView>,
    link: Retained<CADisplayLink>,
}

impl Ticker {
    pub fn new(view: &DrawableView) -> Ticker {
        // SAFETY: the target is the view, which implements `tick:` with
        // the display link's signature and is retained beside the link.
        let link = unsafe { view.displayLinkWithTarget_selector(view, sel!(tick:)) };
        Ticker {
            view: view.retain(),
            link,
        }
    }

    /// One frame: the view's `tick:`, sent as the display link sends it,
    /// while the door's state is `Run`; while it is paused a link sends
    /// nothing, and neither does this.
    pub fn tick(&self) {
        if link().0 != crate::display_link::LinkState::Run {
            return;
        }
        DELIVERED.with(|d| d.set(Delivered(d.get().0.saturating_add(1))));
        // SAFETY: `tick:` is the view's own method, taking the link.
        let () = unsafe { msg_send![&*self.view, tick: &*self.link] };
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}

/// **A pointer gesture sent to the view**: an `NSEvent` made here and
/// handed to the view's own `mouseDown:`, `mouseDragged:` or `mouseUp:`,
/// the methods AppKit calls with a click's events. It is never posted to
/// the application or the window server (`postEvent:`, `CGEventPost`), so
/// it reaches nothing but this view, and nothing takes focus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    Down,
    Dragged,
    Up,
    /// The pointer moved, no button held.
    Moved,
}

/// Whether Command is held with a [`Press`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chord {
    Plain,
    Command,
}

/// Send `press` at `at`, a point in the view's own (flipped) coordinates,
/// to `view`'s own method for it.
pub fn press(view: &DrawableView, press: Press, at: NSPoint, chord: Chord) {
    use macos_liquid_glass::objc2::runtime::AnyObject;
    use macos_liquid_glass::objc2::{class, rc::Retained as R};
    use macos_liquid_glass::objc2_app_kit::{NSEventModifierFlags, NSEventType};
    let in_window = view.convertPoint_toView(at, None);
    let number = view.window().map_or(0, |w| w.windowNumber());
    let kind = match press {
        Press::Down => NSEventType::LeftMouseDown,
        Press::Dragged => NSEventType::LeftMouseDragged,
        Press::Up => NSEventType::LeftMouseUp,
        Press::Moved => NSEventType::MouseMoved,
    };
    let flags = match chord {
        Chord::Plain => NSEventModifierFlags(0),
        Chord::Command => NSEventModifierFlags::Command,
    };
    // SAFETY: `+mouseEventWithType:location:modifierFlags:timestamp:
    // windowNumber:context:eventNumber:clickCount:pressure:`, with a nil
    // context, which the header asks for; nullable.
    let event: Option<R<AnyObject>> = unsafe {
        msg_send![
            class!(NSEvent),
            mouseEventWithType: kind,
            location: in_window,
            modifierFlags: flags,
            timestamp: 0.0f64,
            windowNumber: number,
            context: Option::<&AnyObject>::None,
            eventNumber: 0isize,
            clickCount: 1isize,
            pressure: 1.0f32
        ]
    };
    let Some(event) = event else {
        return;
    };
    // SAFETY: the view's own event methods, each taking an `NSEvent`.
    unsafe {
        let () = match press {
            Press::Down => msg_send![view, mouseDown: &*event],
            Press::Dragged => msg_send![view, mouseDragged: &*event],
            Press::Up => msg_send![view, mouseUp: &*event],
            Press::Moved => msg_send![view, mouseMoved: &*event],
        };
    }
}

/// Send the view a menu item's action, `copy:` and the like, as the menu
/// sends it to the first responder: the view's own method, a nil sender.
pub fn menu(view: &DrawableView, action: macos_liquid_glass::objc2::runtime::Sel) {
    use macos_liquid_glass::objc2::runtime::AnyObject;
    // SAFETY: `-performSelector:withObject:`, the action an `IBAction`
    // taking its sender. An action returns nothing, so what comes back is
    // no object: read as a bare pointer, never retained.
    let _: *mut AnyObject =
        unsafe { msg_send![view, performSelector: action, withObject: Option::<&AnyObject>::None] };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The rig's door replaces the system's answer**: set off, a system
    /// that says on reads off, and set on, one that says off reads on; with
    /// no door the system's answer is read. Break: the door or'd with the
    /// system, and a Mac with Reduce Motion on reads on whatever the rig set.
    #[test]
    fn the_access_door_replaces_the_systems_answer() {
        let off = Access::Set {
            motion: Turned::Off,
            transparency: Turned::Off,
        };
        let on = Access::Set {
            motion: Turned::On,
            transparency: Turned::On,
        };
        for system in [Turned::Off, Turned::On] {
            assert_eq!(off.motion(system), Turned::Off, "{system:?}");
            assert_eq!(off.transparency(system), Turned::Off, "{system:?}");
            assert_eq!(on.motion(system), Turned::On, "{system:?}");
            assert_eq!(on.transparency(system), Turned::On, "{system:?}");
            assert_eq!(Access::System.motion(system), system);
            assert_eq!(Access::System.transparency(system), system);
        }
    }

    /// Every kind is in the table once, and its word reads back as it.
    /// The match has no `_` arm, so a new kind does not compile until it
    /// has a place here.
    #[test]
    fn every_kind_has_one_word() {
        /// A kind's place in the table.
        #[derive(PartialEq, Eq, Debug)]
        struct Place(usize);
        fn place(k: EffectKind) -> Place {
            match k {
                EffectKind::Open => Place(0),
                EffectKind::Reveal => Place(1),
                EffectKind::Command => Place(2),
                EffectKind::Copy => Place(3),
                EffectKind::Clear => Place(4),
                EffectKind::FindText => Place(5),
                EffectKind::Announce => Place(6),
                EffectKind::Authorize => Place(7),
                EffectKind::Notice => Place(8),
                EffectKind::Unnotice => Place(9),
                EffectKind::DockBar => Place(10),
                EffectKind::Attention => Place(11),
                EffectKind::Unattention => Place(12),
                EffectKind::Beep => Place(13),
                EffectKind::Front => Place(14),
                EffectKind::KeyFront => Place(15),
                EffectKind::Activate => Place(16),
                EffectKind::Icon => Place(17),
                EffectKind::Deminiaturize => Place(18),
                EffectKind::Miniaturize => Place(19),
                EffectKind::Back => Place(20),
                EffectKind::FullScreen => Place(21),
                EffectKind::Frame => Place(22),
                EffectKind::OpenWindow => Place(23),
            }
        }
        for (i, (k, w)) in KINDS.iter().enumerate() {
            assert_eq!(place(*k), Place(i), "{k:?}");
            assert_eq!((k.word(), EffectKind::of(w)), (*w, Some(*k)));
        }
        let r = Record {
            kind: EffectKind::KeyFront,
            at: Where::InTurn,
            responder: Responder::Free,
            nested: Nested::Delivered,
        };
        assert_eq!(Record::parse(&r.line()), Some(r));
    }
}
