//! **Every test that opens a pty runs in nextest's `pty` group**, which
//! bounds how many such tests run at once (`.config/nextest.toml`): a
//! stock Mac allows 511 ptys in all (`kern.tty.ptmx_max`), and every
//! window and every other suite running on the machine counts against it.
//! Each open in this crate's tests reaches [`opened`], which fails the test
//! when nextest runs it outside the group, so a new pty test cannot run
//! unbounded by being left out of the filter.
//!
//! Under `cargo test` there is no group: the variables are nextest's, and
//! with none set nothing is asserted.

/// The group a test that opens a pty runs in, as `.config/nextest.toml`
/// names it.
pub(crate) const GROUP: &str = "pty";

/// Called after each pty this crate's tests open: under nextest, the test
/// must be in [`GROUP`].
pub(crate) fn opened() {
    if std::env::var_os("NEXTEST").is_none() {
        return;
    }
    let group = std::env::var("NEXTEST_TEST_GROUP").unwrap_or_default();
    assert_eq!(
        group,
        GROUP,
        "{} opened a pty outside nextest's `{GROUP}` group: add it to the group's filter in \
         .config/nextest.toml",
        std::env::var("NEXTEST_TEST_NAME").unwrap_or_default(),
    );
}
