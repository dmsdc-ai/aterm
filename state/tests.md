# aterm Test Inventory

## Run Commands

```bash
cargo test -p aterm-core          # All aterm-core tests (unit + integration)
cargo test -p aterm-ipc           # IPC crate tests
cargo test -p aterm-session       # Session crate tests
cargo test -p aterm-core -- inject::tests   # Inject module only
cargo test -p aterm-core -- pty::tests      # PTY module only
cargo test -p aterm-core -- renderer::tests # Renderer module only
cargo test -p aterm-core -- ffi_tests::     # FFI crash-safety tests
```

## aterm-core — inject::tests

| Test Name | Suite | Purpose |
|-----------|-------|---------|
| detect_osc133_all_marks | inject | Detects all 4 OSC 133 mark types (A/B/C/D) in sequence |
| detect_osc133_bel_terminated | inject | Parses BEL-terminated (\x07) OSC 133 sequences |
| detect_osc133_no_marks_in_plain_text | inject | No false positives on plain text or ANSI colors |
| detect_osc133_st_terminated | inject | Parses ST-terminated (\x1b\\) OSC 133 sequences |
| detect_osc133_unterminated_ignored | inject | Ignores incomplete/unterminated OSC 133 sequences |
| force_inject_not_triggered_for_fresh_message | inject | Fresh queue messages don't trigger force-inject timeout |
| force_inject_timeout_constant_is_30s | inject | Asserts FORCE_INJECT_TIMEOUT == 30s |
| has_prompt_pattern_detects_ansi_wrapped_prompt | inject | Prompt detection works through ANSI escape wrappers |
| heuristic_prompt_requires_output_settle | inject | Heuristic prompts require OUTPUT_SETTLE delay before inject |
| idle_state_requires_all_conditions_for_inject | inject | Inject needs: prompt detected + idle threshold + output settled + queue non-empty |
| inject_queue_fifo_order | inject | Queue pops messages in FIFO order |
| inject_queue_oldest_enqueued_at_tracks_front | inject | oldest_enqueued_at() returns front message timestamp |
| inject_queue_push_returns_length | inject | push() returns Ok(current_length) |
| inject_queue_rejects_when_full | inject | push() returns Err when queue reaches 256 capacity |
| inject_signal_empty_queue_waits_for_timeout | inject | Signal wait_timeout blocks for ~timeout when not notified |
| inject_signal_notify_before_wait_still_wakes | inject | Pre-notified signal returns true immediately on wait |
| inject_signal_wait_timeout_returns_false_without_notify | inject | wait_timeout returns false when not notified |
| inject_signal_wait_timeout_returns_true_on_notify | inject | wait_timeout returns true when notified before timeout |
| inject_signal_wakes_immediately_on_notify | inject | Condvar signal wakes in <200ms (not 500ms polling) |
| osc133_not_downgraded_by_heuristic | inject | OSC 133 source not overwritten by subsequent heuristic output |
| osc133_prompt_skips_output_settle | inject | OSC 133 prompts bypass OUTPUT_SETTLE wait |
| prompt_detection_stays_latched_until_user_input | inject | Prompt flag persists through non-prompt output, clears on user input |
| record_user_input_clears_osc133 | inject | User input clears OSC 133 prompt state |
| reset_prompt_clears_source | inject | reset_prompt() clears both flag and source |
| throttle_coalesce_is_25ms | inject | Asserts THROTTLE_COALESCE == 25ms |

## aterm-core — pty::tests

| Test Name | Suite | Purpose |
|-----------|-------|---------|
| mark_workspace_dead_closes_writer_and_clears_queue | pty | Dead marking closes writer handle and clears inject queue |
| pty_eof_no_polling_timer_involved | pty | Dead marking is near-instant (<500ms), no timer cadence |
| pty_eof_notifies_condvar_waiters | pty | EOF/dead state notifies Condvar waiters |
| pty_eof_sets_status_to_dead | pty | EOF sets workspace status to "dead" |
| pty_output_signal_can_be_rearmed | pty | Output signal can be reset and re-triggered |
| pty_output_signal_coalesces_until_taken | pty | Multiple output signals coalesce into one |
| shell_ready_bare_prompt_fires_immediately | pty | Bare prompt char triggers shell ready immediately |
| shell_ready_constants | pty | Asserts shell ready timeout/threshold constants |
| shell_ready_fallback_still_works_for_bootstrap | pty | Fallback timeout still triggers shell ready |
| shell_ready_fallback_timeout_fires_without_prompt | pty | Ready fires on timeout when no prompt detected |
| shell_ready_fires_only_once | pty | Shell ready callback fires exactly once |
| shell_ready_fish_nushell_trailing_newline | pty | Handles fish/nushell trailing newline prompts |
| shell_ready_immediate_trigger_enables_fast_bootstrap | pty | Prompt detection enables fast (<100ms) bootstrap |
| shell_ready_large_chunk_with_multibyte_utf8 | pty | Handles large chunks with multibyte UTF-8 |
| shell_ready_large_chunk_with_prompt_at_end | pty | Detects prompt at end of large output chunk |
| shell_ready_large_chunk_without_prompt | pty | Large chunk without prompt doesn't false-trigger |
| shell_ready_prompt_fires_even_after_non_prompt_output | pty | Prompt detected after non-prompt output still fires |
| shell_ready_prompt_fires_immediately | pty | Prompt pattern triggers shell ready without delay |
| trust_prompt_buffer_respects_utf8_boundaries | pty | Trust prompt rolling buffer respects UTF-8 boundaries |
| trust_prompt_detected_on_matching_pattern | pty | Detects trust prompt patterns (Do you trust...) |
| trust_prompt_fires_only_once | pty | Trust prompt callback fires exactly once |
| trust_prompt_non_matching_does_not_fire | pty | Non-matching output doesn't trigger trust prompt |
| trust_prompt_pattern_split_across_feeds | pty | Trust prompt detected when pattern spans multiple feed() calls |
| trust_prompt_rolling_buffer_trims_to_limit | pty | Rolling buffer stays within size limit |

