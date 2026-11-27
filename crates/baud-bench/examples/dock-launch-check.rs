//! The locale a launchd launch gives the shell:
//! `baud_bench::host::dock_check` has what it does and why. This only hands
//! it the arguments.
//!
//! An example rather than a test: it launches the app, so it must never run
//! in a suite.

use baud_bench::host;

fn main() {
    let args: Vec<String> = host::py_args();
    let env: Vec<_> = std::env::vars_os().collect();
    let code = host::dock_check::run(&args, &env, &mut std::io::stdout());
    std::process::exit(code);
}
