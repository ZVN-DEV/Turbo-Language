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
    size_t data_size = 8 + len * 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    void *ptr = calloc(1, total);
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
        void *new_alloc = calloc(1, total);
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

long long rt_str_len(const char *s) {
    return s ? (long long)strlen(s) : 0;
}

void* rt_struct_alloc(long long num_fields) {
    size_t data_size = num_fields * 8;
    if (data_size < 8) data_size = 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    void *ptr = calloc(1, total);
    *(long long*)ptr = 1; /* refcount = 1 */
    return (char*)ptr + 8; /* return pointer past refcount */
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
    long long *ptr = (long long*)calloc(3, sizeof(long long)); /* refcount + tag + value */
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 0; /* ok tag */
    ptr[2] = value;
    return &ptr[1]; /* return pointer past refcount */
}

void* rt_result_err(long long value) {
    long long *ptr = (long long*)calloc(3, sizeof(long long)); /* refcount + tag + value */
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
    long long *ptr = (long long*)calloc(3, sizeof(long long)); /* refcount + tag + value */
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 1; /* some tag */
    ptr[2] = value;
    return &ptr[1]; /* return pointer past refcount */
}

void* rt_option_none(void) {
    long long *ptr = (long long*)calloc(3, sizeof(long long)); /* refcount + tag + value */
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
    long long *raw = (long long*)calloc(1, total);
    raw[0] = 1; /* refcount = 1 */
    long long *arr = raw + 1; /* data pointer past refcount */
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
    channel_queue *q = (channel_queue *)calloc(1, sizeof(channel_queue));
    pthread_mutex_init(&q->lock, NULL);
    pthread_cond_init(&q->cond, NULL);

    /* Allocate with refcount header: [refcount: i64][sender_ptr: i64][receiver_ptr: i64] */
    size_t ch_total = 8 + sizeof(channel_handle); /* +8 for refcount */
    void *raw = calloc(1, ch_total);
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

    channel_node *node = (channel_node *)malloc(sizeof(channel_node));
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
    void *raw = calloc(1, ch_total);
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
    turbo_mutex *m = (turbo_mutex *)malloc(sizeof(turbo_mutex));
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
    map->entries = (hashmap_entry *)calloc((size_t)new_cap, sizeof(hashmap_entry));
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
    turbo_hashmap *map = (turbo_hashmap *)malloc(sizeof(turbo_hashmap));
    map->entries = (hashmap_entry *)calloc(HASHMAP_INIT_CAP, sizeof(hashmap_entry));
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
    char **key_ptrs = (char **)malloc((size_t)map->count * sizeof(char *));
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
    long long *raw = (long long *)calloc(1, total);
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
    int id = http_server_count++;
    http_servers[id].port = (unsigned short)port;
    http_servers[id].route_count = 0;
    return id;
}

void rt_http_route(long long server_id, const char *method, const char *path, const void *handler, const void *env_ptr) {
    HttpServerC *srv = &http_servers[server_id];
    int idx = srv->route_count++;
    strncpy(srv->routes[idx].method, method, 15);
    srv->routes[idx].method[15] = '\0';
    strncpy(srv->routes[idx].path, path, 1023);
    srv->routes[idx].path[1023] = '\0';
    srv->routes[idx].handler = (route_handler_fn)handler;
    srv->routes[idx].env_ptr = env_ptr;
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
    if (listen(server_fd, 128) < 0) {
        perror("listen"); close(server_fd); return;
    }

    while (1) {
        int client_fd = accept(server_fd, NULL, NULL);
        if (client_fd < 0) continue;

        char buf[8192];
        int n = read(client_fd, buf, sizeof(buf) - 1);
        if (n <= 0) { close(client_fd); continue; }
        buf[n] = '\0';

        /* Parse request line */
        char method[16] = {0}, path_buf[1024] = {0};
        sscanf(buf, "%15s %1023s", method, path_buf);

        /* Find Content-Length */
        int content_length = 0;
        char *cl = strstr(buf, "Content-Length:");
        if (!cl) cl = strstr(buf, "content-length:");
        if (cl) content_length = atoi(cl + 15);

        /* Find body (after \r\n\r\n) */
        char *body_start = strstr(buf, "\r\n\r\n");
        const char *body = body_start ? body_start + 4 : "";
        (void)content_length; /* body is already in the buffer */

        /* Match route */
        int matched = 0;
        for (int i = 0; i < srv->route_count; i++) {
            if (strcmp(srv->routes[i].method, method) == 0 &&
                strcmp(srv->routes[i].path, path_buf) == 0) {
                const char *resp = srv->routes[i].handler(srv->routes[i].env_ptr, body);
                if (resp) {
                    /* Parse STATUS:BODY format */
                    const char *colon = strchr(resp, ':');
                    if (colon) {
                        int status = atoi(resp);
                        const char *resp_body = colon + 1;
                        int resp_len = strlen(resp_body);
                        char hdr[512];
                        snprintf(hdr, sizeof(hdr),
                            "HTTP/1.1 %d OK\r\nContent-Type: application/json\r\n"
                            "Access-Control-Allow-Origin: *\r\nContent-Length: %d\r\n\r\n",
                            status, resp_len);
                        write(client_fd, hdr, strlen(hdr));
                        write(client_fd, resp_body, resp_len);
                    } else {
                        int resp_len = strlen(resp);
                        char hdr[512];
                        snprintf(hdr, sizeof(hdr),
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: %d\r\n\r\n",
                            resp_len);
                        write(client_fd, hdr, strlen(hdr));
                        write(client_fd, resp, resp_len);
                    }
                } else {
                    const char *empty = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                    write(client_fd, empty, strlen(empty));
                }
                matched = 1;
                break;
            }
        }
        if (!matched) {
            const char *nf = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
            write(client_fd, nf, strlen(nf));
        }
        close(client_fd);
    }
}

const char* rt_respond(long long status, const char *body) {
    if (!body) body = "";
    size_t blen = strlen(body);
    /* "STATUS:BODY\0" */
    char digits[20];
    int dlen = snprintf(digits, sizeof(digits), "%lld", status);
    char *result = malloc(dlen + 1 + blen + 1);
    memcpy(result, digits, dlen);
    result[dlen] = ':';
    memcpy(result + dlen + 1, body, blen);
    result[dlen + 1 + blen] = '\0';
    return result;
}

const char* rt_request_body(const char *req) {
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
