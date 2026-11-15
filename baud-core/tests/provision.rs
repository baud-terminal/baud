//! **Storage is provisioned within the rendition budget by its type**
//! (`Provision`): the cells rule, `rows * cols + cols + 3 <= NSTYLE_MAX`, at
//! the provision and at every resize after it.

use baud_core::*;

macro_rules! ws {
    ($c:expr, $r:expr) => {
        WinSize {
            ws_col: $c,
            ws_row: $r,
        }
    };
}

macro_rules! cols {
    ($n:expr) => {
        ws!($n, 1).geometry().0
    };
}

/// **The rule admits what it holds and nothing past it**, at today's 2^16:
/// 400x162 is admitted and 400x163 is not; 478x135 and 466x135, the 4K
/// shapes, are admitted; a size with no row or no column is refused; and
/// 2048 columns hold 30 rows.
#[test]
fn the_provision_admits_the_cells_rule_and_nothing_past_it() {
    assert_eq!(NSTYLE_MAX, 1 << 16, "the figures below are at 2^16");
    assert!(Provision::of(&ws!(400, 162)).is_ok());
    assert_eq!(
        Provision::of(&ws!(400, 163)),
        Err(OverBudget {
            cols: cols!(400),
            rows: ws!(400, 163).geometry().1
        })
    );
    assert!(Provision::of(&ws!(478, 135)).is_ok());
    assert!(Provision::of(&ws!(466, 135)).is_ok());
    assert!(Provision::of(&ws!(400, 0)).is_err(), "no rows");
    assert!(Provision::of(&ws!(0, 24)).is_err(), "no columns");
    assert_eq!(Provision::rows_for(cols!(2048)).index(), 30);
    assert_eq!(Provision::rows_for(cols!(400)).index(), 162);
}

/// **`rows_for` is the largest admitted height at every width**: one more
/// row is refused, from one column to `MAX_COLS`, and a width that holds
/// more than `MAX_ROWS` is capped there.
#[test]
fn rows_for_is_the_largest_admitted_at_every_width() {
    for c in 1..=2048u16 {
        let most = Provision::rows_for(cols!(c)).index();
        let most16 = u16::try_from(most).unwrap();
        assert!(Provision::of(&ws!(c, most16)).is_ok(), "{c}x{most}");
        if most < MAX_ROWS {
            assert!(
                Provision::of(&ws!(c, most16 + 1)).is_err(),
                "{c}x{}",
                most + 1
            );
        }
    }
}

/// **A raw size reaches storage clamped into the budget**: its columns
/// kept, its rows cut to what they hold, and a size within it unchanged.
#[test]
fn a_raw_size_is_clamped_into_the_budget() {
    assert_eq!(
        Provision::clamped(&ws!(400, 200)).geometry(),
        ws!(400, 162).geometry()
    );
    assert_eq!(
        Provision::clamped(&ws!(80, 24)).geometry(),
        ws!(80, 24).geometry()
    );
    assert_eq!(
        Provision::clamped(&ws!(80, 0)).geometry(),
        ws!(80, 1).geometry()
    );
    let mut owned = Owned::new(&ws!(400, 200));
    assert_eq!(
        owned.storage().row_capacity().index(),
        162,
        "storage holds the clamped rows"
    );
}

/// **A resize past the provision is clamped by the budget at its new
/// width**, not by the provision's rows alone. A width up to the pitch is
/// allowed, and at 400 columns provisioned with 162 rows a pitch wider than
/// 400 would hold more renditions than the table if the rows stood: those
/// rows are cut to `rows_for` that width, and the resize says so.
#[test]
fn a_resize_past_the_budget_is_clamped_at_its_width() {
    let mut owned = Owned::new(&ws!(400, 162));
    let pitch = owned.stride().index();
    assert!(
        pitch > 400,
        "the control: the pitch is wider than the provision ({pitch})"
    );
    let p16 = u16::try_from(pitch).unwrap();
    let ws0 = ws!(400, 162);
    let mut t = Term::new(owned.storage(), &ws0);
    let most = Provision::rows_for(cols!(p16)).index();
    assert!(most < 162);
    match t.resize(&ws!(p16, 162)) {
        Resized::Truncated { cols: c, rows: r } => {
            assert_eq!((c.index(), r.index()), (pitch, most));
        }
        Resized::Ok => panic!("{pitch}x162 is past the budget and was not clamped"),
    }
    assert_eq!(t.resize(&ws!(400, 162)), Resized::Ok, "back within it");
}

/// **Storage takes a `Provision`**, so no raw-size constructor can come
/// back beside it: read off the source.
#[test]
fn storage_is_made_from_a_provision() {
    let src = include_str!("../src/storage.rs");
    let src = &src[src.find("impl Owned {").expect("Owned's impl")..];
    for f in [
        "pub fn new(",
        "pub fn with_geometry(",
        "pub fn with_history(",
    ] {
        let at = src
            .find(f)
            .unwrap_or_else(|| panic!("{f} is in storage.rs"));
        let sig = &src[at..at + src[at..].find('{').unwrap()];
        assert!(sig.contains("size: impl Into<Provision>"), "{sig}");
    }
}
