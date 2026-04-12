#ifndef ATERM_BRIDGE_H
#define ATERM_BRIDGE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct AtermCore AtermCore;
typedef void (*AtermDirtyCallback)(void* userdata);

// Event types for wakeup+drain pattern
#define ATERM_EVENT_CREATED          0
#define ATERM_EVENT_CLOSED           1
#define ATERM_EVENT_STATUS_CHANGED   2
#define ATERM_EVENT_TITLE_CHANGED    3
#define ATERM_EVENT_SHELL_READY      4
#define ATERM_EVENT_TRUST_PROMPT     5
#define ATERM_EVENT_BATCH_CLOSED     6
#define ATERM_EVENT_CREATION_FAILED  7
#define ATERM_EVENT_RESTORED         8

typedef struct AtermEventFFI {
    uint8_t event_type;
    const char* id;
    const char* name;
    const char* status;
    const char* title;
} AtermEventFFI;

typedef struct AtermEventBatch {
    AtermEventFFI* events;
    uint32_t count;
} AtermEventBatch;

// Named key codes
#define ATERM_KEY_ENTER       1
#define ATERM_KEY_BACKSPACE   2
#define ATERM_KEY_DELETE       3
#define ATERM_KEY_TAB         4
#define ATERM_KEY_ESCAPE      5
#define ATERM_KEY_ARROW_UP    6
#define ATERM_KEY_ARROW_DOWN  7
#define ATERM_KEY_ARROW_RIGHT 8
#define ATERM_KEY_ARROW_LEFT  9
#define ATERM_KEY_HOME        10
#define ATERM_KEY_END         11
#define ATERM_KEY_PAGE_UP     12
#define ATERM_KEY_PAGE_DOWN   13

// Cell data for Metal renderer — populated by aterm_core_get_render_data()
typedef struct CellDataFFI {
    uint16_t col;
    uint16_t row;
    uint8_t fg_r;
    uint8_t fg_g;
    uint8_t fg_b;
    uint8_t fg_a;
    uint8_t bg_r;
    uint8_t bg_g;
    uint8_t bg_b;
    uint8_t bg_a;
    uint32_t character;  // Unicode codepoint
    uint8_t flags;       // bold=1, italic=2, underline=4
} CellDataFFI;

// Lifecycle
AtermCore* aterm_core_new(void);
void aterm_core_stop(AtermCore* core);
void aterm_core_free(AtermCore* core);

// GPU init — pass NSView pointer
int32_t aterm_core_init_gpu(AtermCore* core, void* ns_view, uint32_t width, uint32_t height, float scale);

// PTY
int32_t aterm_core_spawn_shell(AtermCore* core, const char* name, const char* cwd, const char* command, uint16_t cols, uint16_t rows);
void aterm_core_write_pty(AtermCore* core, const char* text, size_t len);
int32_t aterm_core_workspace_is_alive(const AtermCore* core);
void aterm_core_named_key(AtermCore* core, uint32_t key_code);

// Rendering
void aterm_core_render(AtermCore* core);
int32_t aterm_core_try_render(AtermCore* core);
void aterm_core_resize(AtermCore* core, uint32_t width, uint32_t height);
void aterm_core_grid_size(const AtermCore* core, float width, float height, uint16_t* out_cols, uint16_t* out_rows);
void aterm_core_cell_size(const AtermCore* core, float* out_width, float* out_height);
void aterm_core_grid_padding(const AtermCore* core, float width, float height, float* out_pad_x, float* out_pad_y);
void aterm_core_cursor_position(const AtermCore* core, float width, float height, float* out_x, float* out_y);
void aterm_core_set_preedit(AtermCore* core, const uint8_t* text, uint32_t len);
void aterm_core_set_preedit_active(AtermCore* core, bool active);
void aterm_core_suspend_gpu(AtermCore* core);

// Metal renderer data — collect terminal grid cells (character + color + flags per cell)
int32_t aterm_core_get_render_data(AtermCore* core, CellDataFFI* out_cells, uint32_t max_cells, uint32_t* out_count, uint16_t* out_cols, uint16_t* out_rows, uint8_t* out_dirty_rows);

// Dirty state
int32_t aterm_core_take_dirty(AtermCore* core);
void aterm_core_set_dirty_callback(AtermCore* core, AtermDirtyCallback callback, void* userdata);
void aterm_core_sync_pty(AtermCore* core);
void aterm_core_set_theme_mode(AtermCore* core, uint8_t mode);

