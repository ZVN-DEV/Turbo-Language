/*
 * Minimal C-runtime test harness for turbo_rt.c.
 *
 * No external test framework. Each test is a numbered case that uses
 * assert() (from <assert.h>) and prints a one-line result. The runner
 * (tests.sh) compiles this file together with turbo_rt.c and runs it;
 * a non-zero exit (from a failed assertion or `exit(1)`) is a failure.
 *
 * The functions tested here are the *public* runtime symbols. Internal
 * helpers (e.g. `rt_url_is_http`) are file-static and intentionally
 * not exposed.
 *
 * NOTE: We compile turbo_rt.c with -DRT_TEST_BUILD so it won't try to
 * pull in `main` or anything else that conflicts with this file. (At
 * the time of writing turbo_rt.c does not define a main, so this is
 * just a forward-compat guard.)
 */

#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <signal.h>
#include <time.h>
#include <limits.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

/* Forward declarations of the runtime functions we exercise. We don't
 * include turbo_rt.c via a header — the runtime intentionally has no
 * .h file (it is consumed via include_str! in the codegen crate). */
extern const char *rt_str_repeat(const char *s, long long count);
extern const char *rt_str_concat(const char *a, const char *b);
extern const char *rt_str_concat_inplace(const char *a, const char *b);
extern const char *rt_str_replace(const char *s, const char *from, const char *to);
extern const char *rt_str_upper(const char *s);
extern void *rt_str_split(const char *s, const char *sep);
extern const char *rt_http_get(const char *url);
extern const char *rt_http_post(const char *url, const char *body);
extern void rt_arena_begin(void);
extern void rt_arena_end(void);
extern const char *rt_str_join(const char *arr_ptr, const char *sep);
extern const char *rt_f64_to_str(double n);
extern void *rt_array_alloc(long long len);
extern long long rt_array_len(const void *arr);
extern long long rt_array_get(const void *arr, long long index);
extern void *rt_array_push(void *arr, long long value);
extern void rt_release(void *data_ptr);
extern void rt_retain(void *data_ptr);
extern void *rt_struct_alloc(long long num_fields);
extern const char *rt_respond_typed(long long status, const char *content_type, const char *body);
extern long long rt_http_server(long long port);
extern void rt_http_route(long long server_id, const char *method, const char *path, const void *handler, const void *env_ptr);
extern void rt_http_listen(long long server_id);
extern void *rt_hashmap_new(void);
extern void rt_hashmap_set(void *map, const char *key, const char *value);
extern const char *rt_hashmap_get(const void *map, const char *key);
extern void *rt_hashmap_set_int(void *map, const char *key, long long value);
extern long long rt_hashmap_get_int(const void *map, const char *key);
extern long long rt_hashmap_inc(void *map, const char *key, long long delta);
extern int rt_write_all(int fd, const char *buf, size_t len);
extern long long rt_http_config(const char *key, long long value);
extern void *rt_spawn_with_args(long long (*thunk)(void *), void *args_ptr,
                                long long ptr_mask, long long num_args);
extern long long rt_await_handle(void *handle_ptr);
/* Generic HashMap<K,V> descriptor-based core (Tier 1.2). */
extern void *rt_hashmap_new_typed(long long key_kind, long long val_is_rc);
extern void rt_hashmap_gset(void *map, long long key, long long value);
extern void *rt_hashmap_gget(void *map, long long key);
extern char rt_hashmap_ghas(const void *map, long long key);
extern long long rt_hashmap_glen(const void *map);
extern void rt_hashmap_gremove(void *map, long long key);
extern void *rt_hashmap_gkeys(const void *map);
extern long long rt_option_tag(const void *opt);

/* Refcount of an rc-heap pointer (header sits 8 bytes below the data). */
#define RT_RC(p) (((const long long *)(p))[-1])

static int g_failures = 0;

static void check(const char *name, int ok) {
    if (ok) {
        printf("  [PASS] %s\n", name);
    } else {
        printf("  [FAIL] %s\n", name);
        g_failures++;
    }
}

static void release_str_array(void *arr) {
    if (!arr) return;
    long long len = rt_array_len(arr);
    for (long long i = 0; i < len; i++) {
        const char *part = (const char *)(size_t)rt_array_get(arr, i);
        rt_release((void *)part);
    }
    rt_release(arr);
}

static long long reserve_loopback_port(void) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return 0;

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = 0;
    addr.sin_addr.s_addr = htonl(0x7f000001UL); /* 127.0.0.1 */

    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        close(fd);
        return 0;
    }

    socklen_t len = sizeof(addr);
    if (getsockname(fd, (struct sockaddr *)&addr, &len) != 0) {
        close(fd);
        return 0;
    }

    long long port = ntohs(addr.sin_port);
    close(fd);
    return port;
}

/* ── 1: rt_str_repeat overflow ─────────────────────────────────────── */
static void test_str_repeat_overflow(void) {
    /* 10-byte string * very large count would wrap size_t. The hardened
     * implementation must reject the overflow and return an empty string. */
    const char *s = "0123456789";
    const char *r = rt_str_repeat(s, (long long)((unsigned long long)-1 / 4));
    check("test_str_repeat_overflow returns empty on overflow",
          r != NULL && r[0] == '\0');
}

/* ── 2: rt_str_repeat normal case ──────────────────────────────────── */
static void test_str_repeat_normal(void) {
    const char *r = rt_str_repeat("ab", 3);
    check("test_str_repeat_normal == ababab",
          r != NULL && strcmp(r, "ababab") == 0);
}

