/*
 * Turbo Runtime Library
 *
 * Provides runtime functions for AOT-compiled Turbo binaries.
 * These are the same functions that the JIT links as function pointers,
 * but compiled as real symbols for the system linker to resolve.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <math.h>

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
    printf("%g\n", n);
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
    char *result = malloc(a_len + b_len + 1);
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

void* rt_array_alloc(long long len) {
    size_t total = 8 + len * 8;
    void *ptr = calloc(1, total);
    *(long long*)ptr = len;
    return ptr;
}

long long rt_array_get(const void *arr, long long index) {
    long long len = *(const long long*)arr;
    if (index < 0 || index >= len) {
        fprintf(stderr, "runtime error: array index %lld out of bounds (length %lld)\n", index, len);
        exit(1);
    }
    return ((const long long*)arr)[1 + index];
}

void rt_array_set(void *arr, long long index, long long value) {
    long long len = *(const long long*)arr;
    if (index < 0 || index >= len) {
        fprintf(stderr, "runtime error: array index %lld out of bounds (length %lld)\n", index, len);
        exit(1);
    }
    ((long long*)arr)[1 + index] = value;
}

long long rt_array_len(const void *arr) {
    return *(const long long*)arr;
}

long long rt_str_len(const char *s) {
    return s ? (long long)strlen(s) : 0;
}

void* rt_struct_alloc(long long num_fields) {
    size_t size = num_fields * 8;
    if (size < 8) size = 8;
    return calloc(1, size);
}

const char* rt_i64_to_str(long long n) {
    char *buf = malloc(32);
    snprintf(buf, 32, "%lld", n);
    return buf;
}

const char* rt_f64_to_str(double n) {
    char *buf = malloc(32);
    snprintf(buf, 32, "%g", n);
    return buf;
}

const char* rt_bool_to_str(char b) {
    if (b) {
        char *buf = malloc(5);
        memcpy(buf, "true", 5);
        return buf;
    } else {
        char *buf = malloc(6);
        memcpy(buf, "false", 6);
        return buf;
    }
}

/* Result type runtime functions */

void* rt_result_ok(long long value) {
    long long *ptr = (long long*)calloc(2, sizeof(long long));
    ptr[0] = 0; /* ok tag */
    ptr[1] = value;
    return ptr;
}

void* rt_result_err(long long value) {
    long long *ptr = (long long*)calloc(2, sizeof(long long));
    ptr[0] = 1; /* err tag */
    ptr[1] = value;
    return ptr;
}

long long rt_result_tag(const void *result) {
    return ((const long long*)result)[0];
}

long long rt_result_value(const void *result) {
    return ((const long long*)result)[1];
}

/* Optional type runtime functions */

void* rt_option_some(long long value) {
    long long *ptr = (long long*)calloc(2, sizeof(long long));
    ptr[0] = 1; /* some tag */
    ptr[1] = value;
    return ptr;
}

