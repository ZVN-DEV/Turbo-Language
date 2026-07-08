/*
 * Turbo Runtime Library
 *
 * Provides runtime functions for AOT-compiled Turbo binaries.
 * These are the same functions that the JIT links as function pointers,
 * but compiled as real symbols for the system linker to resolve.
 */

/* Feature test macros — must come before any system header.
 *
 * Under -std=c11 the system headers default to strict POSIX/C11, which
 * hides BSD extensions we depend on (strcasecmp/strncasecmp, INADDR_LOOPBACK).
 *
 * - _POSIX_C_SOURCE 200809L: nanosleep, strdup
 * - _DEFAULT_SOURCE / _BSD_SOURCE: BSD extensions on glibc
 * - _GNU_SOURCE: GNU extensions on glibc (broadest umbrella)
 * - _DARWIN_C_SOURCE: BSD extensions on macOS (including INADDR_LOOPBACK)
 *
 * Defining macros for the wrong platform is harmless — the compiler
 * silently ignores them.
 */
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#ifndef _DEFAULT_SOURCE
#define _DEFAULT_SOURCE
#endif
#ifndef _BSD_SOURCE
#define _BSD_SOURCE
#endif
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#ifndef _DARWIN_C_SOURCE
#define _DARWIN_C_SOURCE
#endif

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdatomic.h>
#include <string.h>
#include <strings.h>   /* strcasecmp, strncasecmp (BSD/POSIX) */
#include <limits.h>
#include <pthread.h>
#include <math.h>
#include <time.h>
#include <sys/time.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#include <dirent.h>

/* Shared size-overflow / allocation-cap guards (single source of truth,
 * also included by turbo_rt_wasm.c so the two runtimes never drift). */
#include "turbo_rt_guards.h"

#define TURBO_F64_FORMAT "%.15g"
#define RT_RC_HEADER_BYTES 16
#define RT_RC_IMMORTAL LLONG_MAX
#define RT_RC_ARENA (LLONG_MAX - 1)

static void *rt_rc_alloc(size_t data_size, long long cap);
static char *rt_str_alloc(size_t len);
static char *rt_str_copy_len(const char *s, size_t len);
static char *rt_str_empty(void);
void rt_release(void *data_ptr);
void rt_retain(void *data_ptr);

/* True when `s` is a bare integer literal ([-]?[0-9]+), i.e. a whole-valued
 * float that %g rendered with no fractional part. False for fractional /
 * exponential forms and for inf/-inf/nan (which carry non-digit chars).
 * Mirrors `f64_text_is_integral` in the JIT runtime (runtime.rs). */
static int rt_f64_text_is_integral(const char *s) {
    if (*s == '-') s++;
    if (*s == '\0') return 0;
    for (; *s != '\0'; s++) {
        if (*s < '0' || *s > '9') return 0;
    }
    return 1;
}

static void rt_format_f64(char *buf, size_t cap, double n) {
    if (n == 0.0) n = 0.0; /* normalize negative zero */
    int len = snprintf(buf, cap, TURBO_F64_FORMAT, n);
    /* BL-26: a whole-valued float renders without a fractional part under %g
     * (`2.0` -> "2"), which is indistinguishable from the int `2`. Append a
     * trailing ".0" when the rendered text is a bare integer so floats stay
     * unambiguous. Fractional/exponential forms and inf/-inf/nan already
     * contain a non-digit and are left untouched. The JIT runtime
     * (format_f64 in runtime.rs) applies the identical rule, keeping JIT and
     * AOT byte-identical. The `len + 2 < cap` guard ensures room for ".0\0"
     * (snprintf may return a length >= cap on truncation). */
    if (len >= 0 && (size_t)len + 2 < cap && rt_f64_text_is_integral(buf)) {
        buf[len] = '.';
        buf[len + 1] = '0';
        buf[len + 2] = '\0';
    }
}

/* ── Per-request arena (P2 — fixes the rt_release no-op leak) ──────────
 *
 * The HTTP server thread for each connection installs a thread-local
 * bumper arena via rt_arena_begin() before invoking the user handler,
 * and resets it via rt_arena_end() after the response has been written.
 *
 * While an arena is installed on the current thread, every turbo_alloc /
 * turbo_calloc call uses the arena instead of malloc, so all temporary
 * objects allocated during the request (request strings, response
 * strings, intermediate concatenations, structs, arrays) are reclaimed
 * in O(1) at the end of the request — no per-allocation rt_release
 * tracking required.
 *
 * Allocations that need to live across requests (channels, mutexes,
 * hashmaps, server config) happen outside the arena window and continue
 * to use plain malloc.
 */

typedef struct turbo_arena_block {
    struct turbo_arena_block *next;
    size_t size;
    size_t used;
    /* data follows immediately after this header */
} turbo_arena_block;

typedef struct turbo_arena {
    turbo_arena_block *head;
    size_t total_alloc;     /* bytes given out via this arena (for stats) */
    size_t total_capacity;  /* bytes ever malloced into this arena */
} turbo_arena;

/* Default first-block size. Subsequent blocks double up to a cap. */
#define TURBO_ARENA_BLOCK_MIN  (16 * 1024)
#define TURBO_ARENA_BLOCK_MAX  (1 * 1024 * 1024)
/* Hard cap on total arena capacity to bound DoS via huge requests. */
#define TURBO_ARENA_HARD_CAP   (64 * 1024 * 1024)

static _Thread_local turbo_arena *t_current_arena = NULL;

/* INVARIANT: an arena-backed pointer must never be retained or released
 * after its arena window ends. Arena allocations carry the RT_RC_ARENA
 * header sentinel, so retain/release are no-ops while the memory is
 * alive; once rt_arena_end() bulk-frees the blocks, any surviving
 * pointer is dangling. Every persistent escape route out of a request
 * arena copies (e.g. hashmap_strdup for persistent maps), and values
 * crossing thread boundaries must be heap copies. We deliberately do
 * NOT track freed block address ranges to launder stale releases: a
 * recycled malloc block would make fresh heap allocations at the same
 * address silently skip refcounting (undercount -> double free), which
 * trades an unreachable-path crash for corruption of normal code. */

static void *turbo_arena_alloc(turbo_arena *a, size_t size) {
    /* Round up to 16-byte alignment so refcount headers stay aligned. */
    size = (size + 15) & ~((size_t)15);

    turbo_arena_block *blk = a->head;
    if (!blk || blk->used + size > blk->size) {
        size_t next_size = blk ? blk->size * 2 : TURBO_ARENA_BLOCK_MIN;
        if (next_size > TURBO_ARENA_BLOCK_MAX) next_size = TURBO_ARENA_BLOCK_MAX;
        if (size > next_size) next_size = size;
        if (a->total_capacity + next_size > TURBO_ARENA_HARD_CAP) {
            fprintf(stderr,
                "runtime error: per-request arena exceeded hard cap (%d MB)\n",
                TURBO_ARENA_HARD_CAP / (1024 * 1024));
            exit(1);
        }
        turbo_arena_block *new_blk =
            (turbo_arena_block *)malloc(sizeof(turbo_arena_block) + next_size);
        if (!new_blk) {
            fprintf(stderr, "runtime error: out of memory (arena block)\n");
            exit(1);
        }
        new_blk->next = a->head;
        new_blk->size = next_size;
        new_blk->used = 0;
        a->head = new_blk;
        a->total_capacity += next_size;
        blk = new_blk;
    }
    void *p = (char *)(blk + 1) + blk->used;
    blk->used += size;
    a->total_alloc += size;
    return p;
}

static void turbo_arena_free_all(turbo_arena *a) {
    turbo_arena_block *blk = a->head;
    while (blk) {
        turbo_arena_block *next = blk->next;
        free(blk);
        blk = next;
    }
    a->head = NULL;
    a->total_alloc = 0;
    a->total_capacity = 0;
}

/* Public: install/uninstall an arena on the current thread. The
 * `rt_arena_begin` / `rt_arena_end` symbols are exported so the JIT
 * runtime can call them too if it ever wants per-request scoping. */
void rt_arena_begin(void) {
    if (t_current_arena != NULL) {
        /* Reuse the existing arena rather than nesting; the previous
         * caller forgot to end. Reset and continue. */
        turbo_arena_free_all(t_current_arena);
        return;
    }
    turbo_arena *a = (turbo_arena *)calloc(1, sizeof(turbo_arena));
    if (!a) { fprintf(stderr, "runtime error: out of memory (arena)\n"); exit(1); }
    t_current_arena = a;
}

void rt_arena_end(void) {
    if (!t_current_arena) return;
    turbo_arena_free_all(t_current_arena);
    free(t_current_arena);
    t_current_arena = NULL;
}

/* ── Checked allocation helpers (C-3) ──────────────────────────────── */

static void *turbo_alloc(size_t size) {
    if (t_current_arena != NULL) {
        return turbo_arena_alloc(t_current_arena, size);
    }
    void *p = malloc(size);
    if (!p) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    return p;
}

static void *turbo_calloc(size_t count, size_t size) {
    size_t total;
    if (__builtin_mul_overflow(count, size, &total)) {
        fprintf(stderr, "turbo_calloc: overflow\n");
        abort();
    }
    if (t_current_arena != NULL) {
        void *p = turbo_arena_alloc(t_current_arena, total);
        memset(p, 0, total);
        return p;
    }
    void *p = calloc(count, size);
    if (!p) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    return p;
}

/* Arena-aware free: if ptr falls within the active arena's memory
 * region, do nothing (the arena reclaims in bulk at rt_arena_end).
 * Otherwise delegate to the system free(). Safe to call on any
 * pointer returned by turbo_alloc(), turbo_calloc(), or malloc(). */
static void turbo_free(void *ptr) {
    if (!ptr) return;
    if (t_current_arena != NULL) {
        /* Check whether ptr lies inside any block of the active arena. */
        turbo_arena_block *blk = t_current_arena->head;
        while (blk) {
            char *base = (char *)(blk + 1);
            char *end  = base + blk->size;
            if ((char *)ptr >= base && (char *)ptr < end) {
                return; /* arena-backed — no-op */
            }
            blk = blk->next;
        }
    }
    free(ptr);
}

static char *turbo_raw_strdup(const char *s) {
    if (!s) return NULL;
    size_t len = strlen(s) + 1;
    char *dup = (char *)turbo_alloc(len);
    memcpy(dup, s, len);
    return dup;
}

/* Turbo-language string duplication. Returned pointers have the shared ARC
 * header immediately before the C string bytes, so scope-exit release can free
 * them just like arrays/structs. */
static char *turbo_strdup(const char *s) {
    if (!s) return NULL;
    return rt_str_copy_len(s, strlen(s));
}

static void *turbo_realloc(void *ptr, size_t size) {
    /* When inside an arena (e.g. read_fd_to_string growing its buffer
     * during http_get/http_post called from a request handler), we can't
     * actually grow in place — arena allocations don't track their own
     * size. The safest correct thing is to allocate a fresh slot of the
     * requested size in the same arena and copy the smaller of the new
     * size and the previous size's worth of bytes. The old slot is
     * "leaked" within the arena and will be reclaimed at rt_arena_end().
     *
     * Callers grow geometrically (cap *= 2) so the wasted space is
     * bounded by the final allocation size. */
    if (t_current_arena != NULL) {
        void *new_ptr = turbo_arena_alloc(t_current_arena, size);
        if (ptr) {
            /* We do not know the old length; copy `size` bytes from the
             * old slot. read_fd_to_string only doubles the buffer, so
             * the old slot always has at least `size/2` valid bytes,
             * which is what the caller cares about. The trailing bytes
             * are uninitialized but the caller writes them next. */
            memcpy(new_ptr, ptr, size / 2);
        }
        return new_ptr;
    }
    void *p = realloc(ptr, size);
    if (!p) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    return p;
}

void rt_print_str(const char *s) {
    if (s)
        printf("%s\n", s);
    else
        printf("\n");
}

void rt_print_i64(long long n) {
    printf("%lld\n", n);
}

void rt_print_f64(double n) {
    char buf[64];
    rt_format_f64(buf, sizeof(buf), n);
    printf("%s\n", buf);
}

void rt_print_bool(char b) {
    printf("%s\n", b ? "true" : "false");
}

void rt_panic(const char *msg) {
    if (msg)
        fprintf(stderr, "panic: %s\n", msg);
    else
        fprintf(stderr, "panic: explicit panic\n");
    exit(1);
}

void rt_assert_fail(const char *msg) {
    if (msg)
        fprintf(stderr, "assertion failed: %s\n", msg);
    else
        fprintf(stderr, "assertion failed\n");
    exit(1);
}

/* assert_eq / assert_ne failure helper.
 * kind: 0 = assert_eq, 1 = assert_ne
 * actual / expected are NUL-terminated stringified values.
 * Mirrors rt_assert_eq_fail in src/runtime.rs so JIT and AOT print
 * identical diagnostics on failure. */
void rt_assert_eq_fail(long long kind, const char *actual, const char *expected) {
    const char *actual_str = actual ? actual : "<null>";
    const char *expected_str = expected ? expected : "<null>";
    if (kind == 0) {
        fprintf(stderr, "assertion failed: assert_eq(%s, %s)\n", actual_str, expected_str);
        fprintf(stderr, "  left:  %s\n", actual_str);
        fprintf(stderr, "  right: %s\n", expected_str);
    } else {
        fprintf(stderr, "assertion failed: assert_ne(%s, %s)\n", actual_str, expected_str);
        fprintf(stderr, "  both values are: %s\n", actual_str);
    }
    exit(1);
}

/* ── Styled runtime-error envelope ───────────────────────────────────
 *
 * AOT (built-binary) twin of `runtime_error()` in src/runtime.rs. The two
 * MUST emit byte-identical output so a program run through the JIT
 * (`turbolang run`) and the same program built to a native binary produce
 * the identical diagnostic (JIT ≡ AOT). ANSI color is emitted only when
 * stderr is a terminal, so piped output (the integration-test harness, the
 * web playground) stays clean.
 *
 *   runtime error[E06NN]: <message>
 *   Help: <help>
 *     more info: <doc url>
 *
 * RT_ERROR_DOC_URL_BASE must stay in sync with `error_code_url()` in
 * turbo-cli/src/main.rs and `RT_ERROR_DOC_URL_BASE` in src/runtime.rs. */
#define RT_ERROR_DOC_URL_BASE \
    "https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors"

static void rt_runtime_error(const char *code, const char *message, const char *help) {
    if (isatty(fileno(stderr))) {
        fprintf(stderr, "\033[1;31mruntime error[%s]\033[0m: %s\n", code, message);
        fprintf(stderr, "\033[1;36mHelp\033[0m: %s\n", help);
    } else {
        fprintf(stderr, "runtime error[%s]: %s\n", code, message);
        fprintf(stderr, "Help: %s\n", help);
    }
    fprintf(stderr, "  more info: " RT_ERROR_DOC_URL_BASE "/%s.md\n", code);
}

/* Styled array index-out-of-bounds trap (E0602). Shared by rt_array_get,
 * rt_array_set, and rt_array_oob_exit. Diverges (never returns). */
static void rt_array_oob(long long index, long long len) {
    char message[96];
    char help[128];
    snprintf(message, sizeof message,
             "array index %lld out of bounds (length %lld)", index, len);
    snprintf(help, sizeof help,
             "valid indices are 0..%lld (exclusive); check with `if i < len(arr)`", len);
    rt_runtime_error("E0602", message, help);
    exit(1);
}

/* Styled string index-out-of-bounds trap (E0602). Diverges (never returns). */
static void rt_str_index_oob(long long index, size_t len) {
    char message[96];
    char help[160];
    snprintf(message, sizeof message,
             "string index %lld out of bounds (length %zu)", index, len);
    snprintf(help, sizeof help,
             "valid indices are 0..%zu (exclusive); check the index against the string length", len);
    rt_runtime_error("E0602", message, help);
    exit(1);
}

void rt_div_by_zero(void) {
    rt_runtime_error("E0601", "division by zero",
                     "guard the divisor: `if b != 0 { ... }`");
    exit(1);
}

void rt_int_overflow(void) {
    rt_runtime_error("E0603", "integer overflow",
                     "the result does not fit in a 64-bit signed integer; check the operands' magnitude");
    exit(1);
}

const char* rt_str_concat(const char *a, const char *b) {
    size_t a_len = a ? strlen(a) : 0;
    size_t b_len = b ? strlen(b) : 0;
    char *result = rt_str_alloc(a_len + b_len);
    if (a) memcpy(result, a, a_len);
    if (b) memcpy(result + a_len, b, b_len);
    result[a_len + b_len] = '\0';
    return result;
}

const char* rt_str_copy(const char *s) {
    return turbo_strdup(s ? s : "");
}

const char* rt_str_concat_inplace(const char *old, const char *suffix) {
    /* `s = s + x` lowers to this. Strings now carry a refcount, but this stays
     * allocation-only for identical JIT/AOT semantics; the assignment site drops
     * the overwritten string after observing whether the pointer changed. */
    if (!suffix || !*suffix) return old ? old : rt_str_empty();
    if (!old || !*old) {
        size_t len = strlen(suffix);
        char *r = rt_str_alloc(len);
        memcpy(r, suffix, len + 1);
        return r;
    }
    size_t old_len = strlen(old);
    size_t suffix_len = strlen(suffix);
    size_t new_len = old_len + suffix_len;
    char *result = rt_str_alloc(new_len);
    memcpy(result, old, old_len);
    memcpy(result + old_len, suffix, suffix_len + 1);
    return result;
}

char rt_str_eq(const char *a, const char *b) {
    if (!a && !b) return 1;
    if (!a || !b) return 0;
    return strcmp(a, b) == 0 ? 1 : 0;
}

/* ── Reference-counted allocation header ─────────────────────────────
 *
 * Every heap object that participates in ARC (arrays, structs, enums,
 * channel handles, …) shares a common allocation header layout:
 *
 *     raw + 0  : cap        (8 bytes, arrays only — element capacity;
 *                            zero / unused for non-array types)
 *     raw + 8  : refcount   (8 bytes)
 *     raw + 16 : user data  (arrays: [len][elem0][elem1]…,
 *                            structs: [field0][field1]…,
 *                            enums:   [tag][payload],
 *                            …)
 *
 * The "data pointer" returned to callers is `raw + RT_RC_HEADER_BYTES`.
 * rt_retain / rt_release read the refcount at `data_ptr - 8` (unchanged
 * from the 8-byte-header era) and free from `data_ptr - 16`.
 *
 * The cap slot is used by rt_array_push to support amortised O(1)
 * growth: when the refcount is 1 and `cap > len`, push writes into the
 * existing allocation in place; otherwise it doubles and reallocates.
 * Non-array allocations leave the cap slot at zero, where it stays
 * harmless.
 *
 * The 16-byte header also keeps the user-data region 16-byte aligned
 * on 64-bit hosts, which matches the alignment the arena allocator
 * already rounds to (see turbo_arena_alloc). */
static inline long long *rt_rc_cap_ptr(void *data_ptr) {
    return (long long *)((char *)data_ptr - 16);
}

static inline long long *rt_rc_refcount_ptr(void *data_ptr) {
    return (long long *)((char *)data_ptr - 8);
}

/* Allocate a refcount'd heap object with `data_size` user-data bytes
 * and initial element capacity `cap` (only meaningful for arrays).
 * Returns the data pointer (not the raw allocation base). */
static void *rt_rc_alloc(size_t data_size, long long cap) {
    if (data_size > SIZE_MAX - RT_RC_HEADER_BYTES) {
        fprintf(stderr, "rt_rc_alloc: overflow\n");
        abort();
    }
    size_t total = RT_RC_HEADER_BYTES + data_size;
    void *raw = turbo_calloc(1, total);
    *(long long *)raw = cap;              /* cap at raw + 0 */
    *(long long *)((char *)raw + 8) =
        (t_current_arena != NULL) ? RT_RC_ARENA : 1;
    return (char *)raw + RT_RC_HEADER_BYTES;
}

static char *rt_str_alloc(size_t len) {
    if (len > SIZE_MAX - 1) {
        fprintf(stderr, "runtime error: string allocation overflow\n");
        exit(1);
    }
    char *result = (char *)rt_rc_alloc(len + 1, 0);
    result[len] = '\0';
    return result;
}

static char *rt_str_copy_len(const char *s, size_t len) {
    char *result = rt_str_alloc(len);
    if (len > 0 && s) {
        memcpy(result, s, len);
    }
    result[len] = '\0';
    return result;
}

static char *rt_str_empty(void) {
    return rt_str_copy_len("", 0);
}

