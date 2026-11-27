//! The `gen-marks-payload` tool: `baud_bench::capture::marks` has what it does and why.
//! An example, so `baud` stays the only binary and `cargo bench` runs none
//! of the tools.

fn main() {
    std::process::exit(baud_bench::capture::marks::run(&baud_bench::capture::cli()));
}
