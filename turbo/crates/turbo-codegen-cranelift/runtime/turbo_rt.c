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
#include <pthread.h>
#include <math.h>
#include <time.h>
#include <sys/wait.h>

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
    if (t_current_arena != NULL) {
        size_t total = count * size;
        void *p = turbo_arena_alloc(t_current_arena, total);
        memset(p, 0, total);
        return p;
    }
    void *p = calloc(count, size);
    if (!p) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    return p;
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
    printf("%.7g\n", n);
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

void rt_div_by_zero(void) {
    fprintf(stderr, "runtime error: division by zero\n");
    exit(1);
}

void rt_int_overflow(void) {
    fprintf(stderr, "runtime error: integer overflow\n");
    exit(1);
}

const char* rt_str_concat(const char *a, const char *b) {
    size_t a_len = a ? strlen(a) : 0;
    size_t b_len = b ? strlen(b) : 0;
    char *result = turbo_alloc(a_len + b_len + 1);
    if (a) memcpy(result, a, a_len);
    if (b) memcpy(result + a_len, b, b_len);
    result[a_len + b_len] = '\0';
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
#define RT_RC_HEADER_BYTES 16

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
    size_t total = RT_RC_HEADER_BYTES + data_size;
    void *raw = turbo_calloc(1, total);
    *(long long *)raw = cap;              /* cap at raw + 0 */
    *(long long *)((char *)raw + 8) = 1;  /* refcount = 1 */
    return (char *)raw + RT_RC_HEADER_BYTES;
}

/* Array allocation: element capacity bound that guarantees the final
 * `total = RT_RC_HEADER_BYTES + 8 + cap * 8` does not overflow size_t.
 * Shared by rt_array_alloc and rt_array_push.
 *
 * `new_len` is the requested element count. The bound leaves room for
 * the 16-byte header and the 8-byte length field inside user data. */
