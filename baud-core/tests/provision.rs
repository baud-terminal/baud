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

/// **A banded provision admits the 8K grid that a single table refuses**,
/// and says it is banded with the band size asked; a size with no row or
/// no column, or past `MAX_COLS` x `MAX_ROWS`, is refused either way.
#[test]
fn a_banded_provision_admits_what_one_table_cannot() {
    let band = BandRows::of(ws!(1, 32).geometry().1);
    assert!(Provision::of(&ws!(960, 254)).is_err(), "single refuses 8K");
    let p = Provision::banded(&ws!(960, 254), band).expect("banded admits 8K");
    assert_eq!(p.mode(), Mode::Banded(Banding::Forced(band)));
    assert_eq!(
        Provision::of(&ws!(80, 24)).map(|p| p.mode()),
        Ok(Mode::Single)
    );
    assert!(Provision::banded(&ws!(960, 0), band).is_err(), "no rows");
    assert!(Provision::banded(&ws!(0, 24), band).is_err(), "no columns");
    let past = u16::try_from(MAX_ROWS + 1).unwrap();
    assert!(
        Provision::banded(&ws!(80, past), band).is_err(),
        "past MAX_ROWS"
    );
    assert_eq!(
        BandRows::of(ws!(1, 0).geometry().1).rows().index(),
        1,
        "a band has a row"
    );
}

/// **Storage says what band it gave**: never more rows than it holds and
/// never none, whatever was asked.
#[test]
fn storage_reports_the_band_it_gave() {
    for (c, r, band) in [(80u16, 24u16, 1u16), (200, 50, 7), (960, 254, 32)] {
        let p = Provision::banded(&ws!(c, r), BandRows::of(ws!(1, band).geometry().1)).unwrap();
        let mut owned = Owned::new(p);
        let given = owned.band_rows().rows().index();
        let held = owned.storage().row_capacity().index();
        assert!(
            (1..=held).contains(&given),
            "{c}x{r} band {band}: gave {given} of {held}"
        );
    }
}

/// **A banded shape is refused when not one of its rows fits a band**, at
/// the widest pitch any cache line gives its columns: with bands of 2,048
/// slots a 2048-wide row can't fit and an 80-wide one can. At
/// `BAND_SLOTS` every width up to `MAX_COLS` fits, so 2048x510 is admitted.
#[test]
fn a_band_that_holds_no_row_is_refused() {
    let band = BandRows::of(ws!(1, 1).geometry().1);
    assert!(Provision::banded_in(&ws!(2048, 10), band, BandSlots(2048)).is_err());
    assert!(Provision::banded_in(&ws!(80, 10), band, BandSlots(2048)).is_ok());
    assert!(Provision::banded(&ws!(2048, 510), band).is_ok());
    // The control: a default band holds the widest row.
    const { assert!(BAND_SLOTS.0 > MAX_COLS) };
}

/// **A terminal made larger than its storage takes the storage's rows**:
/// storage provisioned for 960x254 holds the rows the budget clamps it to,
/// and `Term::new` asked for 960x254 over it is that many rows, not 254 --
/// so entering the alternate screen, which walks every screen row, stays
/// inside the store rather than indexing past it.
#[test]
fn a_terminal_larger_than_its_storage_takes_the_storage_rows() {
    let mut owned = Owned::new(&ws!(960, 254));
    let held = owned.storage().row_capacity();
    assert!(
        held.index() < 254,
        "the control: the provision clamped ({held:?})"
    );
    let mut t = Term::new(owned.storage(), &ws!(960, 254));
    assert_eq!(t.geometry().1, held);
    t.write_at(BytePos::ZERO, b"\x1b[?1049h\x1b[254;1Hx\x1b[?1049l");
    assert_eq!(t.geometry().1, held);
}

/// **A provision carries the band size it was given**: `banded_in`'s slots
/// reach storage through it, so a forced-band test at a shape one table
/// could hold gets bands small enough to freeze; every other entry carries
/// `BAND_SLOTS`.
#[test]
fn a_provision_carries_its_band_slots() {
    let band = BandRows::of(ws!(1, 4).geometry().1);
    let small = Provision::banded_in(&ws!(200, 50), band, BandSlots(1024)).unwrap();
    assert_eq!(small.band_slots(), BandSlots(1024));
    assert_eq!(
        Provision::banded(&ws!(200, 50), band).unwrap().band_slots(),
        BAND_SLOTS
    );
    assert_eq!(
        Provision::of(&ws!(80, 24)).unwrap().band_slots(),
        BAND_SLOTS
    );
    assert_eq!(Provision::clamped(&ws!(80, 24)).band_slots(), BAND_SLOTS);
}
