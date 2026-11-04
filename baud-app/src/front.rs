//! **What the two process lights read: OH and CD** (note AH, 3.2 and 3.3).
//!
//! * **OH**, Off Hook, is lit while a job other than the shell is in front:
//!   the shell's `e_tpgid`, "tty process group id", differs from its own
//!   `pbi_pgid` (`[[source: macos-sdk-sys-proc-info-h]]` 78), both out of
//!   one `PROC_PIDTBSDINFO` of the shell, 0.16 us on this Mac (note AH 3.2,
//!   measured). It is not `tcgetpgrp` on the master, which both kernels
//!   answer against POSIX's "shall fail if ... the file is not the
//!   controlling terminal" (`[[source: posix-tcgetpgrp]]`), and costs 2.6 us.
//! * **CD**, Carrier Detect, is lit while a member of that group is an ssh
//!   holding an established TCP socket: its name `ssh` out of
//!   `PROC_PIDT_SHORTBSDINFO`, then `PROC_PIDLISTFDS` and, for each socket,
//!   `PROC_PIDFDSOCKETINFO`'s `soi_kind` `SOCKINFO_TCP` with `tcpsi_state`
//!   `TSI_S_ESTABLISHED` (note AH 3.3, measured on a live ssh client).
//!
//! **Every tick, a few microseconds**: the shell's record each tick, and
//! each ssh member's sockets, about a microsecond each; the group's
//! members (7.96-8.16 us measured for this build, a walk of every
//! process) are listed and named only when the group in front changes,
//! when the process watch saw a fork, an exec or an exit, and once a
//! second otherwise, which catches a member the watch did not report. A
//! read the kernel refuses -- a root ssh under `sudo`, a shell that
//! exec'd a set-user-ID program -- is shown as such
//! ([`Reading::Refused`]), never as a quiet program (note AH, 3.8 and Q8).
//!
//! What it cannot see, from the process model (note AH 3.3): an ssh inside
//! `tmux attach`, whose server runs it on another pty; and an ssh
//! ControlMaster client, said to hold no TCP socket of its own.

use crate::sys::Pid;
use baud_domain::HostTime;
use core::time::Duration;

/// What one read found: an answer, a refusal, or no such process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Got<T> {
    Read(T),
    Refused,
    Gone,
}

/// The shell's own process group and its terminal's foreground group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Groups {
    pub own: Pid,
    pub front: Pid,
}

/// Whether a process holds an established TCP connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Carrier {
    Up,
    Down,
}

/// Whether a process is an ssh client, by its name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Program {
    Ssh,
    Other,
}

/// **The reads the two lights make**, the kernel's or a test's.
pub(crate) trait Procs {
    /// The groups `shell`'s record names.
    fn groups(&mut self, shell: Pid) -> Got<Groups>;
    /// The members of `group`, into `out`.
    fn members(&mut self, group: Pid, out: &mut Vec<Pid>);
    /// What `pid` runs.
    fn program(&mut self, pid: Pid) -> Got<Program>;
    /// Whether `pid` holds an established TCP connection.
    fn carrier(&mut self, pid: Pid) -> Got<Carrier>;
}

/// One light's reading at a tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reading {
    Dark,
    Lit,
    Refused,
}

/// Whether the process watch saw something move since the last listing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stale {
    No,
    Yes,
}

/// How long a listing stands with nothing said to move it.
pub(crate) const RELIST: Duration = Duration::from_secs(1);

/// **How long a listing stands while DEC's indicator line shows the
/// words, on a display that ticks every `tick`**: 070-14's 80 ms
/// (`line_lamps::BOUND`) less
/// what follows an ssh joining the group just after a listing, unheard by
/// the process watch -- up to a tick until the tick that sees the listing
/// due, up to a tick until the frame that draws the word, since the words
/// are read after the tick's pump has drawn, and that frame's vsync. 30
/// ms at 60 Hz and 55 ms at 120; below 37.5 Hz nothing is left, the group
/// is listed every tick and the bound is not met, as the printer field's
/// is not below 38.5 Hz (`print::indicator_every`). A listing is
/// 7.96-8.16 us (note AH 3.3).
pub(crate) fn line_relist(tick: Duration) -> Duration {
    crate::line_lamps::BOUND.saturating_sub(tick * 3)
}

