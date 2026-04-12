#ifndef ATERM_CORE_H
#define ATERM_CORE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#define ATERM_KEY_ENTER 1

#define ATERM_KEY_BACKSPACE 2

#define ATERM_KEY_DELETE 3

#define ATERM_KEY_TAB 4

#define ATERM_KEY_ESCAPE 5

#define ATERM_KEY_ARROW_UP 6

#define ATERM_KEY_ARROW_DOWN 7

#define ATERM_KEY_ARROW_RIGHT 8

#define ATERM_KEY_ARROW_LEFT 9

#define ATERM_KEY_HOME 10

#define ATERM_KEY_END 11

#define ATERM_KEY_PAGE_UP 12

#define ATERM_KEY_PAGE_DOWN 13

#define ATERM_EVENT_CREATED 0

#define ATERM_EVENT_CLOSED 1

#define ATERM_EVENT_STATUS_CHANGED 2

#define ATERM_EVENT_TITLE_CHANGED 3

#define ATERM_EVENT_SHELL_READY 4

#define ATERM_EVENT_TRUST_PROMPT 5

#define BUFFER_MAX_BYTES (1024 * 1024)

#define DEFAULT_SNAPSHOT_BYTES (256 * 1024)

typedef struct AtermCore AtermCore;

/**
 * Per-cell terminal data for Metal renderer — populated by aterm_core_get_render_data().
 * Swift allocates a flat array of these and passes it to the Metal render pass.
 */
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
  uint32_t character;
  uint8_t flags;
} CellDataFFI;

/**
 * C-safe workspace event — flat struct, unused fields are NULL.
 */
typedef struct AtermEventFFI {
  uint8_t event_type;
  const char *id;
  const char *name;
  const char *status;
  const char *title;
} AtermEventFFI;

/**
 * Batch of events returned by aterm_drain_events(). Caller frees with aterm_free_events().
 */
typedef struct AtermEventBatch {
  struct AtermEventFFI *events;
  uint32_t count;
} AtermEventBatch;

/**
 * Per-row selection range for Metal renderer — 12 bytes, matches Shaders.metal SelectionRange.
 * Each range covers one row of selected cells (multi-line selections split into per-row ranges).
 */
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

typedef struct SessionEntryFFI {
  const char *id;
  const char *cwd;
  const char *command;
  const char *args_json;
  const char *custom_command;
  bool is_system;
  const char *resume_command;
} SessionEntryFFI;

/**
 * C callback table for PlatformHost trait.
 */
typedef struct AtermHostCallbacks {
  void *userdata;
  void (*create_workspace_view)(void*, const char*, const char*);
  void (*close_workspace_view)(void*, const char*);
  void (*focus_workspace)(void*, const char*);
  void (*rename_workspace)(void*, const char*, const char*);
  void (*send_key)(void*, const char*, const char*);
  void (*attach_external_session)(void*, const char*);
  void (*reload_settings)(void*);
  char *(*list_workspaces)(void*);
  void (*on_events_available)(void*);
  void (*request_redraw)(void*);
} AtermHostCallbacks;

typedef struct CoreTextGlyphResult {
  uint8_t *bitmap;
  uint32_t width;
  uint32_t height;
  int32_t xmin;
  int32_t ymin;
  float advance_width;
  float ascent;
  float descent;
} CoreTextGlyphResult;

struct AtermCore *aterm_core_new(void);

void aterm_core_free(struct AtermCore *core);

/**
 * Suspend GPU resources for inactive workspace (#208 memory optimization).
 * Drops Surface + renderer caches. Terminal state preserved. Call resume to reactivate.
 */
void aterm_core_suspend_gpu(struct AtermCore *core);

/**
 * No-op stub when wgpu feature is disabled (Metal renderer handles GPU lifecycle).
 */
void aterm_core_suspend_gpu(struct AtermCore *_core);

/**
 * Stop the PTY output signal callback. Must be called BEFORE aterm_core_free
 * to prevent use-after-free when the host view is deallocated.
 */
void aterm_core_stop(struct AtermCore *core);

int32_t aterm_core_init_gpu(struct AtermCore *core,
                            void *ns_view,
                            uint32_t width,
                            uint32_t height,
                            float scale);

