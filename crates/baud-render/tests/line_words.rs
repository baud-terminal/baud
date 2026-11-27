//! **The state lamps' words on DEC's indicator line**: what the line is
//! given (`line_lamps`), which follows DEC STD 070's "shall not exceed 80
//! milliseconds" and "Indicator changes are not queued" (070-14 p. 14-6),
//! and the clause of WCAG 2.2's 2.3.1 the words meet at a cell size
//! (`flash_clause`): the area clause at ordinary sizes, where they follow
//! at once whatever the rate, and past it none at all.

mod support;

use baud_core::{IndicatorLamps, LampState, ReadLamp};
use baud_domain::{Cols, Rows};
use baud_font::Face;
use baud_render::{
    AREA_LIMIT, Blend, FlashClause, PIXELS_A_POINT, PxSize, Renderer, Stack, StatusLineLights,
    Theme, WORD_CELLS, flash_clause, line_lamps,
};
use support::font;

/// The cell, in points, that `points` of JetBrains Mono draws at a scale
/// of one, as the app sizes it.
fn cell_at(points: f32) -> (f32, f32) {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(points).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    let m = r.metrics();
    (m.cell_w.value() as f32, m.cell_h.value() as f32)
}

/// SYN alone at `on`.
fn syn(on: bool) -> IndicatorLamps {
    IndicatorLamps {
        syn: match on {
            true => LampState::Lit,
            false => LampState::Dark,
        },
        ..IndicatorLamps::DARK
    }
}

/// **The words meet the area clause at ordinary sizes, the count above**:
/// their fourteen cells, all changing at once, against a quarter of
/// WCAG's 341 x 256 field at 85.3 pixels an inch. JetBrains Mono's cell is
/// 8 x 17 points at 13, the default, and the area clause holds to 37
/// points; baud's zoom reaches 72, where it does not.
#[test]
fn the_words_meet_the_area_clause_at_ordinary_sizes_and_the_count_above() {
    assert_eq!(cell_at(13.0), (8.0, 17.0));
    let area = |p: f32| {
        let (w, h) = cell_at(p);
        WORD_CELLS * w * h * PIXELS_A_POINT * PIXELS_A_POINT
    };
    let clause = |p: f32| {
        let (w, h) = cell_at(p);
        flash_clause(w, h)
    };
    assert!(area(13.0) < AREA_LIMIT / 7.0, "{}", area(13.0));
    let mut last = FlashClause::Area;
    let mut from = None;
    for p in 6..=72u16 {
        let c = clause(f32::from(p));
        assert_eq!(
            c == FlashClause::Area,
            area(f32::from(p)) <= AREA_LIMIT,
            "{p} points"
        );
        // Once past the limit, never back under it at a larger size.
        assert!(
            !(last == FlashClause::Count && c == FlashClause::Area),
            "{p} points"
        );
        if c == FlashClause::Count && from.is_none() {
            from = Some(p);
        }
        last = c;
    }
    assert_eq!(from, Some(38), "the count clause from 38 points");
    assert_eq!(clause(72.0), FlashClause::Count);
    // The bound is every cell: a quarter of the field is 21,824 of its
    // pixels.
    assert_eq!(AREA_LIMIT, 21_824.0);
}

/// **Under the area clause the line is given the lamps as they are, at
/// any rate**: SYN toggled at every step is handed on at every step,
/// never held and never late, so the line follows DEC's 80 ms whatever the
/// program does; CD unknown is handed on as unknown. **Past the area
/// clause, and with the setting off, the line is given none**: the words
/// are additions to DEC's floor, and leaving them out departs from
/// neither DEC STD 070 nor WCAG, where holding them to three flashes a
/// second would make a change late past 80 ms.
#[test]
fn the_line_is_given_the_lamps_as_they_are_or_none() {
    let mut last = None;
    for i in 0..240u64 {
        let want = syn(i % 2 == 0);
        let got = line_lamps(StatusLineLights::States, FlashClause::Area, want);
        assert_eq!(got, Some(want), "step {i}");
        assert_ne!(got, last, "step {i}: every change is handed on");
        last = got;
    }
    let unknown = IndicatorLamps {
        cd: ReadLamp::Unknown,
        ..IndicatorLamps::DARK
    };
    assert_eq!(
        line_lamps(StatusLineLights::States, FlashClause::Area, unknown),
        Some(unknown)
    );
    for lamps in [syn(true), syn(false), unknown] {
        assert_eq!(
            line_lamps(StatusLineLights::States, FlashClause::Count, lamps),
            None
        );
        assert_eq!(
            line_lamps(StatusLineLights::Off, FlashClause::Area, lamps),
            None
        );
        assert_eq!(
            line_lamps(StatusLineLights::Off, FlashClause::Count, lamps),
            None
        );
    }
    assert_eq!(StatusLineLights::default(), StatusLineLights::Off);
}
