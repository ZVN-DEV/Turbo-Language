/*
 * Turbo Runtime Library — WASM/WASI target
 *
 * Stripped-down version of turbo_rt.c for WebAssembly.
 * Removes: pthreads, sockets, fork/exec, HTTP server/client, channels, mutexes.
 * Keeps: print, string ops, array ops, struct alloc, hashmap, json, math,
 *        result/option, assertions, file I/O (via WASI), ARC.
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>

#define TURBO_F64_FORMAT "%.15g"

static void rt_format_f64(char *buf, size_t cap, double n) {
    if (n == 0.0) n = 0.0; /* normalize negative zero */
    snprintf(buf, cap, TURBO_F64_FORMAT, n);
}

/* ── Checked allocation helpers ──────────────────────────────────── */

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

/* ── Print functions ──────────────────────────────────────────────── */

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

/* ── Error/panic functions ────────────────────────────────────────── */

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

void rt_assert_eq_fail(long long dummy, const char *left, const char *right) {
    (void)dummy;
    fprintf(stderr, "assertion failed: assert_eq\n  left:  %s\n  right: %s\n",
            left ? left : "(null)", right ? right : "(null)");
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

/* ── String operations ────────────────────────────────────────────── */

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

long long rt_str_len(const char *s) {
    return s ? (long long)strlen(s) : 0;
}

/* ── Array operations ─────────────────────────────────────────────── */

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
        (*rc_ptr)--;
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
     *   total = 8 + data_size, data_size = 8 + new_len * 8
     * does not overflow size_t. Turns adversarial or miscomputed
     * lengths into a clean abort instead of a heap overflow. */
    if (new_len < 0 || (size_t)new_len > (SIZE_MAX - 16) / 8) {
        fprintf(stderr, "runtime error: array push size overflow\n");
        exit(1);
    }
    size_t data_size = 8 + (size_t)new_len * 8;
    size_t total = 8 + data_size;
    void *new_alloc = turbo_calloc(1, total);
    *(long long*)new_alloc = 1;
    void *new_data = (char*)new_alloc + 8;
    *(long long*)new_data = new_len;
    memcpy((char*)new_data + 8, (const char*)arr + 8, (size_t)old_len * 8);
    ((long long*)new_data)[1 + old_len] = value;
    return new_data;
}

/* ── Struct allocation ────────────────────────────────────────────── */

void* rt_struct_alloc(long long num_fields) {
    size_t data_size = num_fields * 8;
    if (data_size < 8) data_size = 8;
    size_t total = 8 + data_size; /* +8 for refcount header */
    void *ptr = turbo_calloc(1, total);
    *(long long*)ptr = 1; /* refcount = 1 */
    return (char*)ptr + 8; /* return pointer past refcount */
}

/* ── Type conversion ──────────────────────────────────────────────── */

const char* rt_i64_to_str(long long n) {
    char *buf = turbo_alloc(32);
    snprintf(buf, 32, "%lld", n);
    return buf;
}

const char* rt_f64_to_str(double n) {
    char *buf = turbo_alloc(64);
    rt_format_f64(buf, 64, n);
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

/* ── Result type runtime functions ────────────────────────────────── */

void* rt_result_ok(long long value) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long));
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 0; /* ok tag */
    ptr[2] = value;
    return &ptr[1];
}

void* rt_result_err(long long value) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long));
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 1; /* err tag */
    ptr[2] = value;
    return &ptr[1];
}

long long rt_result_tag(const void *result) {
    return ((const long long*)result)[0];
}

long long rt_result_value(const void *result) {
    return ((const long long*)result)[1];
}

/* ── Optional type runtime functions ──────────────────────────────── */

void* rt_option_some(long long value) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long));
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 1; /* some tag */
    ptr[2] = value;
    return &ptr[1];
}

void* rt_option_none(void) {
    long long *ptr = (long long*)turbo_calloc(3, sizeof(long long));
    ptr[0] = 1; /* refcount = 1 */
    ptr[1] = 0; /* none tag */
    ptr[2] = 0;
    return &ptr[1];
}

long long rt_option_tag(const void *opt) {
    return ((const long long*)opt)[0];
}

long long rt_option_value(const void *opt) {
    return ((const long long*)opt)[1];
}

/* ── Standard library: string functions ───────────────────────────── */

