//! The summary of a `scripts/linux-damage.sh` run: `baud_bench::linux::damage`
//! has what it reads and prints. This only hands it the arguments.

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::process::exit(baud_bench::linux::damage::run(&args).finish());
}
