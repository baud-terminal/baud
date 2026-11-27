//! **The background words have one writer** (`src/background.rs`):
//! `Renderer` reads and writes them only through it, so every write marks
//! the rows a presenter uploads (`Renderer::take_background_dirty`).

#[test]
fn the_background_words_are_written_only_through_their_writer() {
    let src = include_str!("../src/renderer.rs");
    assert!(
        src.contains("self.bg.fill(") && src.contains("self.bg.copy("),
        "the control: the cell write and the row restore go through the writer"
    );
    for direct in ["self.bg[", "self.bg =", "&mut self.bg"] {
        assert_eq!(src.matches(direct).count(), 0, "`{direct}` in renderer.rs");
    }
}