static inline int rt_array_len_fits(long long new_len) {
    if (new_len < 0) return 0;
    /* total = RT_RC_HEADER_BYTES + 8 + new_len * 8 must fit in size_t. */
    return (size_t)new_len <= (SIZE_MAX - (RT_RC_HEADER_BYTES + 8)) / 8;
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

long long rt_array_get(const void *arr, long long index) {
    long long len = *(const long long*)arr;
    if (index < 0 || index >= len) {
        fprintf(stderr, "runtime error: array index %lld out of bounds (length %lld)\n", index, len);
        exit(1);
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
        __sync_fetch_and_sub(rc_ptr, 1);
        target = new_data;
    } else {
        target = arr;
    }
    long long len = *(const long long*)target;
    if (index < 0 || index >= len) {
        fprintf(stderr, "runtime error: array index %lld out of bounds (length %lld)\n", index, len);
        exit(1);
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
    size_t data_size = (size_t)num_fields * 8;
    if (data_size < 8) data_size = 8;
    return rt_rc_alloc(data_size, 0);
}

const char* rt_i64_to_str(long long n) {
    char *buf = turbo_alloc(32);
    snprintf(buf, 32, "%lld", n);
    return buf;
}

const char* rt_f64_to_str(double n) {
    char *buf = turbo_alloc(32);
    snprintf(buf, 32, "%.7g", n);
    return buf;
}

const char* rt_bool_to_str(char b) {
    if (b) {
        char *buf = turbo_alloc(5);
        memcpy(buf, "true", 5);
        return buf;
    } else {
        char *buf = turbo_alloc(6);
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
            char *empty = turbo_alloc(1);
            empty[0] = '\0';
            arr[1] = (long long)(size_t)empty;
        } else {
            for (size_t i = 0; i < slen; i++) {
                char *ch = turbo_alloc(2);
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
            char *part = turbo_alloc(len + 1);
            memcpy(part, p, len);
            part[len] = '\0';
            arr[1 + idx] = (long long)(size_t)part;
            idx++;
            p = next + sep_len;
        }
        /* last part */
        size_t len = strlen(p);
        char *part = turbo_alloc(len + 1);
        memcpy(part, p, len);
        part[len] = '\0';
        arr[1 + idx] = (long long)(size_t)part;
    }
    return arr;
}

const char* rt_str_trim(const char *s) {
    if (!s) { char *e = turbo_alloc(1); e[0] = '\0'; return e; }
    const char *start = s;
    while (*start == ' ' || *start == '\t' || *start == '\n' || *start == '\r') start++;
    const char *end = s + strlen(s);
    while (end > start && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\n' || end[-1] == '\r')) end--;
    size_t len = (size_t)(end - start);
    char *result = turbo_alloc(len + 1);
    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

const char* rt_str_upper(const char *s) {
    if (!s) { char *e = turbo_alloc(1); e[0] = '\0'; return e; }
    size_t len = strlen(s);
    char *result = turbo_alloc(len + 1);
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        result[i] = (c >= 'a' && c <= 'z') ? (char)(c - 32) : (char)c;
    }
    result[len] = '\0';
    return result;
}

const char* rt_str_lower(const char *s) {
    if (!s) { char *e = turbo_alloc(1); e[0] = '\0'; return e; }
    size_t len = strlen(s);
    char *result = turbo_alloc(len + 1);
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
        char *e = turbo_alloc(1); e[0] = '\0'; return e;
    }
    size_t from_len = strlen(from);
    size_t to_len = strlen(to);
    if (from_len == 0) {
        /* No replacement for empty pattern; return copy */
        size_t len = strlen(s);
        char *r = turbo_alloc(len + 1);
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
    char *result = turbo_alloc(new_len + 1);
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
        fprintf(stderr, "runtime error: string index %lld out of bounds (length 0)\n", index);
        exit(1);
    }
    size_t len = strlen(s);
    if (index < 0 || (size_t)index >= len) {
        fprintf(stderr, "runtime error: string index %lld out of bounds (length %zu)\n", index, len);
        exit(1);
    }
    char *result = turbo_alloc(2);
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
        char *empty = (char *)turbo_alloc(1);
        empty[0] = '\0';
        return empty;
    }
    size_t len = strlen(s);
    /* Overflow check: len * count must fit in size_t and leave room for
     * the trailing NUL. Previously this wrapped silently and the malloc
     * would succeed with a much smaller size than the subsequent strcat
     * loop needed, producing a heap overflow. */
    if (len == 0) {
        char *empty = (char *)turbo_alloc(1);
        empty[0] = '\0';
        return empty;
    }
    if ((size_t)count > (SIZE_MAX - 1) / len) {
        fprintf(stderr,
                "[rt_str_repeat] overflow: len=%zu * count=%lld exceeds SIZE_MAX\n",
                len, count);
        char *empty = (char *)turbo_alloc(1);
        empty[0] = '\0';
        return empty;
    }
    size_t total = len * (size_t)count;
    /* Practical cap: 256 MB. Larger totals usually indicate a bug or attack
     * and would exhaust memory; mirror the Rust JIT runtime cap. */
    const size_t RT_STR_REPEAT_MAX_BYTES = 256ULL * 1024ULL * 1024ULL;
    if (total > RT_STR_REPEAT_MAX_BYTES) {
        fprintf(stderr,
                "[rt_str_repeat] refusing allocation: total=%zu > cap %zu\n",
                total, RT_STR_REPEAT_MAX_BYTES);
        char *empty = (char *)turbo_alloc(1);
        empty[0] = '\0';
        return empty;
    }
    char *result = (char *)turbo_alloc(total + 1);
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
    if (!arr_ptr) return "";
    long long len = *(long long *)arr_ptr;
    if (len <= 0) return "";
    const char **elems = (const char **)(arr_ptr + 8);
    /* Calculate total length */
    size_t total = 0;
    size_t sep_len = sep ? strlen(sep) : 0;
    for (long long i = 0; i < len; i++) {
        if (elems[i]) total += strlen(elems[i]);
        if (i < len - 1) total += sep_len;
    }
    char *result = (char *)turbo_alloc(total + 1);
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
        char *e = turbo_alloc(1); e[0] = '\0'; return e;
    }
    /* strip trailing \n / \r */
    while (nread > 0 && (line[nread-1] == '\n' || line[nread-1] == '\r')) {
        line[--nread] = '\0';
    }
    char *result = turbo_alloc((size_t)nread + 1);
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
    char *buf = turbo_alloc((size_t)size + 1);
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
    void *args_ptr;
    long long result;
} spawn_ctx;

