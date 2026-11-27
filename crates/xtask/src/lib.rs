//! The repository's gates and generators, run as `cargo xtask <command>`
//! (`.cargo/config.toml`). Each command prints what the script it replaced
//! printed and exits as it did, so the shells that call it, and anything
//! that reads their output, see no change.
//!
//! - `audit`: the raw-integer budgets (`tools/audit_all.sh`).
//! - `impl-audit <file>`: one file's nine categories and two sums.
//! - `scan-audit [limit]`: the ingest path's scan loops.
//! - `changelog [<context>]`: every commit reaches the changelog: the whole
//!   history through git-cliff, or the log on stdin against a context file.
//! - `fnv1a <src> <lib> <sdig> <ldig>`: the Metal library's digests
//!   (`tools/scripts/build-metallib.sh`).
//! - `features [--check]`: docs/guide/features.md, written from
//!   docs/data/features.toml or checked against it and the tree.
//! - `features-since <rev>|--explain`: the crates a change can reach, for
//!   `feature-arms --since`.
//! - `feature-arms [--since <rev>]`: every feature of every crate builds
//!   alone.
//! - `feature-suites`: baud-core's suite under its feature arms.
//! - `test-scope <from> <to>|<rev>`: the tests and gates a change needs, for
//!   `scoped-test`.
//! - `scoped-test <from> <to>`: the gates `test-scope` names, run; exit 3
//!   when it names every gate.
//! - `sources`: the source-provenance gate (`tools/scripts/check-sources.sh`),
//!   which runs `third-party --check`, `citations --check` and the offline
//!   half of `page-text`.
//! - `citations [--check|--merge <tsv>...]`: docs/citations.md, the sources
//!   the tree cites, written or checked, and code-item rows merged into
//!   docs/data/citations-code.tsv.
//! - `third-party [--check]`: THIRD_PARTY.md, written or checked.
//! - `credits [--check|--list|--crates]`: crates/baud-app/assets/CREDITS.txt, the
//!   notices of every third-party work the binary embeds and every
//!   crates.io crate it links, written or checked, or every embedded file
//!   and the row it falls under, or the register of the linked crates
//!   (docs/research/crates.toml) written from the cargo registry.
//! - `no-python`: no tracked file is Python or runs it.
//! - `paths`: every `path` a `Cargo.toml` names stays inside the
//!   repository.
//! - `publish [--size|--verify|--release|--write-notices|--write-licences]`:
//!   what crates.io would receive, checked before anything is sent.
//! - `page-text [--key K] [--from DIR] [--save DIR] [--print] [--self-test]`:
//!   the learn.microsoft.com pages' article texts against their pins, on
//!   demand and over the network.
//! - `pdf-search <key> [<term>...]`: a term in a pinned PDF, whose positive
//!   control cannot be left out.
//! - `loop-head [--fn NAME] [--no-pageoff] A [B]` and `--self-test`: one
//!   function's machine code compared between two binaries.
//! - `vt-table <results> [<baseline>]`: the table from
//!   `tools/scripts/vt-compare.sh`'s results.
//! - `pmc <export.xml>`: the counter columns of an `xctrace` export
//!   (`tools/scripts/counter-controls/`).
//! - `forbidden PATTERNS CONTROLS FILE...`: files searched for a list kept
//!   outside the repository, each pattern found in its control first; the
//!   README session's `demo` runs the same module over its bytes.
//! - `comments --check BASE [-- PATHSPEC...]`: the comment lines a change
//!   adds, searched for the public rules and for the terms of a file kept
//!   outside the repository, named by `BAUD_COMMENT_TERMS`.
//! - `code-map --out DIR [--prune ROOT --keep N]`: the tree's files, public
//!   items, crates and gates, written from the code into DIR, and the
//!   oldest maps under ROOT pruned to the newest N.

#![forbid(unsafe_code)]

pub mod audit;
pub mod changelog;
pub mod citations;
pub mod cmd;
pub mod code_map;
pub mod comments;
pub mod copy_audit;
pub mod crates;
pub mod credits;
pub mod difflib;
pub mod feature_arms;
pub mod feature_suites;
pub mod features;
pub mod features_since;
pub mod fnv;
pub mod forbidden;
pub mod html;
pub mod impl_audit;
pub mod loop_head;
pub mod md5;
pub mod no_python;
pub mod page_text;
pub mod paths;
pub mod pdf;
pub mod pdf_search;
pub mod pmc;
pub mod publish;
pub mod py;
pub mod py_path;
pub mod scan_audit;
pub mod scoped_test;
pub mod sha256;
pub mod sources;
pub mod tarball;
pub mod test_scope;
pub mod text;
pub mod textwrap;
pub mod third_party;
pub mod toml10;
pub mod vt_table;
pub mod walk;
