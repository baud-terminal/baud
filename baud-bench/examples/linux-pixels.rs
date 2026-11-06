//! Frames compared pixel for pixel, and what `scripts/linux-pixels.sh` reads
//! out of a screenshot and a tree: `baud_bench::linux::pixels`. This only
//! hands it the arguments.

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::process::exit(baud_bench::linux::pixels::run(&args, std::io::stdin().lock()).finish());
}
