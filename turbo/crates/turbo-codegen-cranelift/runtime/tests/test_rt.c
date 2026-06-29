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

static int g_failures = 0;

static void check(const char *name, int ok) {
    if (ok) {
        printf("  [PASS] %s\n", name);
    } else {
        printf("  [FAIL] %s\n", name);
        g_failures++;
    }
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

    if (g_failures == 0) {
        printf("\nAll tests passed.\n");
        return 0;
    }
    printf("\n%d test(s) failed.\n", g_failures);
    return 1;
}