void* rt_option_none(void) {
    long long *ptr = (long long*)calloc(2, sizeof(long long));
    ptr[0] = 0; /* none tag */
    ptr[1] = 0;
    return ptr;
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

    size_t total = 8 + count * 8;
    long long *arr = (long long*)calloc(1, total);
    arr[0] = (long long)count;

    if (sep_len == 0) {
        /* split each character */
        size_t slen = strlen(s);
        if (slen == 0) {
            char *empty = malloc(1);
            empty[0] = '\0';
            arr[1] = (long long)(size_t)empty;
        } else {
            for (size_t i = 0; i < slen; i++) {
                char *ch = malloc(2);
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
            char *part = malloc(len + 1);
            memcpy(part, p, len);
            part[len] = '\0';
            arr[1 + idx] = (long long)(size_t)part;
            idx++;
            p = next + sep_len;
        }
        /* last part */
        size_t len = strlen(p);
        char *part = malloc(len + 1);
        memcpy(part, p, len);
        part[len] = '\0';
        arr[1 + idx] = (long long)(size_t)part;
    }
    return arr;
}

const char* rt_str_trim(const char *s) {
    if (!s) { char *e = malloc(1); e[0] = '\0'; return e; }
    const char *start = s;
    while (*start == ' ' || *start == '\t' || *start == '\n' || *start == '\r') start++;
    const char *end = s + strlen(s);
    while (end > start && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\n' || end[-1] == '\r')) end--;
    size_t len = (size_t)(end - start);
    char *result = malloc(len + 1);
    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

const char* rt_str_upper(const char *s) {
    if (!s) { char *e = malloc(1); e[0] = '\0'; return e; }
    size_t len = strlen(s);
    char *result = malloc(len + 1);
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        result[i] = (c >= 'a' && c <= 'z') ? (char)(c - 32) : (char)c;
    }
    result[len] = '\0';
    return result;
}

const char* rt_str_lower(const char *s) {
    if (!s) { char *e = malloc(1); e[0] = '\0'; return e; }
    size_t len = strlen(s);
    char *result = malloc(len + 1);
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
        char *e = malloc(1); e[0] = '\0'; return e;
    }
    size_t from_len = strlen(from);
    size_t to_len = strlen(to);
    if (from_len == 0) {
        /* No replacement for empty pattern; return copy */
        size_t len = strlen(s);
        char *r = malloc(len + 1);
        memcpy(r, s, len + 1);
        return r;
    }
    /* Count occurrences */
    size_t count = 0;
    const char *p = s;
    while ((p = strstr(p, from)) != NULL) { count++; p += from_len; }
    size_t new_len = strlen(s) + count * (to_len - from_len);
    char *result = malloc(new_len + 1);
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
    char *result = malloc(2);
    result[0] = s[(size_t)index];
    result[1] = '\0';
    return result;
}

/* ── Standard library: I/O functions ───────────────────────────────── */

const char* rt_read_line(void) {
    char buf[4096];
    if (fgets(buf, sizeof(buf), stdin) == NULL) {
        char *e = malloc(1); e[0] = '\0'; return e;
    }
    size_t len = strlen(buf);
    while (len > 0 && (buf[len-1] == '\n' || buf[len-1] == '\r')) { buf[--len] = '\0'; }
    char *result = malloc(len + 1);
    memcpy(result, buf, len + 1);
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
    fseek(f, 0, SEEK_SET);
    char *buf = malloc((size_t)size + 1);
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
    if (exp < 0) return 0;
    long long result = 1;
    for (long long i = 0; i < exp; i++) result *= base;
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
    usleep((unsigned int)(ms * 1000));
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
    spawn_ctx *ctx = (spawn_ctx *)malloc(sizeof(spawn_ctx));
    ctx->thunk = thunk;
    ctx->args_ptr = args_ptr;
    ctx->result = 0;
    pthread_t *handle = (pthread_t *)malloc(sizeof(pthread_t) + sizeof(spawn_ctx *));
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

/* http_get(url) -> str — HTTP GET via system curl */
const char *rt_http_get(const char *url) {
    char cmd[4096];
    snprintf(cmd, sizeof(cmd), "curl -s -L '%s'", url);
    FILE *fp = popen(cmd, "r");
    if (!fp) {
        char *err = strdup("error: cannot run curl");
        return err;
    }
    size_t cap = 4096, len = 0;
    char *buf = (char *)malloc(cap);
    while (1) {
        size_t n = fread(buf + len, 1, cap - len - 1, fp);
        if (n == 0) break;
        len += n;
        if (len + 1 >= cap) {
            cap *= 2;
            buf = (char *)realloc(buf, cap);
        }
    }
    buf[len] = '\0';
    pclose(fp);
    return buf;
}

/* http_post(url, body) -> str — HTTP POST via system curl */
const char *rt_http_post(const char *url, const char *body) {
    /* Build command — uses stdin to pass body safely */
    char cmd[4096];
    snprintf(cmd, sizeof(cmd),
        "curl -s -L -X POST -H 'Content-Type: application/json' -d '%s' '%s'",
        body, url);
    FILE *fp = popen(cmd, "r");
    if (!fp) {
        char *err = strdup("error: cannot run curl");
        return err;
    }
    size_t cap = 4096, len = 0;
    char *buf = (char *)malloc(cap);
    while (1) {
        size_t n = fread(buf + len, 1, cap - len - 1, fp);
        if (n == 0) break;
        len += n;
        if (len + 1 >= cap) {
            cap *= 2;
            buf = (char *)realloc(buf, cap);
        }
    }
    buf[len] = '\0';
    pclose(fp);
    return buf;
}

/* json_get(json, key) -> str — extract top-level key value from JSON string */
const char *rt_json_get(const char *json, const char *key) {
    /* Build search pattern: "key" */
    size_t klen = strlen(key);
    char *search = (char *)malloc(klen + 3);
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
        char *val = (char *)malloc(vlen + 1);
        memcpy(val, start, vlen);
        val[vlen] = '\0';
        return val;
    } else {
        /* Number, bool, or null */
        const char *start = pos;
        while (*pos && *pos != ',' && *pos != '}' && *pos != ']' &&
               *pos != ' ' && *pos != '\n' && *pos != '\r' && *pos != '\t') pos++;
        size_t vlen = pos - start;
        char *val = (char *)malloc(vlen + 1);
        memcpy(val, start, vlen);
        val[vlen] = '\0';
        return val;
    }
}

/* json_stringify(key, value) -> str — build {"key":"value"} */
const char *rt_json_stringify(const char *key, const char *value) {
    /* Simple: no escaping beyond what's needed */
    size_t klen = strlen(key);
    size_t vlen = strlen(value);
    /* {"key":"value"}\0 — worst case 2x for escaping */
    size_t cap = klen + vlen + 8;
    char *buf = (char *)malloc(cap);
    snprintf(buf, cap, "{\"%s\":\"%s\"}", key, value);
    return buf;
}

/* Entry point: calls Turbo's main and returns 0 */
extern void turbo_main(void);
int main(void) {
    turbo_main();
    return 0;
}
