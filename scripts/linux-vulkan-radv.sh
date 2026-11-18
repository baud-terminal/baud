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

out=$(cargo test -p baud-present-vulkan --test radv --test instance_path --test effect_pixels --test fireworks_pixels --test layouts --test sheets --test lend --test cell_path --test scenes_basic --test scenes_bidi --test scenes_compose --test scenes_fonts --test scenes_chrome --test blend_rounding --test presenter -- --ignored 2>&1) || {
  echo "$out" >&2
  exit 1
}
echo "$out" | grep -E '^test |test result'
expect=(
  a_light_change_on_its_line_draws_the_frame_whole_on_both_paths_on_radv
  an_exit_status_draws_the_frame_whole_on_both_paths_on_radv
  a_wrap_mark_draws_the_frame_whole_on_both_paths_on_radv
  a_steady_strip_and_margin_draw_by_rows_on_both_paths_on_radv
  a_guide_across_the_gutter_is_drawn_whole_on_both_paths_on_radv
  radv_clears_one_frame_into_a_linear_dmabuf
  radv_signals_a_frame_on_a_syncobj_timeline
  the_instance_path_is_the_cpu_path_on_radv
  a_pass_over_keeps_the_target_on_radv
  every_effect_matches_the_cpu_path_on_radv
  the_plasma_mixes_match_the_reference_on_radv
  multiply_s_second_step_is_its_second_rounding_on_radv
  multiply_in_two_passes_matches_where_one_record_covers_on_radv
  the_fireworks_match_the_cpu_path_on_radv
  every_cell_pipeline_builds_on_radv
  the_sheets_persist_and_a_glyph_is_copied_as_its_rectangle_on_radv
  lent_memory_is_read_as_owed_on_radv
  the_cell_path_is_the_instance_path_on_radv
  the_cell_path_is_the_cpu_path_on_radv
  two_targets_in_turn_are_each_the_whole_frame_on_radv
  the_cell_arm_draws_what_the_instance_arm_draws_on_radv
  each_decoration_alone_and_the_cursor_line_draw_alike_on_every_arm_on_radv
  a_coloured_bell_draws_alike_on_every_arm_on_radv
  radv_misses_the_cpu_only_a_step_at_a_tie
  right_to_left_rows_draw_in_their_visual_order_on_every_arm_on_radv
  a_re_broken_paragraph_draws_the_same_on_every_arm_on_radv
  a_selection_from_above_the_window_draws_the_same_on_every_arm_on_radv
  a_double_width_right_to_left_line_is_drawn_within_its_half_on_radv
  a_right_to_left_paragraph_is_drawn_against_the_right_edge_on_every_arm_on_radv
  a_table_row_keeps_its_columns_on_every_arm_on_radv
  a_right_to_left_paragraph_drawn_by_rows_is_the_whole_frame_on_radv
  the_cursor_on_a_right_to_left_row_is_drawn_in_its_shape_on_every_arm_on_radv
  mirrored_characters_draw_mirrored_on_every_arm_on_radv
  a_reversed_string_draws_alike_on_every_arm_on_radv
  a_leftward_row_draws_alike_on_every_arm_on_radv
  a_line_scp_turns_draws_alike_on_every_arm_on_radv
  a_paragraph_inside_side_margins_draws_alike_on_every_arm_on_radv
  the_cell_path_rejoins_only_the_rows_the_copy_finds_changed_on_radv
  a_composition_on_a_right_to_left_row_draws_as_the_text_written_on_radv
  a_clustered_composition_draws_as_its_text_written_on_every_arm_on_radv
  a_composition_against_a_devanagari_word_draws_as_written_on_every_arm_on_radv
  a_composition_over_varied_text_draws_as_written_on_every_arm_on_radv
  a_predicted_echo_on_a_right_to_left_row_draws_as_its_echo_on_radv
  a_predicted_bracket_on_a_right_to_left_row_draws_mirrored_as_its_echo_on_radv
  alternative_fonts_draw_alike_on_every_arm_on_radv
  every_precomposed_letter_draws_alike_on_every_arm_on_radv
  italic_hebrew_draws_alike_on_every_arm_on_radv
  an_over_wide_letter_draws_alike_on_every_arm_on_radv
  presentation_variants_draw_alike_on_every_arm_on_radv
  a_syllable_draws_alike_on_every_arm_on_radv
  a_devanagari_word_and_its_lights_draw_alike_on_every_arm_on_radv
  bold_and_italic_hindi_draw_alike_on_every_arm_on_radv
  a_word_whose_face_changes_draws_alike_on_every_arm_on_radv
  an_undecorated_cursor_line_draws_by_rows_as_the_whole_frame_does_on_radv
  a_bell_tint_redraws_whole_on_a_reused_surface_on_radv
  a_cursor_only_cell_frame_draws_its_rows_and_the_whole_frames_pixels_on_radv
  the_cell_path_draws_the_notice_as_the_instance_path_does_on_radv
  a_reversed_default_page_is_raised_alike_on_every_arm_on_radv
  the_cell_path_draws_the_status_line_as_the_instance_path_does_on_radv
  the_message_pair_holds_on_every_arm_on_radv
  the_address_and_the_notice_take_no_colour_from_a_program_on_radv
  no_cursor_covers_an_address_on_any_arm_on_radv
  the_lines_and_the_looks_are_one_picture_on_every_arm_on_radv
  an_unhovered_line_is_amber_and_a_hovered_one_unchanged_on_every_arm_on_radv
  the_splash_draws_alike_on_every_arm_on_radv
  the_shell_is_drawn_in_front_of_the_splash_on_every_arm_on_radv
  a_target_shown_with_points_is_free_at_its_release_point_on_radv
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
