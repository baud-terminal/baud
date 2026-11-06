//! A pointer and a keyboard for a headless wlroots compositor, from a script
//! on stdin: `baud_bench::linux::pointer` has the commands. An example rather
//! than a bench, because a bare `cargo bench` runs every bench and this one
//! wants a compositor.

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::process::exit(baud_bench::linux::pointer::run(&args));
}