void* rt_str_split(const char *s, const char *sep) {
    size_t sep_len = strlen(sep);
    size_t count = 1;
    const char *p = s;
    if (sep_len > 0) {
        while ((p = strstr(p, sep)) != NULL) { count++; p += sep_len; }
    } else {
        count = strlen(s);
        if (count == 0) count = 1;
    }

    size_t data_size = 8 + count * 8;
    size_t total = 8 + data_size;
    long long *raw = (long long*)turbo_calloc(1, total);
    raw[0] = 1;
    long long *arr = raw + 1;
    arr[0] = (long long)count;

    if (sep_len == 0) {
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
        size_t len = strlen(s);
        char *r = turbo_alloc(len + 1);
        memcpy(r, s, len + 1);
        return r;
    }
    size_t count = 0;
    const char *p = s;
    while ((p = strstr(p, from)) != NULL) { count++; p += from_len; }
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
    /* Length-tracked memcpy instead of strcat (O(n^2) scan + footgun). */
    size_t offset = 0;
    for (long long i = 0; i < count; i++) {
        memcpy(result + offset, s, len);
        offset += len;
    }
    result[offset] = '\0';
    return result;
}

long long rt_str_index_of(const char *s, const char *sub) {
    if (!s || !sub) return -1;
    const char *found = strstr(s, sub);
    if (!found) return -1;
    return (long long)(found - s);
}

