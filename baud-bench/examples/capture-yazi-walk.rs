//! The `capture-yazi-walk` tool: `baud_bench::capture::yazi_walk` has what it does and why.
//! An example, so `baud` stays the only binary and `cargo bench` runs none
//! of the tools.

fn main() {
    std::process::exit(baud_bench::capture::yazi_walk::run(
        &baud_bench::capture::cli(),
    ));
}