/* Array allocation: element capacity bound that guarantees the final
 * `total = RT_RC_HEADER_BYTES + 8 + cap * 8` does not overflow size_t.
 * Shared by rt_array_alloc and rt_array_push.
 *
 * `new_len` is the requested element count. The bound leaves room for
 * the 16-byte header and the 8-byte length field inside user data.
 *
 * Delegates to the shared rt_array_len_fits_hdr() in turbo_rt_guards.h so
 * the overflow arithmetic stays identical to the WASM runtime. */
static inline int rt_array_len_fits(long long new_len) {
    return rt_array_len_fits_hdr(new_len, RT_RC_HEADER_BYTES);
}

void* rt_array_alloc(long long len) {
    if (!rt_array_len_fits(len)) {
        fprintf(stderr, "runtime error: array alloc size overflow (length %lld)\n", len);
        exit(1);
    }
    size_t data_size = 8 + (size_t)len * 8;
    void *data_ptr = rt_rc_alloc(data_size, len);
    *(long long *)data_ptr = len;
    return data_ptr;
}

void rt_array_oob_exit(long long index, long long len) {
    rt_array_oob(index, len);
}

long long rt_array_get(const void *arr, long long index) {
    long long len = *(const long long*)arr;
    if (index < 0 || index >= len) {
        rt_array_oob_exit(index, len);
    }
    return ((const long long*)arr)[1 + index];
}

void* rt_array_set(void *arr, long long index, long long value) {
    /* COW: check refcount before mutating.
     * Acquire load matches __sync_fetch_and_{add,sub} on the same word
     * used elsewhere in the ARC surface — keeps the ordering story
     * consistent if an `async` spawn ever shares an array. */
    long long *rc_ptr = rt_rc_refcount_ptr(arr);
    long long rc = __atomic_load_n(rc_ptr, __ATOMIC_ACQUIRE);
    void *target;
    if (rc > 1) {
        /* Copy-on-write: make a private copy */
        long long len = *(const long long*)arr;
        if (!rt_array_len_fits(len)) {
            fprintf(stderr, "runtime error: array COW size overflow (length %lld)\n", len);
            exit(1);
        }
        size_t data_size = (1 + (size_t)len) * 8;
        void *new_data = rt_rc_alloc(data_size, len);
        memcpy(new_data, arr, data_size);
        if (rc != RT_RC_ARENA && rc != RT_RC_IMMORTAL) {
            __sync_fetch_and_sub(rc_ptr, 1);
        }
        target = new_data;
    } else {
        target = arr;
    }
    long long len = *(const long long*)target;
    if (index < 0 || index >= len) {
        rt_array_oob(index, len);
    }
    ((long long*)target)[1 + index] = value;
    return target;
}

long long rt_array_len(const void *arr) {
    return *(const long long*)arr;
}

void* rt_array_push(void *arr, long long value) {
    long long old_len = *(const long long*)arr;
    long long new_len = old_len + 1;
    /* Checked-multiply guard: ensure
     *   total = RT_RC_HEADER_BYTES + 8 + new_len * 8
     * does not overflow size_t. The practical reach of `long long` on
     * 64-bit hosts makes this unreachable from normal programs, but
     * guarding it here turns any miscomputed length (or adversarial
     * caller) into a clean abort instead of a heap overflow. */
    if (!rt_array_len_fits(new_len)) {
        fprintf(stderr, "runtime error: array push size overflow\n");
        exit(1);
    }

    long long *rc_ptr = rt_rc_refcount_ptr(arr);
    long long *cap_ptr = rt_rc_cap_ptr(arr);
    long long cap = *cap_ptr;

    /* Amortised O(1) growth: when we are the sole owner of this array
     * (refcount == 1) and the existing allocation has room, write in
     * place. Otherwise double the capacity (minimum 4) and reallocate.
     *
     * Acquire load pairs with the release-style ordering implied by
     * __sync_fetch_and_{add,sub} elsewhere in the ARC surface. rc == 1
     * means no concurrent holder exists, so the subsequent write is
     * safe. */
    if (cap > old_len && __atomic_load_n(rc_ptr, __ATOMIC_ACQUIRE) == 1) {
        ((long long *)arr)[1 + old_len] = value;
        *(long long *)arr = new_len;
        return arr;
    }

    long long new_cap = cap > 0 ? cap * 2 : 4;
    if (new_cap < new_len) new_cap = new_len;
    if (!rt_array_len_fits(new_cap)) {
        /* Fall back to exact-fit if doubling overflows; the same guard
         * above already proved new_len itself is safe. */
        new_cap = new_len;
    }
    size_t data_size = 8 + (size_t)new_cap * 8;
    void *new_data = rt_rc_alloc(data_size, new_cap);
    *(long long *)new_data = new_len;
    /* Copy old elements */
    memcpy((char*)new_data + 8, (const char*)arr + 8, (size_t)old_len * 8);
    /* Append new element */
    ((long long*)new_data)[1 + old_len] = value;
    return new_data;
}

long long rt_str_len(const char *s) {
    return s ? (long long)strlen(s) : 0;
}

void* rt_struct_alloc(long long num_fields) {
    if (num_fields < 0) {
        fprintf(stderr, "rt_struct_alloc: negative num_fields\n");
        abort();
    }
    size_t data_size;
    if (__builtin_mul_overflow((size_t)num_fields, (size_t)8, &data_size)) {
        fprintf(stderr, "rt_struct_alloc: overflow\n");
        abort();
    }
    if (data_size < 8) data_size = 8;
    return rt_rc_alloc(data_size, 0);
}

/* Copy-on-write guard for struct field assignment.
 *
 * Structs carry the same [cap][refcount] header as arrays (see
 * rt_struct_alloc / rt_rc_alloc), so a `let b = a` / `mut`-param /
 * array-element copy can leave two live bindings pointing at one
 * allocation. Mutating a field through either would alias the other.
 * This mirrors the copy-on-write dance in rt_array_set: if the refcount
 * is > 1, allocate a private copy, memcpy the `num_fields` data slots,
 * drop our reference to the shared original, and return the copy. When
 * the refcount is 1 (sole owner) the original pointer is returned
 * unchanged and the field store proceeds in place. `num_fields` matches
 * the count passed to rt_struct_alloc. */
void* rt_struct_cow(void *s, long long num_fields) {
    if (!s) return s;
    long long *rc_ptr = rt_rc_refcount_ptr(s);
    long long rc = __atomic_load_n(rc_ptr, __ATOMIC_ACQUIRE);
    if (rc <= 1) {
        return s;
    }
    if (num_fields < 0) {
        fprintf(stderr, "rt_struct_cow: negative num_fields\n");
        abort();
    }
    size_t data_size;
    if (__builtin_mul_overflow((size_t)num_fields, (size_t)8, &data_size)) {
        fprintf(stderr, "rt_struct_cow: overflow\n");
        abort();
    }
    if (data_size < 8) data_size = 8;
    void *new_data = rt_rc_alloc(data_size, 0);
    memcpy(new_data, s, data_size);
    if (rc != RT_RC_ARENA && rc != RT_RC_IMMORTAL) {
        __sync_fetch_and_sub(rc_ptr, 1);
    }
    return new_data;
}

const char* rt_i64_to_str(long long n) {
    char tmp[21];
    char *p = tmp + 20;
    *p = '\0';
    unsigned long long v = (n < 0) ? (unsigned long long)(-n) : (unsigned long long)n;
    if (v == 0) { *--p = '0'; }
    else { while (v) { *--p = '0' + (char)(v % 10); v /= 10; } }
    if (n < 0) *--p = '-';
    size_t len = (size_t)(tmp + 20 - p);
    char *buf = rt_str_alloc(len);
    memcpy(buf, p, len + 1);
    return buf;
}

const char* rt_f64_to_str(double n) {
    char *buf = rt_str_alloc(63);
    rt_format_f64(buf, 64, n);
    return buf;
}

const char* rt_bool_to_str(char b) {
    if (b) {
        char *buf = rt_str_alloc(4);
        memcpy(buf, "true", 5);
        return buf;
    } else {
        char *buf = rt_str_alloc(5);
        memcpy(buf, "false", 6);
        return buf;
    }
}

/* Result type runtime functions */

void* rt_result_ok(long long value) {
    /* Data layout: [tag (8)][value (8)]. See rt_rc_alloc for the
     * shared [cap][refcount] header that precedes it. */
    long long *data = (long long *)rt_rc_alloc(2 * sizeof(long long), 0);
    data[0] = 0; /* ok tag */
    data[1] = value;
    return data;
}

void* rt_result_err(long long value) {
    long long *data = (long long *)rt_rc_alloc(2 * sizeof(long long), 0);
    data[0] = 1; /* err tag */
    data[1] = value;
    return data;
}

long long rt_result_tag(const void *result) {
    return ((const long long*)result)[0];
}

long long rt_result_value(const void *result) {
    return ((const long long*)result)[1];
}

/* Optional type runtime functions */

void* rt_option_some(long long value) {
    long long *data = (long long *)rt_rc_alloc(2 * sizeof(long long), 0);
    data[0] = 1; /* some tag */
    data[1] = value;
    return data;
}

void* rt_option_none(void) {
    long long *data = (long long *)rt_rc_alloc(2 * sizeof(long long), 0);
    data[0] = 0; /* none tag */
    data[1] = 0;
    return data;
}

long long rt_option_tag(const void *opt) {
    return ((const long long*)opt)[0];
}

long long rt_option_value(const void *opt) {
    return ((const long long*)opt)[1];
}

/* ── Standard library: string functions ─────────────────────────────── */

void* rt_str_split(const char *s, const char *sep) {
    /* Count splits */
    size_t sep_len = strlen(sep);
    size_t count = 1;
    const char *p = s;
    if (sep_len > 0) {
        while ((p = strstr(p, sep)) != NULL) { count++; p += sep_len; }
    } else {
        /* splitting by empty string: one element per character */
        count = strlen(s);
        if (count == 0) count = 1;
    }

    if (!rt_array_len_fits((long long)count)) {
        fprintf(stderr, "runtime error: array alloc size overflow (split count %zu)\n", count);
        exit(1);
    }
    size_t data_size = 8 + count * 8;
    long long *arr = (long long *)rt_rc_alloc(data_size, (long long)count);
    arr[0] = (long long)count;

    if (sep_len == 0) {
        /* split each character */
        size_t slen = strlen(s);
        if (slen == 0) {
            arr[1] = (long long)(size_t)rt_str_empty();
        } else {
            for (size_t i = 0; i < slen; i++) {
                char *ch = rt_str_alloc(1);
                ch[0] = s[i];
                ch[1] = '\0';
                arr[1 + i] = (long long)(size_t)ch;
            }
        }
    } else {
        p = s;
        size_t idx = 0;
        const char *next;
        while ((next = strstr(p, sep)) != NULL) {
            size_t len = (size_t)(next - p);
            char *part = rt_str_alloc(len);
            memcpy(part, p, len);
            part[len] = '\0';
            arr[1 + idx] = (long long)(size_t)part;
            idx++;
            p = next + sep_len;
        }
        /* last part */
        size_t len = strlen(p);
        char *part = rt_str_alloc(len);
        memcpy(part, p, len);
        part[len] = '\0';
        arr[1 + idx] = (long long)(size_t)part;
    }
    return arr;
}

const char* rt_str_trim(const char *s) {
    if (!s) return rt_str_empty();
    const char *start = s;
    while (*start == ' ' || *start == '\t' || *start == '\n' || *start == '\r') start++;
    const char *end = s + strlen(s);
    while (end > start && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\n' || end[-1] == '\r')) end--;
    size_t len = (size_t)(end - start);
    char *result = rt_str_alloc(len);
    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

const char* rt_str_upper(const char *s) {
    if (!s) return rt_str_empty();
    size_t len = strlen(s);
    char *result = rt_str_alloc(len);
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        result[i] = (c >= 'a' && c <= 'z') ? (char)(c - 32) : (char)c;
    }
    result[len] = '\0';
    return result;
}

const char* rt_str_lower(const char *s) {
    if (!s) return rt_str_empty();
    size_t len = strlen(s);
    char *result = rt_str_alloc(len);
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        result[i] = (c >= 'A' && c <= 'Z') ? (char)(c + 32) : (char)c;
    }
    result[len] = '\0';
    return result;
}

char rt_str_starts_with(const char *s, const char *prefix) {
    if (!s || !prefix) return 0;
    size_t plen = strlen(prefix);
    return strncmp(s, prefix, plen) == 0 ? 1 : 0;
}

char rt_str_ends_with(const char *s, const char *suffix) {
    if (!s || !suffix) return 0;
    size_t slen = strlen(s);
    size_t suflen = strlen(suffix);
    if (suflen > slen) return 0;
    return strcmp(s + slen - suflen, suffix) == 0 ? 1 : 0;
}

const char* rt_str_replace(const char *s, const char *from, const char *to) {
    if (!s || !from || !to) {
        return rt_str_empty();
    }
    size_t from_len = strlen(from);
    size_t to_len = strlen(to);
    if (from_len == 0) {
        /* No replacement for empty pattern; return copy */
        size_t len = strlen(s);
        char *r = rt_str_alloc(len);
        memcpy(r, s, len + 1);
        return r;
    }
    /* Count occurrences */
    size_t count = 0;
    const char *p = s;
    while ((p = strstr(p, from)) != NULL) { count++; p += from_len; }
    /* C-2: Use signed arithmetic to avoid underflow when to_len < from_len */
    long long delta = (long long)to_len - (long long)from_len;
    size_t new_len = (size_t)((long long)strlen(s) + (long long)count * delta);
    char *result = rt_str_alloc(new_len);
    char *w = result;
    p = s;
    const char *next;
    while ((next = strstr(p, from)) != NULL) {
        size_t seg = (size_t)(next - p);
        memcpy(w, p, seg); w += seg;
        memcpy(w, to, to_len); w += to_len;
        p = next + from_len;
    }
    size_t rem = strlen(p);
    memcpy(w, p, rem + 1);
    return result;
}

const char* rt_str_char_at(const char *s, long long index) {
    if (!s) {
        rt_str_index_oob(index, 0);
    }
    size_t len = strlen(s);
    if (index < 0 || (size_t)index >= len) {
        rt_str_index_oob(index, len);
    }
    char *result = rt_str_alloc(1);
    result[0] = s[(size_t)index];
    result[1] = '\0';
    return result;
}

char rt_str_contains(const char *s, const char *sub) {
    if (!s || !sub) return 0;
    return strstr(s, sub) != NULL ? 1 : 0;
}

const char* rt_str_repeat(const char *s, long long count) {
    if (!s || count <= 0) {
        return rt_str_empty();
    }
    size_t len = strlen(s);
    /* Overflow check: len * count must fit in size_t and leave room for
     * the trailing NUL. Previously this wrapped silently and the malloc
     * would succeed with a much smaller size than the subsequent strcat
     * loop needed, producing a heap overflow. */
    if (len == 0) {
        return rt_str_empty();
    }
    /* total = len * count (+1 for the trailing NUL) must fit in size_t.
     * Shared guard with the WASM runtime (turbo_rt_guards.h). */
    if (!rt_mul_add_size_fits((size_t)count, len, 1)) {
        fprintf(stderr,
                "[rt_str_repeat] overflow: len=%zu * count=%lld exceeds SIZE_MAX\n",
                len, count);
        return rt_str_empty();
    }
    size_t total = len * (size_t)count;
    /* Practical cap: 256 MB. Larger totals usually indicate a bug or attack
     * and would exhaust memory; mirror the Rust JIT runtime cap. */
    if (total > TURBO_RT_MAX_ALLOC_BYTES) {
        fprintf(stderr,
                "[rt_str_repeat] refusing allocation: total=%zu > cap %llu\n",
                total, (unsigned long long)TURBO_RT_MAX_ALLOC_BYTES);
        return rt_str_empty();
    }
    char *result = rt_str_alloc(total);
    size_t off = 0;
    for (long long i = 0; i < count; i++) {
        memcpy(result + off, s, len);
        off += len;
    }
    result[off] = '\0';
    return result;
}

long long rt_str_index_of(const char *s, const char *sub) {
    if (!s || !sub) return -1;
    const char *found = strstr(s, sub);
    if (!found) return -1;
    return (long long)(found - s);
}

const char* rt_str_join(const char *arr_ptr, const char *sep) {
    /* arr_ptr is a Turbo array of strings. Layout: [len (8 bytes), elem0, elem1, ...] */
    if (!arr_ptr) return rt_str_empty();
    long long len = *(long long *)arr_ptr;
    if (len <= 0) return rt_str_empty();
    const char **elems = (const char **)(arr_ptr + 8);
    /* Calculate total length */
    size_t total = 0;
    size_t sep_len = sep ? strlen(sep) : 0;
    for (long long i = 0; i < len; i++) {
        if (elems[i]) total += strlen(elems[i]);
        if (i < len - 1) total += sep_len;
    }
    char *result = rt_str_alloc(total);
    /* Length-tracked memcpy instead of strcat. strcat re-scans the
     * growing prefix for its NUL terminator on every call — an O(n^2)
     * footgun on long joins — and any future miscalculation of `total`
     * would silently turn into a heap overflow. Tracking the write
     * offset explicitly makes the write bounded by `total` and O(n). */
    size_t offset = 0;
    for (long long i = 0; i < len; i++) {
        if (elems[i]) {
            size_t n = strlen(elems[i]);
            memcpy(result + offset, elems[i], n);
            offset += n;
        }
        if (i < len - 1 && sep) {
            memcpy(result + offset, sep, sep_len);
            offset += sep_len;
        }
    }
    result[offset] = '\0';
    return result;
}

/* ── Standard library: I/O functions ───────────────────────────────── */

const char* rt_read_line(void) {
    char *line = NULL;
    size_t cap = 0;
    ssize_t nread = getline(&line, &cap, stdin);
    if (nread < 0) {
        free(line);
        return rt_str_empty();
    }
    /* strip trailing \n / \r */
    while (nread > 0 && (line[nread-1] == '\n' || line[nread-1] == '\r')) {
        line[--nread] = '\0';
    }
    char *result = rt_str_alloc((size_t)nread);
    memcpy(result, line, (size_t)nread + 1);
    free(line);
    return result;
}

const char* rt_read_file(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "runtime error: cannot read file '%s'\n", path);
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    if (size < 0) { fclose(f); fprintf(stderr, "runtime error: cannot read file size\n"); exit(1); }
    fseek(f, 0, SEEK_SET);
    char *buf = rt_str_alloc((size_t)size);
    fread(buf, 1, (size_t)size, f);
    buf[size] = '\0';
    fclose(f);
    return buf;
}

void rt_write_file(const char *path, const char *content) {
    FILE *f = fopen(path, "wb");
    if (!f) {
        fprintf(stderr, "runtime error: cannot write file '%s'\n", path);
        exit(1);
    }
    if (content) fwrite(content, 1, strlen(content), f);
    fclose(f);
}

/* ── Standard library: math functions ──────────────────────────────── */

#include <math.h>

long long rt_pow(long long base, long long exp) {
    if (exp < 0) {
        rt_panic("negative exponent in pow");
    }
    long long result = 1;
    for (long long i = 0; i < exp; i++) {
        if (__builtin_mul_overflow(result, base, &result)) {
            rt_int_overflow();
        }
    }
    return result;
}

double rt_sqrt(double x) {
    return sqrt(x);
}

/* ── Math builtins ───────────────────────────────────────────────── */

long long rt_floor(double x) { return (long long)floor(x); }
long long rt_ceil(double x) { return (long long)ceil(x); }
long long rt_round(double x) { return (long long)round(x); }
double rt_sin(double x) { return sin(x); }
double rt_cos(double x) { return cos(x); }
double rt_tan(double x) { return tan(x); }
double rt_log_builtin(double x) { return log(x); }
double rt_log2_builtin(double x) { return log2(x); }
double rt_log10(double x) { return log10(x); }
double rt_exp(double x) { return exp(x); }

static _Thread_local unsigned int rt_rand_state = 0;
static _Thread_local int rt_rand_initialized = 0;

static unsigned int rt_rand_next(void) {
    if (!rt_rand_initialized) {
        rt_rand_state = (unsigned int)time(NULL) ^ (unsigned int)(uintptr_t)&rt_rand_state;
        rt_rand_initialized = 1;
    }
    /* xorshift32 — fast, thread-local, no global state */
    rt_rand_state ^= rt_rand_state << 13;
    rt_rand_state ^= rt_rand_state >> 17;
    rt_rand_state ^= rt_rand_state << 5;
    return rt_rand_state;
}

double rt_random(void) {
    return (double)rt_rand_next() / (double)UINT32_MAX;
}