static void *spawn_thread_fn(void *arg) {
    spawn_ctx *ctx = (spawn_ctx *)arg;
    ctx->result = ctx->thunk(ctx->args_ptr);
    return NULL;
}

void *rt_spawn_with_args(long long (*thunk)(void *), void *args_ptr) {
    spawn_ctx *ctx = (spawn_ctx *)turbo_alloc(sizeof(spawn_ctx));
    ctx->thunk = thunk;
    ctx->args_ptr = args_ptr;
    ctx->result = 0;
    pthread_t *handle = (pthread_t *)turbo_alloc(sizeof(pthread_t) + sizeof(spawn_ctx *));
    /* Store ctx pointer right after the pthread_t */
    *((spawn_ctx **)(handle + 1)) = ctx;
    pthread_create(handle, NULL, spawn_thread_fn, ctx);
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
    char *result = (char *)turbo_alloc(len + 1);
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

static const char *rt_http_empty_response(void) {
    char *empty = (char *)turbo_alloc(1);
    empty[0] = '\0';
    return empty;
}

/* http_get(url) -> str — HTTP GET via fork+exec (no shell interpolation) */
const char *rt_http_get(const char *url) {
    if (!rt_url_is_http(url)) {
        fprintf(stderr, "[rt_http] blocked non-http(s) URL: %s\n",
                url ? url : "(null)");
        return rt_http_empty_response();
    }
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return strdup("error: cannot fork");
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
    if (!rt_url_is_http(url)) {
        fprintf(stderr, "[rt_http] blocked non-http(s) URL: %s\n",
                url ? url : "(null)");
        return rt_http_empty_response();
    }
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return strdup("error: cannot fork");
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

/* shell_exec(cmd) / exec(cmd) -> str — execute a command via fork+execvp,
 * capture stdout+stderr.
 *
 * Security: we DO NOT spawn /bin/sh -c. Any shell metacharacter in `cmd`
 * causes the call to be rejected outright. Accepted commands are tokenized
 * on whitespace and handed to execvp() directly, so there is no shell to
 * interpret quotes, redirections, substitutions, or pipelines. Callers that
 * genuinely need a pipeline should compose one in Turbo code. */
static const char *rt_exec_empty_response(void) {
    char *empty = (char *)turbo_alloc(1);
    empty[0] = '\0';
    return empty;
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
        return strdup("error: cannot create pipe");
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        free(cmd_copy);
        return strdup("error: cannot fork");
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
        const char *empty = (const char *)turbo_alloc(1);
        ((char *)empty)[0] = '\0';
        return empty;
    }
    const char *val = getenv(name);
    if (!val) {
        const char *empty = (const char *)turbo_alloc(1);
        ((char *)empty)[0] = '\0';
        return empty;
    }
    return strdup(val);
}

static char *rt_json_escape_dup(const char *s) {
    if (!s) {
        char *empty = (char *)turbo_alloc(1);
        empty[0] = '\0';
        return empty;
    }
    size_t len = strlen(s);
    char *out = (char *)turbo_alloc(len * 2 + 1);
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
        } else {
            out[j++] = c;
        }
    }
    out[j] = '\0';
    return out;
}

