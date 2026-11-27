//! Writes `crates/baud-font/tests/var/vertical.txt`; see the crate's docs and
//! `baud_oracle::oracle::vertical`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::vertical::ORACLE)
}
