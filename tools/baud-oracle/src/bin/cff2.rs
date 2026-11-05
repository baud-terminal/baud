//! Writes `baud-font/tests/var/cff2-outlines.txt`; see the crate's docs and
//! `baud_oracle::oracle::cff2`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::cff2::ORACLE)
}
