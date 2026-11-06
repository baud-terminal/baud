//! A terminal's reply to a query, read from this process's tty:
//! `baud_bench::host::vt_reply` has what it does and why. `scripts/vt-child.sh`
//! runs it.

use baud_bench::host::{self, Live};
use std::os::fd::AsRawFd;

fn main() {
    let args: Vec<String> = host::py_args();
    let fd = std::io::stdin().as_raw_fd();
    let code = host::vt_reply::run(&args, fd, &mut Live::new(), &mut std::io::stdout());
    std::process::exit(code);
}