long long rt_random_range(long long min_val, long long max_val) {
    if (max_val < min_val) return min_val;
    unsigned long long range = (unsigned long long)(max_val - min_val) + 1;
    return min_val + (long long)(rt_rand_next() % range);
}

/* ── System builtins ─────────────────────────────────────────────── */

void rt_exit(long long code) {
    exit((int)code);
}

/* CLI argument storage. Captured once at process startup (main() calls
 * rt_set_args before turbo_main) so args() can return the program's own
 * arguments. argv lives for the whole process, so storing the pointers is
 * safe; rt_args copies each string into runtime memory when materializing
 * the [str]. */
static int rt_prog_argc = 0;
static char **rt_prog_argv = NULL;

void rt_set_args(int argc, char **argv) {
    rt_prog_argc = argc;
    rt_prog_argv = argv;
}

void *rt_args(void) {
    /* Return the program's CLI arguments as a Turbo [str].
     *
     * Convention: args()[0] is the FIRST user argument — argv[0] (the binary
     * path) is excluded. This is the AOT twin of `rt_args` in runtime.rs; both
     * expose the identical argv[1..] convention so a program produces the same
     * args() whether run via `turbolang run f.tb -- a b c` or built and invoked
     * as `./bin a b c`.
     *
     * Each element is copied through the runtime allocator so it follows the
     * same ownership model as rt_str_split's parts. Array layout matches
     * rt_str_split: [len][ptr0][ptr1]... */
    long long count = (rt_prog_argc > 1) ? (long long)(rt_prog_argc - 1) : 0;
    if (!rt_array_len_fits(count)) {
        fprintf(stderr, "runtime error: too many CLI arguments (%lld)\n", count);
        exit(1);
    }
    size_t data_size = 8 + (size_t)count * 8;
    long long *arr = (long long *)rt_rc_alloc(data_size, count);
    arr[0] = count;
    for (long long i = 0; i < count; i++) {
        const char *src = rt_prog_argv[i + 1];
        char *copy = turbo_strdup(src ? src : "");
        arr[1 + i] = (long long)(size_t)copy;
    }
    return arr;
}

/* ── String parsing builtins ─────────────────────────────────────── */

const char *rt_substring(const char *s, long long start, long long end) {
    if (!s) return rt_str_empty();
    // Character-indexed, UTF-8 aware — must match the Rust JIT `rt_substring`,
    // which slices by `char` count. Walk the string counting char starts (bytes
    // not of the form 0b10xxxxxx) and translate char indices to byte offsets.
    long long nbytes = (long long)strlen(s);
    if (start < 0) start = 0;
    if (end < 0) end = 0;
    long long start_byte = -1, end_byte = nbytes;
    long long char_idx = 0, i = 0;
    while (i <= nbytes) {
        unsigned char c = (unsigned char)s[i];
        int is_char_start = (i == nbytes) || ((c & 0xC0) != 0x80);
        if (is_char_start) {
            if (char_idx == start) start_byte = i;
            if (char_idx == end) { end_byte = i; break; }
            char_idx++;
        }
        i++;
    }
    if (start_byte < 0) start_byte = nbytes; // start past end of string
    if (start_byte >= end_byte) return rt_str_empty();
    long long len = end_byte - start_byte;
    char *result = rt_str_alloc((size_t)len);
    memcpy(result, s + start_byte, (size_t)len);
    result[len] = '\0';
    return result;
}

const char *rt_pad_left(const char *s, long long width, const char *pad_char) {
    if (!s) s = "";
    if (width < 0) return turbo_strdup(s);
    if (!pad_char || pad_char[0] == '\0') pad_char = " ";
    long long slen = (long long)strlen(s);
    if (slen >= width) return turbo_strdup(s);
    long long pad_count = width - slen;
    char *result = rt_str_alloc((size_t)width);
    char c = pad_char[0];
    for (long long i = 0; i < pad_count; i++) result[i] = c;
    memcpy(result + pad_count, s, (size_t)slen);
    result[width] = '\0';
    return result;
}

const char *rt_pad_right(const char *s, long long width, const char *pad_char) {
    if (!s) s = "";
    if (width < 0) return turbo_strdup(s);
    if (!pad_char || pad_char[0] == '\0') pad_char = " ";
    long long slen = (long long)strlen(s);
    if (slen >= width) return turbo_strdup(s);
    long long pad_count = width - slen;
    char *result = rt_str_alloc((size_t)width);
    memcpy(result, s, (size_t)slen);
    char c = pad_char[0];
    for (long long i = 0; i < pad_count; i++) result[slen + i] = c;
    result[width] = '\0';
    return result;
}

void *rt_str_to_int(const char *s) {
    if (!s) {
        const char *msg = "cannot parse empty string as integer";
        char *buf = rt_str_alloc(strlen(msg));
        memcpy(buf, msg, strlen(msg) + 1);
        return rt_result_err((long long)(intptr_t)buf);
    }
    char *endptr;
    long long val = strtoll(s, &endptr, 10);
    if (endptr == s || *endptr != '\0') {
        const char *prefix = "cannot parse '";
        const char *suffix = "' as integer";
        size_t len = strlen(prefix) + strlen(s) + strlen(suffix) + 1;
        char *buf = rt_str_alloc(len - 1);
        snprintf(buf, len, "%s%s%s", prefix, s, suffix);
        return rt_result_err((long long)(intptr_t)buf);
    }
    return rt_result_ok(val);
}

void *rt_str_to_float(const char *s) {
    if (!s) {
        const char *msg = "cannot parse empty string as float";
        char *buf = rt_str_alloc(strlen(msg));
        memcpy(buf, msg, strlen(msg) + 1);
        return rt_result_err((long long)(intptr_t)buf);
    }
    char *endptr;
    double val = strtod(s, &endptr);
    if (endptr == s || *endptr != '\0') {
        const char *prefix = "cannot parse '";
        const char *suffix = "' as float";
        size_t len = strlen(prefix) + strlen(s) + strlen(suffix) + 1;
        char *buf = rt_str_alloc(len - 1);
        snprintf(buf, len, "%s%s%s", prefix, s, suffix);
        return rt_result_err((long long)(intptr_t)buf);
    }
    /* Store the f64 bit pattern in the i64 value slot */
    long long bits;
    memcpy(&bits, &val, sizeof(bits));
    return rt_result_ok(bits);
}

/* ── Async runtime ─────────────────────────────────────────────── */

#ifdef _WIN32
#include <windows.h>
void rt_sleep_ms(long long ms) { Sleep((DWORD)ms); }
#else
#include <unistd.h>
void rt_sleep_ms(long long ms) {
    struct timespec ts = { ms / 1000, (ms % 1000) * 1000000 };
    nanosleep(&ts, NULL);
}
#endif

typedef struct {
    long long (*thunk)(void *);
    void *args_ptr;    /* malloc'd copy of the args struct — owned by ctx */
    long long result;
} spawn_ctx;

static void *spawn_thread_fn(void *arg) {
    spawn_ctx *ctx = (spawn_ctx *)arg;
    ctx->result = ctx->thunk(ctx->args_ptr);
    /* The args struct is a private malloc'd copy (see rt_spawn_with_args);
     * free it now that the thunk has consumed it. Any heap string args it
     * held were themselves copied to independent refcounted allocations. The
     * generated thunk releases those string slots after the callee returns, so
     * we must not free them here. */
    free(ctx->args_ptr);
    ctx->args_ptr = NULL;
    return NULL;
}

/* Spawn a thunk on a new OS thread with a packed args struct
 * `[fn_ptr, arg0, arg1, ...]` (num_args + 1 eight-byte slots).
 *
 * Arena-escape fix (issue #56): the args struct is allocated by the caller
 * via rt_struct_alloc, which — when invoked inside an HTTP request handler —
 * places it in the per-request bump arena (RT_RC_ARENA). The spawned thread
 * outlives the request, so it would read freed memory once rt_arena_end()
 * reclaims the arena. We therefore copy the struct into a plain malloc'd
 * buffer that ctx owns, and deep-copy every flagged string argument into an
 * independent refcounted heap allocation before handing it to the thread.
 * `ptr_mask` bit i marks arg slot i as a string pointer eligible for this
 * copy; `num_args` is the argument count. The JIT twin in src/runtime.rs
 * mirrors this behaviour. */
void *rt_spawn_with_args(long long (*thunk)(void *), void *args_ptr,
                         long long ptr_mask, long long num_args) {
    /* Use malloc directly — spawn context must outlive the current arena
     * since the spawned thread runs independently of the request lifecycle. */
    spawn_ctx *ctx = (spawn_ctx *)malloc(sizeof(spawn_ctx));
    if (!ctx) { fprintf(stderr, "runtime error: out of memory (spawn)\n"); exit(1); }
    ctx->thunk = thunk;
    ctx->result = 0;

    /* Copy the args struct out of any per-request arena into a private
     * malloc'd buffer: slot 0 is the target fn_ptr, slots 1..num_args are the
     * arguments. */
    if (num_args < 0) num_args = 0;
    size_t slots = (size_t)num_args + 1;
    long long *src = (long long *)args_ptr;
    long long *copy = (long long *)malloc(slots * sizeof(long long));
    if (!copy) { fprintf(stderr, "runtime error: out of memory (spawn args)\n"); exit(1); }
    if (src) {
        memcpy(copy, src, slots * sizeof(long long));
    } else {
        memset(copy, 0, slots * sizeof(long long));
    }

    /* Deep-copy flagged string arguments so the spawned thread owns an
     * independent allocation. Detach the current arena while copying so the
     * copy is made through malloc (refcount 1), not back into the arena we are
     * escaping. rt_str_copy_len otherwise honours t_current_arena. */
    if (ptr_mask != 0) {
        turbo_arena *saved_arena = t_current_arena;
        t_current_arena = NULL;
        for (long long i = 0; i < num_args; i++) {
            if (!((ptr_mask >> i) & 1)) continue;
            char *s = (char *)(size_t)copy[i + 1];
            if (!s) continue;
            copy[i + 1] = (long long)(size_t)rt_str_copy_len(s, strlen(s));
        }
        t_current_arena = saved_arena;
    }

    ctx->args_ptr = copy;
    pthread_t *handle = (pthread_t *)malloc(sizeof(pthread_t) + sizeof(spawn_ctx *));
    if (!handle) { fprintf(stderr, "runtime error: out of memory (spawn)\n"); exit(1); }
    /* Store ctx pointer right after the pthread_t */
    *((spawn_ctx **)(handle + 1)) = ctx;
    /* Check the return: on failure (e.g. EAGAIN under thread exhaustion) the
     * pthread_t is left uninitialized, and rt_await_handle would then join a
     * garbage handle — undefined behaviour. Fail cleanly instead. */
    int rc = pthread_create(handle, NULL, spawn_thread_fn, ctx);
    if (rc != 0) {
        fprintf(stderr, "runtime error: failed to spawn thread (%s)\n", strerror(rc));
        free(ctx);
        free(handle);
        exit(1);
    }
    return handle;
}

long long rt_await_handle(void *handle_ptr) {
    if (!handle_ptr) return 0;
    pthread_t *handle = (pthread_t *)handle_ptr;
    spawn_ctx *ctx = *((spawn_ctx **)(handle + 1));
    pthread_join(*handle, NULL);
    long long result = ctx->result;
    free(ctx);
    free(handle);
    return result;
}

/* ── HTTP + JSON builtins ───────────────────────────────────────────── */

/* Helper: read all data from a file descriptor into an arena-managed string.
 *
 * IMPORTANT: we grow the staging buffer with libc malloc/realloc rather than
 * turbo_realloc, because when this function runs inside a request arena the
 * arena's "realloc" can only copy size/2 bytes on each growth step (it does
 * not track prior allocation sizes), which corrupts anything >= 8 KiB. By
 * using real realloc during the read loop and copying to the arena exactly
 * once at the end, callers still receive an arena-lifetime pointer while
 * the growing code avoids the arena-realloc pitfall. */
static char *read_fd_to_string(int fd) {
    size_t cap = 4096, len = 0;
    char *tmp = (char *)malloc(cap);
    if (!tmp) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    while (1) {
        ssize_t n = read(fd, tmp + len, cap - len - 1);
        if (n <= 0) break;
        len += (size_t)n;
        if (len + 1 >= cap) {
            cap *= 2;
            char *grown = (char *)realloc(tmp, cap);
            if (!grown) {
                free(tmp);
                fprintf(stderr, "runtime error: out of memory\n");
                exit(1);
            }
            tmp = grown;
        }
    }
    tmp[len] = '\0';
    /* Copy the final payload into an arena-managed slot so the returned
     * pointer has the lifetime callers expect, then release the libc buffer. */
    char *result = rt_str_alloc(len);
    memcpy(result, tmp, len + 1);
    free(tmp);
    return result;
}

/* Returns 1 if url starts with "http://" or "https://" (case-insensitive),
 * 0 otherwise. Rejects NULL/empty. Used to gate rt_http_get/rt_http_post
 * against SSRF via file://, gopher://, ftp://, etc., and to reject any
 * string that would be interpreted by curl as a flag. */
static int rt_url_is_http(const char *url) {
    if (!url || url[0] == '\0') return 0;
    /* Explicitly reject anything that looks like a flag. curl treats a
     * leading '-' as a flag even when passed as a positional argument
     * unless preceded by `--`, but we belt-and-suspenders this anyway. */
    if (url[0] == '-') return 0;
    if (strncasecmp(url, "http://", 7) == 0) return 1;
    if (strncasecmp(url, "https://", 8) == 0) return 1;
    return 0;
}

/* ── SSRF guard: block loopback / private / link-local destinations ────
 *
 * rt_url_is_http() only validates the *scheme*. By itself that still lets a
 * program reach internal services (databases, admin panels) and — most
 * dangerously — cloud instance-metadata endpoints (169.254.169.254), which
 * is a classic SSRF pivot. The helpers below additionally inspect the host.
 *
 * Default: ON (block private hosts). Opt out with TURBO_ALLOW_PRIVATE_HOSTS=1
 * for trusted environments that legitimately call localhost / internal IPs.
 * We chose default-on because it is the safer default for a language whose
 * http_get/http_post take attacker-influenceable URLs; the escape hatch keeps
 * local development workflows (hitting 127.0.0.1) one env var away.
 *
 * Scope / known gap: we parse numeric IP literals (dotted-quad plus the
 * inet_aton shorthand/octal/hex forms attackers use to smuggle private IPs)
 * and the `localhost` name. We deliberately do NOT resolve arbitrary DNS
 * names here (no getaddrinfo): doing so would add a TOCTOU window versus
 * curl's own resolution and is out of scope for this surgical guard. A
 * hostname that resolves to a private address via DNS rebinding is therefore
 * not caught — documented so operators can layer network egress controls. */

/* Minimal inet_aton-style parser. Accepts the numeric host forms the C
 * resolver (and thus curl) accepts: 1–4 dotted parts, each decimal/octal
 * (leading 0) or hex (leading 0x). Returns 1 and writes the address in host
 * byte order on success, 0 if `host` is not a numeric IPv4 literal. */
static int rt_host_ipv4(const char *host, uint32_t *out) {
    uint32_t parts[4];
    int n = 0;
    const char *p = host;
    if (*p == '\0') return 0;
    while (n < 4) {
        char *end = NULL;
        unsigned long v = strtoul(p, &end, 0); /* base 0: 0x hex, 0 octal */
        if (end == p) return 0;                /* no digits consumed */
        if (v > 0xffffffffUL) return 0;        /* part too large */
        parts[n++] = (uint32_t)v;
        p = end;
        if (*p == '.') {
            p++;
            if (*p == '\0') return 0;          /* trailing dot */
        } else {
            break;
        }
    }
    if (*p != '\0') return 0;                  /* trailing junk */

    uint32_t addr;
    switch (n) {
        case 1:
            addr = parts[0];
            break;
        case 2: /* a.b  -> a.bbbbbb */
            if (parts[0] > 0xff || parts[1] > 0xffffff) return 0;
            addr = (parts[0] << 24) | parts[1];
            break;
        case 3: /* a.b.c -> a.b.cccc */
            if (parts[0] > 0xff || parts[1] > 0xff || parts[2] > 0xffff) return 0;
            addr = (parts[0] << 24) | (parts[1] << 16) | parts[2];
            break;
        case 4: /* a.b.c.d */
            if (parts[0] > 0xff || parts[1] > 0xff ||
                parts[2] > 0xff || parts[3] > 0xff) return 0;
            addr = (parts[0] << 24) | (parts[1] << 16) |
                   (parts[2] << 8) | parts[3];
            break;
        default:
            return 0;
    }
    *out = addr;
    return 1;
}

/* 1 if the host-byte-order IPv4 address is loopback / private / link-local. */
static int rt_ipv4_is_blocked(uint32_t a) {
    unsigned int o1 = (a >> 24) & 0xff;
    unsigned int o2 = (a >> 16) & 0xff;
    if (o1 == 0)   return 1;                          /* 0.0.0.0/8 "this host" */
    if (o1 == 127) return 1;                          /* 127.0.0.0/8 loopback */
    if (o1 == 10)  return 1;                          /* 10.0.0.0/8 private */
    if (o1 == 172 && o2 >= 16 && o2 <= 31) return 1;  /* 172.16.0.0/12 private */
    if (o1 == 192 && o2 == 168) return 1;             /* 192.168.0.0/16 private */
    if (o1 == 169 && o2 == 254) return 1;             /* 169.254.0.0/16 link-local
                                                       * incl. 169.254.169.254 metadata */
    return 0;
}

/* 1 if the (scheme-stripped, port-stripped) host should be blocked. */
static int rt_host_is_blocked(const char *host) {
    if (host[0] == '\0') return 0;
    if (strcasecmp(host, "localhost") == 0) return 1;

    if (strchr(host, ':') != NULL) {
        /* IPv6 textual literal (brackets already stripped by the caller). */
        if (strcmp(host, "::1") == 0) return 1;            /* loopback */
        if (strcmp(host, "::") == 0) return 1;             /* unspecified */
        if (strncasecmp(host, "fe80:", 5) == 0) return 1;  /* link-local */
        if (strncasecmp(host, "fc", 2) == 0 ||
            strncasecmp(host, "fd", 2) == 0) return 1;     /* fc00::/7 unique-local */
        /* IPv4-mapped form e.g. ::ffff:127.0.0.1 — classify the trailing
         * dotted-quad if present. */
        const char *last = strrchr(host, ':');
        uint32_t v4;
        if (last && rt_host_ipv4(last + 1, &v4) && rt_ipv4_is_blocked(v4)) return 1;
        return 0;
    }

    uint32_t v4;
    if (rt_host_ipv4(host, &v4)) return rt_ipv4_is_blocked(v4);
    return 0; /* a regular domain name — not resolved here (documented gap) */
}

/* Copy the host portion of an http(s) URL into `out` (size `out_cap`).
 * Strips scheme, userinfo (user:pass@), port, and IPv6 brackets.
 * Returns 1 on success, 0 if no host could be extracted. */
static int rt_url_extract_host(const char *url, char *out, size_t out_cap) {
    const char *p = url;
    if (strncasecmp(p, "http://", 7) == 0) p += 7;
    else if (strncasecmp(p, "https://", 8) == 0) p += 8;
    else return 0;

    /* authority ends at the first '/', '?', '#', or NUL */
    const char *auth_end = p;
    while (*auth_end && *auth_end != '/' && *auth_end != '?' && *auth_end != '#')
        auth_end++;

    /* userinfo: host starts after the last '@' inside the authority */
    const char *host = p;
    for (const char *q = p; q < auth_end; q++) {
        if (*q == '@') host = q + 1;
    }

    const char *host_end;
    if (*host == '[') {
        host++; /* skip '[' */
        host_end = host;
        while (host_end < auth_end && *host_end != ']') host_end++;
    } else {
        host_end = host;
        while (host_end < auth_end && *host_end != ':') host_end++;
    }

    size_t len = (size_t)(host_end - host);
    if (len == 0 || len >= out_cap) return 0;
    memcpy(out, host, len);
    out[len] = '\0';
    return 1;
}

/* Returns NULL if the URL is allowed, or a static human-readable reason if it
 * should be blocked. Combines the scheme check with the SSRF host check. */