// Settings: color scheme, font size, line height
void aterm_core_set_color_scheme(AtermCore* core, uint8_t scheme);
// Get fg/bg RGB for a color scheme index (always available, no wgpu needed)
void aterm_core_scheme_fg_color(uint8_t scheme, uint8_t* out_r, uint8_t* out_g, uint8_t* out_b);
void aterm_core_scheme_bg_color(uint8_t scheme, uint8_t* out_r, uint8_t* out_g, uint8_t* out_b);
// Set default fg/bg colors used by render_cells for palette resolution
void aterm_core_set_default_colors(AtermCore* core, uint8_t fg_r, uint8_t fg_g, uint8_t fg_b, uint8_t bg_r, uint8_t bg_g, uint8_t bg_b);
void aterm_core_set_font_size(AtermCore* core, float size);
void aterm_core_set_line_height(AtermCore* core, float height);
void aterm_core_set_cell_width(AtermCore* core, float width);
// Deprecated compatibility no-op.
void aterm_core_set_bg_blend_threshold(AtermCore* core, float threshold);

// Scroll
void aterm_core_scroll(AtermCore* core, int32_t delta);
// Scroll to prompt (OSC 133): direction < 0 = up, > 0 = down. Returns 1 if scrolled.
int32_t aterm_core_scroll_to_prompt(AtermCore* core, int32_t direction);
uint32_t aterm_core_prompt_mark_count(const AtermCore* core);

// Selection
void aterm_core_selection_start(AtermCore* core, uint32_t col, int32_t line, uint8_t side);
void aterm_core_selection_update(AtermCore* core, uint32_t col, int32_t line, uint8_t side);
void aterm_core_selection_clear(AtermCore* core);
char* aterm_core_selection_text(const AtermCore* core);
void aterm_core_select_all(AtermCore* core);

// Selection ranges for Metal renderer — 12 bytes, matches Shaders.metal SelectionRange
typedef struct SelectionRangeFFI {
    uint16_t start_col;
    uint16_t start_row;
    uint16_t end_col;
    uint16_t end_row;
    uint8_t r;
    uint8_t g;
    uint8_t b;
    uint8_t a;
} SelectionRangeFFI;

// Export selection as per-row ranges for Metal selection overlay
void aterm_core_selection_ranges(const AtermCore* core, SelectionRangeFFI* out_ranges, uint32_t max_ranges, uint32_t* out_count);

// Screen text search
int32_t aterm_core_screen_contains(const AtermCore* core, const char* pattern);

// Workspace list (JSON string, caller must free)
char* aterm_core_list_workspaces(const AtermCore* core);
char* aterm_core_detect_clis(void);
void aterm_core_free_string(char* ptr);

// Session persistence
typedef struct SessionEntryFFI {
    const char *id;
    const char *cwd;
    const char *command;
    const char *args_json;
    const char *custom_command;
    bool is_system;
    const char *resume_command;
} SessionEntryFFI;

uint32_t aterm_session_count(const AtermCore* core);
SessionEntryFFI aterm_session_get(const AtermCore* core, uint32_t index);
void aterm_session_free(SessionEntryFFI entry);
void aterm_sessions_save(AtermCore* core);
uint32_t aterm_sessions_restore(AtermCore* core);

// Tailscale
int32_t aterm_tailscale_connect(const char* hostname, const char* control_url, const char* auth_key);
void aterm_tailscale_shutdown(void);
char* aterm_tailscale_status_json(void);

// IPC Phase 1 — AtermApp singleton
typedef struct {
    void* userdata;
    void (*create_workspace_view)(void* userdata, const char* id, const char* config_json);
    void (*close_workspace_view)(void* userdata, const char* id);
    void (*focus_workspace)(void* userdata, const char* id);
    void (*rename_workspace)(void* userdata, const char* old_name, const char* new_name);
    void (*send_key)(void* userdata, const char* workspace, const char* key);
    void (*attach_external_session)(void* userdata, const char* session_id);
    void (*reload_settings)(void* userdata);
    char* (*list_workspaces)(void* userdata);
    void (*on_events_available)(void* userdata);
    void (*request_redraw)(void* userdata);
} AtermHostCallbacks;

void aterm_set_host(AtermHostCallbacks callbacks);
char* aterm_dispatch(const char* action_json, size_t action_len);
void aterm_sync_telepty(void);
char* aterm_ipc_socket_path(void);
char* aterm_ipc_token(void);

// Wakeup+drain event API
AtermEventBatch aterm_drain_events(void);
void aterm_free_events(AtermEventBatch batch);

// Session lifecycle — explicit workspace close (deterministic, not ARC-dependent)
void aterm_workspace_close(const char *workspace_id);
void aterm_batch_close(const char *const *workspace_ids, uint32_t count);
void aterm_trigger_save(void);

// Mark workspace as system (orchestrator) in global session registry.
// Call after spawn_shell for isSystem workspaces so SaveCoordinator writes the correct flag.
void aterm_core_set_workspace_system(AtermCore *core, const char *workspace_name, bool is_system);

#endif
