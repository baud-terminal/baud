//! **The suite's ptys stay within a budget well below the machine's
//! limit.** A stock Mac allows 511 ptys in all (`kern.tty.ptmx_max`), and
//! every window and every other suite running on the machine counts
//! against it, so baud's tests hold at most [`BUDGET`] at once under
//! nextest: three suites fit beside 127 terminals.
//!
//! Every test that opens a pty runs in nextest's `pty` group
//! (`.config/nextest.toml`), [`SLOTS`] slots of [`PTYS_PER_SLOT`] ptys. A
//! test takes one slot, or the slots [`HEAVY`] gives it, as its
//! `threads-required`. Each open in this crate's tests reaches [`opened`],
//! which, under nextest, fails the test when it runs outside the group or
//! holds more managers than its slots allow, so a new pty test can neither
//! run unbounded by being left out of the filter nor outgrow its slots
//! unseen. The tests below hold the configuration to the same numbers.
//!
//! Under `cargo test` there is no group, and every test is a thread of one
//! process, so a count is the whole binary's and how many tests run beside
//! each other is the machine's thread count: nothing is asserted. The two
//! [`HEAVY`] tests take [`heavy`] there, so they never run at once.

/// The group a test that opens a pty runs in, as `.config/nextest.toml`
/// names it.
pub(crate) const GROUP: &str = "pty";

/// A count of ptys.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Ptys(pub(crate) usize);

/// A count of the group's slots: a test's `threads-required`, or the
/// group's `max-threads`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Slots(pub(crate) usize);

impl Slots {
    /// The ptys these slots allow.
    pub(crate) const fn ptys(self) -> Ptys {
        Ptys(self.0 * PTYS_PER_SLOT.0)
    }
}

/// The ptys one slot of the group stands for: one more than the 3 the
/// largest of the ordinary pty tests holds at its peak, counted at every
/// open as the managers its process holds.
pub(crate) const PTYS_PER_SLOT: Ptys = Ptys(4);

/// The group's slots, its `max-threads`.
pub(crate) const SLOTS: Slots = Slots(32);

/// The ptys the suite holds at once under nextest, at most.
pub(crate) const BUDGET: Ptys = SLOTS.ptys();

/// The ptys a stock Mac allows (xnu's `PTMX_MAX_DEFAULT`).
const STOCK_MAC: Ptys = Ptys(511);

// 32 slots of 4, and three suites at once beside 127 terminals.
const _: () = assert!(BUDGET.0 == 128);
const _: () = assert!(3 * BUDGET.0 + 127 == STOCK_MAC.0);

/// The tests that hold more than one slot's ptys, and their slots: 16
/// threads holding 4 spawned terminals each, 64 at once, and 48 threads
/// opening and closing one each.
pub(crate) const HEAVY: [(&str, Slots); 2] = [
    (
        "pty::tests::terminals_spawned_at_once_hold_none_of_each_others_ends",
        Slots(16),
    ),
    ("pty::tests::terminals_opened_at_once_all_open", Slots(12)),
];

/// The slots `test` takes.
fn slots_of(test: &str) -> Slots {
    HEAVY
        .iter()
        .find(|(name, _)| *name == test)
        .map_or(Slots(1), |(_, slots)| *slots)
}

/// **The ptys this process holds a manager of**: its descriptors whose
/// `ptsname` answers, one per subsidiary name, as a manager and its copies
/// name one pty. `ptsname` answers for a manager alone: a subsidiary, a
/// pipe or a file fails it, on macOS as on Linux.
pub(crate) fn managers_held() -> Ptys {
    let mut names: Vec<std::path::PathBuf> = crate::inherited::open_from_three()
        .into_iter()
        .filter_map(super::subsidiary_of)
        .collect();
    names.sort_unstable();
    names.dedup();
    Ptys(names.len())
}

/// **No pty opens while [`opened`] counts.** The count lists the
/// descriptors and then asks each its pty, so without this a thread could
/// close one manager and open another between the two, and both count,
/// though they were never held at once: the 48 threads of
/// `terminals_opened_at_once_all_open`, which hold one each, read 49 or 50
/// in 9 of 40 runs on Linux. Each open holds [`opening`] from before the
/// manager opens until it is open, and the count holds the lock alone, so
/// every manager it counts was open when it began, and still open then:
/// it counts no more than were held at once.
static OPENS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Whether nextest runs this test, the only runner the budget is checked
/// under.
fn under_nextest() -> bool {
    std::env::var_os("NEXTEST").is_some()
}

/// Held by each open of a pty in this crate's tests, from before the
/// manager opens until it is open ([`OPENS`]); nothing under `cargo test`.
pub(crate) fn opening() -> Option<std::sync::RwLockReadGuard<'static, ()>> {
    under_nextest().then(|| {
        OPENS
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    })
}

