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
#include <time.h>
#include <sys/wait.h>

/* ── Checked allocation helpers (C-3) ──────────────────────────────── */

static void *turbo_alloc(size_t size) {
    void *p = malloc(size);
    if (!p) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    return p;
}

static void *turbo_calloc(size_t count, size_t size) {
    void *p = calloc(count, size);
    if (!p) { fprintf(stderr, "runtime error: out of memory\n"); exit(1); }
    return p;
}

static void *turbo_realloc(void *ptr, size_t size) {
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

void* rt_array_alloc(long long len) {
    size_t data_size = 8 + len * 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    void *ptr = turbo_calloc(1, total);
    *(long long*)ptr = 1; /* refcount = 1 */
    void *data_ptr = (char*)ptr + 8;
    *(long long*)data_ptr = len;
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
    /* COW: check refcount before mutating */
    long long *rc_ptr = (long long*)((char*)arr - 8);
    long long rc = *rc_ptr;
    void *target;
    if (rc > 1) {
        /* Copy-on-write: make a private copy */
        long long len = *(const long long*)arr;
        size_t data_size = (1 + (size_t)len) * 8;
        size_t total = 8 + data_size;
        void *new_alloc = turbo_calloc(1, total);
        *(long long*)new_alloc = 1;
        void *new_data = (char*)new_alloc + 8;
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
    size_t data_size = 8 + new_len * 8;   /* length field + elements */
    size_t total = 8 + data_size;          /* + refcount header */
    void *new_alloc = turbo_calloc(1, total);
    *(long long*)new_alloc = 1;            /* refcount = 1 */
    void *new_data = (char*)new_alloc + 8;
    *(long long*)new_data = new_len;       /* set length */
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
    size_t data_size = num_fields * 8;
    if (data_size < 8) data_size = 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    void *ptr = turbo_calloc(1, total);
    *(long long*)ptr = 1; /* refcount = 1 */
    return (char*)ptr + 8; /* return pointer past refcount */
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
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long)); /* refcount + tag + value */
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 0; /* ok tag */
    ptr[2] = value;
    return &ptr[1]; /* return pointer past refcount */
}

void* rt_result_err(long long value) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long)); /* refcount + tag + value */
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 1; /* err tag */
    ptr[2] = value;
    return &ptr[1]; /* return pointer past refcount */
}

long long rt_result_tag(const void *result) {
    return ((const long long*)result)[0];
}

long long rt_result_value(const void *result) {
    return ((const long long*)result)[1];
}

/* Optional type runtime functions */

void* rt_option_some(long long value) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long)); /* refcount + tag + value */
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 1; /* some tag */
    ptr[2] = value;
    return &ptr[1]; /* return pointer past refcount */
}

void* rt_option_none(void) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long)); /* refcount + tag + value */
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 0; /* none tag */
    ptr[2] = 0;
    return &ptr[1]; /* return pointer past refcount */
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

    size_t data_size = 8 + count * 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    long long *raw = (long long*)turbo_calloc(1, total);
    raw[0] = 1; /* refcount = 1 */
    long long *arr = raw + 1; /* data pointer past refcount */
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
    size_t total = len * (size_t)count;
    char *result = (char *)turbo_alloc(total + 1);
    result[0] = '\0';
    for (long long i = 0; i < count; i++) {
        strcat(result, s);
    }
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
    result[0] = '\0';
    for (long long i = 0; i < len; i++) {
        if (elems[i]) strcat(result, elems[i]);
        if (i < len - 1 && sep) strcat(result, sep);
    }
    return result;
}

/* ── Standard library: I/O functions ───────────────────────────────── */

