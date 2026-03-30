#ifndef ATERM_BRIDGE_H
#define ATERM_BRIDGE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct AtermCore AtermCore;
typedef void (*AtermDirtyCallback)(void* userdata);

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
void aterm_core_free(AtermCore* core);

// GPU init — pass NSView pointer
int32_t aterm_core_init_gpu(AtermCore* core, void* ns_view, uint32_t width, uint32_t height, float scale);

// PTY
int32_t aterm_core_spawn_shell(AtermCore* core, const char* cwd, uint16_t cols, uint16_t rows);
void aterm_core_write_pty(AtermCore* core, const char* text, size_t len);
void aterm_core_named_key(AtermCore* core, uint32_t key_code);

// Rendering
void aterm_core_render(AtermCore* core);
void aterm_core_resize(AtermCore* core, uint32_t width, uint32_t height);
void aterm_core_grid_size(const AtermCore* core, float width, float height, uint16_t* out_cols, uint16_t* out_rows);

// Dirty state
int32_t aterm_core_take_dirty(AtermCore* core);
void aterm_core_set_dirty_callback(AtermCore* core, AtermDirtyCallback callback, void* userdata);
void aterm_core_sync_pty(AtermCore* core);

// Scroll
void aterm_core_scroll(AtermCore* core, int32_t delta);

// Selection
void aterm_core_selection_start(AtermCore* core, uint32_t col, int32_t line, uint8_t side);
void aterm_core_selection_update(AtermCore* core, uint32_t col, int32_t line, uint8_t side);
void aterm_core_selection_clear(AtermCore* core);
char* aterm_core_selection_text(const AtermCore* core);

// Workspace list (JSON string, caller must free)
char* aterm_core_list_workspaces(const AtermCore* core);
char* aterm_core_detect_clis(void);
void aterm_core_free_string(char* ptr);

// Tailscale
int32_t aterm_tailscale_connect(const char* hostname, const char* control_url, const char* auth_key);
void aterm_tailscale_shutdown(void);
char* aterm_tailscale_status_json(void);

#endif
