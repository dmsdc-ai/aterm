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
void aterm_core_resize(AtermCore* core, uint32_t width, uint32_t height);
void aterm_core_grid_size(const AtermCore* core, float width, float height, uint16_t* out_cols, uint16_t* out_rows);
void aterm_core_cell_size(const AtermCore* core, float* out_width, float* out_height);

// Dirty state
int32_t aterm_core_take_dirty(AtermCore* core);
void aterm_core_set_dirty_callback(AtermCore* core, AtermDirtyCallback callback, void* userdata);
void aterm_core_sync_pty(AtermCore* core);
void aterm_core_set_theme_mode(AtermCore* core, uint8_t mode);

// Settings: color scheme, font size, line height
void aterm_core_set_color_scheme(AtermCore* core, uint8_t scheme);
void aterm_core_set_font_size(AtermCore* core, float size);
void aterm_core_set_line_height(AtermCore* core, float height);
// Deprecated compatibility no-op.
void aterm_core_set_bg_blend_threshold(AtermCore* core, float threshold);

// Scroll
void aterm_core_scroll(AtermCore* core, int32_t delta);

// Selection
void aterm_core_selection_start(AtermCore* core, uint32_t col, int32_t line, uint8_t side);
void aterm_core_selection_update(AtermCore* core, uint32_t col, int32_t line, uint8_t side);
void aterm_core_selection_clear(AtermCore* core);
char* aterm_core_selection_text(const AtermCore* core);

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

#endif