static const char *rt_http_url_blocked_reason(const char *url) {
    if (!rt_url_is_http(url)) return "non-http(s) scheme";

    /* Opt-out for trusted environments. */
    const char *allow = getenv("TURBO_ALLOW_PRIVATE_HOSTS");
    if (allow && allow[0] == '1' && allow[1] == '\0') return NULL;

    char host[256];
    if (!rt_url_extract_host(url, host, sizeof(host))) {
        /* Could not isolate a host: either empty, or longer than the buffer.
         * A valid DNS name is <= 253 chars, so an over-length host is never
         * legitimate — fail CLOSED (block) rather than open. Otherwise an
         * attacker could pad an octal/decimal numeric IP past the 256-byte
         * buffer to make extraction fail and skip the SSRF check entirely. */
        return "unparseable or over-length host blocked "
               "(set TURBO_ALLOW_PRIVATE_HOSTS=1 to allow)";
    }
    if (rt_host_is_blocked(host)) {
        return "private/loopback host blocked "
               "(set TURBO_ALLOW_PRIVATE_HOSTS=1 to allow)";
    }
    return NULL;
}

static const char *rt_http_empty_response(void) {
    return rt_str_empty();
}

/* http_get(url) -> str — HTTP GET via fork+exec (no shell interpolation) */
const char *rt_http_get(const char *url) {
    const char *blocked = rt_http_url_blocked_reason(url);
    if (blocked) {
        fprintf(stderr, "[rt_http] blocked URL (%s): %s\n",
                blocked, url ? url : "(null)");
        return rt_http_empty_response();
    }
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return turbo_strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return turbo_strdup("error: cannot fork");
    }
    if (pid == 0) {
        /* Child: redirect stdout to pipe, exec curl.
         * Notes:
         *   --proto =http,https  lock to http(s) even after redirects
         *   --max-time 30        hard timeout for DoS protection
         *   --max-redirs 5       cap redirect chain
         *   --                   terminate flags so a URL like "--help"
         *                        cannot be re-interpreted */
        close(pipefd[0]);
        dup2(pipefd[1], STDOUT_FILENO);
        close(pipefd[1]);
        execlp("curl", "curl", "-s", "-L",
               "--proto", "=http,https",
               "--max-time", "30",
               "--max-redirs", "5",
               "--", url, (char *)NULL);
        _exit(1);
    }
    /* Parent: read from pipe */
    close(pipefd[1]);
    char *buf = read_fd_to_string(pipefd[0]);
    close(pipefd[0]);
    int status;
    waitpid(pid, &status, 0);
    return buf;
}

/* http_post(url, body) -> str — HTTP POST via fork+exec (no shell interpolation) */
const char *rt_http_post(const char *url, const char *body) {
    const char *blocked = rt_http_url_blocked_reason(url);
    if (blocked) {
        fprintf(stderr, "[rt_http] blocked URL (%s): %s\n",
                blocked, url ? url : "(null)");
        return rt_http_empty_response();
    }
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return turbo_strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return turbo_strdup("error: cannot fork");
    }
    if (pid == 0) {
        /* Child: redirect stdout to pipe, exec curl with POST */
        close(pipefd[0]);
        dup2(pipefd[1], STDOUT_FILENO);
        close(pipefd[1]);
        execlp("curl", "curl", "-s", "-L", "-X", "POST",
               "-H", "Content-Type: application/json",
               "--proto", "=http,https",
               "--max-time", "30",
               "--max-redirs", "5",
               "-d", body ? body : "",
               "--", url, (char *)NULL);
        _exit(1);
    }
    /* Parent: read from pipe */
    close(pipefd[1]);
    char *buf = read_fd_to_string(pipefd[0]);
    close(pipefd[0]);
    int status;
    waitpid(pid, &status, 0);
    return buf;
}

/* http_post_with_headers(url, body, headers) -> str — HTTP POST with custom
 * headers via fork+exec (no shell interpolation). The `headers` parameter is
 * a single string with headers separated by '\n', e.g.:
 *   "Content-Type: application/json\nAuthorization: Bearer sk-xxx"
 * Each header becomes a separate -H argument to curl. */
const char *rt_http_post_with_headers(const char *url, const char *body,
                                      const char *headers) {
    const char *blocked = rt_http_url_blocked_reason(url);
    if (blocked) {
        fprintf(stderr, "[rt_http] blocked URL (%s): %s\n",
                blocked, url ? url : "(null)");
        return rt_http_empty_response();
    }
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return turbo_strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return turbo_strdup("error: cannot fork");
    }
    if (pid == 0) {
        /* Child: redirect stdout to pipe, exec curl with POST + custom headers */
        close(pipefd[0]);
        dup2(pipefd[1], STDOUT_FILENO);
        close(pipefd[1]);

        /* Count headers (number of '\n' + 1 if non-empty) */
        int num_headers = 0;
        if (headers && headers[0] != '\0') {
            num_headers = 1;
            for (const char *h = headers; *h; h++) {
                if (*h == '\n') num_headers++;
            }
        }

        /* Build argv: curl -s -L -X POST [-H hdr]... --proto ... -d body -- url NULL
         * Fixed args: curl, -s, -L, -X, POST, --proto, =http,https,
         *             --max-time, 30, --max-redirs, 5, -d, body, --, url, NULL = 16
         * Each header adds 2 entries: -H, header_value */
        int fixed_args = 16;
        int total = fixed_args + num_headers * 2;
        char **argv = (char **)malloc((size_t)total * sizeof(char *));
        if (!argv) _exit(1);

        int ai = 0;
        argv[ai++] = "curl";
        argv[ai++] = "-s";
        argv[ai++] = "-L";
        argv[ai++] = "-X";
        argv[ai++] = "POST";

        /* Parse headers string and add -H args */
        if (num_headers > 0) {
            /* Make a mutable copy to split on '\n' */
            size_t hlen = strlen(headers);
            char *hcopy = (char *)malloc(hlen + 1);
            if (!hcopy) _exit(1);
            memcpy(hcopy, headers, hlen + 1);
            char *start = hcopy;
            for (char *p = hcopy; ; p++) {
                if (*p == '\n' || *p == '\0') {
                    char was_end = (*p == '\0');
                    *p = '\0';
                    if (start[0] != '\0') { /* skip empty lines */
                        argv[ai++] = "-H";
                        argv[ai++] = start;
                    }
                    if (was_end) break;
                    start = p + 1;
                }
            }
            /* Note: hcopy is intentionally leaked in the child — exec replaces
             * the process image immediately, so no cleanup is needed. */
        }

        argv[ai++] = "--proto";
        argv[ai++] = "=http,https";
        argv[ai++] = "--max-time";
        argv[ai++] = "30";
        argv[ai++] = "--max-redirs";
        argv[ai++] = "5";
        argv[ai++] = "-d";
        argv[ai++] = (char *)(body ? body : "");
        argv[ai++] = "--";
        argv[ai++] = (char *)url;
        argv[ai] = NULL;

        execvp("curl", argv);
        _exit(1);
    }
    /* Parent: read from pipe */
    close(pipefd[1]);
    char *buf = read_fd_to_string(pipefd[0]);
    close(pipefd[0]);
    int status;
    waitpid(pid, &status, 0);
    return buf;
}

/* shell_exec(cmd) / exec(cmd) -> str — execute a command via fork+execvp,
 * capture stdout+stderr.
 *
 * Security: we DO NOT spawn /bin/sh -c. Any shell metacharacter in `cmd`
 * causes the call to be rejected outright. Accepted commands are tokenized
 * on whitespace and handed to execvp() directly, so there is no shell to
 * interpret quotes, redirections, substitutions, or pipelines. Callers that
 * genuinely need a pipeline should compose one in Turbo code. */
static const char *rt_exec_empty_response(void) {
    return rt_str_empty();
}

#define RT_EXEC_MAX_ARGS 64

const char *rt_exec(const char *cmd) {
    if (!cmd || cmd[0] == '\0') {
        return rt_exec_empty_response();
    }
    /* Reject shell metacharacters before we do anything else. The set here
     * covers command separators, pipes, redirections, subshells, command
     * substitution, env expansion, line continuations, and backticks. */
    for (const char *p = cmd; *p; p++) {
        char c = *p;
        if (c == ';' || c == '|' || c == '&' || c == '$' || c == '`' ||
            c == '(' || c == ')' || c == '<' || c == '>' || c == '\n' ||
            c == '\\') {
            fprintf(stderr,
                "rt_exec: refusing command with shell metacharacter: %s\n",
                cmd);
            return rt_exec_empty_response();
        }
    }
    /* Tokenize on whitespace into an argv vector. We copy cmd first because
     * strtok_r mutates its input. */
    size_t cmd_len = strlen(cmd);
    char *cmd_copy = (char *)malloc(cmd_len + 1);
    if (!cmd_copy) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    memcpy(cmd_copy, cmd, cmd_len + 1);

    char *argv[RT_EXEC_MAX_ARGS + 1];
    int argc = 0;
    char *saveptr = NULL;
    char *tok = strtok_r(cmd_copy, " \t\r\n\v\f", &saveptr);
    while (tok) {
        if (argc >= RT_EXEC_MAX_ARGS) {
            fprintf(stderr,
                "rt_exec: refusing command with too many arguments (>%d): %s\n",
                RT_EXEC_MAX_ARGS, cmd);
            free(cmd_copy);
            return rt_exec_empty_response();
        }
        argv[argc++] = tok;
        tok = strtok_r(NULL, " \t\r\n\v\f", &saveptr);
    }
    argv[argc] = NULL;
    if (argc == 0) {
        free(cmd_copy);
        return rt_exec_empty_response();
    }

    int pipefd[2];
    if (pipe(pipefd) != 0) {
        free(cmd_copy);
        return turbo_strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        free(cmd_copy);
        return turbo_strdup("error: cannot fork");
    }
    if (pid == 0) {
        /* Child: redirect stdout+stderr to pipe, exec directly (no shell). */
        close(pipefd[0]);
        dup2(pipefd[1], STDOUT_FILENO);
        dup2(pipefd[1], STDERR_FILENO);
        close(pipefd[1]);
        execvp(argv[0], argv);
        _exit(1);
    }
    /* Parent: read from pipe */
    close(pipefd[1]);
    char *buf = read_fd_to_string(pipefd[0]);
    close(pipefd[0]);
    int status;
    waitpid(pid, &status, 0);
    free(cmd_copy);
    return buf;
}

/* env_get(name) -> str — get an environment variable, returns "" if not set */
const char *rt_env_get(const char *name) {
    if (!name) {
        return rt_str_empty();
    }
    const char *val = getenv(name);
    if (!val) {
        return rt_str_empty();
    }
    return turbo_strdup(val);
}

static char *rt_json_escape_dup(const char *s) {
    if (!s) {
        return rt_str_empty();
    }
    size_t len = strlen(s);
    char *out = rt_str_alloc(len * 2);
    size_t j = 0;
    for (size_t i = 0; i < len; i++) {
        char c = s[i];
        if (c == '\\' || c == '"') {
            out[j++] = '\\';
            out[j++] = c;
        } else if (c == '\n') {
            out[j++] = '\\';
            out[j++] = 'n';
        } else if (c == '\r') {
            out[j++] = '\\';
            out[j++] = 'r';
        } else if (c == '\t') {
            out[j++] = '\\';
            out[j++] = 't';
        } else if (c == '\b') {
            out[j++] = '\\';
            out[j++] = 'b';
        } else if (c == '\f') {
            out[j++] = '\\';
            out[j++] = 'f';
        } else {
            out[j++] = c;
        }
    }
    out[j] = '\0';
    return out;
}

/* json_get(json, key) -> str — extract top-level key value from JSON string.
 * Walks the JSON character-by-character, tracking brace/bracket depth so that
 * only keys at depth 1 (the top-level object) are matched. Properly skips
 * over string literals including escaped quotes. */
