//! The repository's gates and generators, run as `cargo xtask <command>`
//! (`.cargo/config.toml`). Each command prints what the script it replaced
//! printed and exits as it did, so the shells that call it, and anything
//! that reads their output, see no change.
//!
//! - `audit`: the raw-integer budgets (`audit_all.sh`).
//! - `impl-audit <file>`: one file's nine categories and two sums.
//! - `scan-audit [limit]`: the ingest path's scan loops.
//! - `changelog <context>`: every commit on stdin reaches the changelog
//!   (`scripts/check-changelog.sh`).
//! - `fnv1a <src> <lib> <sdig> <ldig>`: the Metal library's digests
//!   (`scripts/build-metallib.sh`).

#![forbid(unsafe_code)]

pub mod audit;
pub mod changelog;
pub mod fnv;
pub mod impl_audit;
pub mod scan_audit;
pub mod text;
pub mod walk;
