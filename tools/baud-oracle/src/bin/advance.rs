//! Writes `baud-font/tests/var/advances.txt`; see the crate's docs and
//! `baud_oracle::oracle::advance`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::advance::ORACLE)
}