const char *rt_json_get(const char *json, const char *key) {
    if (!json || !key) return turbo_strdup("");
    size_t json_len = strlen(json);
    const char *json_end = json + json_len;
    size_t klen = strlen(key);

    /* Walk the JSON, tracking depth. We want to match "key" only at depth 1. */
    int depth = 0;
    const char *p = json;
    const char *pos = NULL;

    while (p < json_end) {
        char c = *p;

        /* Skip over string literals */
        if (c == '"') {
            const char *str_start = p;
            p++; /* skip opening quote */
            /* Scan to closing quote, handling backslash escapes */
            while (p < json_end) {
                if (*p == '\\') {
                    p += 2; /* skip escaped character */
                    continue;
                }
                if (*p == '"') break;
                p++;
            }
            /* p now points at closing quote (or end of string) */
            if (p >= json_end) break;

            /* Check if this quoted string is our key at depth 1 */
            if (depth == 1) {
                /* The string content is between str_start+1 and p */
                size_t slen = (size_t)(p - str_start - 1);
                if (slen == klen && memcmp(str_start + 1, key, klen) == 0) {
                    /* Verify this is a key by checking for ':' after the quote */
                    const char *after = p + 1;
                    while (after < json_end && (*after == ' ' || *after == '\t' ||
                           *after == '\n' || *after == '\r')) after++;
                    if (after < json_end && *after == ':') {
                        pos = p; /* found our key — pos points at closing quote */
                        break;
                    }
                }
            }
            p++; /* skip closing quote */
            continue;
        }

        if (c == '{' || c == '[') {
            depth++;
        } else if (c == '}' || c == ']') {
            depth--;
        }
        p++;
    }

    if (!pos) return turbo_strdup("");

    /* Advance past closing quote of the key, skip whitespace and colon */
    pos += 1;
    if (pos >= json_end) return turbo_strdup("");
    while (pos < json_end && (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r')) pos++;
    if (pos >= json_end || *pos != ':') return turbo_strdup("");
    pos++;
    if (pos >= json_end) return turbo_strdup("");
    while (pos < json_end && (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r')) pos++;
    if (pos >= json_end) return turbo_strdup("");

    if (*pos == '"') {
        /* String value — use proper escape scanning like the key scanner */
        pos++;
        const char *start = pos;
        while (pos < json_end) {
            if (*pos == '\\') { pos += 2; continue; }
            if (*pos == '"') break;
            pos++;
        }
        size_t vlen = pos - start;
        char *val = rt_str_alloc(vlen);
        memcpy(val, start, vlen);
        val[vlen] = '\0';
        return val;
    } else {
        /* Number, bool, or null */
        const char *start = pos;
        while (*pos && *pos != ',' && *pos != '}' && *pos != ']' &&
               *pos != ' ' && *pos != '\n' && *pos != '\r' && *pos != '\t') pos++;
        size_t vlen = pos - start;
        char *val = rt_str_alloc(vlen);
        memcpy(val, start, vlen);
        val[vlen] = '\0';
        return val;
    }
}

/* json_stringify(key, value) -> str — build {"key":"value"} */
const char *rt_json_stringify(const char *key, const char *value) {
    const char *esc_key = rt_json_escape_dup(key);
    const char *esc_value = rt_json_escape_dup(value);
    size_t eklen = strlen(esc_key);
    size_t evlen = strlen(esc_value);
    size_t cap = eklen + evlen + 8;
    char *buf = rt_str_alloc(cap - 1);
    snprintf(buf, cap, "{\"%s\":\"%s\"}", esc_key, esc_value);
    rt_release((void *)esc_key);
    rt_release((void *)esc_value);
    return buf;
}

/* json_build(pairs) -> str — build a JSON object from key-value pairs.
 * The `pairs` string uses ASCII Unit Separator (\x1F, 0x1F) as delimiter:
 *   "key1\x1Fvalue1\x1Fkey2\x1Fvalue2"
 * Returns: {"key1":"value1","key2":"value2"} */
const char *rt_json_build(const char *pairs) {
    if (!pairs || pairs[0] == '\0') return turbo_strdup("{}");

    /* Count tokens (number of \x1F separators + 1) */
    size_t num_tokens = 1;
    for (const char *p = pairs; *p; p++) {
        if (*p == '\x1F') num_tokens++;
    }
    /* Must have even number of tokens (key-value pairs) */
    if (num_tokens % 2 != 0) num_tokens--; /* ignore trailing key with no value */
    size_t num_pairs = num_tokens / 2;
    if (num_pairs == 0) return turbo_strdup("{}");

    /* Parse tokens into an array */
    size_t pairs_len = strlen(pairs);
    char *copy = (char *)malloc(pairs_len + 1);
    if (!copy) return turbo_strdup("{}");
    memcpy(copy, pairs, pairs_len + 1);

    /* Split on \x1F */
    const char **tokens = (const char **)malloc(num_tokens * sizeof(char *));
    if (!tokens) { free(copy); return turbo_strdup("{}"); }
    size_t ti = 0;
    tokens[ti++] = copy;
    for (char *p = copy; *p; p++) {
        if (*p == '\x1F') {
            *p = '\0';
            if (ti < num_tokens) tokens[ti++] = p + 1;
        }
    }

    /* Escape keys and values, compute total size */
    size_t total = 2; /* { and } */
    const char **esc_keys = (const char **)malloc(num_pairs * sizeof(char *));
    const char **esc_vals = (const char **)malloc(num_pairs * sizeof(char *));
    size_t *eklen = (size_t *)malloc(num_pairs * sizeof(size_t));
    size_t *evlen = (size_t *)malloc(num_pairs * sizeof(size_t));
    if (!esc_keys || !esc_vals || !eklen || !evlen) {
        free(copy); free(tokens);
        free(esc_keys); free(esc_vals); free(eklen); free(evlen);
        return turbo_strdup("{}");
    }

    for (size_t i = 0; i < num_pairs; i++) {
        esc_keys[i] = rt_json_escape_dup(tokens[i * 2]);
        esc_vals[i] = rt_json_escape_dup(tokens[i * 2 + 1]);
        eklen[i] = strlen(esc_keys[i]);
        evlen[i] = strlen(esc_vals[i]);
        /* "key":"value" = 1+eklen+1+1+1+evlen+1 = eklen+evlen+5 */
        total += eklen[i] + evlen[i] + 5;
        if (i > 0) total++; /* comma separator */
    }
    total++; /* null terminator */

    char *buf = rt_str_alloc(total - 1);
    size_t pos = 0;
    buf[pos++] = '{';
    for (size_t i = 0; i < num_pairs; i++) {
        if (i > 0) buf[pos++] = ',';
        buf[pos++] = '"';
        memcpy(buf + pos, esc_keys[i], eklen[i]); pos += eklen[i];
        buf[pos++] = '"';
        buf[pos++] = ':';
        buf[pos++] = '"';
        memcpy(buf + pos, esc_vals[i], evlen[i]); pos += evlen[i];
        buf[pos++] = '"';
    }
    buf[pos++] = '}';
    buf[pos] = '\0';

    for (size_t i = 0; i < num_pairs; i++) {
        rt_release((void *)esc_keys[i]);
        rt_release((void *)esc_vals[i]);
    }
    /* Cleanup temporary allocations. */
    free(copy);
    free(tokens);
    free(esc_keys);
    free(esc_vals);
    free(eklen);
    free(evlen);

    return buf;
}

const char *rt_json_root(const char *json) {
    if (!json) return turbo_strdup("");
    while (*json == ' ' || *json == '\n' || *json == '\r' || *json == '\t') json++;
    size_t len = strlen(json);
    while (len > 0 && (json[len - 1] == ' ' || json[len - 1] == '\n' || json[len - 1] == '\r' || json[len - 1] == '\t')) len--;
    if (len >= 2 && json[0] == '"' && json[len - 1] == '"') {
        /* Escape sequences shrink (e.g. \\n -> \n), so output <= input size.
           len-2 chars of content + NUL = len-1 bytes is always sufficient. */
        char *out = rt_str_alloc(len - 2);
        size_t j = 0;
        for (size_t i = 1; i + 1 < len; i++) {
            if (json[i] == '\\' && i + 1 < len - 1) {
                i++;
                switch (json[i]) {
                    case 'n': out[j++] = '\n'; break;
                    case 'r': out[j++] = '\r'; break;
                    case 't': out[j++] = '\t'; break;
                    case 'b': out[j++] = '\b'; break;
                    case 'f': out[j++] = '\f'; break;
                    case '"': out[j++] = '"'; break;
                    case '\\': out[j++] = '\\'; break;
                    default: out[j++] = json[i]; break;
                }
            } else {
                out[j++] = json[i];
            }
        }
        out[j] = '\0';
        return out;
    }
    char *out = rt_str_alloc(len);
    memcpy(out, json, len);
    out[len] = '\0';
    return out;
}

long long rt_str_to_i64(const char *s) {
    if (!s) return 0;
    return strtoll(s, NULL, 10);
}

double rt_str_to_f64(const char *s) {
    if (!s) return 0.0;
    return strtod(s, NULL);
}

char rt_str_to_bool(const char *s) {
    if (!s) return 0;
    return strcasecmp(s, "true") == 0 ? 1 : 0;
}

long long rt_float_to_int(double f) {
    return (long long)f;
}

double rt_int_to_float(long long i) {
    return (double)i;
}

const char *rt_str_from_char(long long code) {
    char *s = rt_str_alloc(1);
    s[0] = (char)(code & 0xFF);
    s[1] = '\0';
    return s;
}

/* ── Channel runtime ────────────────────────────────────────────────── */

/* Channel: a struct of [sender_pipe_write_fd, receiver_pipe_read_fd]
 * For simplicity in C AOT, we implement channels using a linked-list queue
 * protected by a mutex and condition variable.
 */

typedef struct channel_node {
    long long value;
    struct channel_node *next;
} channel_node;

typedef struct {
    channel_node *head;
    channel_node *tail;
    pthread_mutex_t lock;
    pthread_cond_t cond;
} channel_queue;

typedef struct {
    long long sender_ptr;
    long long receiver_ptr;
} channel_handle;

void *rt_channel_create(void) {
    channel_queue *q = (channel_queue *)turbo_calloc(1, sizeof(channel_queue));
    pthread_mutex_init(&q->lock, NULL);
    pthread_cond_init(&q->cond, NULL);

    /* Data layout: [sender_ptr: i64][receiver_ptr: i64]; prefixed by
     * the shared [cap][refcount] header from rt_rc_alloc. */
    channel_handle *h = (channel_handle *)rt_rc_alloc(sizeof(channel_handle), 0);
    /* Both sender and receiver point to the same queue */
    h->sender_ptr = (long long)(size_t)q;
    h->receiver_ptr = (long long)(size_t)q;
    return h;
}

void rt_channel_send(const void *ch, long long value) {
    const channel_handle *h = (const channel_handle *)ch;
    channel_queue *q = (channel_queue *)(size_t)h->sender_ptr;

    /* Use malloc — channel nodes cross thread boundaries and must not
     * be allocated on the per-request arena. */
    channel_node *node = (channel_node *)malloc(sizeof(channel_node));
    if (!node) { fprintf(stderr, "runtime error: out of memory (channel)\n"); exit(1); }
    node->value = value;
    node->next = NULL;

    pthread_mutex_lock(&q->lock);
    if (q->tail) {
        q->tail->next = node;
    } else {
        q->head = node;
    }
    q->tail = node;
    pthread_cond_signal(&q->cond);
    pthread_mutex_unlock(&q->lock);
}

long long rt_channel_recv(const void *ch) {
    const channel_handle *h = (const channel_handle *)ch;
    channel_queue *q = (channel_queue *)(size_t)h->receiver_ptr;

    pthread_mutex_lock(&q->lock);
    while (q->head == NULL) {
        pthread_cond_wait(&q->cond, &q->lock);
    }
    channel_node *node = q->head;
    q->head = node->next;
    if (q->head == NULL) q->tail = NULL;
    pthread_mutex_unlock(&q->lock);

    long long val = node->value;
    free(node);
    return val;
}

void *rt_channel_clone_sender(const void *ch) {
    const channel_handle *h = (const channel_handle *)ch;
    channel_handle *nh = (channel_handle *)rt_rc_alloc(sizeof(channel_handle), 0);
    nh->sender_ptr = h->sender_ptr;
    nh->receiver_ptr = h->receiver_ptr;
    return nh;
}

/* ── Mutex runtime ─────────────────────────────────────────────────── */

typedef struct {
    long long value;
    pthread_mutex_t lock;
    _Atomic int refcount;
} turbo_mutex;

void *rt_mutex_create(long long value) {
    /* Mutex lives across threads — use malloc, not the arena. */
    turbo_mutex *m = (turbo_mutex *)malloc(sizeof(turbo_mutex));
    if (!m) { fprintf(stderr, "runtime error: mutex alloc failed\n"); exit(1); }
    m->value = value;
    pthread_mutex_init(&m->lock, NULL);
    atomic_store(&m->refcount, 1);
    return m;
}

long long rt_mutex_get(const void *mptr) {
    turbo_mutex *m = (turbo_mutex *)mptr;
    pthread_mutex_lock(&m->lock);
    long long val = m->value;
    pthread_mutex_unlock(&m->lock);
    return val;
}

void rt_mutex_set(const void *mptr, long long value) {
    turbo_mutex *m = (turbo_mutex *)mptr;
    pthread_mutex_lock(&m->lock);
    m->value = value;
    pthread_mutex_unlock(&m->lock);
}

/* Closure callback ABI: (env_ptr, old_value) -> new_value.
 * Closures are passed as a (fn_ptr, env_ptr) pair; see rt_http_route. */
typedef long long (*mutex_update_fn)(const void *, long long);

/* Atomic read-modify-write: runs `closure(old)` UNDER the lock, stores the
 * result, and returns the new value. This is the only way to express a
 * critical section spanning both a read and a write (e.g. a shared counter).
 * The closure must NOT touch the same mutex — pthread mutexes are not
 * recursive and would deadlock. The lock is released on the single normal
 * exit path; Turbo runtime errors abort the process rather than unwinding
 * through C, so the lock is never silently leaked into continued execution. */
long long rt_mutex_update(const void *mptr, const void *fn, const void *env_ptr) {
    turbo_mutex *m = (turbo_mutex *)mptr;
    mutex_update_fn cb = (mutex_update_fn)fn;
    pthread_mutex_lock(&m->lock);
    long long new_val = cb(env_ptr, m->value);
    m->value = new_val;
    pthread_mutex_unlock(&m->lock);
    return new_val;
}

void *rt_mutex_clone(const void *mptr) {
    turbo_mutex *m = (turbo_mutex *)mptr;
    atomic_fetch_add(&m->refcount, 1);
    return (void *)m;
}

void rt_mutex_drop(const void *mptr) {
    if (!mptr) return;
    turbo_mutex *m = (turbo_mutex *)mptr;
    if (atomic_fetch_sub(&m->refcount, 1) == 1) {
        pthread_mutex_destroy(&m->lock);
        free(m);
    }
}

/* ── HashMap runtime ─────────────────────────────────────────────────── */

/* Simple hash table using open addressing with linear probing.
 * Keys and values are C strings (owned copies).
 */

#define HASHMAP_INIT_CAP 16
#define HASHMAP_LOAD_FACTOR 0.75

/* A value is stored either as an owned string (`value`, with is_int == 0) or
 * inline as a 64-bit integer (`ivalue`, with is_int == 1). The inline-int
 * variant lets the str->int API (set_int / get_int / inc) avoid stringifying,
 * re-parsing, and re-allocating the value on every update — the hot path for
 * word-count style counters. str->str semantics are unchanged: rt_hashmap_get
 * and rt_hashmap_set always observe strings (an int entry is stringified on
 * demand by rt_hashmap_get). */
typedef struct {
    char *key;
    char *value;      /* owned string value; NULL when is_int */
    long long ivalue; /* inline integer value; valid when is_int */
    char occupied;
    char is_int; /* 1 if the value lives in ivalue, 0 if in value */
} hashmap_entry;

typedef struct {
    hashmap_entry *entries;
    long long capacity;
    long long count;
    /* BL-25 A2: lifetime scope of this map's *storage* (the entries array and
     * the strdup'd keys / string values). Set once at creation:
     *   1 (persistent) — the map was created outside any per-request arena
     *       (e.g. server state built in main()). Its storage uses real
     *       malloc/free so it survives rt_arena_end() and can be mutated
     *       across requests without dangling. This is the str->str twin of
     *       the JIT runtime, where Rust's HashMap owns its String key/values.
     *   0 (request-local) — the map was created while a request arena was
     *       active. Its storage lives in that arena and is reclaimed in bulk
     *       at rt_arena_end(), so a map built and dropped inside one handler
     *       does not leak.
     * Reads (rt_hashmap_get / rt_hashmap_keys) still hand back arena-scoped
     * copies regardless, since those results are request-local. */
    char persistent;
    /* ── Generic HashMap<K,V> descriptors (Tier 1.2) ──────────────────
     * Legacy `hashmap()` maps leave these at their zero defaults (str keys,
     * non-rc values) and go through the legacy str->str / str->int accessors
     * above, which keep their per-entry `is_int` polymorphism. A typed map
     * created via rt_hashmap_new_typed() sets a fixed key-kind and value-kind
     * and is only ever touched by the generic rt_hashmap_g* accessors, which
     * store the raw 8-byte value in `entry.ivalue` (is_int == 1). */
    char key_kind;  /* HM_KEY_STR (0) or HM_KEY_INT (1) */
    char val_is_rc; /* 1 if values are rc-heap pointers needing retain/release */
    long long refcount;
    void (*value_release_fn)(void *);
    void (*value_retain_fn)(void *);
} turbo_hashmap;

#define HM_KEY_STR 0
#define HM_KEY_INT 1

/* Duplicate `s` into storage whose lifetime matches the owning map's scope
 * (see turbo_hashmap.persistent). A persistent map gets a malloc'd copy that
 * outlives the per-request arena; a request-local map gets an arena copy.
 * The matching free is plain turbo_free(), which already frees malloc'd
 * pointers and no-ops on arena-backed ones. */
static char *hashmap_strdup(const turbo_hashmap *map, const char *s) {
    if (!s) return NULL;
    if (!map->persistent) {
        return turbo_raw_strdup(s);
    }
    size_t len = strlen(s) + 1;
    char *dup = (char *)malloc(len);
    if (!dup) {
        fprintf(stderr, "runtime error: out of memory (hashmap key/value)\n");
        exit(1);
    }
    memcpy(dup, s, len);
    return dup;
}

/* Allocate a zeroed entries array for `map` in the map's storage scope. */
static hashmap_entry *hashmap_alloc_entries(const turbo_hashmap *map, long long count) {
    size_t total;
    if (__builtin_mul_overflow((size_t)count, sizeof(hashmap_entry), &total)) {
        fprintf(stderr, "runtime error: hashmap capacity overflow\n");
        exit(1);
    }
    if (!map->persistent) {
        /* Request-local: arena storage, reclaimed at rt_arena_end(). */
        hashmap_entry *e = (hashmap_entry *)turbo_arena_alloc(t_current_arena, total);
        memset(e, 0, total);
        return e;
    }
    hashmap_entry *e = (hashmap_entry *)calloc((size_t)count, sizeof(hashmap_entry));
    if (!e) {
        fprintf(stderr, "runtime error: out of memory (hashmap entries)\n");
        exit(1);
    }
    return e;
}

static unsigned long hashmap_hash(const char *key) {
    unsigned long hash = 5381;
    int c;
    while ((c = (unsigned char)*key++))
        hash = ((hash << 5) + hash) + (unsigned long)c; /* hash * 33 + c */
    return hash;
}

static void hashmap_resize(turbo_hashmap *map, long long new_cap) {
    hashmap_entry *old = map->entries;
    long long old_cap = map->capacity;
    /* New bucket array must live in the map's storage scope, not whatever
     * arena happens to be active during the request that triggered the
     * resize (BL-25 A2). */
    map->entries = hashmap_alloc_entries(map, new_cap);
    map->capacity = new_cap;
    map->count = 0;
    for (long long i = 0; i < old_cap; i++) {
        if (old[i].occupied) {
            /* Re-insert */
            unsigned long h = hashmap_hash(old[i].key) % (unsigned long)new_cap;
            while (map->entries[h].occupied) {
                h = (h + 1) % (unsigned long)new_cap;
            }
            map->entries[h].key = old[i].key;
            map->entries[h].value = old[i].value;
            map->entries[h].ivalue = old[i].ivalue;
            map->entries[h].is_int = old[i].is_int;
            map->entries[h].occupied = 1;
            map->count++;
        }
    }
    turbo_free(old);
}

void *rt_hashmap_new(void) {
    turbo_hashmap *map = (turbo_hashmap *)turbo_alloc(sizeof(turbo_hashmap));
    /* A map created with no request arena active is server-scoped (persistent);
     * one created inside a handler is request-local. The struct itself follows
     * the same scope via turbo_alloc above, so it is reclaimed (request-local)
     * or lives for the process (persistent) consistently with its storage. */
    map->persistent = (t_current_arena == NULL) ? 1 : 0;
    map->entries = hashmap_alloc_entries(map, HASHMAP_INIT_CAP);
    map->capacity = HASHMAP_INIT_CAP;
    map->count = 0;
    /* Legacy maps: str keys, non-rc (stringified) values; the legacy accessors
     * ignore these fields but we set them so a shared read is well-defined. */
    map->key_kind = HM_KEY_STR;
    map->val_is_rc = 0;
    map->refcount = 1;
    map->value_release_fn = NULL;
    map->value_retain_fn = NULL;
    return map;
}

void rt_hashmap_set(void *map_ptr, const char *key, const char *value) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    /* Check if we need to resize */
    if ((double)(map->count + 1) > (double)map->capacity * HASHMAP_LOAD_FACTOR) {
        hashmap_resize(map, map->capacity * 2);
    }
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    while (map->entries[h].occupied) {
        if (strcmp(map->entries[h].key, key) == 0) {
            /* Update existing key. The previous value may have been an inline
             * int (value == NULL), in which case there is nothing to free. */
            if (map->entries[h].value) {
                turbo_free(map->entries[h].value);
            }
            map->entries[h].value = hashmap_strdup(map, value);
            map->entries[h].is_int = 0;
            return;
        }
        h = (h + 1) % (unsigned long)map->capacity;
    }
    map->entries[h].key = hashmap_strdup(map, key);
    map->entries[h].value = hashmap_strdup(map, value);
    map->entries[h].is_int = 0;
    map->entries[h].occupied = 1;
    map->count++;
}

const char *rt_hashmap_get(const void *map_ptr, const char *key) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (strcmp(map->entries[h].key, key) == 0) {
            if (map->entries[h].is_int) {
                /* Stringify the inline int on demand so str->str callers still
                 * observe a decimal string (matches the JIT runtime). */
                char buf[32];
                snprintf(buf, sizeof(buf), "%lld", map->entries[h].ivalue);
                return turbo_strdup(buf);
            }
            return turbo_strdup(map->entries[h].value);
        }
        h = (h + 1) % (unsigned long)map->capacity;
        checked++;
    }
    return NULL;
}

char rt_hashmap_has(const void *map_ptr, const char *key) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (strcmp(map->entries[h].key, key) == 0) {
            return 1;
        }
        h = (h + 1) % (unsigned long)map->capacity;
        checked++;
    }
    return 0;
}

long long rt_hashmap_len(const void *map_ptr) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    return map->count;
}

void *rt_hashmap_keys(const void *map_ptr) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    /* Collect keys, then sort for deterministic order */
    char **key_ptrs = (char **)turbo_alloc((size_t)map->count * sizeof(char *));
    long long idx = 0;
    for (long long i = 0; i < map->capacity; i++) {
        if (map->entries[i].occupied) {
            key_ptrs[idx++] = map->entries[i].key;
        }
    }
    /* Simple insertion sort for deterministic output */
    for (long long i = 1; i < idx; i++) {
        char *tmp = key_ptrs[i];
        long long j = i - 1;
        while (j >= 0 && strcmp(key_ptrs[j], tmp) > 0) {
            key_ptrs[j + 1] = key_ptrs[j];
            j--;
        }
        key_ptrs[j + 1] = tmp;
    }
    /* Build array in same format as rt_str_split: [len][ptr0][ptr1]...
     * prefixed by the shared [cap][refcount] header. */
    if (!rt_array_len_fits(idx)) {
        fprintf(stderr, "runtime error: array alloc size overflow (hashmap keys %lld)\n", idx);
        exit(1);
    }
    size_t data_size = 8 + (size_t)idx * 8;
    long long *arr = (long long *)rt_rc_alloc(data_size, idx);
    arr[0] = idx;
    for (long long i = 0; i < idx; i++) {
        arr[1 + i] = (long long)(size_t)turbo_strdup(key_ptrs[i]);
    }
    turbo_free(key_ptrs);
    return arr;
}

void rt_hashmap_remove(void *map_ptr, const char *key) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (strcmp(map->entries[h].key, key) == 0) {
            turbo_free(map->entries[h].key);
            if (map->entries[h].value) {
                turbo_free(map->entries[h].value);
            }
            map->entries[h].key = NULL;
            map->entries[h].value = NULL;
            map->entries[h].is_int = 0;
            map->entries[h].occupied = 0;
            map->count--;
            /* Re-insert any entries that may have been displaced */
            unsigned long next = (h + 1) % (unsigned long)map->capacity;
            while (map->entries[next].occupied) {
                char *rk = map->entries[next].key;
                char *rv = map->entries[next].value;
                long long riv = map->entries[next].ivalue;
                char ris = map->entries[next].is_int;
                map->entries[next].key = NULL;
                map->entries[next].value = NULL;
                map->entries[next].is_int = 0;
                map->entries[next].occupied = 0;
                map->count--;
                /* Re-insert */
                unsigned long rh = hashmap_hash(rk) % (unsigned long)map->capacity;
                while (map->entries[rh].occupied) {
                    rh = (rh + 1) % (unsigned long)map->capacity;
                }
                map->entries[rh].key = rk;
                map->entries[rh].value = rv;
                map->entries[rh].ivalue = riv;
                map->entries[rh].is_int = ris;
                map->entries[rh].occupied = 1;
                map->count++;
                next = (next + 1) % (unsigned long)map->capacity;
            }
            return;
        }
        h = (h + 1) % (unsigned long)map->capacity;
        checked++;
    }
}

/* ── HashMap str→int variant ─────────────────────────────────────────
 * Int values are stored inline in the entry (is_int == 1, value in ivalue),
 * so set_int / get_int / inc do a single hash + single probe with no
 * stringification, no re-parse, and no per-update allocation. A str->str
 * write to the same key transparently switches the entry back to string
 * storage; rt_hashmap_get stringifies an int entry on demand. Returns the
 * same map pointer so call sites can write `m = hashmap_set_int(m, k, v)`
 * and treat the map as a value. */
void *rt_hashmap_set_int(void *map_ptr, const char *key, long long value) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    if ((double)(map->count + 1) > (double)map->capacity * HASHMAP_LOAD_FACTOR) {
        hashmap_resize(map, map->capacity * 2);
    }
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    while (map->entries[h].occupied) {
        if (strcmp(map->entries[h].key, key) == 0) {
            if (map->entries[h].value) {
                turbo_free(map->entries[h].value);
                map->entries[h].value = NULL;
            }
            map->entries[h].ivalue = value;
            map->entries[h].is_int = 1;
            return map_ptr;
        }
        h = (h + 1) % (unsigned long)map->capacity;
    }
    map->entries[h].key = hashmap_strdup(map, key);
    map->entries[h].value = NULL;
    map->entries[h].ivalue = value;
    map->entries[h].is_int = 1;
    map->entries[h].occupied = 1;
    map->count++;
    return map_ptr;
}

/* Get an int value by key. Returns 0 on miss (no way to distinguish
 * missing from a stored 0 — callers that need that distinction should
 * guard with hashmap_has() first). A string-typed value is parsed. */
long long rt_hashmap_get_int(const void *map_ptr, const char *key) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (strcmp(map->entries[h].key, key) == 0) {
            if (map->entries[h].is_int) {
                return map->entries[h].ivalue;
            }
            return map->entries[h].value ? strtoll(map->entries[h].value, NULL, 10) : 0;
        }
        h = (h + 1) % (unsigned long)map->capacity;
        checked++;
    }
    return 0;
}

/* Fused increment: add `delta` to the int value at `key` (a missing key
 * counts as 0), store the result inline, and return the new value. A single
 * hash + single probe + no allocation on the hot update path — this is the
 * str->int counterpart of C's idiomatic `table[k]++`, and the lowering
 * target for word-count style `count = get_int; set_int(count + 1)` loops. */
long long rt_hashmap_inc(void *map_ptr, const char *key, long long delta) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    if ((double)(map->count + 1) > (double)map->capacity * HASHMAP_LOAD_FACTOR) {
        hashmap_resize(map, map->capacity * 2);
    }
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    while (map->entries[h].occupied) {
        if (strcmp(map->entries[h].key, key) == 0) {
            long long cur;
            if (map->entries[h].is_int) {
                cur = map->entries[h].ivalue;
            } else {
                cur = map->entries[h].value ? strtoll(map->entries[h].value, NULL, 10) : 0;
                if (map->entries[h].value) {
                    turbo_free(map->entries[h].value);
                    map->entries[h].value = NULL;
                }
            }
            cur += delta;
            map->entries[h].ivalue = cur;
            map->entries[h].is_int = 1;
            return cur;
        }
        h = (h + 1) % (unsigned long)map->capacity;
    }
    map->entries[h].key = hashmap_strdup(map, key);
    map->entries[h].value = NULL;
    map->entries[h].ivalue = delta;
    map->entries[h].is_int = 1;
    map->entries[h].occupied = 1;
    map->count++;
    return delta;
}

