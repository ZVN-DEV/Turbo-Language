/* Optional observer shared by JIT and AOT. No user allocation layout changes.
 * All bookkeeping allocations are uninstrumented and separately accounted. */
#include "turbo_alloc_profile.h"
#include <inttypes.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define PROFILE_BUCKETS 65536
typedef struct Entry {
    const void *raw;
    const void *arena;
    size_t data;
    size_t header;
    struct Entry *next;
} Entry;

static Entry *buckets[PROFILE_BUCKETS];
static atomic_flag guard = ATOMIC_FLAG_INIT;
static _Atomic int active = 0;
static TurboAllocationProfile stats;

static void lock(void) {
    while (atomic_flag_test_and_set_explicit(&guard, memory_order_acquire)) {}
}
static void unlock(void) { atomic_flag_clear_explicit(&guard, memory_order_release); }
static void error(void) {
    stats.valid = 0;
    if (stats.errors != UINT64_MAX) stats.errors++;
}
static void add(uint64_t *counter, uint64_t n) {
    if (UINT64_MAX - *counter < n) { *counter = UINT64_MAX; error(); }
    else *counter += n;
}
static void subtract(uint64_t *counter, uint64_t n) {
    if (*counter < n) { *counter = 0; error(); }
    else *counter -= n;
}
static size_t bucket(const void *p) {
    uintptr_t key = (uintptr_t)p;
    return ((key >> 4) ^ (key >> 20)) % PROFILE_BUCKETS;
}
static void clear_entries(void) {
    for (size_t i = 0; i < PROFILE_BUCKETS; i++) {
        Entry *p = buckets[i];
        while (p) { Entry *next = p->next; free(p); p = next; }
        buckets[i] = NULL;
    }
}

int turbo_profile_begin(void) {
    lock();
    if (atomic_load_explicit(&active, memory_order_relaxed)) {
        error(); unlock(); return 0;
    }
    clear_entries();
    memset(&stats, 0, sizeof(stats));
    stats.valid = 1;
    stats.peak_observer_bytes = sizeof(buckets);
    atomic_store_explicit(&active, 1, memory_order_release);
    unlock();
    return 1;
}

int turbo_profile_start_if_requested(void) {
    const char *value = getenv("TURBO_ALLOC_PROFILE");
    return value && strcmp(value, "1") == 0 ? turbo_profile_begin() : 0;
}

void turbo_profile_alloc(const void *raw, size_t total, size_t header, const void *arena) {
    if (!atomic_load_explicit(&active, memory_order_acquire)) return;
    lock();
    if (!atomic_load_explicit(&active, memory_order_relaxed)) { unlock(); return; }
    if (!raw || total < header) { error(); unlock(); return; }
    size_t index = bucket(raw);
    for (Entry *p = buckets[index]; p; p = p->next) {
        if (p->raw == raw) { error(); unlock(); return; }
    }
    Entry *p = malloc(sizeof(*p));
    if (!p) { error(); unlock(); return; }
    *p = (Entry){ raw, arena, total - header, header, buckets[index] };
    buckets[index] = p;
    add(&stats.allocations, 1);
    add(arena ? &stats.arena_allocations : &stats.heap_allocations, 1);
    add(&stats.total_data_bytes, p->data);
    add(&stats.total_header_bytes, p->header);
    add(&stats.live_allocations, 1);
    add(&stats.live_data_bytes, p->data);
    add(&stats.live_header_bytes, p->header);
    if (stats.live_allocations > stats.peak_live_allocations)
        stats.peak_live_allocations = stats.live_allocations;
    if (stats.live_data_bytes > stats.peak_live_data_bytes)
        stats.peak_live_data_bytes = stats.live_data_bytes;
    if (stats.live_allocations > (UINT64_MAX - sizeof(buckets)) / sizeof(Entry)) error();
    else {
        uint64_t overhead = sizeof(buckets) + stats.live_allocations * sizeof(Entry);
        if (overhead > stats.peak_observer_bytes) stats.peak_observer_bytes = overhead;
    }
    unlock();
}

static void reclaim(Entry *p) {
    add(p->arena ? &stats.arena_reclaims : &stats.heap_frees, 1);
    subtract(&stats.live_allocations, 1);
    subtract(&stats.live_data_bytes, p->data);
    subtract(&stats.live_header_bytes, p->header);
    free(p); /* Bookkeeping only; never free the program's allocation here. */
}

void turbo_profile_free(const void *raw) {
    if (!raw || !atomic_load_explicit(&active, memory_order_acquire)) return;
    lock();
    if (!atomic_load_explicit(&active, memory_order_relaxed)) { unlock(); return; }
    Entry **cursor = &buckets[bucket(raw)];
    while (*cursor && (*cursor)->raw != raw) cursor = &(*cursor)->next;
    if (*cursor) {
        Entry *p = *cursor;
        *cursor = p->next;
        reclaim(p);
    } else {
        add(&stats.unknown_frees, 1);
        error();
    }
    unlock();
}

void turbo_profile_arena_reset(const void *arena) {
    if (!arena || !atomic_load_explicit(&active, memory_order_acquire)) return;
    lock();
    if (!atomic_load_explicit(&active, memory_order_relaxed)) { unlock(); return; }
    for (size_t i = 0; i < PROFILE_BUCKETS; i++) {
        Entry **cursor = &buckets[i];
        while (*cursor) {
            Entry *p = *cursor;
            if (p->arena == arena) { *cursor = p->next; reclaim(p); }
            else cursor = &p->next;
        }
    }
    unlock();
}

void turbo_profile_rc(int operation) {
    if (!atomic_load_explicit(&active, memory_order_acquire)) return;
    lock();
    if (atomic_load_explicit(&active, memory_order_relaxed)) {
        switch (operation) {
            case TURBO_RETAIN_CALL: add(&stats.retain_calls, 1); break;
            case TURBO_RELEASE_CALL: add(&stats.release_calls, 1); break;
            case TURBO_RETAIN_OP: add(&stats.retain_ops, 1); break;
            case TURBO_RELEASE_OP: add(&stats.release_ops, 1); break;
            default: error();
        }
    }
    unlock();
}

void turbo_profile_snapshot(TurboAllocationProfile *out) {
    lock();
    if (out) *out = stats;
    unlock();
}

void turbo_profile_end(TurboAllocationProfile *out) {
    lock();
    atomic_store_explicit(&active, 0, memory_order_release);
    if (out) *out = stats;
    clear_entries();
    unlock();
}

void turbo_profile_finish(void) {
    TurboAllocationProfile s;
    turbo_profile_end(&s);
    char line[4096];
    int n = snprintf(line, sizeof(line),
        "TURBO_ALLOC_PROFILE {\"schema_version\":1,\"coverage\":\"shared_header_arc\","
        "\"scope_end\":\"entry_return\",\"valid\":%s", s.valid ? "true" : "false");
#define EMIT(name) do { \
    if (n < 0 || (size_t)n >= sizeof(line)) return; \
    int wrote = snprintf(line + n, sizeof(line) - (size_t)n, ",\"" #name "\":%" PRIu64, s.name); \
    if (wrote < 0 || (size_t)wrote >= sizeof(line) - (size_t)n) return; \
    n += wrote; \
} while (0);
    TURBO_PROFILE_FIELDS(EMIT)
#undef EMIT
    if (n < 0 || (size_t)n + 3 > sizeof(line)) return;
    memcpy(line + n, "}\n", 3);
    fputs(line, stderr);
}
