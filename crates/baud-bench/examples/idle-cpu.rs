//! The CPU a binary spends with nothing to draw:
//! `baud_bench::host::idle_cpu` has what it does and why. This only hands it
//! the arguments.
//!
//! An example rather than a bench: a bare `cargo bench` runs every bench,
//! and this one launches the app.

use baud_bench::host::{self, Live};

fn main() {
    baud_bench::warm::refuse_while_warm();
    host::catch_interrupt();
    let args: Vec<String> = host::py_args();
    let env: Vec<_> = std::env::vars_os().collect();
    let code = host::idle_cpu::run(&args, &env, &mut Live::new(), &mut std::io::stdout());
    std::process::exit(code);
}
