//! Keystroke to pixel for any terminal, from outside the app:
//! `baud_bench::linux::keypixel` has the method. An example rather than a
//! bench, because a bare `cargo bench` runs every bench and this one wants a
//! compositor and a terminal on it.

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::process::exit(baud_bench::linux::keypixel::run(&args));
}
