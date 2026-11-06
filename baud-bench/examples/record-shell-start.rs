//! The `record-shell-start` tool: `baud_bench::capture::shell_start` has what it does and why.
//! An example, so `baud` stays the only binary and `cargo bench` runs none
//! of the tools.

fn main() {
    std::process::exit(baud_bench::capture::shell_start::run(
        &baud_bench::capture::cli(),
    ));
}