/* ── Generic HashMap<K,V> core (Tier 1.2) ────────────────────────────────
 * A typed map (rt_hashmap_new_typed) fixes a per-map key-kind and value-kind
 * and stores each value as a raw 8-byte slot in `entry.ivalue` (is_int == 1).
 * Keys are strdup'd strings (HM_KEY_STR) or raw integers bit-stored in the
 * `char* key` field (HM_KEY_INT). rc-heap values are retained on insert and
 * released on overwrite / remove, mirroring the array/string ARC discipline.
 * The map object itself stays leak-but-safe (arena/persistent), exactly like
 * the legacy maps — see the scope-cut note in docs/stdlib.md. */

static unsigned long hashmap_ghash(const turbo_hashmap *map, long long key) {
    if (map->key_kind == HM_KEY_INT) {
        /* splitmix64 finalizer — good dispersion for identity int keys. */
        unsigned long long k = (unsigned long long)key;
        k = (k ^ (k >> 30)) * 0xbf58476d1ce4e5b9ULL;
        k = (k ^ (k >> 27)) * 0x94d049bb133111ebULL;
        k = k ^ (k >> 31);
        return (unsigned long)k;
    }
    return hashmap_hash((const char *)(size_t)key);
}

static int hashmap_gkey_eq(const turbo_hashmap *map, long long a, long long b) {
    if (map->key_kind == HM_KEY_INT) {
        return a == b;
    }
    return strcmp((const char *)(size_t)a, (const char *)(size_t)b) == 0;
}

static int hashmap_gkey_gt(const turbo_hashmap *map, long long a, long long b) {
    if (map->key_kind == HM_KEY_INT) {
        return a > b;
    }
    return strcmp((const char *)(size_t)a, (const char *)(size_t)b) > 0;
}

/* Probe for `key`; return the entry index if present, or -1. */
static long long hashmap_gfind(const turbo_hashmap *map, long long key) {
    unsigned long h = hashmap_ghash(map, key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (hashmap_gkey_eq(map, (long long)(size_t)map->entries[h].key, key)) {
            return (long long)h;
        }
        h = (h + 1) % (unsigned long)map->capacity;
        checked++;
    }
    return -1;
}

/* Generic-aware resize: rehash by the map's key-kind and copy the whole entry
 * (raw value slot included). The legacy hashmap_resize cannot be reused for
 * int-keyed maps because it hashes the key field as a C string. */
static void hashmap_gresize(turbo_hashmap *map, long long new_cap) {
    hashmap_entry *old = map->entries;
    long long old_cap = map->capacity;
    map->entries = hashmap_alloc_entries(map, new_cap);
    map->capacity = new_cap;
    map->count = 0;
    for (long long i = 0; i < old_cap; i++) {
        if (old[i].occupied) {
            long long k = (long long)(size_t)old[i].key;
            unsigned long h = hashmap_ghash(map, k) % (unsigned long)new_cap;
            while (map->entries[h].occupied) {
                h = (h + 1) % (unsigned long)new_cap;
            }
            map->entries[h] = old[i];
            map->count++;
        }
    }
    turbo_free(old);
}

static void hashmap_gretain_value(const turbo_hashmap *map, long long value) {
    if (map->value_retain_fn) {
        map->value_retain_fn((void *)(size_t)value);
    } else if (map->val_is_rc) {
        rt_retain((void *)(size_t)value);
    }
}

static void hashmap_grelease_value(const turbo_hashmap *map, long long value) {
    if (map->value_release_fn) {
        map->value_release_fn((void *)(size_t)value);
    } else if (map->val_is_rc) {
        rt_release((void *)(size_t)value);
    }
}

static void hashmap_grelease_entries(turbo_hashmap *map) {
    for (long long i = 0; i < map->capacity; i++) {
        if (!map->entries[i].occupied) continue;
        if (map->key_kind == HM_KEY_STR) {
            turbo_free(map->entries[i].key);
            map->entries[i].key = NULL;
        }
        hashmap_grelease_value(map, map->entries[i].ivalue);
        map->entries[i].value = NULL;
        map->entries[i].ivalue = 0;
        map->entries[i].is_int = 0;
        map->entries[i].occupied = 0;
    }
    map->count = 0;
}

/* Construct a typed map. key_kind is HM_KEY_STR/HM_KEY_INT; val_is_rc says
 * whether values are rc-heap pointers that need retain/release. A non-NULL
 * value_release_fn replaces plain rt_release for value eviction/drop; a
 * non-NULL value_retain_fn replaces plain rt_retain for non-ARC handle values
 * such as nested typed maps. */
void *rt_hashmap_new_typed(long long key_kind, long long val_is_rc,
                           void *value_release_fn, void *value_retain_fn) {
    turbo_hashmap *map = (turbo_hashmap *)turbo_alloc(sizeof(turbo_hashmap));
    map->persistent = (t_current_arena == NULL) ? 1 : 0;
    map->entries = hashmap_alloc_entries(map, HASHMAP_INIT_CAP);
    map->capacity = HASHMAP_INIT_CAP;
    map->count = 0;
    map->key_kind = (char)key_kind;
    map->val_is_rc = (char)(val_is_rc ? 1 : 0);
    map->refcount = 1;
    map->value_release_fn = (void (*)(void *))value_release_fn;
    map->value_retain_fn = (void (*)(void *))value_retain_fn;
    return map;
}

void rt_hashmap_gretain(void *map_ptr) {
    if (!map_ptr) return;
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    __sync_fetch_and_add(&map->refcount, 1);
}

void rt_hashmap_grelease(void *map_ptr) {
    if (!map_ptr) return;
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    long long prev = __sync_fetch_and_sub(&map->refcount, 1);
    if (prev != 1) return;
    hashmap_grelease_entries(map);
    turbo_free(map->entries);
    map->entries = NULL;
    turbo_free(map);
}

/* Insert or overwrite. `value` is the raw 8-byte slot (int / float bits / bool
 * / pointer). rc-heap values are retained here and released on overwrite. */
void rt_hashmap_gset(void *map_ptr, long long key, long long value) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    if ((double)(map->count + 1) > (double)map->capacity * HASHMAP_LOAD_FACTOR) {
        hashmap_gresize(map, map->capacity * 2);
    }
    long long idx = hashmap_gfind(map, key);
    if (idx >= 0) {
        /* Retain the new value before releasing the old so a self-overwrite
         * (value aliases the stored pointer) never transiently hits zero. */
        hashmap_gretain_value(map, value);
        hashmap_grelease_value(map, map->entries[idx].ivalue);
        map->entries[idx].ivalue = value;
        map->entries[idx].is_int = 1;
        return;
    }
    unsigned long h = hashmap_ghash(map, key) % (unsigned long)map->capacity;
    while (map->entries[h].occupied) {
        h = (h + 1) % (unsigned long)map->capacity;
    }
    if (map->key_kind == HM_KEY_STR) {
        map->entries[h].key = hashmap_strdup(map, (const char *)(size_t)key);
    } else {
        map->entries[h].key = (char *)(size_t)key;
    }
    hashmap_gretain_value(map, value);
    map->entries[h].value = NULL;
    map->entries[h].ivalue = value;
    map->entries[h].is_int = 1;
    map->entries[h].occupied = 1;
    map->count++;
}

/* Look up `key`, returning an Optional: some(value) on hit (rc values are
 * retained into the returned Optional), none on miss. */
void *rt_hashmap_gget(void *map_ptr, long long key) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    long long idx = hashmap_gfind(map, key);
    if (idx < 0) {
        return rt_option_none();
    }
    long long v = map->entries[idx].ivalue;
    hashmap_gretain_value(map, v);
    return rt_option_some(v);
}

char rt_hashmap_ghas(const void *map_ptr, long long key) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    return hashmap_gfind(map, key) >= 0 ? 1 : 0;
}

long long rt_hashmap_glen(const void *map_ptr) {
    return ((const turbo_hashmap *)map_ptr)->count;
}

void rt_hashmap_gremove(void *map_ptr, long long key) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    long long fi = hashmap_gfind(map, key);
    if (fi < 0) {
        return;
    }
    unsigned long h = (unsigned long)fi;
    hashmap_grelease_value(map, map->entries[h].ivalue);
    if (map->key_kind == HM_KEY_STR) {
        turbo_free(map->entries[h].key);
    }
    map->entries[h].key = NULL;
    map->entries[h].value = NULL;
    map->entries[h].ivalue = 0;
    map->entries[h].is_int = 0;
    map->entries[h].occupied = 0;
    map->count--;
    /* Re-insert entries displaced along the probe chain. */
    unsigned long next = (h + 1) % (unsigned long)map->capacity;
    while (map->entries[next].occupied) {
        hashmap_entry e = map->entries[next];
        map->entries[next].key = NULL;
        map->entries[next].value = NULL;
        map->entries[next].occupied = 0;
        map->count--;
        long long rk = (long long)(size_t)e.key;
        unsigned long rh = hashmap_ghash(map, rk) % (unsigned long)map->capacity;
        while (map->entries[rh].occupied) {
            rh = (rh + 1) % (unsigned long)map->capacity;
        }
        map->entries[rh] = e;
        map->count++;
        next = (next + 1) % (unsigned long)map->capacity;
    }
}

/* Return all keys as an array. For str keys the elements are strdup'd string
 * pointers ([str]); for int keys they are the raw integers ([int]). Sorted for
 * deterministic output, matching rt_hashmap_keys. */
void *rt_hashmap_gkeys(const void *map_ptr) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    long long *tmp = (long long *)turbo_alloc((size_t)map->count * sizeof(long long));
    long long idx = 0;
    for (long long i = 0; i < map->capacity; i++) {
        if (map->entries[i].occupied) {
            tmp[idx++] = (long long)(size_t)map->entries[i].key;
        }
    }
    for (long long i = 1; i < idx; i++) {
        long long t = tmp[i];
        long long j = i - 1;
        while (j >= 0 && hashmap_gkey_gt(map, tmp[j], t)) {
            tmp[j + 1] = tmp[j];
            j--;
        }
        tmp[j + 1] = t;
    }
    if (!rt_array_len_fits(idx)) {
        fprintf(stderr, "runtime error: array alloc size overflow (hashmap keys %lld)\n", idx);
        exit(1);
    }
    size_t data_size = 8 + (size_t)idx * 8;
    long long *arr = (long long *)rt_rc_alloc(data_size, idx);
    arr[0] = idx;
    for (long long i = 0; i < idx; i++) {
        if (map->key_kind == HM_KEY_STR) {
            arr[1 + i] = (long long)(size_t)turbo_strdup((const char *)(size_t)tmp[i]);
        } else {
            arr[1 + i] = tmp[i];
        }
    }
    turbo_free(tmp);
    return arr;
}

/* ── HTTP server runtime ─────────────────────────────────────────────── */

#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <unistd.h>
#include <errno.h>
#include <limits.h>
#include <signal.h>

/* Default request body cap. Anything larger gets 413 Payload Too Large.
 * 32 MB is already generous for a language-level HTTP server primitive;
 * the user should front this with a real proxy for production. This is the
 * default value of `g_http_config.max_body_bytes`, overridable at startup
 * via `http_config("max_body_bytes", N)` before `http_listen`. */
#define RT_HTTP_MAX_BODY (32 * 1024 * 1024)

typedef const char* (*route_handler_fn)(const void*, const char*);

typedef struct {
    char method[16];
    char path[1024];
    route_handler_fn handler;
    const void *env_ptr;
} Route;

typedef struct {
    unsigned short port;
    /* Bind address in network byte order. Defaults to 127.0.0.1 for
     * rt_http_server; rt_http_server_public sets this to INADDR_ANY. */
    unsigned int bind_addr;
    Route routes[64];
    int route_count;
    _Atomic int active_connections;
} HttpServerC;

static HttpServerC http_servers[16];
static int http_server_count = 0;
static const char RT_RESPONSE_SEP = '\x1f';

/* ── HTTP server tunables (http_config) ──────────────────────────────────
 *
 * A single process-wide config, set at startup via `http_config(key, value)`
 * BEFORE `http_listen`. It is written only during the single-threaded startup
 * phase and read by the accept loop and per-connection worker threads once the
 * server is listening, so plain reads require no synchronisation (there is no
 * concurrent writer). The JIT twin lives in `src/runtime.rs` — keep the keys,
 * defaults, and validation in lockstep. */
typedef struct {
    long long max_body_bytes;         /* request body cap; over => 413 */
    long long max_header_bytes;       /* request header cap; over => 431 */
    long long max_connections;        /* concurrent connection cap; over => 503 */
    long long read_timeout_ms;        /* SO_RCVTIMEO while reading a request */
    long long write_timeout_ms;       /* SO_SNDTIMEO while writing a response */
    long long keepalive_max_requests; /* requests served per connection */
    long long idle_timeout_ms;        /* wait for next keep-alive request */
} HttpConfig;

#define RT_HTTP_DEFAULT_MAX_BODY         RT_HTTP_MAX_BODY
#define RT_HTTP_DEFAULT_MAX_HEADER       (16 * 1024)
#define RT_HTTP_DEFAULT_MAX_CONN         256
#define RT_HTTP_DEFAULT_READ_TIMEOUT_MS  10000
#define RT_HTTP_DEFAULT_WRITE_TIMEOUT_MS 10000
#define RT_HTTP_DEFAULT_KEEPALIVE_MAX    1000
#define RT_HTTP_DEFAULT_IDLE_TIMEOUT_MS  10000

/* Sanity upper bounds so a bad config value cannot itself become a DoS
 * (e.g. a 1 GB header buffer per connection). Shared with the JIT twin. */
#define RT_HTTP_CFG_MAX_HEADER_LIMIT (16 * 1024 * 1024)
#define RT_HTTP_CFG_MAX_CONN_LIMIT   1000000
#define RT_HTTP_CFG_MIN_HEADER       256

static HttpConfig g_http_config = {
    RT_HTTP_DEFAULT_MAX_BODY,
    RT_HTTP_DEFAULT_MAX_HEADER,
    RT_HTTP_DEFAULT_MAX_CONN,
    RT_HTTP_DEFAULT_READ_TIMEOUT_MS,
    RT_HTTP_DEFAULT_WRITE_TIMEOUT_MS,
    RT_HTTP_DEFAULT_KEEPALIVE_MAX,
    RT_HTTP_DEFAULT_IDLE_TIMEOUT_MS,
};

/* Graceful-shutdown flag. Set from the SIGTERM/SIGINT handler (which runs on
 * the accept thread — worker threads block those signals via pthread_sigmask).
 * `volatile sig_atomic_t` is the only type an async signal handler may touch. */
static volatile sig_atomic_t g_http_shutdown = 0;

static void rt_http_signal_handler(int sig) {
    (void)sig;
    g_http_shutdown = 1;
}

/* Install SIGTERM/SIGINT handlers on the calling (accept) thread and ignore
 * SIGPIPE process-wide so a write to a dead peer returns EPIPE instead of
 * killing the process. Handlers are installed WITHOUT SA_RESTART so a blocked
 * accept() returns EINTR and the loop can observe g_http_shutdown. */
static void rt_http_install_signal_handlers(void) {
    signal(SIGPIPE, SIG_IGN);
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = rt_http_signal_handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = 0; /* no SA_RESTART: interrupt accept() on signal */
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);
}

/* Write exactly `len` bytes to `fd`, looping over partial writes and retrying
 * EINTR. A dead peer (EPIPE/ECONNRESET) or a write timeout (EAGAIN under
 * SO_SNDTIMEO) returns -1 so the caller can tear the connection down instead
 * of writing a truncated response or blocking a slow-reader forever. Returns
 * 0 on success, -1 on any unrecoverable error. Exported (non-static) so the
 * C-runtime test harness can exercise it directly. */
int rt_write_all(int fd, const char *buf, size_t len) {
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, buf + off, len - off);
        if (n > 0) {
            off += (size_t)n;
            continue;
        }
        if (n < 0 && errno == EINTR) {
            continue;
        }
        return -1; /* EPIPE / ECONNRESET / EAGAIN (SO_SNDTIMEO) / other */
    }
    return 0;
}

/* Set a millisecond timeout on `fd` for the given socket option
 * (SO_RCVTIMEO or SO_SNDTIMEO). A value <= 0 leaves the socket blocking. */
static void rt_set_sock_timeout_ms(int fd, int which, long long ms) {
    if (ms <= 0) return;
    struct timeval tv;
    tv.tv_sec = (time_t)(ms / 1000);
    tv.tv_usec = (suseconds_t)((ms % 1000) * 1000);
    setsockopt(fd, SOL_SOCKET, which, &tv, sizeof(tv));
}

/* http_config(key, value) -> 1 on success, 0 on unknown key or bad value.
 * Must be called before http_listen. Unknown keys and out-of-range values
 * print a runtime error to stderr and return 0 — never panic/exit. */
long long rt_http_config(const char *key, long long value) {
    if (!key) {
        fprintf(stderr, "runtime error: http_config: null key\n");
        return 0;
    }
    if (value < 1) {
        fprintf(stderr,
                "runtime error: http_config: '%s' must be >= 1 (got %lld)\n",
                key, value);
        return 0;
    }
    if (strcmp(key, "max_body_bytes") == 0) {
        g_http_config.max_body_bytes = value;
        return 1;
    }
    if (strcmp(key, "max_header_bytes") == 0) {
        if (value < RT_HTTP_CFG_MIN_HEADER || value > RT_HTTP_CFG_MAX_HEADER_LIMIT) {
            fprintf(stderr,
                    "runtime error: http_config: 'max_header_bytes' must be in "
                    "[%d, %d] (got %lld)\n",
                    RT_HTTP_CFG_MIN_HEADER, RT_HTTP_CFG_MAX_HEADER_LIMIT, value);
            return 0;
        }
        g_http_config.max_header_bytes = value;
        return 1;
    }
    if (strcmp(key, "max_connections") == 0) {
        if (value > RT_HTTP_CFG_MAX_CONN_LIMIT) {
            fprintf(stderr,
                    "runtime error: http_config: 'max_connections' must be <= %d "
                    "(got %lld)\n",
                    RT_HTTP_CFG_MAX_CONN_LIMIT, value);
            return 0;
        }
        g_http_config.max_connections = value;
        return 1;
    }
    if (strcmp(key, "read_timeout_ms") == 0) {
        g_http_config.read_timeout_ms = value;
        return 1;
    }
    if (strcmp(key, "write_timeout_ms") == 0) {
        g_http_config.write_timeout_ms = value;
        return 1;
    }
    if (strcmp(key, "keepalive_max_requests") == 0) {
        g_http_config.keepalive_max_requests = value;
        return 1;
    }
    if (strcmp(key, "idle_timeout_ms") == 0) {
        g_http_config.idle_timeout_ms = value;
        return 1;
    }
    fprintf(stderr, "runtime error: http_config: unknown key '%s'\n", key);
    return 0;
}

long long rt_http_server(long long port) {
    if (http_server_count >= 16) { fprintf(stderr, "error: max 16 HTTP servers\n"); exit(1); }
    int id = http_server_count++;
    http_servers[id].port = (unsigned short)port;
    /* Secure default: listen only on the loopback interface. Users who
     * need external access must opt in via rt_http_server_public. */
    http_servers[id].bind_addr = htonl(INADDR_LOOPBACK);
    http_servers[id].route_count = 0;
    atomic_store(&http_servers[id].active_connections, 0);
    return id;
}

/* Like rt_http_server, but binds INADDR_ANY — exposes the server on all
 * interfaces. Callers are expected to know what they are doing and to
 * front this with a reverse proxy in production. */
long long rt_http_server_public(long long port) {
    if (http_server_count >= 16) { fprintf(stderr, "error: max 16 HTTP servers\n"); exit(1); }
    int id = http_server_count++;
    http_servers[id].port = (unsigned short)port;
    http_servers[id].bind_addr = htonl(INADDR_ANY);
    http_servers[id].route_count = 0;
    atomic_store(&http_servers[id].active_connections, 0);
    return id;
}

void rt_http_route(long long server_id, const char *method, const char *path, const void *handler, const void *env_ptr) {
    if (server_id < 0 || server_id >= http_server_count) {
        fprintf(stderr, "runtime error: invalid HTTP server id %lld\n", server_id);
        return;
    }
    HttpServerC *srv = &http_servers[server_id];
    if (srv->route_count >= 64) { fprintf(stderr, "error: max 64 routes per server\n"); exit(1); }
    int idx = srv->route_count++;
    strncpy(srv->routes[idx].method, method, 15);
    srv->routes[idx].method[15] = '\0';
    strncpy(srv->routes[idx].path, path, 1023);
    srv->routes[idx].path[1023] = '\0';
    srv->routes[idx].handler = (route_handler_fn)handler;
    srv->routes[idx].env_ptr = env_ptr;
}

