/* The one translation unit that owns sfte.
 *
 * sfte is a single header; defining SFTE_IMPL here compiles its implementation
 * into this object, and this file is the ONLY place that may do so. Every
 * accessor the Rust side needs is `static inline` inside that implementation
 * (sfte_dev.h:2247 declares _sfte_grid_get_cell that way), so a reader can
 * exist only in the TU that defines SFTE_IMPL -- which is why the Rust side
 * sees eight plain functions over integers and never an `sfte_cell`. That
 * struct's layout is a function of eight macros, and a Rust mirror of it would
 * desynchronise silently the day one changed.
 *
 * The header compiled is NOT docs/research/sources/sfte.h itself but the copy
 * build.rs writes into OUT_DIR with crates/baud-sfte-difftest/patches/ applied; the
 * pristine file stays hashed in MANIFEST.toml. SFTE_HDR is the path build.rs
 * passes in.
 */

/* --- the fairness configuration --------------------------------------- */
#define SFTE_CUSTOM_BACKEND 1  /* no Wayland: this arm has no display        */
#define SFTE_NO_POSIX 1        /* no forkpty: the harness owns the bytes     */
#define SFTE_NO_LOGGING 1      /* _SFTE_WARN runs after every dispatch       */
#define SFTE_CURSOR_BLINK 0    /* else sfte_parse reads the clock per call   */
/* Geometry at COMPILE time, and asserted below. sfte_resize takes pixels and
   divides by the font's cell width, which is 0 with no font loaded: that is
   undefined behaviour that reads 200x50 at -O2 and 1x1 at -O0. The init
   defaults are the one font-free route that involves no division. */
#define SFTE_TERM_INIT_COLS 200
#define SFTE_TERM_INIT_ROWS 50
/* Scrollback matches baud-core's Depth::DEFAULT (2,000). Both arms are
   compared over the visible grid only, but retention still decides what a
   reverse scroll can show, so the two must agree. */
#define SFTE_TERM_SCROLLBACK_CAP 2000
#define SFTE_TERM_SCROLL_SMOOTH 0  /* depends on scrollback; render-only    */
#define SFTE_IMG_SIXEL 0           /* baud consumes the strings and draws    */
#define SFTE_IMG_KITTY 0           /* nothing; also drops stb_image           */
/* SFTE_INPUT_HYPERLINKS stays at its default of 1: baud implements OSC 8.
   SFTE_FONT_MAX_COMBINING stays at 2: raising it to baud's 8 would grow the
   cell 36 -> 60 bytes, and this arm compares content, not memory.           */

#define SFTE_IMPL
#include SFTE_HDR

#include <locale.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* Compile-time proof that the geometry macros took. If a future sfte renames
   them, this fails to build rather than the arm silently running at 80x24. */
_Static_assert(SFTE_TERM_INIT_COLS == 200 && SFTE_TERM_INIT_ROWS == 50,
               "geometry macros did not take");

/* --- replies ------------------------------------------------------------ */
/* sfte answers DA, DSR and friends through the write callback given to
   sfte_init. The harness accumulates them here so the Rust side can compare
   reply bytes against baud's Term::reply at every read boundary. Bounded and
   counted: an overflow is reported, never silently truncated. */
#define REPLY_CAP 65536
typedef struct {
    sfte_ctx *ctx;
    uint8_t reply[REPLY_CAP];
    size_t reply_len;
    size_t reply_dropped;
} shim;

static void shim_write(void *user, const char *data, size_t len) {
    shim *s = (shim *)user;
    size_t room = REPLY_CAP - s->reply_len;
    size_t take = len < room ? len : room;
    memcpy(s->reply + s->reply_len, data, take);
    s->reply_len += take;
    s->reply_dropped += len - take;
}

/* --- locale -------------------------------------------------------------- */
/* sfte's character width is libc wcwidth (sfte_dev.h:1594) and its only
   setlocale call is inside the Wayland loop this build compiles out, so a
   headless sfte sits in the C locale, where Darwin's wcwidth answers -1 for
   every non-ASCII codepoint and sfte clamps that to width 1. C.UTF-8 is the
   one table on this platform under which marks are 0 and emoji 2. The name
   that took is RETURNED so the Rust side can refuse anything else: an older
   host falls back to "C" silently. */
const char *sfte_shim_set_locale(void) {
    const char *got = setlocale(LC_CTYPE, "C.UTF-8");
    return got ? got : "";
}

/* --- lifecycle ----------------------------------------------------------- */
shim *sfte_shim_new(void) {
    shim *s = (shim *)calloc(1, sizeof *s);
    if (!s) return NULL;
    s->ctx = sfte_init(shim_write, s);
    if (!s->ctx) { free(s); return NULL; }
    /* The positive control for the whole arm. */
    if (s->ctx->term.cols != SFTE_TERM_INIT_COLS || s->ctx->term.rows != SFTE_TERM_INIT_ROWS) {
        sfte_free(s->ctx);
        free(s);
        return NULL;
    }
    return s;
}

void sfte_shim_free(shim *s) {
    if (!s) return;
    sfte_free(s->ctx);
    free(s);
}

void sfte_shim_feed(shim *s, const uint8_t *data, size_t len) {
    sfte_parse(s->ctx, data, len);
}

int16_t sfte_shim_cols(const shim *s) { return s->ctx->term.cols; }
int16_t sfte_shim_rows(const shim *s) { return s->ctx->term.rows; }
int16_t sfte_shim_cursor_col(const shim *s) { return s->ctx->term.cursor_col; }
int16_t sfte_shim_cursor_row(const shim *s) { return s->ctx->term.cursor_row; }

/* --- the cell, as three integers ---------------------------------------- */
/* Reads through sfte's own accessor, never a respelled rotation: the
   physical row is (logical + grid_off) % rows, and a dump loop that spells
   that itself is one edit from reading the wrong row after a scroll and
   producing a plausible screen rather than a crash. */
uint32_t sfte_shim_rune(const shim *s, int16_t col, int16_t row) {
    return _sfte_grid_get_cell(s->ctx, col, row)->rune;
}
/* 1 if the cell is the trailing half of a double-width character. sfte
   stamps the LEAD rune again into that cell and marks it _SFTE_ATTR_DUMMY,
   so a glyph-only read would see the character twice; the flag is what makes
   it a tail. */
uint8_t sfte_shim_is_wide_tail(const shim *s, int16_t col, int16_t row) {
    return (_sfte_grid_get_cell(s->ctx, col, row)->attr & _SFTE_ATTR_DUMMY) ? 1 : 0;
}
/* 1 if the background is the default. Under truecolour bg == 0 means default
   (sfte_dev.h:3035-3041), which also makes SGR 48;2;0;0;0 unrepresentable --
   a one-directional fold the Rust side records. */
uint8_t sfte_shim_bg_is_default(const shim *s, int16_t col, int16_t row) {
    return _sfte_grid_get_cell(s->ctx, col, row)->bg == 0 ? 1 : 0;
}

/* --- replies out --------------------------------------------------------- */
size_t sfte_shim_reply_len(const shim *s) { return s->reply_len; }
const uint8_t *sfte_shim_reply(const shim *s) { return s->reply; }
size_t sfte_shim_reply_dropped(const shim *s) { return s->reply_dropped; }
void sfte_shim_clear_reply(shim *s) { s->reply_len = 0; }

/* The cell's size, so the Rust side can print it beside the comparison: it
   is what the fairness section's 36-byte figure rests on, and it moves when
   a macro does. */
size_t sfte_shim_cell_bytes(void) { return sizeof(sfte_cell); }
