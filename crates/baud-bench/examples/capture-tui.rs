//! The `capture-tui` tool: `baud_bench::capture::tui` has what it does and why.
//! An example, so `baud` stays the only binary and `cargo bench` runs none
//! of the tools.

fn main() {
    std::process::exit(baud_bench::capture::tui::run(&baud_bench::capture::cli()));
}
