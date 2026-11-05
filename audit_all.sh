#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype
# internals, if the ingest path gains a scan loop, if the other
# architecture's ISA crate stops compiling, if baud-domain gains a `bool`
# field, or if an optimised crate gains an address operation. Every budget,
# and the reason for each, is in `xtask/src/audit.rs`.
cd "$(dirname "$0")" || exit 1
exec cargo xtask audit
