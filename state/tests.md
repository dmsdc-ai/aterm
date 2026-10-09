# aterm Test Inventory

Generated from `ATERM_TELEPTY_PORT=9 cargo test --offline --workspace -- --list` (measured, not hand-counted).

## Run Commands

```bash
ATERM_TELEPTY_PORT=9 cargo test --workspace --locked           # Everything (CI runs this)
ATERM_TELEPTY_PORT=9 cargo test -p aterm-core --lib             # aterm-core unit tests
ATERM_TELEPTY_PORT=9 cargo test -p aterm-core --test cli_dispatch  # bin/aterm CLI against a fake socket
ATERM_TELEPTY_PORT=9 cargo test -p aterm-ipc --lib              # IPC server tests
ATERM_TELEPTY_PORT=9 cargo test -p aterm-core --lib -- inject::tests   # Inject module only
```

`.cargo/config.toml` sets `ATERM_HERMETIC=1`; tests never touch the production telepty port.

## Count per file

| File | Tests |
|------|------:|
| `aterm-core/src/app.rs` | 10 |
| `aterm-core/src/lib.rs` | 4 |
| `aterm-core/src/inject.rs` | 27 |
| `aterm-core/src/pty.rs` | 24 |
| `aterm-core/src/telepty_bridge.rs` | 12 |
| `aterm-core/tests/cli_dispatch.rs` | 15 |
| `aterm-core/tests/hermetic_guard.rs` | 1 |
| `aterm-core/tests/telepty_no_restart.rs` | 1 |
| `aterm-ipc/src/server.rs` | 3 |
| **Total** | **97** |

## aterm-core/src/app.rs

| Test Name | Purpose |
|-----------|---------|
| app::tests::create_workspace_rejects_existing_name | CreateWorkspace returns an "already exists" Error for a registered name (I2) |
| app::tests::create_workspace_unknown_name_without_host_is_unsupported | CreateWorkspace for a new name without a platform host is Unsupported |
| app::tests::deregister_clears_report | Deregister drops the completion report; a reused name starts without it |
| app::tests::mark_complete_dead_workspace_errors | MarkComplete on a dead workspace returns Error |
| app::tests::mark_complete_sets_state_and_report | MarkComplete sets state "complete" and stores the report |
| app::tests::mark_complete_unknown_workspace_errors | MarkComplete on an unknown workspace returns Error |
| app::tests::mark_complete_wakes_condvar_waiter | MarkComplete wakes a thread blocked in wait_for_state |
| app::tests::report_truncated_to_4096_bytes_on_char_boundary | Completion report is capped at 4096 bytes on a UTF-8 char boundary |
| app::tests::wait_for_state_returns_early_on_terminal_state | wait_for_state returns before the timeout once a terminal state is reached |
| app::tests::wait_for_state_times_out | wait_for_state returns after the timeout when the state never changes |

## aterm-core/src/lib.rs

| Test Name | Purpose |
|-----------|---------|
| ffi_tests::all_ffi_entry_points_wrapped_in_catch_unwind | Source grep verifies every #[no_mangle] FFI entry point is panic-guarded |
| ffi_tests::aterm_core_resize_does_not_panic_on_very_large_size | Resize FFI boundary tolerates extremely large dimensions without panicking |
| ffi_tests::aterm_core_resize_does_not_panic_on_zero_size | Resize FFI boundary tolerates zero dimensions without panicking |
| ffi_tests::bridge_header_matches_exports | Names declared in macos/aterm-bridge.h == #[no_mangle] exports in lib.rs |

## aterm-core/src/inject.rs