/**
 * No-wgpu init: skip GPU setup but create Terminal state (alacritty_terminal is GPU-independent).
 * Without this, c.terminal stays None and get_render_data returns empty.
 */
int32_t aterm_core_init_gpu(struct AtermCore *core,
                            void *_ns_view,
                            uint32_t width,
                            uint32_t height,
                            float _scale);

int32_t aterm_core_spawn_shell(struct AtermCore *core,
                               const char *name,
                               const char *cwd,
                               const char *command,
                               uint16_t cols,
                               uint16_t rows);

void aterm_core_write_pty(struct AtermCore *core, const char *text, uintptr_t len);

int32_t aterm_core_workspace_is_alive(const struct AtermCore *core);

void aterm_core_named_key(struct AtermCore *core, uint32_t key_code);

/**
 * Render — acquires render_lock with brief spin (max 8ms) for direct UI calls
 * (mouseDown, scroll, theme change). Skips if lock cannot be acquired in time.
 */
void aterm_core_render(struct AtermCore *core);

/**
 * No-op stub when wgpu feature is disabled (Metal renderer on Swift side).
 */
void aterm_core_render(struct AtermCore *_core);

/**
 * Try to render if dirty. Returns 1 if rendered, 0 if skipped.
 * Thread-safe — used by CVDisplayLink and PTY dirty callback for immediate
 * render without CVDisplayLink latency (Ghostty/Alacritty pattern, Fix #153).
 */
int32_t aterm_core_try_render(struct AtermCore *core);

/**
 * No-op stub when wgpu feature is disabled.
 */
int32_t aterm_core_try_render(struct AtermCore *_core);

void aterm_core_resize(struct AtermCore *core, uint32_t width, uint32_t height);

void aterm_core_grid_size(const struct AtermCore *core,
                          float width,
                          float height,
                          uint16_t *out_cols,
                          uint16_t *out_rows);

/**
 * Compute grid size from pixel dimensions (no-wgpu: uses hardcoded cell metrics).
 */
void aterm_core_grid_size(const struct AtermCore *_core,
                          float width,
                          float height,
                          uint16_t *out_cols,
                          uint16_t *out_rows);

/**
 * Set IME preedit text for inline rendering (#206). Empty string clears.
 */
void aterm_core_set_preedit(struct AtermCore *core, const uint8_t *text, uint32_t len);

/**
 * Get cursor position in backing pixels (for IME popup placement, #206).
 */
void aterm_core_cursor_position(const struct AtermCore *core,
                                float width,
                                float height,
                                float *out_x,
                                float *out_y);

/**
 * Compute cursor position from terminal grid (no-wgpu: uses hardcoded cell metrics).
 */
void aterm_core_cursor_position(const struct AtermCore *core,
                                float width,
                                float height,
                                float *out_x,
                                float *out_y);

void aterm_core_cell_size(const struct AtermCore *core, float *out_width, float *out_height);

/**
 * Return cell dimensions (no-wgpu: uses runtime value from set_line_height, or default).
 */
void aterm_core_cell_size(const struct AtermCore *_core, float *out_width, float *out_height);

void aterm_core_grid_padding(const struct AtermCore *core,
                             float width,
                             float height,
                             float *out_pad_x,
                             float *out_pad_y);

/**
 * Compute centered grid padding (no-wgpu: uses hardcoded cell metrics).
 */
void aterm_core_grid_padding(const struct AtermCore *_core,
                             float width,
                             float height,
                             float *out_pad_x,
                             float *out_pad_y);

int32_t aterm_core_take_dirty(struct AtermCore *core);

void aterm_core_set_dirty_callback(struct AtermCore *core, void (*callback)(void*), void *userdata);

void aterm_core_sync_pty(struct AtermCore *core);

/**
 * Collect terminal grid data for Metal renderer.
 * Swift calls this each frame; Rust locks the terminal grid and fills
 * out_cells[0..out_count] with per-cell character + color + flags.
 * PTY advance happens on the reader thread — this is a pure read.
 * Colors are resolved using the Tokyo Night Dark palette. Returns 0 on success, -1 on error.
 */
int32_t aterm_core_get_render_data(struct AtermCore *core,
                                   struct CellDataFFI *out_cells,
                                   uint32_t max_cells,
                                   uint32_t *out_count,
                                   uint16_t *out_cols,
                                   uint16_t *out_rows,
                                   uint8_t *out_dirty_rows);