/// **Who the readings are for at a tick**: no one, and nothing is read;
/// the lights, wherever they are drawn, listed every [`RELIST`]; or DEC's
/// line, listed every [`line_relist`] of the display's tick, with the
/// lights or without them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reads {
    None,
    Lights,
    Line(Duration),
}

impl Reads {
    /// For the lights at `lights`, shown or off, and the line showing the
    /// words or not, on a display that ticks every `tick`.
    pub(crate) fn of(
        lights: baud_render::ActivityLights,
        line: crate::line_lamps::Shown,
        tick: Duration,
    ) -> Reads {
        match (line, lights.shown()) {
            (crate::line_lamps::Shown::Words, _) => Reads::Line(line_relist(tick)),
            (crate::line_lamps::Shown::None, true) => Reads::Lights,
            (crate::line_lamps::Shown::None, false) => Reads::None,
        }
    }
}

/// **What a reading shows on DEC's line**: lit or dark as read, and a
/// refused read as unknown, `CD?`.
pub(crate) fn lamp(r: Reading) -> baud_core::ReadLamp {
    match r {
        Reading::Dark => baud_core::ReadLamp::Read(baud_core::LampState::Dark),
        Reading::Lit => baud_core::ReadLamp::Read(baud_core::LampState::Lit),
        Reading::Refused => baud_core::ReadLamp::Unknown,
    }
}

/// **The two lights' sampler**: the group listed last, its ssh members,
/// and when it was listed.
pub(crate) struct Front {
    listed: Option<(Pid, HostTime)>,
    ssh: Vec<Pid>,
    members: Vec<Pid>,
    stale: Stale,
}

impl Front {
    pub(crate) fn new() -> Front {
        Front {
            listed: None,
            ssh: Vec::new(),
            members: Vec::new(),
            stale: Stale::No,
        }
    }

    /// The process watch saw a fork, an exec or an exit: list again.
    pub(crate) fn moved(&mut self) {
        self.stale = Stale::Yes;
    }

    /// **OH's and CD's readings on a tick**, or none and no read at all
    /// while neither the lights nor DEC's line's words are on, or there is
    /// no shell to read.
    pub(crate) fn tick(
        &mut self,
        reads: Reads,
        procs: &mut impl Procs,
        shell: Option<Pid>,
        now: HostTime,
    ) -> Option<(Reading, Reading)> {
        match (reads, shell) {
            (Reads::Lights, Some(shell)) => Some(self.sample(procs, shell, now, RELIST)),
            (Reads::Line(every), Some(shell)) => Some(self.sample(procs, shell, now, every)),
            (Reads::None, _) | (_, None) => None,
        }
    }

    /// **OH's and CD's readings at `now`**, `shell` the session's root,
    /// the group listed again once a listing is `relist` old.
    pub(crate) fn sample(
        &mut self,
        procs: &mut impl Procs,
        shell: Pid,
        now: HostTime,
        relist: Duration,
    ) -> (Reading, Reading) {
        let g = match procs.groups(shell) {
            Got::Read(g) => g,
            Got::Refused => return (Reading::Refused, Reading::Refused),
            Got::Gone => return (Reading::Dark, Reading::Dark),
        };
        let oh = match g.front > 0 && g.front != g.own {
            true => Reading::Lit,
            false => Reading::Dark,
        };
        if g.front <= 0 {
            self.listed = None;
            self.ssh.clear();
            return (oh, Reading::Dark);
        }
        let due = match self.listed {
            Some((group, at)) => {
                group != g.front || self.stale == Stale::Yes || now >= at.plus(relist)
            }
            None => true,
        };
        if due {
            self.list(procs, g.front, now);
        }
        let mut refused = false;
        for &pid in &self.ssh {
            match procs.carrier(pid) {
                Got::Read(Carrier::Up) => return (oh, Reading::Lit),
                Got::Refused => refused = true,
                Got::Read(Carrier::Down) | Got::Gone => {}
            }
        }
        let cd = match refused {
            true => Reading::Refused,
            false => Reading::Dark,
        };
        (oh, cd)
    }