/// Called after each pty this crate's tests open, with [`opening`] let go:
/// under nextest, the test is in [`GROUP`] and holds no more managers than
/// its slots allow.
pub(crate) fn opened() {
    if !under_nextest() {
        return;
    }
    let test = std::env::var("NEXTEST_TEST_NAME").unwrap_or_default();
    let group = std::env::var("NEXTEST_TEST_GROUP").unwrap_or_default();
    assert_eq!(
        group, GROUP,
        "{test} opened a pty outside nextest's `{GROUP}` group: add it to the group's filter in \
         .config/nextest.toml",
    );
    let slots = slots_of(&test);
    let held = {
        let _alone = OPENS
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        managers_held()
    };
    assert!(
        held <= slots.ptys(),
        "{test} holds {} ptys at once, past its {} slot(s) of {}: give it \
         more in `HEAVY` (crates/baud-app/src/pty/budget.rs) and its `threads-required` in \
         .config/nextest.toml",
        held.0,
        slots.0,
        PTYS_PER_SLOT.0,
    );
}

/// Held by each [`HEAVY`] test while it runs, so under `cargo test`, where
/// no group bounds them, they never run at once.
pub(crate) fn heavy() -> std::sync::MutexGuard<'static, ()> {
    static HEAVY_RUNS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    HEAVY_RUNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The test-group and override settings `.config/nextest.toml` gives.
#[cfg(test)]
mod config {
    pub(super) const TOML: &str = include_str!("../../../../.config/nextest.toml");

    /// The `max-threads` of the group named `group`.
    pub(super) fn max_threads(group: &str) -> Option<super::Slots> {
        TOML.lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix(&format!("{group} = {{ max-threads = "))
            })
            .and_then(|rest| rest.strip_suffix(" }"))
            .and_then(|n| n.parse().ok().map(super::Slots))
    }

    /// One `[[profile.default.overrides]]` block's settings.
    pub(super) struct Override {
        pub(super) filter: String,
        pub(super) group: Option<String>,
        pub(super) threads: Option<super::Slots>,
    }

    /// Every override, in the file's order.
    pub(super) fn overrides() -> Vec<Override> {
        TOML.split("[[profile.default.overrides]]")
            .skip(1)
            .map(|block| {
                let value = |key: &str| {
                    block.lines().find_map(|l| {
                        let (k, v) = l.split_once(" = ")?;
                        (k.trim() == key).then(|| v.trim().trim_matches('\'').to_string())
                    })
                };
                Override {
                    filter: value("filter").unwrap_or_default(),
                    group: value("test-group"),
                    threads: value("threads-required")
                        .and_then(|n| n.parse().ok().map(super::Slots)),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The group's slots hold the suite to its budget.** The file's
    /// `max-threads` is [`SLOTS`], so the most the group holds at once is
    /// [`BUDGET`], which three suites at once fit beside 127 terminals (the
    /// assertions on the constants).
    #[test]
    fn the_groups_slots_hold_the_suite_to_its_budget() {
        assert_eq!(
            config::max_threads(GROUP),
            Some(SLOTS),
            "the `{GROUP}` group's max-threads in .config/nextest.toml"
        );
    }

    /// **Each heavy test's slots are its `threads-required`, and only the
    /// heavy tests take more than one.** The per-test check reads
    /// [`HEAVY`]; nextest schedules by the file; they are one list.
    #[test]
    fn the_heavy_tests_take_their_slots_in_the_file() {
        let overrides = config::overrides();
        assert!(!overrides.is_empty(), "the probe reads the overrides");
        let mut in_file: Vec<(String, Slots)> = overrides
            .iter()
            .filter(|o| o.group.as_deref() == Some(GROUP))
            .filter_map(|o| Some((o.filter.clone(), o.threads?)))
            .collect();
        in_file.sort();
        let mut table: Vec<(String, Slots)> = HEAVY
            .iter()
            .map(|(name, slots)| (format!("package(baud) & test(={name})"), *slots))
            .collect();
        table.sort();
        assert_eq!(in_file, table);
    }

    /// **A test that needs every slot of its group is never run.** nextest
    /// 0.9.146 left a test whose `threads-required` was the group's
    /// `max-threads` unrun, with the tests queued after it, and failed the
    /// run without naming a test: at 16 slots, the 16-slot test and the 8
    /// and 9 behind it. So the heaviest takes fewer than all of them.
    #[test]
    fn a_test_needing_every_slot_of_its_group_is_never_run() {
        let heaviest = config::overrides()
            .iter()
            .filter(|o| o.group.as_deref() == Some(GROUP))
            .filter_map(|o| o.threads)
            .max()
            .expect("a heavy test in the group");
        assert!(
            heaviest < SLOTS,
            "a test takes {} of the group's {} slots",
            heaviest.0,
            SLOTS.0
        );
    }
}
