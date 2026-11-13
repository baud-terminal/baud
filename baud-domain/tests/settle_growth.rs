//! **A growth leaves every row's mark true of the wider screen**
//! ([`View::settle_growth`]), a wrapped row's included: its mark is the
//! width it wrapped at, which `RowExtent::reached` cannot raise, so after a
//! growth it has to be at least the new width or a write past it lies
//! beyond a mark nothing will move.

use baud_domain::*;

#[test]
fn a_growth_raises_a_wrapped_rows_mark_to_the_new_width() {
    let (s, ring) = (24usize, 3usize);
    let mut cells = vec![PackedCell::EMPTY; s * ring];
    let mut used = vec![RowExtent::ZERO; ring];
    let mut gens = vec![RowGen::ZERO; ring];
    let (mut ac, mut au, mut ag) = (vec![], vec![], vec![]);
    let mut store = SurfaceStore::new(
        Stride::of_cells(s),
        (&mut cells, &mut used, &mut gens),
        (&mut ac, &mut au, &mut ag),
    );
    let mut v = View::new(
        store.stamped(Which::Primary),
        Cols::of_index(13),
        Rows::of_index(ring),
        Rows::of_index(ring),
        RowRot::NONE,
    );
    for x in 0..13 {
        v.put(
            Row::ZERO,
            Col::at_index(x),
            PackedCell::glyph(Codepoint::from_char('a'), StyleId::DEFAULT, CellFlags::NONE),
        );
    }
    v.set_wrapped(Row::ZERO);
    // A short line under it, uniform from its mark: the control.
    v.put(
        Row::from_index(1),
        Col::ZERO,
        PackedCell::glyph(Codepoint::from_char('b'), StyleId::DEFAULT, CellFlags::NONE),
    );
    v.set_geometry(Cols::of_index(17), Rows::of_index(ring));
    v.settle_growth(Cols::of_index(13));
    assert_eq!(v.extent(Row::ZERO).index(), 17);
    assert!(v.extent(Row::ZERO).wrapped());
    assert_eq!(v.extent(Row::from_index(1)).index(), 1);
}
