//! The `gen-yazi-files` tool: `baud_bench::capture::yazi_files` has what it does and why.
//! An example, so `baud` stays the only binary and `cargo bench` runs none
//! of the tools.

fn main() {
    std::process::exit(baud_bench::capture::yazi_files::run(
        &baud_bench::capture::cli(),
    ));
}
