#!/usr/bin/env bash
# Source-provenance gate: `cargo xtask sources` (xtask/src/sources.rs),
# which says what it checks. This file stays for the callers that name it,
# devenv's test and its `check-sources` script among them.
set -euo pipefail
cd "$(dirname "$0")/.."
exec cargo xtask sources