| Test Name | Purpose |
|-----------|---------|
| inject::tests::detect_osc133_all_marks | Detects all 4 OSC 133 mark types (A/B/C/D) in sequence |
| inject::tests::detect_osc133_bel_terminated | Parses BEL-terminated (\x07) OSC 133 sequences |
| inject::tests::detect_osc133_d_with_exit_status | Parses OSC 133 D with an exit status |
| inject::tests::detect_osc133_d_without_exit_status | Parses OSC 133 D without an exit status |
| inject::tests::detect_osc133_no_marks_in_plain_text | No false positives on plain text or ANSI colors |
| inject::tests::detect_osc133_st_terminated | Parses ST-terminated (\x1b\\) OSC 133 sequences |
| inject::tests::detect_osc133_unterminated_ignored | Ignores incomplete/unterminated OSC 133 sequences |
| inject::tests::force_inject_not_triggered_for_fresh_message | Fresh queue messages don't trigger force-inject timeout |
| inject::tests::force_inject_timeout_constant_is_30s | Asserts FORCE_INJECT_TIMEOUT == 30s |
| inject::tests::has_prompt_pattern_detects_ansi_wrapped_prompt | Prompt detection works through ANSI escape wrappers |
| inject::tests::heuristic_prompt_requires_output_settle | Heuristic prompts require OUTPUT_SETTLE delay before inject |
| inject::tests::idle_state_requires_all_conditions_for_inject | Inject needs: prompt detected + idle threshold + output settled + queue non-empty |
| inject::tests::inject_queue_fifo_order | Queue pops messages in FIFO order |
| inject::tests::inject_queue_oldest_enqueued_at_tracks_front | oldest_enqueued_at() returns front message timestamp |
| inject::tests::inject_queue_push_returns_length | push() returns Ok(current_length) |
| inject::tests::inject_queue_rejects_when_full | push() returns Err when the queue reaches INJECT_QUEUE_CAPACITY (20) |
| inject::tests::inject_signal_empty_queue_waits_for_timeout | Signal wait_timeout blocks for ~timeout when not notified |
| inject::tests::inject_signal_notify_before_wait_still_wakes | Pre-notified signal returns true immediately on wait |
| inject::tests::inject_signal_wait_timeout_returns_false_without_notify | wait_timeout returns false when not notified |
| inject::tests::inject_signal_wait_timeout_returns_true_on_notify | wait_timeout returns true when notified before timeout |
| inject::tests::inject_signal_wakes_immediately_on_notify | Condvar signal wakes in <200ms (not 500ms polling) |
| inject::tests::osc133_not_downgraded_by_heuristic | OSC 133 source not overwritten by subsequent heuristic output |
| inject::tests::osc133_prompt_skips_output_settle | OSC 133 prompts bypass OUTPUT_SETTLE wait |
| inject::tests::prompt_detection_stays_latched_until_user_input | Prompt flag persists through non-prompt output, clears on user input |
| inject::tests::record_user_input_clears_osc133 | User input clears OSC 133 prompt state |
| inject::tests::reset_prompt_clears_source | reset_prompt() clears both flag and source |
| inject::tests::throttle_coalesce_is_25ms | Asserts THROTTLE_COALESCE == 25ms |

## aterm-core/src/pty.rs

| Test Name | Purpose |
|-----------|---------|
| pty::tests::mark_workspace_dead_closes_writer_and_clears_queue | Dead marking closes writer handle and clears inject queue |
| pty::tests::pty_eof_no_polling_timer_involved | Dead marking is near-instant (<500ms), no timer cadence |
| pty::tests::pty_eof_notifies_condvar_waiters | EOF/dead state notifies Condvar waiters |
| pty::tests::pty_eof_sets_status_to_dead | EOF sets workspace status to "dead" |
| pty::tests::pty_output_signal_can_be_rearmed | Output signal can be reset and re-triggered |
| pty::tests::pty_output_signal_coalesces_until_taken | Multiple output signals coalesce into one |
| pty::tests::shell_ready_bare_prompt_fires_immediately | Bare prompt char triggers shell ready immediately |
| pty::tests::shell_ready_constants | Asserts shell ready timeout/threshold constants |
| pty::tests::shell_ready_fallback_still_works_for_bootstrap | Fallback timeout still triggers shell ready |
| pty::tests::shell_ready_fallback_timeout_fires_without_prompt | Ready fires on timeout when no prompt detected |
| pty::tests::shell_ready_fires_only_once | Shell ready callback fires exactly once |
| pty::tests::shell_ready_fish_nushell_trailing_newline | Handles fish/nushell trailing newline prompts |
| pty::tests::shell_ready_immediate_trigger_enables_fast_bootstrap | Prompt detection enables fast (<100ms) bootstrap |
| pty::tests::shell_ready_large_chunk_with_multibyte_utf8 | Handles large chunks with multibyte UTF-8 |
| pty::tests::shell_ready_large_chunk_with_prompt_at_end | Detects prompt at end of large output chunk |
| pty::tests::shell_ready_large_chunk_without_prompt | Large chunk without prompt doesn't false-trigger |
| pty::tests::shell_ready_prompt_fires_even_after_non_prompt_output | Prompt detected after non-prompt output still fires |
| pty::tests::shell_ready_prompt_fires_immediately | Prompt pattern triggers shell ready without delay |
| pty::tests::trust_prompt_buffer_respects_utf8_boundaries | Trust prompt rolling buffer respects UTF-8 boundaries |
| pty::tests::trust_prompt_detected_on_matching_pattern | Detects trust prompt patterns (Do you trust...) |
| pty::tests::trust_prompt_fires_only_once | Trust prompt callback fires exactly once |
| pty::tests::trust_prompt_non_matching_does_not_fire | Non-matching output doesn't trigger trust prompt |
| pty::tests::trust_prompt_pattern_split_across_feeds | Trust prompt detected when pattern spans multiple feed() calls |
| pty::tests::trust_prompt_rolling_buffer_trims_to_limit | Rolling buffer stays within size limit |

## aterm-core/src/telepty_bridge.rs

