//! Writes `crates/baud-font/tests/var/marks.txt`; see the crate's docs and
//! `baud_oracle::oracle::marks`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::marks::ORACLE)
}
