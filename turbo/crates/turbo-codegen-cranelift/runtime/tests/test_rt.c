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

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Forward declarations of the runtime functions we exercise. We don't
 * include turbo_rt.c via a header — the runtime intentionally has no
 * .h file (it is consumed via include_str! in the codegen crate). */
extern const char *rt_str_repeat(const char *s, long long count);
extern const char *rt_str_concat(const char *a, const char *b);
extern const char *rt_http_get(const char *url);
extern const char *rt_http_post(const char *url, const char *body);

static int g_failures = 0;

static void check(const char *name, int ok) {
    if (ok) {
        printf("  [PASS] %s\n", name);
    } else {
        printf("  [FAIL] %s\n", name);
        g_failures++;
    }
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

    if (g_failures == 0) {
        printf("\nAll tests passed.\n");
        return 0;
    }
    printf("\n%d test(s) failed.\n", g_failures);
    return 1;
}
