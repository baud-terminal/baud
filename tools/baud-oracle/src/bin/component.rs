//! Writes `crates/baud-font/tests/var/components.txt`; see the crate's docs and
//! `baud_oracle::oracle::component`.

fn main() -> std::process::ExitCode {
    baud_oracle::main(&baud_oracle::oracle::component::ORACLE)
}
