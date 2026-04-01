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

#define BUFFER_MAX_BYTES (1024 * 1024)

#define DEFAULT_SNAPSHOT_BYTES (256 * 1024)

typedef struct AtermCore AtermCore;

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
  void (*on_workspace_event)(void*, const char*);
  void (*request_redraw)(void*);
} AtermHostCallbacks;

struct AtermCore *aterm_core_new(void);

void aterm_core_free(struct AtermCore *core);

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

int32_t aterm_core_spawn_shell(struct AtermCore *core,
                               const char *name,
                               const char *cwd,
                               const char *command,
                               uint16_t cols,
                               uint16_t rows);

void aterm_core_write_pty(struct AtermCore *core, const char *text, uintptr_t len);

int32_t aterm_core_workspace_is_alive(const struct AtermCore *core);

void aterm_core_named_key(struct AtermCore *core, uint32_t key_code);

void aterm_core_render(struct AtermCore *core);

void aterm_core_resize(struct AtermCore *core, uint32_t width, uint32_t height);

void aterm_core_grid_size(const struct AtermCore *core,
                          float width,
                          float height,
                          uint16_t *out_cols,
                          uint16_t *out_rows);

void aterm_core_cell_size(const struct AtermCore *core, float *out_width, float *out_height);

int32_t aterm_core_take_dirty(struct AtermCore *core);

void aterm_core_set_dirty_callback(struct AtermCore *core, void (*callback)(void*), void *userdata);

void aterm_core_sync_pty(struct AtermCore *core);

void aterm_core_set_theme_mode(struct AtermCore *core, uint8_t mode);

/**
 * Set color scheme: 0=Dark, 1=Light, 2=SolarizedDark, 3=SolarizedLight,
 * 4=Monokai, 5=Dracula, 6=Nord, 7=TokyoNight
 */
void aterm_core_set_color_scheme(struct AtermCore *core, uint8_t scheme);

/**
 * Set font size in pixels (clamped to 8..32)
 */
void aterm_core_set_font_size(struct AtermCore *core, float size);

/**
 * Set line height in pixels (clamped to 12..64)
 */
void aterm_core_set_line_height(struct AtermCore *core, float height);

void aterm_core_scroll(struct AtermCore *core, int32_t delta);

void aterm_core_selection_start(struct AtermCore *core, uint32_t col, int32_t line, uint8_t side);

void aterm_core_selection_update(struct AtermCore *core, uint32_t col, int32_t line, uint8_t side);

void aterm_core_selection_clear(struct AtermCore *core);

/**
 * Returns selected text or NULL. Caller must free with aterm_core_free_string.
 */
char *aterm_core_selection_text(const struct AtermCore *core);

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
 * Get the IPC socket path. Caller must free with aterm_core_free_string.
 */
char *aterm_ipc_socket_path(void);

/**
 * Get the IPC auth token. Caller must free with aterm_core_free_string.
 */
char *aterm_ipc_token(void);

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
