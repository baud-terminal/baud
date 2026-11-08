//! A launch with the Metal caches and without:
//! `baud_bench::host::shader_cache` has what it does and why. This only hands
//! it the arguments, and turns an interrupt into a way out that puts the
//! caches back.
//!
//! An example rather than a bench: a bare `cargo bench` runs every bench,
//! and this one launches the app and moves caches every app on the box uses.

use baud_bench::host::{self, Live};

fn main() {
    baud_bench::warm::refuse_while_warm();
    host::catch_interrupt();
    let args: Vec<String> = host::py_args();
    let env: Vec<_> = std::env::vars_os().collect();
    let code = host::shader_cache::run(&args, &env, &mut Live::new(), &mut std::io::stdout());
    std::process::exit(code);
}