const char* rt_str_join(const char *arr_ptr, const char *sep) {
    if (!arr_ptr) return "";
    long long len = *(long long *)arr_ptr;
    if (len <= 0) return "";
    const char **elems = (const char **)(arr_ptr + 8);
    size_t total = 0;
    size_t sep_len = sep ? strlen(sep) : 0;
    for (long long i = 0; i < len; i++) {
        if (elems[i]) total += strlen(elems[i]);
        if (i < len - 1) total += sep_len;
    }
    char *result = (char *)turbo_alloc(total + 1);
    /* Length-tracked memcpy instead of strcat (O(n^2) scan + footgun). */
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

/* ── Standard library: I/O functions ──────────────────────────────── */

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

/* ── Standard library: math functions ─────────────────────────────── */

long long rt_pow(long long base, long long exp) {
    if (exp < 0) return 0;
    long long result = 1;
    for (long long i = 0; i < exp; i++) result *= base;
    return result;
}

double rt_sqrt(double x) {
    return sqrt(x);
}

/* ── Async stubs (not supported in WASM) ──────────────────────────── */

void rt_sleep_ms(long long ms) {
    (void)ms;
    /* No-op in WASM — sleep not supported */
}

void *rt_spawn_with_args(long long (*thunk)(void *), void *args_ptr) {
    (void)thunk; (void)args_ptr;
    fprintf(stderr, "runtime error: spawn not supported in WASM target\n");
    exit(1);
    return NULL;
}

long long rt_await_handle(void *handle_ptr) {
    (void)handle_ptr;
    fprintf(stderr, "runtime error: await not supported in WASM target\n");
    exit(1);
    return 0;
}

/* ── HTTP stubs (not supported in WASM) ───────────────────────────── */

const char *rt_http_get(const char *url) {
    (void)url;
    fprintf(stderr, "runtime error: http_get not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char *rt_http_post(const char *url, const char *body) {
    (void)url; (void)body;
    fprintf(stderr, "runtime error: http_post not supported in WASM target\n");
    exit(1);
    return NULL;
}

/* ── JSON functions (kept — pure string manipulation) ─────────────── */

const char *rt_json_get(const char *json, const char *key) {
    size_t klen = strlen(key);
    char *search = (char *)turbo_alloc(klen + 3);
    search[0] = '"';
    memcpy(search + 1, key, klen);
    search[klen + 1] = '"';
    search[klen + 2] = '\0';

    const char *pos = strstr(json, search);
    free(search);
    if (!pos) return strdup("");

    pos += klen + 2;
    while (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r') pos++;
    if (*pos != ':') return strdup("");
    pos++;
    while (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r') pos++;

    if (*pos == '"') {
        pos++;
        const char *start = pos;
        while (*pos && !(*pos == '"' && *(pos - 1) != '\\')) pos++;
        size_t vlen = pos - start;
        char *val = (char *)turbo_alloc(vlen + 1);
        memcpy(val, start, vlen);
        val[vlen] = '\0';
        return val;
    } else {
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

const char *rt_json_stringify(const char *key, const char *value) {
    size_t klen = strlen(key);
    size_t vlen = strlen(value);
    size_t cap = klen + vlen + 8;
    char *buf = (char *)turbo_alloc(cap);
    snprintf(buf, cap, "{\"%s\":\"%s\"}", key, value);
    return buf;
}

/* ── Channel stubs (not supported in WASM) ────────────────────────── */

void *rt_channel_create(void) {
    fprintf(stderr, "runtime error: channels not supported in WASM target\n");
    exit(1);
    return NULL;
}

void rt_channel_send(const void *ch, long long value) {
    (void)ch; (void)value;
    fprintf(stderr, "runtime error: channels not supported in WASM target\n");
    exit(1);
}

long long rt_channel_recv(const void *ch) {
    (void)ch;
    fprintf(stderr, "runtime error: channels not supported in WASM target\n");
    exit(1);
    return 0;
}

void *rt_channel_clone_sender(const void *ch) {
    (void)ch;
    fprintf(stderr, "runtime error: channels not supported in WASM target\n");
    exit(1);
    return NULL;
}

/* ── Mutex stubs (not supported in WASM) ──────────────────────────── */

void *rt_mutex_create(long long value) {
    (void)value;
    fprintf(stderr, "runtime error: mutexes not supported in WASM target\n");
    exit(1);
    return NULL;
}

long long rt_mutex_get(const void *mptr) {
    (void)mptr;
    fprintf(stderr, "runtime error: mutexes not supported in WASM target\n");
    exit(1);
    return 0;
}

void rt_mutex_set(const void *mptr, long long value) {
    (void)mptr; (void)value;
    fprintf(stderr, "runtime error: mutexes not supported in WASM target\n");
    exit(1);
}

void *rt_mutex_clone(const void *mptr) {
    (void)mptr;
    fprintf(stderr, "runtime error: mutexes not supported in WASM target\n");
    exit(1);
    return NULL;
}

/* ── HashMap runtime ──────────────────────────────────────────────── */

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
        hash = ((hash << 5) + hash) + (unsigned long)c;
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
    if ((double)(map->count + 1) > (double)map->capacity * HASHMAP_LOAD_FACTOR) {
        hashmap_resize(map, map->capacity * 2);
    }
    unsigned long h = hashmap_hash(key) % (unsigned long)map->capacity;
    while (map->entries[h].occupied) {
        if (strcmp(map->entries[h].key, key) == 0) {
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
    char **key_ptrs = (char **)turbo_alloc((size_t)map->count * sizeof(char *));
    long long idx = 0;
    for (long long i = 0; i < map->capacity; i++) {
        if (map->entries[i].occupied) {
            key_ptrs[idx++] = map->entries[i].key;
        }
    }
    for (long long i = 1; i < idx; i++) {
        char *tmp = key_ptrs[i];
        long long j = i - 1;
        while (j >= 0 && strcmp(key_ptrs[j], tmp) > 0) {
            key_ptrs[j + 1] = key_ptrs[j];
            j--;
        }
        key_ptrs[j + 1] = tmp;
    }
    size_t data_size = 8 + (size_t)idx * 8;
    size_t total = 8 + data_size;
    long long *raw = (long long *)turbo_calloc(1, total);
    raw[0] = 1;
    long long *arr = raw + 1;
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
            unsigned long next = (h + 1) % (unsigned long)map->capacity;
            while (map->entries[next].occupied) {
                char *rk = map->entries[next].key;
                char *rv = map->entries[next].value;
                map->entries[next].key = NULL;
                map->entries[next].value = NULL;
                map->entries[next].occupied = 0;
                map->count--;
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

/* ── HTTP server stubs (not supported in WASM) ────────────────────── */

long long rt_http_server(long long port) {
    (void)port;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return 0;
}

long long rt_http_server_public(long long port) {
    (void)port;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return 0;
}

void rt_http_route(long long server_id, const char *method, const char *path, const void *handler, const void *env_ptr) {
    (void)server_id; (void)method; (void)path; (void)handler; (void)env_ptr;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
}

void rt_http_listen(long long server_id) {
    (void)server_id;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
}

const char* rt_respond(long long status, const char *body) {
    (void)status; (void)body;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char* rt_respond_typed(long long status, const char *content_type, const char *body) {
    (void)status; (void)content_type; (void)body;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char* rt_request_body(const char *req) {
    (void)req;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char* rt_request_method(const char *req) {
    (void)req;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char* rt_request_path(const char *req) {
    (void)req;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char* rt_request_query(const char *req, const char *key) {
    (void)req; (void)key;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

const char* rt_request_header(const char *req, const char *name) {
    (void)req; (void)name;
    fprintf(stderr, "runtime error: HTTP server not supported in WASM target\n");
    exit(1);
    return NULL;
}

/* ── ARC (Automatic Reference Counting) runtime ──────────────────── */

void rt_retain(void *data_ptr) {
    if (!data_ptr) return;
    long long *rc = (long long*)((char*)data_ptr - 8);
    (*rc)++;
}

void rt_release(void *data_ptr) {
    if (!data_ptr) return;
    long long *rc = (long long*)((char*)data_ptr - 8);
    long long prev = (*rc)--;
    if (prev == 1) {
        /* Refcount reached 0 — could free here in the future */
    }
}

/* ── Entry point: WASI expects _start, which calls turbo_main ─────── */

extern void turbo_main(void);
int main(void) {
    turbo_main();
    return 0;
}