    /// The members of `group` and which of them are ssh.
    fn list(&mut self, procs: &mut impl Procs, group: Pid, now: HostTime) {
        self.members.clear();
        procs.members(group, &mut self.members);
        self.ssh.clear();
        for &pid in &self.members {
            if procs.program(pid) == Got::Read(Program::Ssh) {
                self.ssh.push(pid);
            }
        }
        self.listed = Some((group, now));
        self.stale = Stale::No;
    }
}

/// **The kernel's answers**, with the buffers each read reuses.
pub(crate) struct Live {
    pids: Vec<Pid>,
    fds: Vec<u8>,
}

impl Live {
    pub(crate) fn new() -> Live {
        Live {
            pids: vec![0; 256],
            fds: vec![0; 256 * crate::sys::FDINFO_SIZE],
        }
    }
}

/// A refusal, from the error the call left: permission is a refusal,
/// anything else is a process gone.
fn failed<T>() -> Got<T> {
    match std::io::Error::last_os_error().kind() {
        std::io::ErrorKind::PermissionDenied => Got::Refused,
        _ => Got::Gone,
    }
}

/// A record's 32-bit field, native-endian, as an `int`: `field` is its
/// four bytes, `None` past the record's end or for a value no `int` holds.
fn int(field: Option<&[u8]>) -> Option<core::ffi::c_int> {
    let w = u32::from_ne_bytes(field?.try_into().ok()?);
    core::ffi::c_int::try_from(w).ok()
}

/// The four bytes at `at` in `buf`.
fn four(buf: &[u8], at: core::ops::RangeFrom<usize>) -> Option<&[u8]> {
    buf.get(at)?.get(..4)
}

/// `proc_pidinfo`'s record of `flavor` for `pid` into `buf`, whole or a
/// failure: it answers the bytes it wrote, 0 on a failure
/// (`[[source: xnu-libproc-c]]` 100-110).
fn record(pid: Pid, flavor: core::ffi::c_int, buf: &mut [u8]) -> Got<()> {
    let Ok(size) = core::ffi::c_int::try_from(buf.len()) else {
        return Got::Gone;
    };
    // SAFETY: a buffer of the size passed beside it.
    let n = unsafe {
        crate::sys::proc_pidinfo(
            pid,
            flavor,
            crate::sys::ProcArg(0),
            buf.as_mut_ptr().cast(),
            size,
        )
    };
    match n == size {
        true => Got::Read(()),
        false => failed(),
    }
}

impl Procs for Live {
    fn groups(&mut self, shell: Pid) -> Got<Groups> {
        use crate::sys::{BSDINFO_PGID, BSDINFO_SIZE, BSDINFO_TPGID, PROC_PIDTBSDINFO};
        let mut buf = [0u8; BSDINFO_SIZE];
        match record(shell, PROC_PIDTBSDINFO, &mut buf) {
            Got::Read(()) => {}
            Got::Refused => return Got::Refused,
            Got::Gone => return Got::Gone,
        }
        let pid = |at| int(four(&buf, at..));
        match (pid(BSDINFO_PGID), pid(BSDINFO_TPGID)) {
            (Some(own), Some(front)) if own > 0 => Got::Read(Groups { own, front }),
            _ => Got::Gone,
        }
    }

    fn members(&mut self, group: Pid, out: &mut Vec<Pid>) {
        let Ok(size) = core::ffi::c_int::try_from(core::mem::size_of_val(&self.pids[..])) else {
            return;
        };
        // SAFETY: a buffer of the size passed beside it; the answer is a
        // count of pids (`[[source: xnu-libproc-c]]` 76-85).
        let n =
            unsafe { crate::sys::proc_listpgrppids(group, self.pids.as_mut_ptr().cast(), size) };
        let n = usize::try_from(n).unwrap_or(0).min(self.pids.len());
        out.extend_from_slice(&self.pids[..n]);
    }

