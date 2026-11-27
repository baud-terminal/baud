//! Writes `crates/baud-font/tests/var/outlines.txt`; see the crate's docs and
//! `baud_oracle::oracle::gvar`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::gvar::ORACLE)
}
