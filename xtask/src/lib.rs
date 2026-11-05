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
//! - `features [--check]`: docs/guide/features.md and docs/roadmap.md,
//!   written from docs/data/ or checked against it and the tree.
//! - `sources`: the source-provenance gate (`scripts/check-sources.sh`),
//!   which runs `third-party --check` and the offline half of `page-text`.
//! - `third-party [--check]`: THIRD_PARTY.md, written or checked.
//! - `publish [--size|--verify|--release|--write-notices|--write-licences]`:
//!   what crates.io would receive, checked before anything is sent.
//! - `page-text [--key K] [--from DIR] [--save DIR] [--print] [--self-test]`:
//!   the learn.microsoft.com pages' article texts against their pins, on
//!   demand and over the network.

#![forbid(unsafe_code)]

pub mod audit;
pub mod changelog;
pub mod features;
pub mod fnv;
pub mod html;
pub mod impl_audit;
pub mod page_text;
pub mod publish;
pub mod py;
pub mod scan_audit;
pub mod sha256;
pub mod sources;
pub mod tarball;
pub mod text;
pub mod textwrap;
pub mod third_party;
pub mod toml10;
pub mod walk;
