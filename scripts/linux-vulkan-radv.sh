#!/usr/bin/env bash
# The GPU path's checks on an AMD GPU (RADV): `scripts/linux-vulkan-radv.sh`,
# from the development shell, which puts the Vulkan loader on the library
# path.
#
# `baud-present-vulkan/tests/radv.rs` and `tests/instance_path.rs` hold
# them, `#[ignore]`d so a run on a box without the GPU never passes them
# having drawn nothing. This runs
# them by name where the GPU exists: **a Linux host whose Vulkan devices
# include one driven by RADV**. Anywhere else it refuses and exits
# non-zero. It asserts that every test named in `expect` ran and passed, and
# that nothing else ran, so a filter that matched nothing, or a test that
# was renamed, cannot pass either.
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

out=$(cargo test -p baud-present-vulkan --test radv --test instance_path --test effect_pixels --test fireworks_pixels -- --ignored 2>&1) || {
  echo "$out" >&2
  exit 1
}
echo "$out" | grep -E '^test |test result'
expect=(
  radv_clears_one_frame_into_a_linear_dmabuf
  radv_signals_a_frame_on_a_syncobj_timeline
  the_instance_path_is_exact_on_radv
  a_pass_over_keeps_the_target_on_radv
  every_effect_matches_the_cpu_path_on_radv
  the_plasma_mixes_match_the_reference_on_radv
  multiply_s_second_step_is_its_second_rounding_on_radv
  multiply_in_two_passes_matches_where_one_record_covers_on_radv
  the_fireworks_match_the_cpu_path_on_radv
)
for t in "${expect[@]}"; do
  grep -q "^test $t \.\.\. ok$" <<<"$out" || {
    echo "linux-vulkan-radv: $t did not run and pass" >&2
    exit 1
  }
done
ran=$(grep -cE '^test .* \.\.\. ok$' <<<"$out" || true)
[ "$ran" -eq "${#expect[@]}" ] || {
  echo "linux-vulkan-radv: $ran tests passed, ${#expect[@]} expected" >&2
  exit 1
}
echo "linux-vulkan-radv: ok, $ran of ${#expect[@]}"
