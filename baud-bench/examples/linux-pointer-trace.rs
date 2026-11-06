//! The app's pointer traffic from a compositor's log, one request or event a
//! line: `baud_bench::linux::trace` has the lines. This only hands it stdin.

fn main() {
    std::process::exit(baud_bench::linux::trace::run(std::io::stdin().lock()).finish());
}