    fn program(&mut self, pid: Pid) -> Got<Program> {
        use crate::sys::{BSDSHORTINFO_COMM, BSDSHORTINFO_SIZE, MAXCOMLEN, PROC_PIDT_SHORTBSDINFO};
        let mut buf = [0u8; BSDSHORTINFO_SIZE];
        match record(pid, PROC_PIDT_SHORTBSDINFO, &mut buf) {
            Got::Read(()) => {}
            Got::Refused => return Got::Refused,
            Got::Gone => return Got::Gone,
        }
        let comm = &buf[BSDSHORTINFO_COMM..BSDSHORTINFO_COMM + MAXCOMLEN];
        let end = comm.iter().position(|&b| b == 0).unwrap_or(comm.len());
        match &comm[..end] {
            b"ssh" => Got::Read(Program::Ssh),
            _ => Got::Read(Program::Other),
        }
    }

    fn carrier(&mut self, pid: Pid) -> Got<Carrier> {
        use crate::sys::{
            FDINFO_FD, FDINFO_SIZE, FDINFO_TYPE, PROC_PIDFDSOCKETINFO, PROC_PIDLISTFDS,
            PROX_FDTYPE_SOCKET, SOCKET_FDINFO_KIND, SOCKET_FDINFO_SIZE, SOCKET_FDINFO_TCP_STATE,
            SOCKINFO_TCP, TSI_S_ESTABLISHED,
        };
        let Ok(size) = core::ffi::c_int::try_from(self.fds.len()) else {
            return Got::Gone;
        };
        // SAFETY: a buffer of the size passed beside it; the answer is the
        // bytes of `proc_fdinfo` written.
        let n = unsafe {
            crate::sys::proc_pidinfo(
                pid,
                PROC_PIDLISTFDS,
                crate::sys::ProcArg(0),
                self.fds.as_mut_ptr().cast(),
                size,
            )
        };
        let Ok(n) = usize::try_from(n) else {
            return failed();
        };
        if n == 0 {
            return failed();
        }
        let mut info = [0u8; SOCKET_FDINFO_SIZE];
        let Ok(info_size) = core::ffi::c_int::try_from(info.len()) else {
            return Got::Gone;
        };
        let mut refused = false;
        for fd in self.fds[..n.min(self.fds.len())].chunks_exact(FDINFO_SIZE) {
            let (Some(number), Some(kind)) =
                (int(four(fd, FDINFO_FD..)), int(four(fd, FDINFO_TYPE..)))
            else {
                continue;
            };
            if kind != PROX_FDTYPE_SOCKET {
                continue;
            }
            // SAFETY: a buffer of the size passed beside it.
            let got = unsafe {
                crate::sys::proc_pidfdinfo(
                    pid,
                    number,
                    PROC_PIDFDSOCKETINFO,
                    info.as_mut_ptr().cast(),
                    info_size,
                )
            };
            if got != info_size {
                refused |= matches!(failed::<()>(), Got::Refused);
                continue;
            }
            let field = |at| int(four(&info, at..));
            if field(SOCKET_FDINFO_KIND) == Some(SOCKINFO_TCP)
                && field(SOCKET_FDINFO_TCP_STATE) == Some(TSI_S_ESTABLISHED)
            {
                return Got::Read(Carrier::Up);
            }
        }
        match refused {
            true => Got::Refused,
            false => Got::Read(Carrier::Down),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_render::Tally;

    /// A system of processes a test writes, and a count of the reads made.
    #[derive(Default)]
    struct Fake {
        groups: Option<Got<Groups>>,
        members: Vec<(Pid, Vec<Pid>)>,
        ssh: Vec<Pid>,
        refused: Vec<Pid>,
        up: Vec<Pid>,
        calls: Calls,
    }

    #[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
    struct Calls {
        groups: Tally,
        members: Tally,
        program: Tally,
        carrier: Tally,
    }

    impl Procs for Fake {
        fn groups(&mut self, _: Pid) -> Got<Groups> {
            self.calls.groups.bump();
            self.groups.unwrap_or(Got::Gone)
        }
        fn members(&mut self, group: Pid, out: &mut Vec<Pid>) {
            self.calls.members.bump();
            if let Some((_, m)) = self.members.iter().find(|(g, _)| *g == group) {
                out.extend_from_slice(m);
            }
        }
        fn program(&mut self, pid: Pid) -> Got<Program> {
            self.calls.program.bump();
            match self.ssh.contains(&pid) {
                true => Got::Read(Program::Ssh),
                false => Got::Read(Program::Other),
            }
        }
        fn carrier(&mut self, pid: Pid) -> Got<Carrier> {
            self.calls.carrier.bump();
            if self.refused.contains(&pid) {
                return Got::Refused;
            }
            match self.up.contains(&pid) {
                true => Got::Read(Carrier::Up),
                false => Got::Read(Carrier::Down),
            }
        }
    }

    const SHELL: Pid = 100;

    const MS: Duration = Duration::from_millis(1);

    fn at(d: Duration) -> HostTime {
        HostTime::ZERO.plus(Duration::from_secs(1) + d)
    }

    fn front(f: Pid) -> Option<Got<Groups>> {
        Some(Got::Read(Groups {
            own: SHELL,
            front: f,
        }))
    }

    /// **OH is lit only while a job other than the shell is in front**:
    /// dark at the prompt, where the group in front is the shell's own,
    /// lit for a job's group, dark again when the shell takes the terminal
    /// back, dark with no group in front.
    #[test]
    fn off_hook_is_a_job_other_than_the_shell_in_front() {
        let mut f = Fake {
            groups: front(SHELL),
            ..Fake::default()
        };
        let mut s = Front::new();
        assert_eq!(
            s.sample(&mut f, SHELL, at(Duration::ZERO), RELIST).0,
            Reading::Dark,
            "prompt"
        );
        f.groups = front(200);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 10), RELIST).0,
            Reading::Lit,
            "a job"
        );
        f.groups = front(SHELL);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 20), RELIST).0,
            Reading::Dark,
            "back"
        );
        f.groups = front(0);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 30), RELIST).0,
            Reading::Dark,
            "none"
        );
    }

    /// **CD is lit only while an ssh in front holds an established
    /// connection**: dark for a job with no ssh, dark for an ssh not yet
    /// connected, lit once it is, dark when it drops; an ssh that is not a
    /// member of the group in front lights nothing. The control: a member
    /// that is not ssh holds a connection and lights nothing.
    #[test]
    fn carrier_detect_is_an_ssh_in_front_with_its_connection_up() {
        let mut f = Fake {
            groups: front(200),
            members: vec![(200, vec![200, 201]), (300, vec![300])],
            ssh: vec![201, 300],
            up: vec![200],
            ..Fake::default()
        };
        let mut s = Front::new();
        assert_eq!(
            s.sample(&mut f, SHELL, at(Duration::ZERO), RELIST).1,
            Reading::Dark,
            "not up"
        );
        f.up.push(201);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 10), RELIST).1,
            Reading::Lit,
            "up"
        );
        f.up.retain(|&p| p != 201);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 20), RELIST).1,
            Reading::Dark,
            "down"
        );
        f.up.push(300);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 30), RELIST).1,
            Reading::Dark,
            "not in front"
        );
        f.groups = front(300);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 40), RELIST).1,
            Reading::Lit,
            "in front"
        );
    }

    /// **A refused read is shown as refused, not as quiet**: the shell's
    /// record refused makes both refused; an ssh whose sockets may not be
    /// read makes CD refused, unless another ssh is up; a gone shell is
    /// dark.
    #[test]
    fn a_refused_read_is_neither_lit_nor_dark() {
        let mut f = Fake {
            groups: Some(Got::Refused),
            ..Fake::default()
        };
        let mut s = Front::new();
        assert_eq!(
            s.sample(&mut f, SHELL, at(Duration::ZERO), RELIST),
            (Reading::Refused, Reading::Refused)
        );
        f.groups = Some(Got::Gone);
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 10), RELIST),
            (Reading::Dark, Reading::Dark)
        );
        f.groups = front(200);
        f.members = vec![(200, vec![200, 201])];
        f.ssh = vec![200, 201];
        f.refused = vec![200];
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 20), RELIST).1,
            Reading::Refused,
            "sudo"
        );
        f.up = vec![201];
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 30), RELIST).1,
            Reading::Lit,
            "one up"
        );
    }

    /// **Off reads nothing**: with the lights off and DEC's line showing
    /// no words, or no shell, a tick makes no process read at all and
    /// gives no reading; shown anywhere, in the title strip or on a line
    /// of their own, it reads. The line's words read with the lights off.
    #[test]
    fn the_lights_off_read_no_process() {
        use crate::line_lamps::Shown;
        use baud_render::ActivityLights::{BottomLine, Off, TitleBar, TopLine};
        let tick = Duration::from_micros(16_667);
        let (off, on) = (
            Reads::of(Off, Shown::None, tick),
            Reads::of(TitleBar, Shown::None, tick),
        );
        assert_eq!((off, on), (Reads::None, Reads::Lights));
        for place in [BottomLine, TopLine] {
            assert_eq!(Reads::of(place, Shown::None, tick), Reads::Lights);
        }
        let line = Reads::Line(line_relist(tick));
        for place in [Off, TitleBar, BottomLine, TopLine] {
            assert_eq!(Reads::of(place, Shown::Words, tick), line, "{place:?}");
        }
        let mut f = Fake {
            groups: front(200),
            members: vec![(200, vec![200])],
            ssh: vec![200],
            up: vec![200],
            ..Fake::default()
        };
        let mut s = Front::new();
        for ms in 0..50 {
            assert_eq!(s.tick(off, &mut f, Some(SHELL), at(MS * (ms * 8))), None);
            assert_eq!(s.tick(on, &mut f, None, at(MS * (ms * 8))), None);
        }
        assert_eq!(f.calls, Calls::default(), "nothing read");
        assert_eq!(
            s.tick(on, &mut f, Some(SHELL), at(MS * 400)),
            Some((Reading::Lit, Reading::Lit))
        );
        assert_ne!(f.calls, Calls::default(), "the control: on reads");
        let mut s = Front::new();
        assert_eq!(
            s.tick(line, &mut f, Some(SHELL), at(MS * 400)),
            Some((Reading::Lit, Reading::Lit)),
            "the line's words read with the lights off"
        );
    }

    /// **While DEC's line shows the words, an ssh the watch did not hear
    /// of is drawn within 80 ms** (070-14 p. 14-6): the group is listed
    /// again 80 ms less three ticks after the last listing, and an ssh
    /// that joins just after one is read at the first tick past that,
    /// which leaves a tick for the frame that draws it and one for its
    /// vsync. At 60 Hz the listing stands 30 ms and at 120 Hz 55; below
    /// 37.5 Hz it stands no time, every tick lists, and the bound is not
    /// met. The lights alone wait the second. Its socket is read each tick
    /// either way.
    #[test]
    fn the_lines_words_draw_a_missed_ssh_within_80_ms() {
        let us = Duration::from_micros;
        assert_eq!(line_relist(us(16_667)), us(29_999));
        assert_eq!(line_relist(us(8_333)), us(55_001));
        assert_eq!(line_relist(us(26_667)), us(0));
        assert_eq!(line_relist(us(33_333)), us(0));
        for tick in [us(16_667), us(8_333), us(11_111)] {
            for (reads, bound) in [
                (Reads::Line(line_relist(tick)), Duration::from_millis(80)),
                (Reads::Lights, RELIST + tick * 3),
            ] {
                let mut f = Fake {
                    groups: front(200),
                    members: vec![(200, vec![200])],
                    ssh: vec![201],
                    up: vec![201],
                    ..Fake::default()
                };
                let mut s = Front::new();
                assert!(
                    s.tick(reads, &mut f, Some(SHELL), at(Duration::ZERO))
                        .is_some()
                );
                // 201 joins just after the first listing, with no fork
                // the watch saw.
                f.members = vec![(200, vec![200, 201])];
                let joined = Duration::from_micros(1);
                let mut read = None;
                for k in 1..200u32 {
                    let now = tick * k;
                    let r = s.tick(reads, &mut f, Some(SHELL), at(now));
                    if r.map(|r| r.1) == Some(Reading::Lit) {
                        read = Some(now);
                        break;
                    }
                }
                let read = read.expect("found");
                // Drawn by the next tick's frame, shown at the vsync after.
                let shown = read + tick * 2 - joined;
                assert!(shown <= bound, "{tick:?} {reads:?}: {shown:?}");
                assert!(
                    shown > bound - tick * 2,
                    "{tick:?} {reads:?}: listed too often, {shown:?}"
                );
            }
        }
        assert_eq!(lamp(Reading::Refused), baud_core::ReadLamp::Unknown);
        assert_eq!(
            lamp(Reading::Lit),
            baud_core::ReadLamp::Read(baud_core::LampState::Lit)
        );
        assert_eq!(
            lamp(Reading::Dark),
            baud_core::ReadLamp::Read(baud_core::LampState::Dark)
        );
    }

    /// **The members are listed only when something moved**: a new group
    /// in front, the watch, or a second gone by; the sockets of the ssh
    /// members are read every tick, and a tick with none reads only the
    /// shell's record. A member who joins without the watch hearing is
    /// found within the second.
    #[test]
    fn the_group_is_listed_on_a_change_and_once_a_second() {
        let mut f = Fake {
            groups: front(200),
            members: vec![(200, vec![200])],
            ssh: vec![201],
            up: vec![201],
            ..Fake::default()
        };
        let mut s = Front::new();
        s.sample(&mut f, SHELL, at(Duration::ZERO), RELIST);
        assert_eq!(f.calls.members, Tally::of(1), "listed at first");
        for ms in 1..100 {
            s.sample(&mut f, SHELL, at(MS * (ms * 8)), RELIST);
        }
        assert_eq!(f.calls.members, Tally::of(1), "not again inside the second");
        assert_eq!(f.calls.carrier, Tally::of(0), "no ssh, no socket read");
        assert_eq!(
            f.calls.groups,
            Tally::of(100),
            "the shell's record every tick"
        );
        // 201 joins with no fork the watch saw: found once the second is up.
        f.members = vec![(200, vec![200, 201])];
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 990), RELIST).1,
            Reading::Dark
        );
        assert_eq!(
            s.sample(&mut f, SHELL, at(MS * 1000), RELIST).1,
            Reading::Lit
        );
        assert_eq!(f.calls.members, Tally::of(2));
        let before = f.calls.carrier;
        s.sample(&mut f, SHELL, at(MS * 1008), RELIST);
        assert_eq!(f.calls.carrier, before.inc(), "its socket each tick");
        assert_eq!(f.calls.members, Tally::of(2));
        s.moved();
        s.sample(&mut f, SHELL, at(MS * 1016), RELIST);
        assert_eq!(f.calls.members, Tally::of(3), "the watch moved it");
        f.groups = front(300);
        s.sample(&mut f, SHELL, at(MS * 1024), RELIST);
        assert_eq!(f.calls.members, Tally::of(4), "a new group in front");
    }

    /// **The kernel's answers, against processes this test made**: the
    /// test process's record names its group, which lists it; a child `nc`
    /// connected to a listener here is not named ssh and holds an
    /// established TCP socket; a child `sleep` holds none; pid 1's record
    /// is refused, not gone.
    #[test]
    fn the_kernel_answers_for_real_processes() {
        use std::io::Read as _;
        let mut live = Live::new();
        let me = Pid::try_from(std::process::id()).unwrap();
        let Got::Read(g) = live.groups(me) else {
            panic!("our own record");
        };
        assert!(g.own > 0, "{g:?}");
        assert_eq!(live.groups(1), Got::Refused, "launchd's record");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let mut nc = std::process::Command::new("/usr/bin/nc")
            .args(["127.0.0.1", &port])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("nc");
        let (_conn, _) = listener.accept().unwrap();
        let kid = Pid::try_from(nc.id()).unwrap();
        assert_eq!(
            live.program(kid),
            Got::Read(Program::Other),
            "nc is not ssh"
        );
        // The connection is up on the child's side once `accept` returned.
        assert_eq!(live.carrier(kid), Got::Read(Carrier::Up), "its socket");
        let mut sleep = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep");
        let quiet = Pid::try_from(sleep.id()).unwrap();
        assert_eq!(live.carrier(quiet), Got::Read(Carrier::Down), "no socket");
        sleep.kill().unwrap();
        sleep.wait().unwrap();
        let mut members = Vec::new();
        live.members(g.own, &mut members);
        assert!(members.contains(&me), "{members:?}");
        nc.kill().unwrap();
        let mut rest = Vec::new();
        let _ = nc.stdout.take().unwrap().read_to_end(&mut rest);
        nc.wait().unwrap();
    }
}