/**
 * Drain all pending workspace events as a C struct batch.
 * Caller must free the returned batch with aterm_free_events().
 */
struct AtermEventBatch aterm_drain_events(void);

/**
 * Free a batch returned by aterm_drain_events().
 */
void aterm_free_events(struct AtermEventBatch batch);

void aterm_core_set_theme_mode(struct AtermCore *core, uint8_t mode);

/**
 * Store theme mode when wgpu feature is disabled (used by get_render_data).
 */
void aterm_core_set_theme_mode(struct AtermCore *core, uint8_t mode);

/**
 * Set color scheme: 0=Dark, 1=Light, 2=SolarizedDark, 3=SolarizedLight,
 * 4=Monokai, 5=Dracula, 6=Nord, 7=TokyoNight
 */
void aterm_core_set_color_scheme(struct AtermCore *core, uint8_t scheme);

void aterm_core_set_color_scheme(struct AtermCore *_core, uint8_t scheme);

/**
 * Set preedit (IME composition) active state.
 * When active, cursor cell INVERSE is suppressed so preedit text is visible.
 */
void aterm_core_set_preedit_active(struct AtermCore *_core, bool active);

/**
 * Set default fg/bg colors used by render_cells for palette Background/Foreground.
 * Also syncs colors to terminal listener for OSC 10/11 query responses.
 */
void aterm_core_set_default_colors(struct AtermCore *core,
                                   uint8_t fg_r,
                                   uint8_t fg_g,
                                   uint8_t fg_b,
                                   uint8_t bg_r,
                                   uint8_t bg_g,
                                   uint8_t bg_b);

/**
 * Return the foreground RGB for a color scheme index.
 * Always available (no wgpu gate) — used by Swift Metal path.
 */
void aterm_core_scheme_fg_color(uint8_t scheme, uint8_t *out_r, uint8_t *out_g, uint8_t *out_b);

/**
 * Return the background RGB for a color scheme index.
 * Always available (no wgpu gate) — used by Swift Metal path.
 * scheme: 0=Dark, 1=Light, 2=SolarizedDark, 3=SolarizedLight,
 *         4=Monokai, 5=Dracula, 6=Nord, 7=TokyoNight, 8=Default
 */
void aterm_core_scheme_bg_color(uint8_t scheme, uint8_t *out_r, uint8_t *out_g, uint8_t *out_b);

/**
 * Set font size in pixels (clamped to 8..32)
 */
void aterm_core_set_font_size(struct AtermCore *core, float size);

/**
 * No-op stub when wgpu feature is disabled (font size managed on Swift side).
 */
void aterm_core_set_font_size(struct AtermCore *_core, float _size);

/**
 * Set line height in pixels (clamped to 12..64)
 */
void aterm_core_set_line_height(struct AtermCore *core, float height);

/**
 * Store line height for no-wgpu path (clamped to 12..64).
 */
void aterm_core_set_line_height(struct AtermCore *_core, float height);

/**
 * Set cell width in pixels (clamped to 6..32)
 */
void aterm_core_set_cell_width(struct AtermCore *core, float width);

/**
 * Store cell width for no-wgpu path (clamped to 6..32).
 */
void aterm_core_set_cell_width(struct AtermCore *_core, float width);

/**
 * Deprecated compatibility no-op. Background blending has been removed and
 * terminal cell colors are now rendered exactly as provided by the terminal.
 */
void aterm_core_set_bg_blend_threshold(struct AtermCore *_core, float _threshold);

void aterm_core_scroll(struct AtermCore *core, int32_t delta);

/**
 * Scroll to the next/previous shell prompt (OSC 133 marks).
 * direction < 0 = previous prompt (up), direction > 0 = next prompt (down).
 * Returns 1 if scrolled, 0 if no prompt found in that direction.
 */
int32_t aterm_core_scroll_to_prompt(struct AtermCore *core, int32_t direction);

/**
 * Returns the number of OSC 133 prompt marks currently stored.
 */
uint32_t aterm_core_prompt_mark_count(const struct AtermCore *core);

void aterm_core_selection_start(struct AtermCore *core, uint32_t col, int32_t line, uint8_t side);

void aterm_core_selection_update(struct AtermCore *core, uint32_t col, int32_t line, uint8_t side);

