#!/usr/bin/env bash
# The GPU path's checks on an AMD GPU (RADV): `scripts/linux-vulkan-radv.sh`,
# from the development shell, which puts the Vulkan loader on the library
# path.
#
# `baud-present-vulkan/tests/radv.rs` holds them, `#[ignore]`d so a run on
# a box without the GPU never passes them having drawn nothing. This runs
# them by name where the GPU exists: **a Linux host whose Vulkan devices
# include one driven by RADV**. Anywhere else it refuses and exits
# non-zero. It asserts that both tests ran, so a filter that matched
# nothing cannot pass either.
set -euo pipefail
cd "$(dirname "$0")/.."

[ "$(uname -s)" = Linux ] || {
  echo "linux-vulkan-radv: needs Linux" >&2
  exit 1
}
probe=$(cargo run -q -p baud-present-vulkan --example vk-probe 2>&1) || {
  echo "linux-vulkan-radv: no Vulkan here: $probe" >&2
  exit 1
}
grep -q '^Gpu ' <<<"$probe" || {
  echo "linux-vulkan-radv: no GPU among the Vulkan devices" >&2
  exit 1
}

out=$(cargo test -p baud-present-vulkan --test radv -- --ignored 2>&1) || {
  echo "$out" >&2
  exit 1
}
echo "$out" | grep -E '^test |test result'
ran=$(grep -c '^test radv_.* ok$' <<<"$out" || true)
[ "$ran" -eq 2 ] || {
  echo "linux-vulkan-radv: $ran of 2 tests ran" >&2
  exit 1
}
echo "linux-vulkan-radv: ok, 2 of 2"
