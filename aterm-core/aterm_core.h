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

struct AtermCore *aterm_core_new(void);

void aterm_core_free(struct AtermCore *core);

int32_t aterm_core_init_gpu(struct AtermCore *core,
                            void *ns_view,
                            uint32_t width,
                            uint32_t height,
                            float scale);

int32_t aterm_core_spawn_shell(struct AtermCore *core,
                               const char *cwd,
                               uint16_t cols,
                               uint16_t rows);

void aterm_core_write_pty(struct AtermCore *core, const char *text, uintptr_t len);

void aterm_core_named_key(struct AtermCore *core, uint32_t key_code);

void aterm_core_render(struct AtermCore *core);

void aterm_core_resize(struct AtermCore *core, uint32_t width, uint32_t height);

void aterm_core_grid_size(const struct AtermCore *core,
                          float width,
                          float height,
                          uint16_t *out_cols,
                          uint16_t *out_rows);

int32_t aterm_core_take_dirty(struct AtermCore *core);

void aterm_core_set_dirty_callback(struct AtermCore *core, void (*callback)(void*), void *userdata);

void aterm_core_sync_pty(struct AtermCore *core);

void aterm_core_scroll(struct AtermCore *core, int32_t delta);

void aterm_core_selection_start(struct AtermCore *core, uint32_t col, int32_t line, uint8_t side);

void aterm_core_selection_update(struct AtermCore *core, uint32_t col, int32_t line, uint8_t side);

void aterm_core_selection_clear(struct AtermCore *core);

/**
 * Returns selected text or NULL. Caller must free with aterm_core_free_string.
 */
char *aterm_core_selection_text(const struct AtermCore *core);

/**
 * Returns JSON string of internal workspaces. Caller must free with aterm_core_free_string.
 */
char *aterm_core_list_workspaces(const struct AtermCore *core);

void aterm_core_free_string(char *ptr);

#endif  /* ATERM_CORE_H */
