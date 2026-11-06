//! The `gen-vim-unicode` tool: `baud_bench::capture::vim_unicode` has what it does and why.
//! An example, so `baud` stays the only binary and `cargo bench` runs none
//! of the tools.

fn main() {
    std::process::exit(baud_bench::capture::vim_unicode::run(
        &baud_bench::capture::cli(),
    ));
}
