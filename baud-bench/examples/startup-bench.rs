//! The launch wall, from outside the app: `baud_bench::host::startup` has
//! what it does and why. This only hands it the arguments.
//!
//! An example rather than a bench: a bare `cargo bench` runs every bench,
//! and this one launches the app.

use baud_bench::host::{self, Live};

fn main() {
    let args: Vec<String> = host::py_args();
    let env: Vec<_> = std::env::vars_os().collect();
    let code = host::startup::run(&args, &env, &mut Live::new(), &mut std::io::stdout());
    std::process::exit(code);
}