const char* rt_read_line(void) {
    char buf[4096];
    if (fgets(buf, sizeof(buf), stdin) == NULL) {
        char *e = turbo_alloc(1); e[0] = '\0'; return e;
    }
    size_t len = strlen(buf);
    while (len > 0 && (buf[len-1] == '\n' || buf[len-1] == '\r')) { buf[--len] = '\0'; }
    char *result = turbo_alloc(len + 1);
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

/* Helper: read all data from a file descriptor into a heap-allocated string */
static char *read_fd_to_string(int fd) {
    size_t cap = 4096, len = 0;
    char *buf = (char *)turbo_alloc(cap);
    while (1) {
        ssize_t n = read(fd, buf + len, cap - len - 1);
        if (n <= 0) break;
        len += (size_t)n;
        if (len + 1 >= cap) {
            cap *= 2;
            buf = (char *)turbo_realloc(buf, cap);
        }
    }
    buf[len] = '\0';
    return buf;
}

/* http_get(url) -> str — HTTP GET via fork+exec (no shell interpolation) */
const char *rt_http_get(const char *url) {
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
        /* Child: redirect stdout to pipe, exec curl */
        close(pipefd[0]);
        dup2(pipefd[1], STDOUT_FILENO);
        close(pipefd[1]);
        execlp("curl", "curl", "-s", "-L", url, (char *)NULL);
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
               "-d", body, url, (char *)NULL);
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
    /* Simple: no escaping beyond what's needed */
    size_t klen = strlen(key);
    size_t vlen = strlen(value);
    /* {"key":"value"}\0 — worst case 2x for escaping */
    size_t cap = klen + vlen + 8;
    char *buf = (char *)turbo_alloc(cap);
    snprintf(buf, cap, "{\"%s\":\"%s\"}", key, value);
    return buf;
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

    /* Allocate with refcount header: [refcount: i64][sender_ptr: i64][receiver_ptr: i64] */
    size_t ch_total = 8 + sizeof(channel_handle); /* +8 for refcount */
    void *raw = turbo_calloc(1, ch_total);
    *(long long*)raw = 1; /* refcount = 1 */
    channel_handle *h = (channel_handle *)((char*)raw + 8);
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
    /* Allocate with refcount header */
    size_t ch_total = 8 + sizeof(channel_handle);
    void *raw = turbo_calloc(1, ch_total);
    *(long long*)raw = 1; /* refcount = 1 */
    channel_handle *nh = (channel_handle *)((char*)raw + 8);
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
    /* Build array in same format as rt_str_split: [refcount][len][ptr0][ptr1]... */
    size_t data_size = 8 + (size_t)idx * 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    long long *raw = (long long *)turbo_calloc(1, total);
    raw[0] = 1; /* refcount = 1 */
    long long *arr = raw + 1; /* data pointer past refcount */
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

/* ── HTTP server runtime ─────────────────────────────────────────────── */

#include <sys/socket.h>
#include <netinet/in.h>
#include <unistd.h>

typedef const char* (*route_handler_fn)(const void*, const char*);

typedef struct {
    char method[16];
    char path[1024];
    route_handler_fn handler;
    const void *env_ptr;
} Route;

typedef struct {
    unsigned short port;
    Route routes[64];
    int route_count;
} HttpServerC;

static HttpServerC http_servers[16];
static int http_server_count = 0;

long long rt_http_server(long long port) {
    if (http_server_count >= 16) { fprintf(stderr, "error: max 16 HTTP servers\n"); exit(1); }
    int id = http_server_count++;
    http_servers[id].port = (unsigned short)port;
    http_servers[id].route_count = 0;
    return id;
}

void rt_http_route(long long server_id, const char *method, const char *path, const void *handler, const void *env_ptr) {
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
        /* Find end of headers (\r\n\r\n) */
        char *hdr_end = strstr(buf, "\r\n\r\n");
        if (!hdr_end) continue; /* Need more data */

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

        /* Determine content-length */
        int content_length = 0;
        {
            const char *cl = headers_raw;
            for (size_t i = 0; i + 15 < headers_len; i++) {
                if (strncasecmp(cl + i, "content-length:", 15) == 0) {
                    content_length = atoi(cl + i + 15);
                    break;
                }
            }
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
        if (body_available < content_length) continue; /* Need more data */

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
                    const char *colon = strchr(resp, ':');
                    if (colon) {
                        int status = atoi(resp);
                        const char *resp_body = colon + 1;
                        int resp_len = strlen(resp_body);
                        char hdr[512];
                        snprintf(hdr, sizeof(hdr),
                            "HTTP/1.1 %d OK\r\nContent-Type: application/json\r\n"
                            "Access-Control-Allow-Origin: *\r\nConnection: %s\r\n"
                            "Content-Length: %d\r\n\r\n",
                            status, conn_hdr, resp_len);
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
        int consumed = (int)(body_start - buf) + content_length;
        int remaining = buf_len - consumed;
        if (remaining > 0) {
            memmove(buf, buf + consumed, remaining);
        }
        buf_len = remaining;

        if (!keep_alive) break;

        /* If there's already another request in the buffer, process it immediately */
        if (buf_len > 0 && strstr(buf, "\r\n\r\n")) {
            goto process_request;
        }
    }

    close(fd);
    return NULL;
}

void rt_http_listen(long long server_id) {
    HttpServerC *srv = &http_servers[server_id];

    int server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) { perror("socket"); return; }

    int opt = 1;
    setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
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

        conn_thread_data *data = malloc(sizeof(conn_thread_data));
        if (!data) { close(client_fd); continue; }
        data->client_fd = client_fd;
        data->srv = srv;

        pthread_t tid;
        pthread_attr_t attr;
        pthread_attr_init(&attr);
        pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
        if (pthread_create(&tid, &attr, handle_http_conn, data) != 0) {
            close(client_fd);
            free(data);
        }
        pthread_attr_destroy(&attr);
    }
}

const char* rt_respond(long long status, const char *body) {
    if (!body) body = "";
    size_t blen = strlen(body);
    /* "STATUS:BODY\0" */
    char digits[20];
    int dlen = snprintf(digits, sizeof(digits), "%lld", status);
    char *result = turbo_alloc(dlen + 1 + blen + 1);
    memcpy(result, digits, dlen);
    result[dlen] = ':';
    memcpy(result + dlen + 1, body, blen);
    result[dlen + 1 + blen] = '\0';
    return result;
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
    long long *rc = (long long*)((char*)data_ptr - 8);
    __sync_fetch_and_add(rc, 1);
}

void rt_release(void *data_ptr) {
    if (!data_ptr) return;
    long long *rc = (long long*)((char*)data_ptr - 8);
    long long prev = __sync_fetch_and_sub(rc, 1);
    if (prev == 1) {
        /* Refcount reached 0 — memory could be freed here.
         * TODO: store allocation size in header for proper dealloc.
         * For now we just let it leak (same as before ARC). */
    }
}

/* Entry point: calls Turbo's main and returns 0 */
extern void turbo_main(void);
int main(void) {
    turbo_main();
    return 0;
}