| Test Name | Purpose |
|-----------|---------|
| telepty_bridge::tests::auth_header_reaches_the_daemon | The telepty auth header is on the wire, not merely compiled in |
| telepty_bridge::tests::hermetic_guards_still_in_source | Source assertion: the production port appears once and the hermetic guards remain |
| telepty_bridge::tests::hermetic_is_armed_in_this_binary | TeleptyBridge::hermetic() is true inside cargo test |
| telepty_bridge::tests::missing_or_malformed_config_degrades_to_none | Absent or malformed telepty config degrades to no token, never panics |
| telepty_bridge::tests::production_port_never_resolves_under_test | The production telepty port is unresolvable from a test process |
| telepty_bridge::tests::stale_ids_ignores_non_aterm | Stale cleanup ignores non-aterm telepty sessions |
| telepty_bridge::tests::stale_ids_keeps_known_name | Stale cleanup keeps this instance's live workspace names |
| telepty_bridge::tests::stale_ids_keeps_other_live_instance | Stale cleanup keeps sessions of another live aterm instance (I7) |
| telepty_bridge::tests::stale_ids_removes_dead_socket | Stale cleanup removes sessions whose socket no longer accepts connections |
| telepty_bridge::tests::stale_ids_removes_legacy_without_address | Stale cleanup removes legacy entries with no delivery address |
| telepty_bridge::tests::stale_ids_removes_own_unknown_name | Stale cleanup removes this instance's sessions with unknown names |
| telepty_bridge::tests::token_read_from_config_file | The telepty token is read from the config file |

## aterm-core/tests/cli_dispatch.rs

| Test Name | Purpose |
|-----------|---------|
| dispatch_closes_inject_failed_subs | dispatch: a sub whose Inject gets no reply is reported inject_failed and closed, with no WaitUntil (B2 F6) |
| dispatch_create_unsupported_skips_inject | dispatch: Unsupported create reports create_failed with no Inject/WaitUntil/Close |
| dispatch_names_carry_run_id | dispatch: sub-session names are `dispatch-<task>-<run id>-sub<i>` (B2 I2b) |
| dispatch_timeout_leaves_session_running | dispatch: a timed-out sub-session is left running |
| dispatch_wait_error_reports_dead_not_left_running | dispatch: a WaitUntil error reports dead, not left_running |
| dispatch_waits_for_complete_and_closes_completed | dispatch: waits for complete and closes completed sub-sessions |
| done_sends_mark_complete_with_workspace_and_report | aterm done sends MarkComplete with the workspace and report |
| export_unsupported_writes_no_file | export: an Unsupported ReadScreenText reply exits 2 and writes no file (B2 I3) |
| help_has_no_hardcoded_installed_block | help (en, ko) has no hard-coded "already installed" ecosystem block (B2 R14) |
| lessons_add_is_atomic | lessons add replaces the file via temp + rename: new inode, no .tmp- left (B2 F5) |
| subscribe_help_lists_emitted_types | help (en, ko) subscribe row lists the five emitted event types, no TitleChanged (B2) |
| tasks_add_ids_unique_and_atomic | tasks add assigns unique ids and writes the board atomically (no .tmp left) |
| tasks_add_replaces_inode | tasks add replaces the board via temp + rename: new inode, no .tmp- left (B2 F5) |
| tasks_workspace_flag_uses_list_tasks | tasks --workspace sends ListTasks and leaves the local board untouched |
| unsupported_reply_is_an_error | An Unsupported IPC reply exits 2 with an error on stderr, never "ok" (B2 I3) |

## aterm-core/tests/hermetic_guard.rs

| Test Name | Purpose |
|-----------|---------|
| hermetic_env_reaches_integration_tests | ATERM_HERMETIC from .cargo/config.toml reaches integration tests |

## aterm-core/tests/telepty_no_restart.rs

| Test Name | Purpose |
|-----------|---------|
| version_mismatch_never_restarts_the_daemon | A telepty version mismatch never restarts the daemon |

## aterm-ipc/src/server.rs

| Test Name | Purpose |
|-----------|---------|
| server::tests::closed_peer_evicted_after_three_failures | A closed subscriber is evicted after three failed writes |
| server::tests::filtered_subscriber_survives_quiet_period | A subscriber whose filter skips events survives a quiet period (I4) |
| server::tests::quiet_subscriber_still_receives_event | A subscriber that has been quiet still receives the next event (I4) |

---

*Total: 97 tests (10 app.rs + 4 lib.rs + 27 inject.rs + 24 pty.rs + 12 telepty_bridge.rs + 15 cli_dispatch.rs + 1 hermetic_guard.rs + 1 telepty_no_restart.rs + 3 server.rs)*
*Regenerated 2026-10-09 from `cargo test -- --list` (task 1209 B7); the 18 standalone Condvar tests that never imported aterm_core were deleted (R1). The 8 B2 `cli_dispatch` rows were added from the source `#[test]` count; re-confirm with `cargo test -- --list`.*
