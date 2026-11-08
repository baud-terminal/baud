//! The keystroke-to-pixel driver: `baud_bench::key_latency` has what it
//! does and why. This only hands it the arguments.
//!
//! An example rather than a bench: a bare `cargo bench` runs every bench,
//! and this one launches the app and posts keys to its window.

fn main() {
    baud_bench::warm::refuse_while_warm();
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    std::process::exit(baud_bench::key_latency::run(&args));
}