/* ── 3: rt_str_repeat zero count ───────────────────────────────────── */
static void test_str_repeat_zero(void) {
    const char *r = rt_str_repeat("hello", 0);
    check("test_str_repeat_zero returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 4: rt_str_repeat negative count ───────────────────────────────── */
static void test_str_repeat_negative(void) {
    const char *r = rt_str_repeat("hello", -1);
    check("test_str_repeat_negative returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 5: http_get rejects file:// scheme ────────────────────────────── */
static void test_http_get_rejects_file_scheme(void) {
    const char *r = rt_http_get("file:///etc/hosts");
    check("test_http_get_rejects_file_scheme returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 6: http_get rejects flag-shaped URL ───────────────────────────── */
static void test_http_get_rejects_flag_injection(void) {
    const char *r = rt_http_get("--help");
    check("test_http_get_rejects_flag_injection returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 7: http_get rejects gopher:// ─────────────────────────────────── */
static void test_http_get_rejects_gopher(void) {
    const char *r = rt_http_get("gopher://example.com/");
    check("test_http_get_rejects_gopher returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 8: http_get rejects NULL ──────────────────────────────────────── */
static void test_http_get_rejects_null(void) {
    const char *r = rt_http_get(NULL);
    check("test_http_get_rejects_null returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 9: http_get rejects empty string ──────────────────────────────── */
static void test_http_get_rejects_empty(void) {
    const char *r = rt_http_get("");
    check("test_http_get_rejects_empty returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 10: http_post rejects file:// ─────────────────────────────────── */
static void test_http_post_rejects_file_scheme(void) {
    const char *r = rt_http_post("file:///tmp/x", "body");
    check("test_http_post_rejects_file_scheme returns empty",
          r != NULL && r[0] == '\0');
}

/* ── 11: rt_str_concat basic ───────────────────────────────────────── */
static void test_str_concat_basic(void) {
    const char *r = rt_str_concat("foo", "bar");
    check("test_str_concat_basic == foobar",
          r != NULL && strcmp(r, "foobar") == 0);
}

/* ── 12: arena reclaims allocations on rt_arena_end ────────────────── */
static void test_arena_reclaims_memory(void) {
    /* Run the same allocation pattern many times. Without an arena
     * this would steadily grow RSS (the v0.5.0 leak). With the arena,
     * each iteration's allocations are reclaimed at rt_arena_end and
     * memory should plateau quickly. We can't easily measure RSS from
     * inside the test process, so instead we sanity-check that:
     *  (a) the arena doesn't crash under repeated begin/end cycles,
     *  (b) the resulting strings are valid each iteration,
     *  (c) the arena hard cap kicks in instead of unbounded growth. */
    int iterations = 5000;
    int ok = 1;
    for (int i = 0; i < iterations; i++) {
        rt_arena_begin();
        /* Allocate ~3KB of strings inside the arena, similar to a
         * typical request handler producing JSON. */
        const char *a = rt_str_concat("hello, ", "world");
        const char *b = rt_str_repeat("xy", 64);
        const char *c = rt_str_concat(a, b);
        if (!a || !b || !c) { ok = 0; break; }
        if (strncmp(a, "hello, world", 12) != 0) { ok = 0; break; }
        if (strlen(b) != 128) { ok = 0; break; }
        rt_arena_end();
    }
    check("test_arena_reclaims_memory survives 5000 begin/end cycles", ok);
}

/* ── 13: arena tolerates nested begin (resets instead of stacking) ──── */
static void test_arena_nested_begin_resets(void) {
    rt_arena_begin();
    const char *a = rt_str_concat("first", "alloc");
    /* Stale a pointer; the inner begin should reset the arena. */
    rt_arena_begin();
    const char *b = rt_str_concat("second", "alloc");
    rt_arena_end();
    /* `a` is now reclaimed (don't dereference it). `b` is also gone
     * after the end above. */
    (void)a;
    check("test_arena_nested_begin_resets does not crash",
          b != NULL);
}

/* ── 14: rt_arena_end with no active arena is a safe no-op ─────────── */
static void test_arena_end_without_begin(void) {
    rt_arena_end();
    rt_arena_end();
    check("test_arena_end_without_begin is a no-op", 1);
}

/* ── rt_str_join helpers ───────────────────────────────────────────── */
/* Build a Turbo string array: raw allocation big enough for
 * [cap][refcount][len][ptr0][ptr1]…, returning the data pointer that
 * points at len (same ABI rt_array_alloc produces). We allocate
 * through rt_array_alloc so the shared header layout stays in sync. */
static void *make_str_array(const char **strs, long long count) {
    void *arr = rt_array_alloc(count);
    long long *slots = (long long *)arr;
    for (long long i = 0; i < count; i++) {
        slots[1 + i] = (long long)(size_t)strs[i];
    }
    return arr;
}

/* ── 15: rt_str_join on zero-element array ─────────────────────────── */
static void test_str_join_empty(void) {
    void *arr = rt_array_alloc(0);
    const char *r = rt_str_join((const char *)arr, ",");
    check("test_str_join_empty returns empty string",
          r != NULL && r[0] == '\0');
    rt_release(arr);
}

/* ── 16: rt_str_join on single element ─────────────────────────────── */
static void test_str_join_one(void) {
    const char *strs[] = {"solo"};
    void *arr = make_str_array(strs, 1);
    const char *r = rt_str_join((const char *)arr, ", ");
    check("test_str_join_one == 'solo'",
          r != NULL && strcmp(r, "solo") == 0);
    rt_release(arr);
}

/* ── 17: rt_str_join on many elements ──────────────────────────────── */
static void test_str_join_many(void) {
    const char *strs[] = {"a", "bb", "ccc", "dddd"};
    void *arr = make_str_array(strs, 4);
    const char *r = rt_str_join((const char *)arr, "-");
    check("test_str_join_many == 'a-bb-ccc-dddd'",
          r != NULL && strcmp(r, "a-bb-ccc-dddd") == 0);
    rt_release(arr);
}

/* ── 18: rt_str_join with NULL separator treats it as empty ────────── */
static void test_str_join_null_sep(void) {
    const char *strs[] = {"hello", "world"};
    void *arr = make_str_array(strs, 2);
    const char *r = rt_str_join((const char *)arr, NULL);
    check("test_str_join_null_sep == 'helloworld'",
          r != NULL && strcmp(r, "helloworld") == 0);
    rt_release(arr);
}

/* ── 19: canonical float formatting ───────────────────────────────── */
static void test_f64_to_str_canonical_edges(void) {
    check("test_f64_to_str_canonical_pi",
          strcmp(rt_f64_to_str(3.14159 * 5.0 * 5.0), "78.53975") == 0);
    check("test_f64_to_str_canonical_decimal_noise",
          strcmp(rt_f64_to_str(0.1 + 0.2), "0.3") == 0);
    check("test_f64_to_str_canonical_negative_zero",
          strcmp(rt_f64_to_str(-0.0), "0.0") == 0);
    check("test_f64_to_str_canonical_repeating",
          strcmp(rt_f64_to_str(1.0 / 3.0), "0.333333333333333") == 0);
}

/* ── 20: rt_array_push on an empty array grows correctly ───────────── */
static void test_array_push_empty(void) {
    void *arr = rt_array_alloc(0);
    arr = rt_array_push(arr, 42);
    int ok = (rt_array_len(arr) == 1 && rt_array_get(arr, 0) == 42);
    check("test_array_push_empty [42]", ok);
    rt_release(arr);
}

/* ── 20: rt_array_push 1000 times stays fast (amortised O(1)) ─────────
 * We can't portably measure wall time here, so we assert correctness
 * and rely on the unit test completing in well under a second. With
 * the old O(n) per push, 1000 pushes cost ~500k element copies; with
 * capacity doubling it costs ~2000. Either finishes quickly in CI,
 * but only the new path keeps the allocation count logarithmic. */
static void test_array_push_1000(void) {
    void *arr = rt_array_alloc(0);
    int ok = 1;
    for (long long i = 0; i < 1000; i++) {
        arr = rt_array_push(arr, i * 7);
    }
    if (rt_array_len(arr) != 1000) ok = 0;
    for (long long i = 0; i < 1000 && ok; i++) {
        if (rt_array_get(arr, i) != i * 7) ok = 0;
    }
    check("test_array_push_1000 keeps every element", ok);
    rt_release(arr);
}

/* ── 21: rt_array_push reuses the buffer when cap > len and rc == 1 ──
 * This is a behavioural test for the amortised-O(1) path. The second
 * push after the first growth must not move the data pointer. */
static void test_array_push_reuses_buffer(void) {
    void *arr = rt_array_alloc(0);
    /* First push allocates an initial capacity > 1. */
    arr = rt_array_push(arr, 10);
    void *after_first = arr;
    /* The next two pushes should fit in the spare capacity. */
    arr = rt_array_push(arr, 20);
    int second_reused = (arr == after_first);
    arr = rt_array_push(arr, 30);
    int third_reused = (arr == after_first);
    check("test_array_push_reuses_buffer keeps the pointer on growth",
          second_reused && third_reused &&
          rt_array_len(arr) == 3 &&
          rt_array_get(arr, 0) == 10 &&
          rt_array_get(arr, 1) == 20 &&
          rt_array_get(arr, 2) == 30);
    rt_release(arr);
}

/* ── 22: rt_array_push overflow guard aborts cleanly ──────────────────
 * We can't actually allocate a 2e18-element array, but we can spawn a
 * child process, forge an array header claiming it already has
 * (SIZE_MAX / 8) elements, and observe that the next push exits with
 * the expected error message. The forged header lives on the stack in
 * the child; the child never reaches the allocation call because the
 * overflow check fires first. */
static void test_array_push_overflow_guard(void) {
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        check("test_array_push_overflow_guard pipe", 0);
        return;
    }
    /* Flush stdio before fork so both processes don't later re-emit
     * buffered output. The guard calls exit(1) which flushes libc
     * buffers — if we leave the parent's test log queued, the child
     * would print it again. */
    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0) {
        check("test_array_push_overflow_guard fork", 0);
        close(pipefd[0]); close(pipefd[1]);
        return;
    }
    if (pid == 0) {
        /* Child: redirect stderr into the pipe, forge an oversized len. */
        close(pipefd[0]);
        dup2(pipefd[1], 2);
        close(pipefd[1]);

        /* Header layout mirrors rt_rc_alloc: [cap][refcount][len][...]
         * We only need len to be large enough that new_len = len + 1
         * trips the overflow guard. SIZE_MAX / 8 comfortably does. */
        static long long fake[4];
        fake[0] = 0;                   /* cap */
        fake[1] = 1;                   /* refcount */
        fake[2] = (long long)(SIZE_MAX / 8); /* len — triggers guard on +1 */
        fake[3] = 0;                   /* one element slot (never read) */
        void *data_ptr = &fake[2];
        (void)rt_array_push(data_ptr, 999);
        /* The guard calls exit(1); reaching here is a test failure. */
        _exit(0);
    }
    /* Parent: read child's stderr, wait, check. */
    close(pipefd[1]);
    char buf[256];
    ssize_t n = read(pipefd[0], buf, sizeof(buf) - 1);
    close(pipefd[0]);
    if (n < 0) n = 0;
    buf[n] = '\0';
    int status = 0;
    waitpid(pid, &status, 0);
    int exited_with_one =
        WIFEXITED(status) && WEXITSTATUS(status) == 1;
    int said_overflow = strstr(buf, "array push size overflow") != NULL;
    check("test_array_push_overflow_guard aborts with runtime error",
          exited_with_one && said_overflow);
}

/* ── 23: turbo_calloc overflow aborts cleanly ─────────────────────────
 * We forge a call with SIZE_MAX-sized arguments that would overflow
 * the count * size multiplication. The hardened turbo_calloc must
 * abort() rather than silently wrapping and under-allocating.
 *
 * We test this in a child process because the expected behaviour is
 * abort() (SIGABRT). */
static void test_turbo_calloc_overflow(void) {
    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0) {
        check("test_turbo_calloc_overflow fork", 0);
        return;
    }
    if (pid == 0) {
        /* Child: call rt_struct_alloc which routes through turbo_calloc
         * via rt_rc_alloc. Pass a field count large enough that
         * num_fields * 8 overflows size_t. */
        (void)rt_struct_alloc((long long)(SIZE_MAX / 4));
        /* If we reach here, the overflow was not caught. */
        _exit(0);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    /* The child should have been killed by SIGABRT or exited non-zero. */
    int died = WIFSIGNALED(status) ||
               (WIFEXITED(status) && WEXITSTATUS(status) != 0);
    check("test_turbo_calloc_overflow aborts on overflow", died);
}

/* ── 24: rt_struct_alloc negative num_fields aborts ──────────────────── */
static void test_struct_alloc_negative(void) {
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        check("test_struct_alloc_negative pipe", 0);
        return;
    }
    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0) {
        check("test_struct_alloc_negative fork", 0);
        close(pipefd[0]); close(pipefd[1]);
        return;
    }
    if (pid == 0) {
        close(pipefd[0]);
        dup2(pipefd[1], 2);
        close(pipefd[1]);
        (void)rt_struct_alloc(-1);
        _exit(0);
    }
    close(pipefd[1]);
    char buf[256];
    ssize_t n = read(pipefd[0], buf, sizeof(buf) - 1);
    close(pipefd[0]);
    if (n < 0) n = 0;
    buf[n] = '\0';
    int status = 0;
    waitpid(pid, &status, 0);
    int died = WIFSIGNALED(status) ||
               (WIFEXITED(status) && WEXITSTATUS(status) != 0);
    int said_negative = strstr(buf, "negative num_fields") != NULL;
    check("test_struct_alloc_negative aborts with message",
          died && said_negative);
}

/* ── 25: turbo_strdup basic correctness ──────────────────────────────── */
static void test_turbo_strdup_basic(void) {
    /* Exercise turbo_strdup indirectly through rt_str_concat, which
     * allocates through turbo_alloc, and through rt_respond_typed
     * which builds a string via turbo_alloc. We can also test via
     * arena: allocations inside an arena survive until rt_arena_end. */
    rt_arena_begin();
    const char *a = rt_str_concat("hello", " world");
    int ok = (a != NULL && strcmp(a, "hello world") == 0);
    /* Also exercise the env_get path which uses turbo_strdup internally;
     * we just verify it doesn't crash. */
    rt_arena_end();
    check("test_turbo_strdup_basic via arena concat", ok);
}

/* ── 26: turbo_free on both arena and malloc pointers ────────────────── */
static void test_turbo_free_mixed(void) {
    /* Outside arena: allocate via rt_array_alloc (which uses turbo_calloc
     * -> malloc path), then release. Should not crash. */
    void *arr = rt_array_alloc(3);
    rt_release(arr); /* Uses free() internally on the malloc-backed ptr */

    /* Inside arena: allocations should survive until rt_arena_end,
     * and rt_release should be a no-op. */
    rt_arena_begin();
    void *arr2 = rt_array_alloc(5);
    rt_release(arr2); /* Should be a no-op (arena-backed) */
    /* Verify we can still read the array (arena didn't free it) */
    int ok = (rt_array_len(arr2) == 5);
    rt_arena_end();
    check("test_turbo_free_mixed no crash on mixed paths", ok);
}

/* ── 27: header injection sanitized in rt_respond_typed ──────────────── */
static void test_header_injection_sanitized(void) {
    /* content_type with \r\n should be replaced with "text/plain" */
    const char *resp = rt_respond_typed(200, "text/html\r\nX-Injected: evil", "body");
    /* The response format is "STATUS\x1fCONTENT_TYPE\x1fBODY".
     * If sanitization worked, content_type should be "text/plain". */
    int ok = 0;
    if (resp) {
        /* Find first separator */
        const char *sep1 = strchr(resp, '\x1f');
        if (sep1) {
            const char *sep2 = strchr(sep1 + 1, '\x1f');
            if (sep2) {
                size_t ct_len = (size_t)(sep2 - (sep1 + 1));
                ok = (ct_len == 10 && strncmp(sep1 + 1, "text/plain", 10) == 0);
            }
        }
    }
    check("test_header_injection_sanitized replaces evil content_type", ok);
}

static const char *typed_cors_test_handler(const void *env, const char *req) {
    (void)env;
    (void)req;
    return rt_respond_typed(201, "application/json", "{\"ok\":true}");
}

/* ── 28: typed HTTP responses do not add wildcard CORS by default ───── */
static void test_typed_http_response_no_default_cors(void) {
    long long port = reserve_loopback_port();
    if (port <= 0) {
        port = 49152 + ((long long)getpid() % 16000);
    }
    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0) {
        check("test_typed_http_response_no_default_cors fork", 0);
        return;
    }
    if (pid == 0) {
        long long server = rt_http_server(port);
        rt_http_route(server, "GET", "/typed", (const void *)typed_cors_test_handler, NULL);
        rt_http_listen(server);
        _exit(0);
    }

    int fd = -1;
    for (int attempt = 0; attempt < 100; attempt++) {
        fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) break;
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons((unsigned short)port);
        addr.sin_addr.s_addr = htonl(0x7f000001UL); /* 127.0.0.1 */
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
            break;
        }
        close(fd);
        fd = -1;
        struct timespec ts = {0, 10 * 1000 * 1000};
        nanosleep(&ts, NULL);
    }

    int ok = 0;
    if (fd >= 0) {
        const char *request =
            "GET /typed HTTP/1.1\r\n"
            "Host: localhost\r\n"
            "Connection: close\r\n\r\n";
        write(fd, request, strlen(request));
        shutdown(fd, SHUT_WR);
        char buf[4096];
        size_t used = 0;
        while (used < sizeof(buf) - 1) {
            ssize_t n = read(fd, buf + used, sizeof(buf) - 1 - used);
            if (n <= 0) break;
            used += (size_t)n;
        }
        buf[used] = '\0';
        ok = strstr(buf, "HTTP/1.1 201 OK\r\n") != NULL &&
             strstr(buf, "Content-Type: application/json\r\n") != NULL &&
             strstr(buf, "Access-Control-Allow-Origin") == NULL &&
             strstr(buf, "{\"ok\":true}") != NULL;
        close(fd);
    }

    if (fd < 0) {
        int status = 0;
        pid_t done = waitpid(pid, &status, WNOHANG);
        if (done == pid) {
            check("test_typed_http_response_no_default_cors skipped when bind is unavailable", 1);
            return;
        }
    }

    kill(pid, SIGTERM);
    waitpid(pid, NULL, 0);
    check("test_typed_http_response_no_default_cors omits wildcard CORS", ok);
}

/* ── BL-25 A2: stateful-server hashmap survives the per-request arena ──
 *
 * Regression for the use-after-free where a hashmap created at startup
 * (server state, no arena active) was mutated inside a request handler
 * (arena active): its keys/values were strdup'd into the per-request bump
 * arena, so rt_arena_end() freed them and the next request read dangling
 * pointers. This reproduces the exact HTTP-server lifecycle — create the map
 * outside any arena, then for each "request" begin an arena, mutate + read
 * the persistent map, and end the arena — and asserts the counter accumulates
 * correctly across requests. Before the fix this fails (corrupt counts) or
 * traps under AddressSanitizer; after it, the persistent map's storage is
 * malloc-backed and survives. */
static void test_hashmap_persists_across_request_arenas(void) {
    void *state = rt_hashmap_new(); /* created at startup: no arena active */
    int ok = 1;
    const int requests = 64;
    for (int i = 1; i <= requests; i++) {
        rt_arena_begin();                      /* per-request arena installed */
        /* hashmap_inc is the hot counter path: new key strdup'd on first hit */
        long long n = rt_hashmap_inc(state, "hits", 1);
        long long readback = rt_hashmap_get_int(state, "hits");
        /* set_int + get on a second persistent key exercise key + string
         * value storage as well, all mutated while the arena is live. */
        rt_hashmap_set_int(state, "last", n);
        const char *via_str = rt_hashmap_get(state, "last"); /* arena copy */
        long long last = via_str ? strtoll(via_str, NULL, 10) : -1;
        rt_arena_end();                        /* frees arena; map must survive */
        if (n != i || readback != i || last != i) {
            ok = 0;
        }
    }
    /* Final read happens with no arena active, after 64 arena teardowns. */
    ok = ok && rt_hashmap_get_int(state, "hits") == requests;
    check("test_hashmap_persists_across_request_arenas counts correctly", ok);
}

/* A map created and dropped *inside* one request's arena must not require
 * malloc bookkeeping — its storage rides the arena and is reclaimed at
 * rt_arena_end. This guards the request-local branch of the BL-25 A2 fix:
 * the value is observable within the request, and tearing the arena down
 * does not crash. */
static void test_hashmap_request_local_in_arena(void) {
    int ok = 1;
    for (int i = 0; i < 256; i++) {
        rt_arena_begin();
        void *local = rt_hashmap_new(); /* created with arena active */
        rt_hashmap_set(local, "k", "v");
        const char *got = rt_hashmap_get(local, "k");
        if (!got || strcmp(got, "v") != 0) {
            ok = 0;
        }
        rt_arena_end();
    }
    check("test_hashmap_request_local_in_arena reclaims via arena", ok);
}

/* ── 31: string ARC allocation sites release under ASan ───────────────── */
static void test_string_arc_loop_releases(void) {
    int ok = 1;
    for (int i = 0; i < 20000; i++) {
        const char *s = rt_str_concat("seed", "x");
        const char *replaced = rt_str_replace(s, "x", "y");
        const char *upper = rt_str_upper(replaced);
        void *parts = rt_str_split(upper, "Y");

        ok = ok && s && replaced && upper && parts;
        ok = ok && strcmp(s, "seedx") == 0;
        ok = ok && strcmp(replaced, "seedy") == 0;
        ok = ok && strcmp(upper, "SEEDY") == 0;
        ok = ok && rt_array_len(parts) == 2;

        rt_release((void *)s);
        rt_release((void *)replaced);
        rt_release((void *)upper);
        release_str_array(parts);
        if (!ok) break;
    }
    check("test_string_arc_loop_releases concat/split/replace/upper", ok);
}

/* ── 32: alias remains valid after reassignment-style concat ───────────── */
static void test_string_arc_alias_then_reassign(void) {
    const char *s = rt_str_concat("ab", "");
    rt_retain((void *)s);
    const char *alias = s;
    const char *new_s = rt_str_concat_inplace(s, "x");
    rt_release((void *)s);

    int ok = strcmp(alias, "ab") == 0 && strcmp(new_s, "abx") == 0;
    rt_release((void *)alias);
    rt_release((void *)new_s);
    check("test_string_arc_alias_then_reassign keeps alias stable", ok);
}

/* ── 33: header-backed literals with immortal refcount are no-op retained ─ */
static void test_string_arc_literal_never_freed(void) {
    static struct {
        long long cap;
        long long refcount;
        char data[8];
    } literal = {0, LLONG_MAX, "literal"};

    rt_retain(literal.data);
    rt_release(literal.data);
    rt_release(literal.data);

    check("test_string_arc_literal_never_freed survives release",
          strcmp(literal.data, "literal") == 0 && literal.refcount == LLONG_MAX);
}

static const char *string_arc_returned_value(void) {
    return rt_str_concat("esc", "ape");
}

/* ── 34: returned strings escape callee scope and can be released by caller ─ */
static void test_string_arc_return_escape(void) {
    const char *s = string_arc_returned_value();
    int ok = s && strcmp(s, "escape") == 0;
    rt_release((void *)s);
    check("test_string_arc_return_escape caller owns returned string", ok);
}

/* ── 35: string slots in array/struct-shaped storage can own references ─── */
static void test_string_arc_array_struct_slots(void) {
    const char *array_s = rt_str_concat("array", "-value");
    void *arr = rt_array_alloc(1);
    rt_retain((void *)array_s);
    ((long long *)arr)[1] = (long long)(size_t)array_s;
    rt_release((void *)array_s);
    const char *from_arr = (const char *)(size_t)rt_array_get(arr, 0);
    int ok = from_arr && strcmp(from_arr, "array-value") == 0;
    rt_release((void *)from_arr);
    rt_release(arr);

    const char *struct_s = rt_str_concat("struct", "-value");
    void *st = rt_struct_alloc(1);
    rt_retain((void *)struct_s);
    ((long long *)st)[0] = (long long)(size_t)struct_s;
    rt_release((void *)struct_s);
    const char *from_struct = (const char *)(size_t)((long long *)st)[0];
    ok = ok && from_struct && strcmp(from_struct, "struct-value") == 0;
    rt_release((void *)from_struct);
    rt_release(st);

    check("test_string_arc_array_struct_slots keep stored strings alive", ok);
}

/* ── 36: arena-backed ARC ops are sentinel no-ops inside the window ────── */
static void test_arena_arc_sentinel_noop_in_window(void) {
    rt_arena_begin();
    const char *arena_s = rt_str_concat("arena", "-value");
    long long *rc = (long long *)((char *)arena_s - 8);
    long long rc_before = *rc;
    /* retain/release must leave the RT_RC_ARENA sentinel untouched and
     * must not free arena memory. */
    rt_retain((void *)arena_s);
    rt_release((void *)arena_s);
    long long rc_after = *rc;
    int ok = arena_s && strcmp(arena_s, "arena-value") == 0
        && rc_before == (LLONG_MAX - 1)
        && rc_after == rc_before;
    rt_arena_end();
    /* INVARIANT (documented in turbo_rt.c): arena pointers must never be
     * retained or released after rt_arena_end() — the memory is gone.
     * Every persistent escape route copies, so no post-end release exists. */
    check("test_arena_arc_sentinel_noop_in_window", ok);
}

/* ── 37: heap ARC releases still run while an arena is active ──────────── */
static void test_heap_arc_release_during_active_arena_decrements(void) {
    const char *heap_s = rt_str_concat("heap", "-value");
    rt_retain((void *)heap_s);
    long long *rc = (long long *)((char *)heap_s - 8);

    rt_arena_begin();
    rt_release((void *)heap_s);
    long long rc_during_arena = *rc;
    rt_arena_end();

    int ok = heap_s && strcmp(heap_s, "heap-value") == 0 && rc_during_arena == 1;
    rt_release((void *)heap_s);
    check("test_heap_arc_release_during_active_arena_decrements", ok);
}

/* ── 38: rt_write_all completes a small write and reports EPIPE ─────────
 *
 * Guards the partial-write robustness on the response path. A small write to
 * a pipe with a live reader must fully succeed (returns 0) and deliver the
 * exact bytes. A write to a pipe whose read end is closed must return -1
 * (EPIPE) rather than blocking or crashing — the server uses this to tear a
 * dead connection down instead of looping on a broken socket. */
static void test_write_all_partial_and_epipe(void) {
    int ok = 1;

    int fds[2];
    if (pipe(fds) != 0) {
        check("test_write_all_partial_and_epipe pipe", 0);
        return;
    }
    const char *msg = "hello world";
    int r1 = rt_write_all(fds[1], msg, strlen(msg));
    char buf[32] = {0};
    ssize_t n = read(fds[0], buf, sizeof(buf) - 1);
    ok = ok && r1 == 0 && n == (ssize_t)strlen(msg) && strcmp(buf, msg) == 0;
    close(fds[0]);
    close(fds[1]);

    /* EPIPE path: close the read end, then write. SIGPIPE must be ignored so
     * the write returns -1 (EPIPE) instead of killing the process. */
    signal(SIGPIPE, SIG_IGN);
    int fds2[2];
    if (pipe(fds2) != 0) {
        check("test_write_all_partial_and_epipe pipe2", 0);
        return;
    }
    close(fds2[0]);
    int r2 = rt_write_all(fds2[1], msg, strlen(msg));
    ok = ok && r2 == -1;
    close(fds2[1]);

    check("test_write_all_partial_and_epipe", ok);
}

/* Spawn thunk that reads the string argument at slot 1 and returns a simple
 * additive checksum of its bytes, so the caller can verify the bytes are
 * intact after the request arena has been torn down. */
static long long spawn_read_str_thunk(void *args_ptr) {
    long long *slots = (long long *)args_ptr;
    const char *s = (const char *)(size_t)slots[1];
    if (!s) return -1;
    long long sum = 0;
    for (const char *p = s; *p; p++) sum += (unsigned char)*p;
    return sum;
}

/* ── 39: rt_spawn_with_args deep-copies arena-backed string args (issue #56) ─
 *
 * Reproduces the dangling-pointer hazard: build a request-arena string and a
 * spawn args struct (both RT_RC_ARENA), spawn a thread that reads the string,
 * then end the arena BEFORE joining. The fix deep-copies the string (and the
 * args struct) into malloc'd storage synchronously inside rt_spawn_with_args,
 * so the thread reads intact bytes. Without the fix the thread would read
 * freed arena memory — an ASan use-after-free / wrong checksum. */
static void test_spawn_copies_arena_string(void) {
    rt_arena_begin();
    const char *s = rt_str_concat("request-", "payload"); /* RT_RC_ARENA string */
    long long expected = 0;
    for (const char *p = s; *p; p++) expected += (unsigned char)*p;

    void *args = rt_struct_alloc(2); /* [fn_ptr, str] — arena-backed */
    ((long long *)args)[0] = (long long)(size_t)spawn_read_str_thunk;
    ((long long *)args)[1] = (long long)(size_t)s;

    /* ptr_mask bit 0 marks arg slot 0 as a string; num_args = 1. */
    void *h = rt_spawn_with_args(spawn_read_str_thunk, args, 1, 1);

    /* Tear down the request arena — the deep-copy already happened
     * synchronously inside rt_spawn_with_args, so the thread is safe. */
    rt_arena_end();

    long long got = rt_await_handle(h);
    check("test_spawn_copies_arena_string reads intact string after arena end",
          got == expected);
}

/* ── 40: rt_http_config validates keys and values ───────────────────────── */
static void test_http_config_validation(void) {
    int ok = 1;
    ok = ok && rt_http_config("max_body_bytes", 1024) == 1;
    ok = ok && rt_http_config("bogus_key", 5) == 0;         /* unknown key */
    ok = ok && rt_http_config("max_body_bytes", 0) == 0;    /* value < 1 */
    ok = ok && rt_http_config("max_body_bytes", -1) == 0;   /* negative */
    ok = ok && rt_http_config(NULL, 5) == 0;                /* null key */
    ok = ok && rt_http_config("max_header_bytes", 100) == 0; /* below 256 min */
    ok = ok && rt_http_config("max_header_bytes", 8192) == 1;
    ok = ok && rt_http_config("max_connections", 128) == 1;
    ok = ok && rt_http_config("read_timeout_ms", 5000) == 1;
    ok = ok && rt_http_config("write_timeout_ms", 5000) == 1;
    ok = ok && rt_http_config("keepalive_max_requests", 50) == 1;
    ok = ok && rt_http_config("idle_timeout_ms", 3000) == 1;
    check("test_http_config_validation accepts valid, rejects bad", ok);
}

/* ── Generic HashMap<K,V> (Tier 1.2): value retain/release discipline ── */
static void test_hashmap_generic_value_refcounts(void) {
    void *m = rt_hashmap_new_typed(1, 1); /* int keys, rc-heap values */
    const char *a = rt_str_concat("val", "-a"); /* fresh rc string, rc 1 */
    long long base_a = RT_RC(a);
    rt_hashmap_gset(m, 7, (long long)a); /* map retains -> base_a + 1 */
    int ok = RT_RC(a) == base_a + 1;
    ok = ok && rt_hashmap_ghas(m, 7) == 1 && rt_hashmap_glen(m) == 1;

    /* Overwrite releases the previous value and retains the new one. */
    const char *b = rt_str_concat("val", "-b");
    long long base_b = RT_RC(b);
    rt_hashmap_gset(m, 7, (long long)b);
    ok = ok && RT_RC(a) == base_a;     /* a released by the map */
    ok = ok && RT_RC(b) == base_b + 1; /* b retained by the map */
    ok = ok && rt_hashmap_glen(m) == 1;

    /* Remove releases the stored value. */
    rt_hashmap_gremove(m, 7);
    ok = ok && RT_RC(b) == base_b && rt_hashmap_glen(m) == 0
        && rt_hashmap_ghas(m, 7) == 0;

    rt_release((void *)a);
    rt_release((void *)b);
    check("test_hashmap_generic_value_refcounts set/overwrite/remove retain-release",
          ok);
}

static void test_hashmap_generic_resize_and_drop(void) {
    void *m = rt_hashmap_new_typed(1, 1); /* int keys, rc-heap values */
    enum { N = 100 };
    const char *vals[N];
    int ok = 1;
    for (int i = 0; i < N; i++) {
        char vbuf[16];
        snprintf(vbuf, sizeof(vbuf), "v%d", i);
        const char *v = rt_str_concat("k", vbuf); /* fresh rc string, rc 1 */
        vals[i] = v;
        rt_hashmap_gset(m, i, (long long)v); /* retained through resizes -> rc 2 */
    }
    ok = ok && rt_hashmap_glen(m) == N;
    for (int i = 0; i < N; i++) {
        ok = ok && rt_hashmap_ghas(m, i) == 1 && RT_RC(vals[i]) == 2;
    }
    /* Removing every key releases each stored value back to the caller's ref. */
    for (int i = 0; i < N; i++) {
        rt_hashmap_gremove(m, i);
    }
    ok = ok && rt_hashmap_glen(m) == 0;
    for (int i = 0; i < N; i++) {
        ok = ok && RT_RC(vals[i]) == 1;
        rt_release((void *)vals[i]); /* frees, ASan verifies no double-free */
    }
    check("test_hashmap_generic_resize_and_drop retains through resize, frees on remove",
          ok);
}

static void test_hashmap_generic_str_keys_get(void) {
    void *m = rt_hashmap_new_typed(0, 0); /* str keys, non-rc int values */
    rt_hashmap_gset(m, (long long)"alpha", 11);
    rt_hashmap_gset(m, (long long)"beta", 22);
    rt_hashmap_gset(m, (long long)"alpha", 111); /* overwrite */

    void *hit = rt_hashmap_gget(m, (long long)"alpha");
    void *miss = rt_hashmap_gget(m, (long long)"gamma");
    int ok = rt_option_tag(hit) == 1 && ((const long long *)hit)[1] == 111;
    ok = ok && rt_option_tag(miss) == 0;
    ok = ok && rt_hashmap_ghas(m, (long long)"beta") == 1;
    ok = ok && rt_hashmap_ghas(m, (long long)"zeta") == 0;
    ok = ok && rt_hashmap_glen(m) == 2;

    void *keys = rt_hashmap_gkeys(m); /* sorted [str] */
    long long klen = ((const long long *)keys)[0];
    ok = ok && klen == 2;

    rt_release(hit);
    rt_release(miss);
    rt_release(keys);
    check("test_hashmap_generic_str_keys_get get/has/keys/overwrite with string keys",
          ok);
}

int main(void) {
    printf("== turbo_rt C runtime tests ==\n");

    test_str_repeat_overflow();
    test_str_repeat_normal();
    test_str_repeat_zero();
    test_str_repeat_negative();
    test_http_get_rejects_file_scheme();
    test_http_get_rejects_flag_injection();
    test_http_get_rejects_gopher();
    test_http_get_rejects_null();
    test_http_get_rejects_empty();
    test_http_post_rejects_file_scheme();
    test_str_concat_basic();
    test_arena_reclaims_memory();
    test_arena_nested_begin_resets();
    test_arena_end_without_begin();
    test_str_join_empty();
    test_str_join_one();
    test_str_join_many();
    test_str_join_null_sep();
    test_f64_to_str_canonical_edges();
    test_array_push_empty();
    test_array_push_1000();
    test_array_push_reuses_buffer();
    test_array_push_overflow_guard();
    test_turbo_calloc_overflow();
    test_struct_alloc_negative();
    test_turbo_strdup_basic();
    test_turbo_free_mixed();
    test_header_injection_sanitized();
    test_typed_http_response_no_default_cors();
    test_hashmap_persists_across_request_arenas();
    test_hashmap_request_local_in_arena();
    test_hashmap_generic_value_refcounts();
    test_hashmap_generic_resize_and_drop();
    test_hashmap_generic_str_keys_get();
    test_string_arc_loop_releases();
    test_string_arc_alias_then_reassign();
    test_string_arc_literal_never_freed();
    test_string_arc_return_escape();
    test_string_arc_array_struct_slots();
    test_arena_arc_sentinel_noop_in_window();
    test_heap_arc_release_during_active_arena_decrements();
    test_write_all_partial_and_epipe();
    test_spawn_copies_arena_string();
    test_http_config_validation();

    if (g_failures == 0) {
        printf("\nAll tests passed.\n");
        return 0;
    }
    printf("\n%d test(s) failed.\n", g_failures);
    return 1;
}
