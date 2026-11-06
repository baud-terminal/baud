//! How the app's frames reached the screen, from a compositor's log:
//! `baud_bench::linux::frames` has the checks. This only hands it the
//! arguments.

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::process::exit(baud_bench::linux::frames::run(&args).finish());
}