/* ── Connection handler data for pthreads ── */

typedef struct {
    int client_fd;
    HttpServerC *srv;
} conn_thread_data;

static const char *rt_http_content_type(const char *body) {
    if (!body) return "text/plain";
    while (*body == ' ' || *body == '\t' || *body == '\r' || *body == '\n') body++;
    if (strncasecmp(body, "<!doctype html", 14) == 0 ||
        strncasecmp(body, "<html", 5) == 0) {
        return "text/html";
    }
    if (*body == '{' || *body == '[') return "application/json";
    return "text/plain";
}

static int rt_parse_response(
    const char *resp,
    int *status_out,
    char *content_type_out,
    size_t content_type_out_len,
    const char **body_out
);

/* Handle a single HTTP connection with keep-alive support. */
static void *handle_http_conn(void *arg) {
    conn_thread_data *data = (conn_thread_data *)arg;
    int fd = data->client_fd;
    HttpServerC *srv = data->srv;
    free(data);

    /* Worker threads must not handle SIGTERM/SIGINT — those are delivered to
     * the accept thread, which flips g_http_shutdown. Block them here so a
     * signal never interrupts a worker mid-response (and so process-directed
     * signals are steered to the accept thread). */
    sigset_t block_set;
    sigemptyset(&block_set);
    sigaddset(&block_set, SIGTERM);
    sigaddset(&block_set, SIGINT);
    pthread_sigmask(SIG_BLOCK, &block_set, NULL);

    /* Snapshot config once per connection (set before listen, never mutated
     * afterwards). */
    const long long max_body = g_http_config.max_body_bytes;
    long long buf_cap = g_http_config.max_header_bytes;
    if (buf_cap < RT_HTTP_CFG_MIN_HEADER) buf_cap = RT_HTTP_CFG_MIN_HEADER;
    const long long read_timeout_ms = g_http_config.read_timeout_ms;
    const long long idle_timeout_ms = g_http_config.idle_timeout_ms;
    const long long keepalive_max = g_http_config.keepalive_max_requests;

    /* Slowloris-on-write protection: bound how long a single response write
     * may block on a slow-reading client. */
    rt_set_sock_timeout_ms(fd, SO_SNDTIMEO, g_http_config.write_timeout_ms);

    /* Persistent read buffer for keep-alive pipelining, sized to the
     * configured header cap. It doubles as a hard cap on total request
     * header size — a client whose headers do not fit gets 431 Request
     * Header Fields Too Large and the connection is closed. */
    char *buf = (char *)malloc((size_t)buf_cap + 1);
    if (!buf) {
        close(fd);
        atomic_fetch_sub(&srv->active_connections, 1);
        return NULL;
    }
    int buf_len = 0;
    long long requests_served = 0;

    while (1) {
        /* During graceful shutdown, stop accepting new requests on an idle
         * keep-alive connection promptly (there is no partial request in the
         * buffer to finish). A request already mid-flight is allowed to
         * complete below. */
        if (g_http_shutdown && buf_len == 0) break;

        /* Read timeout policy: while waiting for the first byte of the next
         * keep-alive request use the (typically longer) idle timeout; once a
         * request has started arriving switch to the active read timeout so a
         * slow trickle cannot hold the worker forever. */
        rt_set_sock_timeout_ms(fd, SO_RCVTIMEO,
                               buf_len == 0 ? idle_timeout_ms : read_timeout_ms);

        /* Read more data into buffer */
        int space = (int)buf_cap - buf_len;
        if (space <= 0) {
            /* Buffer full and no complete header found yet — headers are
             * too large. Respond with 431 and close. */
            const char *too_large =
                "HTTP/1.1 431 Request Header Fields Too Large\r\n"
                "Content-Length: 0\r\n"
                "Connection: close\r\n\r\n";
            rt_write_all(fd, too_large, strlen(too_large));
            break;
        }
        int n = read(fd, buf + buf_len, space);
        if (n <= 0) break;
        buf_len += n;
        buf[buf_len] = '\0';

    process_request:;
        /* Install per-request arena. All turbo_alloc calls during this
         * request — including allocations made by the user handler —
         * will go through the arena and be reclaimed at the bottom of
         * the loop. Fixes the per-request memory leak (S5 in the v0.5.0
         * audit) without requiring scope-tracking codegen changes. */
        rt_arena_begin();

        /* Find end of headers (\r\n\r\n) */
        char *hdr_end = strstr(buf, "\r\n\r\n");
        if (!hdr_end) {
            rt_arena_end();
            /* If the buffer is full and headers are still incomplete,
             * reject with 431. This guards against DoS via oversized
             * headers that never send the terminator. */
            if (buf_len >= (int)buf_cap) {
                const char *too_large =
                    "HTTP/1.1 431 Request Header Fields Too Large\r\n"
                    "Content-Length: 0\r\n"
                    "Connection: close\r\n\r\n";
                rt_write_all(fd, too_large, strlen(too_large));
                goto conn_done;
            }
            continue;
        }

        /* Parse request line */
        char method[16] = {0}, raw_path[1024] = {0};
        if (sscanf(buf, "%15s %1023s", method, raw_path) != 2) {
            const char *bad = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            rt_write_all(fd, bad, strlen(bad));
            goto conn_done;
        }

        /* Split path and query string */
        char path_buf[1024] = {0};
        char query_buf[1024] = {0};
        char *qmark = strchr(raw_path, '?');
        if (qmark) {
            size_t plen = qmark - raw_path;
            if (plen > 1023) plen = 1023;
            memcpy(path_buf, raw_path, plen);
            path_buf[plen] = '\0';
            strncpy(query_buf, qmark + 1, 1023);
            query_buf[1023] = '\0';
        } else {
            strncpy(path_buf, raw_path, 1023);
            path_buf[1023] = '\0';
        }

        /* Find headers (between first \r\n and \r\n\r\n) */
        char *first_line_end = strstr(buf, "\r\n");
        const char *headers_raw = "";
        size_t headers_len = 0;
        if (first_line_end && first_line_end + 2 < hdr_end) {
            headers_raw = first_line_end + 2;
            headers_len = hdr_end - headers_raw;
        }

        /* Determine content-length.
         *
         * The previous implementation used atoi(), which returned 0 on
         * parse failure and silently accepted negative numbers — a
         * negative value would then be interpreted as a huge size_t and
         * fed into memcpy(), crashing the server. We now use strtoll
         * with ERANGE detection. A malformed/negative value is 400 Bad
         * Request; a well-formed value above the configured cap is 413
         * Payload Too Large. */
        long long content_length = 0;
        int content_length_invalid = 0;
        int content_length_too_large = 0;
        {
            const char *cl = headers_raw;
            for (size_t i = 0; i + 15 < headers_len; i++) {
                if (strncasecmp(cl + i, "content-length:", 15) == 0) {
                    const char *vstart = cl + i + 15;
                    /* Skip leading whitespace */
                    while (*vstart == ' ' || *vstart == '\t') vstart++;
                    char *endptr = NULL;
                    errno = 0;
                    long long val = strtoll(vstart, &endptr, 10);
                    if (errno == ERANGE || endptr == vstart || val < 0) {
                        content_length_invalid = 1;
                    } else if (val > max_body) {
                        content_length_too_large = 1;
                    } else {
                        content_length = val;
                    }
                    break;
                }
            }
        }

        if (content_length_invalid) {
            /* Reject the request entirely — do NOT pass a bad length
             * into memcpy(). Close after replying. */
            const char *bad =
                "HTTP/1.1 400 Bad Request\r\n"
                "Content-Length: 0\r\n"
                "Connection: close\r\n\r\n";
            rt_write_all(fd, bad, strlen(bad));
            rt_arena_end();
            break;
        }

        if (content_length_too_large) {
            /* Body exceeds the configured cap — reject before reading it. */
            const char *too_large =
                "HTTP/1.1 413 Payload Too Large\r\n"
                "Content-Length: 0\r\n"
                "Connection: close\r\n\r\n";
            rt_write_all(fd, too_large, strlen(too_large));
            rt_arena_end();
            break;
        }

        /* Check keep-alive (HTTP/1.1 default is keep-alive) */
        int keep_alive = 1;
        {
            const char *cl = headers_raw;
            for (size_t i = 0; i + 11 < headers_len; i++) {
                if (strncasecmp(cl + i, "connection:", 11) == 0) {
                    const char *v = cl + i + 11;
                    while (*v == ' ') v++;
                    if (strncasecmp(v, "close", 5) == 0) keep_alive = 0;
                    break;
                }
            }
        }

        /* Body starts after \r\n\r\n */
        char *body_start = hdr_end + 4;
        int body_available = buf_len - (int)(body_start - buf);

        /* Where the body bytes ultimately live, and how to clean up.
         * For the common case the body sits inside the read buffer
         * (`body_ptr == body_start`). For large bodies that do not fit,
         * we read them into a heap allocation, mirroring the JIT's
         * `read_exact(vec![0u8; content_length])` (src/runtime.rs) so AOT
         * accepts the same bodies the JIT does instead of spuriously
         * rejecting them with 431. */
        const char *body_ptr = body_start;
        char *heap_body = NULL;

        if ((long long)body_available < content_length) {
            /* The full body is not yet buffered. If the headers plus the
             * full body still fit inside the read buffer, keep reading
             * into it (the original fast path). Otherwise the body is too
             * large for the header buffer — read the remainder onto the
             * heap so we do NOT spuriously hit the `space <= 0` / 431
             * path. content_length was already bounded above by
             * max_body, so the allocation size is capped. */
            long long header_bytes = (long long)(body_start - buf);
            if (header_bytes + content_length <= buf_cap) {
                /* Need more data — release the arena before looping back
                 * to read more bytes, otherwise it would accumulate
                 * across the read calls until the request is complete. */
                rt_arena_end();
                continue;
            }

            heap_body = (char *)malloc((size_t)content_length + 1);
            if (!heap_body) {
                const char *oom =
                    "HTTP/1.1 500 Internal Server Error\r\n"
                    "Content-Length: 0\r\n"
                    "Connection: close\r\n\r\n";
                rt_write_all(fd, oom, strlen(oom));
                rt_arena_end();
                break;
            }
            /* Copy the body bytes already buffered, then read the rest
             * straight off the socket until we have exactly
             * content_length bytes — never over-reading into a following
             * pipelined request. */
            size_t have = body_available > 0 ? (size_t)body_available : 0;
            if (have > 0) {
                memcpy(heap_body, body_start, have);
            }
            int read_failed = 0;
            while (have < (size_t)content_length) {
                ssize_t got = read(fd, heap_body + have, (size_t)content_length - have);
                if (got <= 0) { read_failed = 1; break; }
                have += (size_t)got;
            }
            if (read_failed) {
                /* Client closed or timed out (SO_RCVTIMEO) before sending
                 * the full body. Mirror the JIT's
                 * `read_exact(..).is_err() => break` and drop the
                 * connection. */
                free(heap_body);
                rt_arena_end();
                break;
            }
            heap_body[content_length] = '\0';
            body_ptr = heap_body;
        }

        /* Build structured request: METHOD\x01PATH\x01QUERY\x01HEADERS\x01BODY */
        size_t mlen = strlen(method);
        size_t plen = strlen(path_buf);
        size_t qlen = strlen(query_buf);
        size_t blen = (size_t)content_length;
        size_t total = mlen + 1 + plen + 1 + qlen + 1 + headers_len + 1 + blen + 1;
        char *req_str = turbo_alloc(total);
        char *p = req_str;
        memcpy(p, method, mlen); p += mlen; *p++ = '\x01';
        memcpy(p, path_buf, plen); p += plen; *p++ = '\x01';
        memcpy(p, query_buf, qlen); p += qlen; *p++ = '\x01';
        memcpy(p, headers_raw, headers_len); p += headers_len; *p++ = '\x01';
        memcpy(p, body_ptr, blen); p += blen; *p = '\0';

        /* This is the last request on the connection if the client asked to
         * close, we have hit the per-connection keep-alive request cap, or a
         * graceful shutdown is in progress. Advertise `close` accordingly. */
        requests_served++;
        if (keepalive_max > 0 && requests_served >= keepalive_max) keep_alive = 0;
        if (g_http_shutdown) keep_alive = 0;
        const char *conn_hdr = keep_alive ? "keep-alive" : "close";

        /* A failed response write (dead peer / write timeout) marks the
         * connection dead: we stop writing and tear it down rather than
         * looping on a broken socket. */
        int conn_dead = 0;

        /* Match route */
        int matched = 0;
        for (int i = 0; i < srv->route_count; i++) {
            if (strcmp(srv->routes[i].method, method) == 0 &&
                strcmp(srv->routes[i].path, path_buf) == 0) {
                const char *resp = srv->routes[i].handler(srv->routes[i].env_ptr, req_str);
                if (resp) {
                    int status = 200;
                    char content_type[64];
                    const char *resp_body = NULL;
                    if (rt_parse_response(resp, &status, content_type, sizeof(content_type), &resp_body)) {
                        int resp_len = strlen(resp_body);
                        char hdr[512];
                        snprintf(hdr, sizeof(hdr),
                            "HTTP/1.1 %d OK\r\nContent-Type: %s\r\n"
                            "Connection: %s\r\nContent-Length: %d\r\n\r\n",
                            status, content_type, conn_hdr, resp_len);
                        conn_dead = rt_write_all(fd, hdr, strlen(hdr)) != 0 ||
                                    rt_write_all(fd, resp_body, resp_len) != 0;
                    } else {
                        int resp_len = strlen(resp);
                        char hdr[512];
                        snprintf(hdr, sizeof(hdr),
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n"
                            "Connection: %s\r\nContent-Length: %d\r\n\r\n",
                            conn_hdr, resp_len);
                        conn_dead = rt_write_all(fd, hdr, strlen(hdr)) != 0 ||
                                    rt_write_all(fd, resp, resp_len) != 0;
                    }
                } else {
                    char hdr[256];
                    snprintf(hdr, sizeof(hdr),
                        "HTTP/1.1 200 OK\r\nConnection: %s\r\nContent-Length: 0\r\n\r\n",
                        conn_hdr);
                    conn_dead = rt_write_all(fd, hdr, strlen(hdr)) != 0;
                }
                matched = 1;
                break;
            }
        }
        if (!matched) {
            char hdr[256];
            snprintf(hdr, sizeof(hdr),
                "HTTP/1.1 404 Not Found\r\nConnection: %s\r\nContent-Length: 9\r\n\r\nNot Found",
                conn_hdr);
            conn_dead = rt_write_all(fd, hdr, strlen(hdr)) != 0;
        }

        /* If the write failed, drop the connection: free the heap body (if
         * any) and break with the arena still installed — conn_done ends it. */
        if (conn_dead) {
            if (heap_body != NULL) free(heap_body);
            break;
        }

        if (heap_body != NULL) {
            /* Heap-body path: the entire stack buffer (headers + the
             * partially-buffered body) was consumed, and the remainder of
             * the body was read straight off the socket without
             * over-reading into any following request. Nothing is left to
             * carry over, so reset the buffer and release the heap body. */
            free(heap_body);
            heap_body = NULL;
            buf_len = 0;
        } else {
            /* Consume processed bytes from buffer */
            int consumed = (int)(body_start - buf) + (int)content_length;
            int remaining = buf_len - consumed;
            if (remaining > 0) {
                memmove(buf, buf + consumed, remaining);
            }
            buf_len = remaining;
        }

        /* End the per-request arena: all temporaries allocated during
         * this request — request struct, parsed fields, response body,
         * intermediate concatenations from the user handler — are
         * reclaimed in O(1). The response bytes have already been
         * written to the socket, so the pointers are safe to drop. */
        rt_arena_end();

        if (!keep_alive) break;

        /* If there's already another request in the buffer, process it immediately */
        if (buf_len > 0 && strstr(buf, "\r\n\r\n")) {
            goto process_request;
        }
    }
conn_done:
    /* Defensive: if we broke out of the loop with an arena still
     * installed (e.g. n <= 0 read after process_request labeled goto),
     * make sure we don't leave the thread-local pointer dangling. */
    rt_arena_end();

    free(buf);
    close(fd);
    atomic_fetch_sub(&srv->active_connections, 1);
    return NULL;
}

void rt_http_listen(long long server_id) {
    if (server_id < 0 || server_id >= http_server_count) {
        fprintf(stderr, "runtime error: invalid HTTP server id %lld\n", server_id);
        return;
    }
    HttpServerC *srv = &http_servers[server_id];

    int server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) { perror("socket"); return; }

    int opt = 1;
    setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    /* Bind address was chosen at server-creation time:
     *   rt_http_server        -> 127.0.0.1
     *   rt_http_server_public -> 0.0.0.0 */
    addr.sin_addr.s_addr = srv->bind_addr;
    addr.sin_port = htons(srv->port);

    if (bind(server_fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("bind"); close(server_fd); return;
    }
    if (listen(server_fd, 1024) < 0) {
        perror("listen"); close(server_fd); return;
    }

    /* Install SIGTERM/SIGINT handlers on this (the accept) thread. Worker
     * threads block those signals (see handle_http_conn), so shutdown signals
     * land here and flip g_http_shutdown. Handlers use no SA_RESTART, so the
     * blocking accept() below returns EINTR and the loop observes the flag. */
    rt_http_install_signal_handlers();

    const long long max_conn = g_http_config.max_connections;

    while (!g_http_shutdown) {
        int client_fd = accept(server_fd, NULL, NULL);
        if (client_fd < 0) {
            /* EINTR: a shutdown signal (or any signal) interrupted accept();
             * loop to re-check g_http_shutdown. Other errors: skip this
             * iteration rather than spin-crash. */
            continue;
        }

        int prev = atomic_fetch_add(&srv->active_connections, 1);
        if (prev >= max_conn) {
            atomic_fetch_sub(&srv->active_connections, 1);
            const char *busy =
                "HTTP/1.1 503 Service Unavailable\r\n"
                "Content-Type: text/plain\r\n"
                "Connection: close\r\n"
                "Content-Length: 17\r\n\r\nserver overloaded";
            rt_write_all(client_fd, busy, strlen(busy));
            close(client_fd);
            continue;
        }

        conn_thread_data *data = malloc(sizeof(conn_thread_data));
        if (!data) {
            atomic_fetch_sub(&srv->active_connections, 1);
            close(client_fd);
            continue;
        }
        data->client_fd = client_fd;
        data->srv = srv;

        pthread_t tid;
        pthread_attr_t attr;
        pthread_attr_init(&attr);
        pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
        if (pthread_create(&tid, &attr, handle_http_conn, data) != 0) {
            atomic_fetch_sub(&srv->active_connections, 1);
            close(client_fd);
            free(data);
        }
        pthread_attr_destroy(&attr);
    }

    /* Graceful shutdown: stop accepting, then drain in-flight connections up
     * to a bounded deadline before exiting. Detached workers observe
     * g_http_shutdown and close after finishing their current request; idle
     * keep-alive connections close on their next idle-timeout wakeup. */
    close(server_fd);

    const long long drain_deadline_ms = 10000; /* bounded drain window */
    long long waited_ms = 0;
    while (atomic_load(&srv->active_connections) > 0 && waited_ms < drain_deadline_ms) {
        struct timespec ts = {0, 50 * 1000 * 1000}; /* 50 ms */
        nanosleep(&ts, NULL);
        waited_ms += 50;
    }

    /* Exit 0: a graceful shutdown is a clean, expected termination. */
    exit(0);
}

const char* rt_respond_typed(long long status, const char *content_type, const char *body) {
    if (!content_type) content_type = "text/plain";
    if (!body) body = "";
    /* Sanitize content_type: if it contains \r or \n, an attacker can
     * inject arbitrary HTTP headers into the response. Fall back to a
     * safe default. */
    for (const char *c = content_type; *c; c++) {
        if (*c == '\r' || *c == '\n') {
            content_type = "text/plain";
            break;
        }
    }
    size_t blen = strlen(body);
    size_t clen = strlen(content_type);
    /* "STATUS<sep>CONTENT_TYPE<sep>BODY\0" */
    char digits[20];
    int dlen = snprintf(digits, sizeof(digits), "%lld", status);
    char *result = rt_str_alloc((size_t)dlen + 1 + clen + 1 + blen);
    memcpy(result, digits, dlen);
    result[dlen] = RT_RESPONSE_SEP;
    memcpy(result + dlen + 1, content_type, clen);
    result[dlen + 1 + clen] = RT_RESPONSE_SEP;
    memcpy(result + dlen + 1 + clen + 1, body, blen);
    result[dlen + 1 + clen + 1 + blen] = '\0';
    return result;
}

