//! `Tracked`: every `&mut` of the value moves its generation, and `set`
//! moves it only when the value differs.

use baud_domain::Tracked;

/// **An edit moves the generation whether or not the value changed**, and
/// each edit to a new one: a reader comparing generations never misses a
/// change made through `edit`. Named break: `edit` that lends the value
/// without moving the generation -- the second compare reads equal.
#[test]
fn every_edit_moves_the_generation() {
    let mut t = Tracked::new(7u8);
    let first = t.generation();
    assert_eq!(t.generation(), first, "the control: reading moves nothing");
    let _ = t.edit();
    let second = t.generation();
    assert_ne!(second, first, "an edit that changed nothing");
    *t.edit() = 8;
    assert_ne!(t.generation(), second, "an edit that changed the value");
    assert_eq!(*t.get(), 8);
}

/// **`set` moves the generation only for another value**: setting the
/// value it holds leaves a reader with nothing new, and another value is
/// a change. Named break: `set` that always edits -- the equal set moves
/// the generation.
#[test]
fn set_moves_the_generation_only_for_another_value() {
    let mut t = Tracked::new(vec![1u8, 2]);
    let was = t.generation();
    t.set(vec![1, 2]);
    assert_eq!(t.generation(), was, "the same value set");
    t.set(vec![2, 1]);
    assert_ne!(t.generation(), was, "the control: another value set");
    assert_eq!(t.get(), &vec![2, 1]);
}
