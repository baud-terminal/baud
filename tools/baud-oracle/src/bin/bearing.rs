//! Writes `baud-font/tests/var/bearings.txt`; see the crate's docs and
//! `baud_oracle::oracle::bearing`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::bearing::ORACLE)
}