const char* rt_respond(long long status, const char *body) {
    return rt_respond_typed(status, "text/plain", body);
}

/* Parse the half-open range [start, end) as an unsigned decimal status
 * code in the u16 range [0, 65535], mirroring Rust's
 * `str::parse::<u16>()` (used by the JIT in `parse_rt_response`,
 * src/runtime.rs). The slice must be non-empty and contain ONLY ASCII
 * digits; leading/trailing whitespace, signs, or any non-digit byte are
 * a parse failure. Returns the value on success, or -1 on any failure
 * (empty, non-digit char, or value out of u16 range). */
static long rt_parse_status_u16(const char *start, const char *end) {
    if (start >= end) return -1;
    unsigned long value = 0;
    for (const char *p = start; p < end; p++) {
        if (*p < '0' || *p > '9') return -1;
        value = value * 10UL + (unsigned long)(*p - '0');
        if (value > 65535UL) return -1;
    }
    return (long)value;
}

static int rt_parse_response(
    const char *resp,
    int *status_out,
    char *content_type_out,
    size_t content_type_out_len,
    const char **body_out
) {
    /* Typed form: STATUS\x1fCONTENT_TYPE\x1fBODY (produced by
     * rt_respond_typed). Mirrors the JIT's `splitn(3, SEP)` followed by
     * `status.parse::<u16>()`: if the status segment is not a valid u16
     * we fall through to the colon path exactly as the JIT does, rather
     * than blindly trusting atoi(). */
    const char *sep1 = strchr(resp, RT_RESPONSE_SEP);
    if (sep1) {
        const char *sep2 = strchr(sep1 + 1, RT_RESPONSE_SEP);
        if (sep2) {
            long status = rt_parse_status_u16(resp, sep1);
            if (status >= 0) {
                *status_out = (int)status;
                size_t clen = (size_t)(sep2 - (sep1 + 1));
                if (clen >= content_type_out_len) clen = content_type_out_len - 1;
                memcpy(content_type_out, sep1 + 1, clen);
                content_type_out[clen] = '\0';
                /* Sanitize: reject \r or \n to prevent header injection. */
                for (size_t i = 0; i < clen; i++) {
                    if (content_type_out[i] == '\r' || content_type_out[i] == '\n') {
                        strncpy(content_type_out, "text/plain", content_type_out_len);
                        content_type_out[content_type_out_len - 1] = '\0';
                        break;
                    }
                }
                *body_out = sep2 + 1;
                return 1;
            }
        }
    }

    /* Colon fallback: STATUS:BODY. Mirrors the JIT's `split_once(':')`
     * followed by `status.parse::<u16>()`. The pre-colon prefix is only
     * treated as a status code when it is a FULLY numeric, valid u16;
     * otherwise the whole response is returned to the caller as a plain
     * 200 body (return 0).
     *
     * The previous implementation used atoi(resp), which returns 0 on a
     * non-numeric prefix and never fails — so a handler returning a
     * plain computed string containing a colon (e.g. "the time is 12:30
     * now") produced a bogus "HTTP/1.1 0 OK" status line and a body
     * truncated at the first colon. The JIT, using parse::<u16>(),
     * rejected the non-numeric prefix and treated the whole string as a
     * 200 body — this now matches. */
    const char *colon = strchr(resp, ':');
    if (!colon) return 0;
    long status = rt_parse_status_u16(resp, colon);
    if (status < 0) return 0;
    *status_out = (int)status;
    strncpy(content_type_out, "text/plain", content_type_out_len);
    content_type_out[content_type_out_len - 1] = '\0';
    *body_out = colon + 1;
    return 1;
}

/* ── Request field extraction (structured request: METHOD\x01PATH\x01QUERY\x01HEADERS\x01BODY) */

static const char* req_field(const char *req, int field_index) {
    if (!req) return rt_str_empty();
    const char *start = req;
    for (int i = 0; i < field_index; i++) {
        const char *sep = strchr(start, '\x01');
        if (!sep) return rt_str_empty();
        start = sep + 1;
    }
    /* Find end of this field */
    const char *end = strchr(start, '\x01');
    if (!end) {
        /* Last field — copy to end of string */
        size_t len = strlen(start);
        char *result = rt_str_alloc(len);
        memcpy(result, start, len);
        result[len] = '\0';
        return result;
    }
    size_t len = end - start;
    char *result = rt_str_alloc(len);
    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

const char* rt_request_method(const char *req) {
    return req_field(req, 0);
}

const char* rt_request_path(const char *req) {
    return req_field(req, 1);
}

const char* rt_request_query(const char *req, const char *key) {
    if (!req || !key) return rt_str_empty();
    const char *qs = req_field(req, 2);
    if (!qs || !*qs) {
        rt_release((void *)qs);
        return rt_str_empty();
    }
    size_t klen = strlen(key);
    const char *p = qs;
    while (*p) {
        if (strncmp(p, key, klen) == 0 && p[klen] == '=') {
            const char *val_start = p + klen + 1;
            const char *val_end = strchr(val_start, '&');
            size_t vlen = val_end ? (size_t)(val_end - val_start) : strlen(val_start);
            char *result = rt_str_alloc(vlen);
            memcpy(result, val_start, vlen);
            result[vlen] = '\0';
            rt_release((void *)qs);
            return result;
        }
        const char *amp = strchr(p, '&');
        if (!amp) break;
        p = amp + 1;
    }
    rt_release((void *)qs);
    return rt_str_empty();
}

const char* rt_request_header(const char *req, const char *name) {
    if (!req || !name) return rt_str_empty();
    const char *headers = req_field(req, 3);
    if (!headers || !*headers) {
        rt_release((void *)headers);
        return rt_str_empty();
    }
    size_t nlen = strlen(name);
    const char *p = headers;
    while (*p) {
        /* Case-insensitive header name match */
        if (strncasecmp(p, name, nlen) == 0 && p[nlen] == ':') {
            const char *val = p + nlen + 1;
            while (*val == ' ') val++; /* skip optional whitespace */
            const char *eol = strstr(val, "\r\n");
            size_t vlen = eol ? (size_t)(eol - val) : strlen(val);
            char *result = rt_str_alloc(vlen);
            memcpy(result, val, vlen);
            result[vlen] = '\0';
            rt_release((void *)headers);
            return result;
        }
        const char *next = strstr(p, "\r\n");
        if (!next) break;
        p = next + 2;
    }
    rt_release((void *)headers);
    return rt_str_empty();
}

const char* rt_request_body(const char *req) {
    if (!req) return rt_str_empty();
    /* If structured request (contains \x01), extract body field */
    if (strchr(req, '\x01')) {
        return req_field(req, 4);
    }
    /* Backward compat: plain string is the body */
    return turbo_strdup(req);
}

/* ── ARC (Automatic Reference Counting) runtime ─────────────────────── */

void rt_retain(void *data_ptr) {
    if (!data_ptr) return;
    long long *rc = rt_rc_refcount_ptr(data_ptr);
    long long current = __atomic_load_n(rc, __ATOMIC_ACQUIRE);
    if (current == RT_RC_IMMORTAL || current == RT_RC_ARENA) {
        return;
    }
    __sync_fetch_and_add(rc, 1);
}

void rt_release(void *data_ptr) {
    if (!data_ptr) return;
    long long *rc = rt_rc_refcount_ptr(data_ptr);
    long long current = __atomic_load_n(rc, __ATOMIC_ACQUIRE);
    if (current == RT_RC_IMMORTAL || current == RT_RC_ARENA) {
        return;
    }
    long long prev = __sync_fetch_and_sub(rc, 1);
    if (prev == 1) {
        /* Free from the raw allocation base, which sits RT_RC_HEADER_BYTES
         * below the data pointer (cap slot at raw+0, refcount at raw+8). */
        free((char *)data_ptr - RT_RC_HEADER_BYTES);
    }
}

/* Entry point: calls Turbo's main and returns 0.
 *
 * Suppressed under RT_TEST_BUILD so the C runtime test harness
 * (runtime/tests/test_rt.c) can link against turbo_rt.c without
 * pulling in the unresolved `turbo_main` symbol or colliding with
 * the harness's own `main`. */
#ifndef RT_TEST_BUILD
extern void turbo_main(void);
int main(int argc, char **argv) {
    rt_set_args(argc, argv);
    turbo_main();
    return 0;
}
#endif

/* ── Fallible I/O (v0.8.0 "Safe Core") ──────────────────────────────────
 *
 * Result-returning variants of the panicking rt_read_file / rt_write_file
 * above. Errors are returned as rt_result_err(str) instead of aborting
 * the process. Success cases match the existing Turbo Result encoding
 * used by rt_result_ok / rt_result_err (see line ~487): a heap-allocated
 * [tag (8)][value (8)] block with tag 0 = ok, tag 1 = err.
 *
 * For try_write_file we encode success as rt_result_ok with a boolean
 * payload of 1 (Turbo bools are stored in a 64-bit slot anyway because
 * the Result value slot is long long).
 */

#include <errno.h>

/* try_read_file(path) -> str ! str  */
void *rt_try_read_file(const char *path) {
    if (!path) {
        const char *msg = "null path";
        size_t n = strlen(msg);
        char *buf = rt_str_alloc(n);
        memcpy(buf, msg, n + 1);
        return rt_result_err((long long)(intptr_t)buf);
    }
    FILE *f = fopen(path, "rb");
    if (!f) {
        const char *err = strerror(errno);
        size_t n = strlen(err);
        char *buf = rt_str_alloc(n);
        memcpy(buf, err, n + 1);
        return rt_result_err((long long)(intptr_t)buf);
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        const char *err = strerror(errno);
        size_t n = strlen(err);
        char *buf = rt_str_alloc(n);
        memcpy(buf, err, n + 1);
        fclose(f);
        return rt_result_err((long long)(intptr_t)buf);
    }
    long size = ftell(f);
    if (size < 0) {
        const char *err = strerror(errno);
        size_t n = strlen(err);
        char *buf = rt_str_alloc(n);
        memcpy(buf, err, n + 1);
        fclose(f);
        return rt_result_err((long long)(intptr_t)buf);
    }
    fseek(f, 0, SEEK_SET);
    char *contents = rt_str_alloc((size_t)size);
    size_t nread = fread(contents, 1, (size_t)size, f);
    contents[nread] = '\0';
    fclose(f);
    return rt_result_ok((long long)(intptr_t)contents);
}

/* try_write_file(path, content) -> bool ! str  */
void *rt_try_write_file(const char *path, const char *content) {
    if (!path) {
        const char *msg = "null path";
        size_t n = strlen(msg);
        char *buf = rt_str_alloc(n);
        memcpy(buf, msg, n + 1);
        return rt_result_err((long long)(intptr_t)buf);
    }
    FILE *f = fopen(path, "wb");
    if (!f) {
        const char *err = strerror(errno);
        size_t n = strlen(err);
        char *buf = rt_str_alloc(n);
        memcpy(buf, err, n + 1);
        return rt_result_err((long long)(intptr_t)buf);
    }
    if (content) {
        size_t len = strlen(content);
        size_t written = fwrite(content, 1, len, f);
        if (written != len) {
            const char *err = strerror(errno);
            size_t n = strlen(err);
            char *buf = rt_str_alloc(n);
            memcpy(buf, err, n + 1);
            fclose(f);
            return rt_result_err((long long)(intptr_t)buf);
        }
    }
    fclose(f);
    /* ok(true) — bool payload stored in the 64-bit value slot. */
    return rt_result_ok(1);
}

/* ── Filesystem builtins ──────────────────────────────────────────── */

long long rt_file_exists(const char *path) {
    if (!path) return 0;
    return access(path, F_OK) == 0 ? 1 : 0;
}

long long rt_delete_file(const char *path) {
    if (!path) return 0;
    return remove(path) == 0 ? 1 : 0;
}

void *rt_list_dir(const char *path) {
    if (!path) path = ".";
    DIR *dir = opendir(path);
    if (!dir) {
        /* Return empty array */
        if (!rt_array_len_fits(0)) { fprintf(stderr, "runtime error: array alloc overflow\n"); exit(1); }
        size_t data_size = 8;
        long long *arr = (long long *)rt_rc_alloc(data_size, 0);
        arr[0] = 0;
        return arr;
    }
    /* First pass: collect entries */
    long long count = 0;
    long long capacity = 64;
    char **names = (char **)malloc((size_t)capacity * sizeof(char *));
    if (!names) { closedir(dir); fprintf(stderr, "runtime error: list_dir alloc failed\n"); exit(1); }
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
            continue;
        if (count >= capacity) {
            capacity *= 2;
            char **tmp = (char **)realloc(names, (size_t)capacity * sizeof(char *));
            if (!tmp) { free(names); closedir(dir); fprintf(stderr, "runtime error: list_dir realloc failed\n"); exit(1); }
            names = tmp;
        }
        names[count++] = turbo_strdup(entry->d_name);
    }
    closedir(dir);
    /* Build Turbo array */
    if (!rt_array_len_fits(count)) { fprintf(stderr, "runtime error: array alloc overflow\n"); exit(1); }
    size_t data_size = 8 + (size_t)count * 8;
    long long *arr = (long long *)rt_rc_alloc(data_size, count);
    arr[0] = count;
    for (long long i = 0; i < count; i++) {
        arr[1 + i] = (long long)(intptr_t)names[i];
    }
    free(names);
    return arr;
}

long long rt_mkdir(const char *path) {
    if (!path) return 0;
    /* Recursive mkdir: walk the path and create each component */
    size_t len = strlen(path);
    char *buf = turbo_alloc(len + 1);
    memcpy(buf, path, len + 1);
    for (size_t i = 1; i <= len; i++) {
        if (buf[i] == '/' || buf[i] == '\0') {
            char saved = buf[i];
            buf[i] = '\0';
            mkdir(buf, 0755);  /* ignore EEXIST */
            buf[i] = saved;
        }
    }
    turbo_free(buf);
    /* Check that final path now exists */
    struct stat st;
    return (stat(path, &st) == 0 && S_ISDIR(st.st_mode)) ? 1 : 0;
}

const char *rt_path_join(const char *a, const char *b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t a_len = strlen(a);
    size_t b_len = strlen(b);
    int need_sep = (a_len > 0 && a[a_len - 1] != '/') ? 1 : 0;
    size_t total = a_len + need_sep + b_len + 1;
    char *buf = rt_str_alloc(total - 1);
    memcpy(buf, a, a_len);
    if (need_sep) buf[a_len] = '/';
    memcpy(buf + a_len + need_sep, b, b_len + 1);
    return buf;
}

const char *rt_path_dir(const char *path) {
    if (!path || !*path) return turbo_strdup(".");
    const char *last_sep = strrchr(path, '/');
    if (!last_sep) return turbo_strdup(".");
    if (last_sep == path) return turbo_strdup("/");
    size_t len = (size_t)(last_sep - path);
    char *buf = rt_str_alloc(len);
    memcpy(buf, path, len);
    buf[len] = '\0';
    return buf;
}

const char *rt_path_base(const char *path) {
    if (!path || !*path) return turbo_strdup("");
    const char *last_sep = strrchr(path, '/');
    if (last_sep) return turbo_strdup(last_sep + 1);
    return turbo_strdup(path);
}

const char *rt_path_ext(const char *path) {
    if (!path || !*path) return turbo_strdup("");
    /* Get the basename first */
    const char *base = strrchr(path, '/');
    if (base) base++; else base = path;
    const char *dot = strrchr(base, '.');
    if (!dot || dot == base) return turbo_strdup("");
    return turbo_strdup(dot + 1);
}

/* ── Collection builtins ──────────────────────────────────────────── */

static int cmp_i64(const void *a, const void *b) {
    long long va = *(const long long *)a;
    long long vb = *(const long long *)b;
    return (va > vb) - (va < vb);
}

static int cmp_str(const void *a, const void *b) {
    const char *sa = (const char *)(intptr_t)(*(const long long *)a);
    const char *sb = (const char *)(intptr_t)(*(const long long *)b);
    if (!sa) sa = "";
    if (!sb) sb = "";
    return strcmp(sa, sb);
}

void *rt_sort_int(const void *arr) {
    long long len = *(const long long *)arr;
    if (!rt_array_len_fits(len)) { fprintf(stderr, "runtime error: array alloc overflow\n"); exit(1); }
    size_t data_size = 8 + (size_t)len * 8;
    long long *result = (long long *)rt_rc_alloc(data_size, len);
    result[0] = len;
    memcpy(result + 1, ((const long long *)arr) + 1, (size_t)len * 8);
    if (len > 1) qsort(result + 1, (size_t)len, 8, cmp_i64);
    return result;
}

void *rt_sort_str(const void *arr) {
    long long len = *(const long long *)arr;
    if (!rt_array_len_fits(len)) { fprintf(stderr, "runtime error: array alloc overflow\n"); exit(1); }
    size_t data_size = 8 + (size_t)len * 8;
    long long *result = (long long *)rt_rc_alloc(data_size, len);
    result[0] = len;
    memcpy(result + 1, ((const long long *)arr) + 1, (size_t)len * 8);
    if (len > 1) qsort(result + 1, (size_t)len, 8, cmp_str);
    return result;
}

void *rt_reverse(const void *arr) {
    long long len = *(const long long *)arr;
    if (!rt_array_len_fits(len)) { fprintf(stderr, "runtime error: array alloc overflow\n"); exit(1); }
    size_t data_size = 8 + (size_t)len * 8;
    long long *result = (long long *)rt_rc_alloc(data_size, len);
    result[0] = len;
    const long long *src = ((const long long *)arr) + 1;
    for (long long i = 0; i < len; i++) {
        result[1 + i] = src[len - 1 - i];
    }
    return result;
}

long long rt_array_contains_int(const void *arr, long long val) {
    long long len = *(const long long *)arr;
    const long long *elems = ((const long long *)arr) + 1;
    for (long long i = 0; i < len; i++) {
        if (elems[i] == val) return 1;
    }
    return 0;
}

long long rt_array_contains_str(const void *arr, const char *val) {
    long long len = *(const long long *)arr;
    const long long *elems = ((const long long *)arr) + 1;
    if (!val) val = "";
    for (long long i = 0; i < len; i++) {
        const char *s = (const char *)(intptr_t)elems[i];
        if (!s) s = "";
        if (strcmp(s, val) == 0) return 1;
    }
    return 0;
}

void *rt_slice(const void *arr, long long start, long long end) {
    long long len = *(const long long *)arr;
    /* Clamp bounds */
    if (start < 0) start = 0;
    if (end > len) end = len;
    if (start > end) start = end;
    long long new_len = end - start;
    if (!rt_array_len_fits(new_len)) { fprintf(stderr, "runtime error: array alloc overflow\n"); exit(1); }
    size_t data_size = 8 + (size_t)new_len * 8;
    long long *result = (long long *)rt_rc_alloc(data_size, new_len);
    result[0] = new_len;
    const long long *src = ((const long long *)arr) + 1;
    memcpy(result + 1, src + start, (size_t)new_len * 8);
    return result;
}

/* ── Date/Time builtins ───────────────────────────────────────────── */

double rt_time_now(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (double)tv.tv_sec + (double)tv.tv_usec / 1000000.0;
}

long long rt_time_ms(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (long long)tv.tv_sec * 1000LL + (long long)tv.tv_usec / 1000LL;
}

const char *rt_format_time(double timestamp, const char *fmt) {
    if (!fmt) fmt = "%Y-%m-%d %H:%M:%S";
    time_t t = (time_t)timestamp;
    /* localtime_r: reentrant. HTTP handler threads call rt_format_time
     * concurrently, and the non-reentrant localtime() shares one static
     * `struct tm`, so two threads formatting at once would race. */
    struct tm tm_buf;
    struct tm *tm = localtime_r(&t, &tm_buf);
    if (!tm) return turbo_strdup("");
    char buf[256];
    size_t n = strftime(buf, sizeof(buf), fmt, tm);
    if (n == 0) return turbo_strdup("");
    return turbo_strdup(buf);
}

/* ── SQLite builtins ─────────────────────────────────────────────────
 * The SQLite shim lives in its own file to keep this runtime lean. It is
 * pulled into this translation unit (so it can share turbo_alloc /
 * rt_result_ok / rt_result_err) only when the AOT/link path opts in with
 * -DTURBO_WITH_SQLITE. The JIT twins live in src/runtime.rs. */
#ifdef TURBO_WITH_SQLITE
#include "turbo_rt_sqlite.c"
#endif