/* json_get(json, key) -> str — extract top-level key value from JSON string */
const char *rt_json_get(const char *json, const char *key) {
    /* Build search pattern: "key" */
    size_t klen = strlen(key);
    char *search = (char *)turbo_alloc(klen + 3);
    search[0] = '"';
    memcpy(search + 1, key, klen);
    search[klen + 1] = '"';
    search[klen + 2] = '\0';

    const char *pos = strstr(json, search);
    free(search);
    if (!pos) return strdup("");

    /* Advance past key, skip whitespace and colon */
    pos += klen + 2;
    while (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r') pos++;
    if (*pos != ':') return strdup("");
    pos++;
    while (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r') pos++;

    if (*pos == '"') {
        /* String value */
        pos++;
        const char *start = pos;
        while (*pos && !(*pos == '"' && *(pos - 1) != '\\')) pos++;
        size_t vlen = pos - start;
        char *val = (char *)turbo_alloc(vlen + 1);
        memcpy(val, start, vlen);
        val[vlen] = '\0';
        return val;
    } else {
        /* Number, bool, or null */
        const char *start = pos;
        while (*pos && *pos != ',' && *pos != '}' && *pos != ']' &&
               *pos != ' ' && *pos != '\n' && *pos != '\r' && *pos != '\t') pos++;
        size_t vlen = pos - start;
        char *val = (char *)turbo_alloc(vlen + 1);
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
    char *buf = (char *)turbo_alloc(cap);
    snprintf(buf, cap, "{\"%s\":\"%s\"}", esc_key, esc_value);
    return buf;
}

const char *rt_json_root(const char *json) {
    if (!json) return strdup("");
    while (*json == ' ' || *json == '\n' || *json == '\r' || *json == '\t') json++;
    size_t len = strlen(json);
    while (len > 0 && (json[len - 1] == ' ' || json[len - 1] == '\n' || json[len - 1] == '\r' || json[len - 1] == '\t')) len--;
    if (len >= 2 && json[0] == '"' && json[len - 1] == '"') {
        char *out = (char *)turbo_alloc(len - 1);
        size_t j = 0;
        for (size_t i = 1; i + 1 < len; i++) {
            if (json[i] == '\\' && i + 1 < len - 1) {
                i++;
                switch (json[i]) {
                    case 'n': out[j++] = '\n'; break;
                    case 'r': out[j++] = '\r'; break;
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
    char *out = (char *)turbo_alloc(len + 1);
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

    channel_node *node = (channel_node *)turbo_alloc(sizeof(channel_node));
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
} turbo_mutex;

void *rt_mutex_create(long long value) {
    turbo_mutex *m = (turbo_mutex *)turbo_alloc(sizeof(turbo_mutex));
    m->value = value;
    pthread_mutex_init(&m->lock, NULL);
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

void *rt_mutex_clone(const void *mptr) {
    /* Mutex is shared — just return the same pointer.
     * In the C implementation we don't use Arc, so cloning is a no-op. */
    return (void *)mptr;
}

/* ── HashMap runtime ─────────────────────────────────────────────────── */

/* Simple hash table using open addressing with linear probing.
 * Keys and values are C strings (owned copies).
 */

#define HASHMAP_INIT_CAP 16
#define HASHMAP_LOAD_FACTOR 0.75

typedef struct {
    char *key;
    char *value;
    char occupied;
} hashmap_entry;

typedef struct {
    hashmap_entry *entries;
    long long capacity;
    long long count;
} turbo_hashmap;

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
    map->entries = (hashmap_entry *)turbo_calloc((size_t)new_cap, sizeof(hashmap_entry));
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
            map->entries[h].occupied = 1;
            map->count++;
        }
    }
    free(old);
}

void *rt_hashmap_new(void) {
    turbo_hashmap *map = (turbo_hashmap *)turbo_alloc(sizeof(turbo_hashmap));
    map->entries = (hashmap_entry *)turbo_calloc(HASHMAP_INIT_CAP, sizeof(hashmap_entry));
    map->capacity = HASHMAP_INIT_CAP;
    map->count = 0;
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
            /* Update existing key */
            free(map->entries[h].value);
            map->entries[h].value = strdup(value);
            return;
        }
        h = (h + 1) % (unsigned long)map->capacity;
    }
    map->entries[h].key = strdup(key);
    map->entries[h].value = strdup(value);
    map->entries[h].occupied = 1;
    map->count++;
}

const char *rt_hashmap_get(const void *map_ptr, const char *key) {
    const turbo_hashmap *map = (const turbo_hashmap *)map_ptr;
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (strcmp(map->entries[h].key, key) == 0) {
            return strdup(map->entries[h].value);
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
        arr[1 + i] = (long long)(size_t)strdup(key_ptrs[i]);
    }
    free(key_ptrs);
    return arr;
}

void rt_hashmap_remove(void *map_ptr, const char *key) {
    turbo_hashmap *map = (turbo_hashmap *)map_ptr;
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    long long checked = 0;
    while (map->entries[h].occupied && checked < map->capacity) {
        if (strcmp(map->entries[h].key, key) == 0) {
            free(map->entries[h].key);
            free(map->entries[h].value);
            map->entries[h].key = NULL;
            map->entries[h].value = NULL;
            map->entries[h].occupied = 0;
            map->count--;
            /* Re-insert any entries that may have been displaced */
            unsigned long next = (h + 1) % (unsigned long)map->capacity;
            while (map->entries[next].occupied) {
                char *rk = map->entries[next].key;
                char *rv = map->entries[next].value;
                map->entries[next].key = NULL;
                map->entries[next].value = NULL;
                map->entries[next].occupied = 0;
                map->count--;
                /* Re-insert */
                unsigned long rh = hashmap_hash(rk) % (unsigned long)map->capacity;
                while (map->entries[rh].occupied) {
                    rh = (rh + 1) % (unsigned long)map->capacity;
                }
                map->entries[rh].key = rk;
                map->entries[rh].value = rv;
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
 * Pragmatic implementation: stringify the int and reuse the str→str
 * storage. Avoids changing the entry union for the v0.8.0 MVP. A proper
 * generic HashMap<K,V> with a tagged value union is planned post-1.0.
 * Returns the same map pointer so call sites can write
 *   m = hashmap_set_int(m, k, v)
 * and treat the map as a value. */
void *rt_hashmap_set_int(void *map_ptr, const char *key, long long value) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%lld", value);
    rt_hashmap_set(map_ptr, key, buf);
    return map_ptr;
}

/* Get an int value by key. Returns 0 on miss (no way to distinguish
 * missing from a stored 0 — callers that need that distinction should
 * guard with hashmap_has() first). */
long long rt_hashmap_get_int(const void *map_ptr, const char *key) {
    const char *s = rt_hashmap_get(map_ptr, key);
    if (s == NULL) {
        return 0;
    }
    long long v = strtoll(s, NULL, 10);
    /* rt_hashmap_get returns strdup'd memory; free it to avoid leak. */
    free((void *)s);
    return v;
}

/* ── HTTP server runtime ─────────────────────────────────────────────── */

#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <unistd.h>
#include <errno.h>
#include <limits.h>

/* Hard cap on request body size. Anything larger gets 400 Bad Request.
 * 32 MB is already generous for a language-level HTTP server primitive;
 * the user should front this with a real proxy for production. */
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
static const int RT_HTTP_MAX_ACTIVE_CONNECTIONS = 256;
static const char RT_RESPONSE_SEP = '\x1f';

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

    /* Set read timeout (10 seconds) for keep-alive idle connections */
    struct timeval tv;
    tv.tv_sec = 10;
    tv.tv_usec = 0;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));

    /* Persistent read buffer for keep-alive pipelining */
    char buf[16384];
    int buf_len = 0;

    while (1) {
        /* Read more data into buffer */
        int n = read(fd, buf + buf_len, sizeof(buf) - 1 - buf_len);
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
        if (!hdr_end) { rt_arena_end(); continue; } /* Need more data */

        /* Parse request line */
        char method[16] = {0}, raw_path[1024] = {0};
        sscanf(buf, "%15s %1023s", method, raw_path);

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
         * with ERANGE detection and reject anything out of [0, RT_HTTP_MAX_BODY]. */
        long long content_length = 0;
        int content_length_invalid = 0;
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
                    if (errno == ERANGE || endptr == vstart || val < 0 ||
                        val > (long long)RT_HTTP_MAX_BODY) {
                        content_length_invalid = 1;
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
            write(fd, bad, strlen(bad));
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
        if ((long long)body_available < content_length) {
            /* Need more data — release the arena before looping back to
             * read more bytes, otherwise it would accumulate across the
             * read calls until the request is complete. */
            rt_arena_end();
            continue;
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
        memcpy(p, body_start, blen); p += blen; *p = '\0';

        const char *conn_hdr = keep_alive ? "keep-alive" : "close";

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
                            "Access-Control-Allow-Origin: *\r\nConnection: %s\r\n"
                            "Content-Length: %d\r\n\r\n",
                            status, content_type, conn_hdr, resp_len);
                        write(fd, hdr, strlen(hdr));
                        write(fd, resp_body, resp_len);
                    } else {
                        int resp_len = strlen(resp);
                        char hdr[512];
                        snprintf(hdr, sizeof(hdr),
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n"
                            "Connection: %s\r\nContent-Length: %d\r\n\r\n",
                            conn_hdr, resp_len);
                        write(fd, hdr, strlen(hdr));
                        write(fd, resp, resp_len);
                    }
                } else {
                    char hdr[256];
                    snprintf(hdr, sizeof(hdr),
                        "HTTP/1.1 200 OK\r\nConnection: %s\r\nContent-Length: 0\r\n\r\n",
                        conn_hdr);
                    write(fd, hdr, strlen(hdr));
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
            write(fd, hdr, strlen(hdr));
        }

        /* Consume processed bytes from buffer */
        int consumed = (int)(body_start - buf) + (int)content_length;
        int remaining = buf_len - consumed;
        if (remaining > 0) {
            memmove(buf, buf + consumed, remaining);
        }
        buf_len = remaining;

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
    /* Defensive: if we broke out of the loop with an arena still
     * installed (e.g. n <= 0 read after process_request labeled goto),
     * make sure we don't leave the thread-local pointer dangling. */
    rt_arena_end();

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

    while (1) {
        int client_fd = accept(server_fd, NULL, NULL);
        if (client_fd < 0) continue;

        int prev = atomic_fetch_add(&srv->active_connections, 1);
        if (prev >= RT_HTTP_MAX_ACTIVE_CONNECTIONS) {
            atomic_fetch_sub(&srv->active_connections, 1);
            const char *busy =
                "HTTP/1.1 503 Service Unavailable\r\n"
                "Content-Type: text/plain\r\n"
                "Connection: close\r\n"
                "Content-Length: 17\r\n\r\nserver overloaded";
            write(client_fd, busy, strlen(busy));
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
}

const char* rt_respond_typed(long long status, const char *content_type, const char *body) {
    if (!content_type) content_type = "text/plain";
    if (!body) body = "";
    size_t blen = strlen(body);
    size_t clen = strlen(content_type);
    /* "STATUS<sep>CONTENT_TYPE<sep>BODY\0" */
    char digits[20];
    int dlen = snprintf(digits, sizeof(digits), "%lld", status);
    char *result = turbo_alloc(dlen + 1 + clen + 1 + blen + 1);
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

static int rt_parse_response(
    const char *resp,
    int *status_out,
    char *content_type_out,
    size_t content_type_out_len,
    const char **body_out
) {
    const char *sep1 = strchr(resp, RT_RESPONSE_SEP);
    if (sep1) {
        const char *sep2 = strchr(sep1 + 1, RT_RESPONSE_SEP);
        if (sep2) {
            char status_buf[32];
            size_t slen = (size_t)(sep1 - resp);
            if (slen >= sizeof(status_buf)) slen = sizeof(status_buf) - 1;
            memcpy(status_buf, resp, slen);
            status_buf[slen] = '\0';
            *status_out = atoi(status_buf);
            size_t clen = (size_t)(sep2 - (sep1 + 1));
            if (clen >= content_type_out_len) clen = content_type_out_len - 1;
            memcpy(content_type_out, sep1 + 1, clen);
            content_type_out[clen] = '\0';
            *body_out = sep2 + 1;
            return 1;
        }
    }

    const char *colon = strchr(resp, ':');
    if (!colon) return 0;
    *status_out = atoi(resp);
    strncpy(content_type_out, "text/plain", content_type_out_len);
    content_type_out[content_type_out_len - 1] = '\0';
    *body_out = colon + 1;
    return 1;
}

/* ── Request field extraction (structured request: METHOD\x01PATH\x01QUERY\x01HEADERS\x01BODY) */

static const char* req_field(const char *req, int field_index) {
    if (!req) return "";
    const char *start = req;
    for (int i = 0; i < field_index; i++) {
        const char *sep = strchr(start, '\x01');
        if (!sep) return "";
        start = sep + 1;
    }
    /* Find end of this field */
    const char *end = strchr(start, '\x01');
    if (!end) {
        /* Last field — copy to end of string */
        size_t len = strlen(start);
        char *result = turbo_alloc(len + 1);
        memcpy(result, start, len);
        result[len] = '\0';
        return result;
    }
    size_t len = end - start;
    char *result = turbo_alloc(len + 1);
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
    if (!req || !key) return "";
    const char *qs = req_field(req, 2);
    if (!qs || !*qs) return "";
    size_t klen = strlen(key);
    const char *p = qs;
    while (*p) {
        if (strncmp(p, key, klen) == 0 && p[klen] == '=') {
            const char *val_start = p + klen + 1;
            const char *val_end = strchr(val_start, '&');
            size_t vlen = val_end ? (size_t)(val_end - val_start) : strlen(val_start);
            char *result = turbo_alloc(vlen + 1);
            memcpy(result, val_start, vlen);
            result[vlen] = '\0';
            return result;
        }
        const char *amp = strchr(p, '&');
        if (!amp) break;
        p = amp + 1;
    }
    return "";
}

const char* rt_request_header(const char *req, const char *name) {
    if (!req || !name) return "";
    const char *headers = req_field(req, 3);
    if (!headers || !*headers) return "";
    size_t nlen = strlen(name);
    const char *p = headers;
    while (*p) {
        /* Case-insensitive header name match */
        if (strncasecmp(p, name, nlen) == 0 && p[nlen] == ':') {
            const char *val = p + nlen + 1;
            while (*val == ' ') val++; /* skip optional whitespace */
            const char *eol = strstr(val, "\r\n");
            size_t vlen = eol ? (size_t)(eol - val) : strlen(val);
            char *result = turbo_alloc(vlen + 1);
            memcpy(result, val, vlen);
            result[vlen] = '\0';
            return result;
        }
        const char *next = strstr(p, "\r\n");
        if (!next) break;
        p = next + 2;
    }
    return "";
}

const char* rt_request_body(const char *req) {
    if (!req) return "";
    /* If structured request (contains \x01), extract body field */
    if (strchr(req, '\x01')) {
        return req_field(req, 4);
    }
    /* Backward compat: plain string is the body */
    return req;
}

/* ── ARC (Automatic Reference Counting) runtime ─────────────────────── */

void rt_retain(void *data_ptr) {
    if (!data_ptr) return;
    long long *rc = rt_rc_refcount_ptr(data_ptr);
    __sync_fetch_and_add(rc, 1);
}

void rt_release(void *data_ptr) {
    if (!data_ptr) return;
    if (t_current_arena != NULL) {
        /* Arena-backed request allocations are reclaimed in bulk at
         * rt_arena_end(); never call free() on arena memory. */
        return;
    }
    long long *rc = rt_rc_refcount_ptr(data_ptr);
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
int main(void) {
    turbo_main();
    return 0;
}
#endif
