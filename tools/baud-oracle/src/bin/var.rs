//! Writes `baud-font/tests/var/normalize.txt`; see the crate's docs and
//! `baud_oracle::oracle::var`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::var::ORACLE)
}