void aterm_core_selection_clear(struct AtermCore *core);

/**
 * Returns selected text or NULL. Caller must free with aterm_core_free_string.
 */
char *aterm_core_selection_text(const struct AtermCore *core);

void aterm_core_select_all(struct AtermCore *core);

/**
 * Export the current selection as per-row ranges for Metal selection overlay.
 * Multi-line selections are split into one range per visible row.
 * Returns the number of ranges written via `out_count`.
 */
void aterm_core_selection_ranges(const struct AtermCore *core,
                                 struct SelectionRangeFFI *out_ranges,
                                 uint32_t max_ranges,
                                 uint32_t *out_count);

/**
 * Check if the visible terminal screen contains a text pattern. Returns 1 if found, 0 otherwise.
 */
int32_t aterm_core_screen_contains(const struct AtermCore *core, const char *pattern);

/**
 * Returns JSON string of internal workspaces. Caller must free with aterm_core_free_string.
 */
char *aterm_core_list_workspaces(const struct AtermCore *core);

void aterm_core_free_string(char *ptr);

int32_t aterm_tailscale_connect(const char *hostname,
                                const char *control_url,
                                const char *auth_key);

void aterm_tailscale_shutdown(void);

char *aterm_tailscale_status_json(void);

char *aterm_core_detect_clis(void);

uint32_t aterm_session_count(const struct AtermCore *_core);

struct SessionEntryFFI aterm_session_get(const struct AtermCore *_core, uint32_t index);

void aterm_session_free(struct SessionEntryFFI entry);

void aterm_sessions_save(struct AtermCore *core);

uint32_t aterm_sessions_restore(struct AtermCore *core);

/**
 * Register platform host callbacks. Call once at startup.
 */
void aterm_set_host(struct AtermHostCallbacks callbacks);

/**
 * Dispatch a SessionAction (JSON) and return a response (JSON).
 * Caller must free the returned string with aterm_core_free_string.
 */
char *aterm_dispatch(const char *action_json, uintptr_t action_len);

/**
 * Re-register all workspaces with telepty daemon.
 * Call after session restore to ensure all sessions are visible.
 */
void aterm_sync_telepty(void);

/**
 * Get the IPC socket path. Caller must free with aterm_core_free_string.
 */
char *aterm_ipc_socket_path(void);

/**
 * Get the IPC auth token. Caller must free with aterm_core_free_string.
 */
char *aterm_ipc_token(void);

/**
 * Explicit single workspace close — deterministic, not ARC-dependent.
 * Idempotent: double-close is a no-op.
 */
void aterm_workspace_close(const char *workspace_id);

/**
 * Batch close multiple workspaces. More efficient than individual close calls.
 * After all workspaces are closed, emits a single WorkspaceBatchClosed event
 * and triggers a debounced save.
 */
void aterm_batch_close(const char *const *workspace_ids, uint32_t count);

/**
 * Hint Rust to save sessions if debounce allows.
 * Marks the session store as dirty and starts a 500ms debounce timer.
 * Actual save happens after 500ms of quiet (no new triggers).
 */
void aterm_trigger_save(void);

/**
 * Update the is_system flag for a workspace in the global session registry.
 * Swift calls this after spawn to mark orchestrator/system workspaces so that
 * SaveCoordinator writes the correct flag to sessions.json.
 */
void aterm_core_set_workspace_system(struct AtermCore *core, const char *workspace_name, bool is_system);

extern int32_t aterm_coretext_rasterize_glyph(uint32_t codepoint,
                                              const char *base_font_name,
                                              float font_size,
                                              struct CoreTextGlyphResult *result);

extern void aterm_coretext_free_bitmap(uint8_t *bitmap);

extern int tailscale_new(void);

extern int tailscale_start(int sd);

extern int tailscale_up(int sd);

extern int tailscale_close(int sd);

extern int tailscale_set_dir(int sd, const char *dir);

extern int tailscale_set_hostname(int sd, const char *hostname);

extern int tailscale_set_authkey(int sd, const char *authkey);

extern int tailscale_set_control_url(int sd, const char *control_url);

extern int tailscale_set_logfd(int sd, int fd);

extern int tailscale_errmsg(int sd, char *buf, uintptr_t buflen);

#endif  /* ATERM_CORE_H */