## aterm-core — integration tests (polling_event_tests.rs)

| Test Name | Suite | Purpose |
|-----------|-------|---------|
| create_workspace_blocks_until_registered | integration | create_workspace blocks until workspace is registered |
| create_workspace_no_sleep_2_in_ready_detection | integration | Ready detection is faster than old 2s sleep |
| create_workspace_returns_after_shell_running | integration | Returns when shell status becomes "running" |
| create_workspace_returns_immediately_if_already_registered | integration | No block if workspace already exists |
| create_workspace_timeout_after_15s_if_never_registered | integration | 15s timeout when workspace never registers |
| create_workspace_timeout_returns_after_specified_duration | integration | Custom timeout is respected |
| ipc_event_broadcast_closed_event | integration | IPC broadcasts "closed" events to subscribers |
| ipc_event_broadcast_status_changed | integration | IPC broadcasts status change events |
| ipc_event_broadcast_to_subscribers | integration | IPC event fanout to multiple subscribers |
| ipc_subscriber_disconnect_detected_on_write_failure | integration | Detects and cleans up disconnected subscribers |
| stale_cleanup_fires_once_on_closed_event | integration | Stale workspace cleanup triggers on close, once |
| stale_cleanup_not_repeating_timer | integration | Cleanup is event-driven, not timer-based |
| task_queue_file_change_detected | integration | Detects task-queue.json file changes |
| task_queue_reload_reads_updated_content | integration | Reloads task queue content after file change |
| wait_until_blocks_until_workspace_reaches_target_state | integration | wait_until blocks until target status reached |
| wait_until_returns_immediately_if_already_in_target_state | integration | No block if already in target state |
| wait_until_status_change_notifies_all_waiters | integration | Status change wakes all waiting threads |
| wait_until_timeout_returns_after_specified_duration | integration | wait_until respects timeout parameter |

## aterm-core — renderer::tests

| Test Name | Suite | Purpose |
|-----------|-------|---------|
| aterm_core_render_does_not_panic_on_empty_grid | renderer | Render-path batching handles empty grid without panicking |
| aterm_core_render_does_not_panic_on_mismatched_grid_dimensions | renderer | Render-path batching tolerates grid/dimension mismatch without panicking |
| glyph_bitmap_padding_matches_cell_dimensions | renderer | Rasterized glyph uploads are padded to exactly one cell's pixel dimensions |
| atlas_clear_triggers_glyph_cache_clear_no_stale_glyphs | renderer | Atlas clear path also drops cached glyph metadata so stale entries cannot survive |
| font_size_change_resets_both_atlas_and_glyph_cache | renderer | Font-size changes clear both atlas pages and cached glyph lookups before re-rasterizing |
| ascii_preload_uses_cell_dimensions | renderer | ASCII preload populates atlas entries using the current cell width and height |
| cjk_wide_char_padding_matches_two_cell_width | renderer | Wide CJK glyph uploads expand to two cell widths while keeping one cell height |
| damage_tracker_full_damage_processes_all_lines | renderer | Full damage marks every visual row dirty |
| damage_tracker_is_line_damaged_returns_false_for_unchanged_lines | renderer | Unchanged rows remain skipped in partial damage mode |
| damage_tracker_partial_damage_skips_unchanged_lines | renderer | Partial damage marks only changed rows dirty |
| run_based_batching_handles_empty_line | renderer | Empty lines produce no text runs |
| run_based_batching_handles_line_with_all_different_attributes | renderer | Different per-cell attributes split runs at each cell |
| run_based_batching_handles_single_character_line | renderer | Single-character lines produce exactly one run |
| run_based_batching_produces_fewer_text_areas_than_per_cell | renderer | Consecutive same-color cells batch into fewer text areas than per-cell rendering |

## aterm-core — ffi_tests

| Test Name | Suite | Purpose |
|-----------|-------|---------|
| all_ffi_entry_points_wrapped_in_catch_unwind | ffi | Source grep verifies every #[no_mangle] FFI entry point is panic-guarded |
| aterm_core_resize_does_not_panic_on_very_large_size | ffi | Resize FFI boundary tolerates extremely large dimensions without panicking |
| aterm_core_resize_does_not_panic_on_zero_size | ffi | Resize FFI boundary tolerates zero dimensions without panicking |

---

*Total: 84 tests (25 inject + 24 pty + 18 integration + 14 renderer + 3 ffi)*
*Generated: 2026-04-03*
