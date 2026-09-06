#include "../turbo_alloc_profile.h"
#include <assert.h>
#include <pthread.h>
#include <stdint.h>

static void *churn(void *arg) {
    uintptr_t base = (uintptr_t)arg;
    for (uintptr_t i = 1; i <= 1000; i++) {
        void *p = (void *)(base + i);
        turbo_profile_alloc(p, 48, 16, NULL);
        turbo_profile_rc(TURBO_RETAIN_CALL);
        turbo_profile_rc(TURBO_RETAIN_OP);
        turbo_profile_rc(TURBO_RELEASE_CALL);
        turbo_profile_rc(TURBO_RELEASE_OP);
        turbo_profile_free(p);
    }
    return NULL;
}

int main(void) {
    TurboAllocationProfile s;
    assert(turbo_profile_begin());
    turbo_profile_alloc((void *)1, 48, 16, NULL);
    turbo_profile_snapshot(&s);
    assert(s.valid && s.allocations == 1 && s.live_data_bytes == 32);
    assert(s.total_header_bytes == 16 && s.live_header_bytes == 16);
    turbo_profile_free((void *)1);
    turbo_profile_end(&s);
    assert(s.valid && s.heap_frees == 1 && s.live_allocations == 0);
    assert(s.live_data_bytes == 0 && s.peak_live_data_bytes == 32);

    assert(turbo_profile_begin());
    turbo_profile_alloc((void *)1, 48, 16, (void *)101);
    turbo_profile_alloc((void *)2, 80, 16, (void *)102);
    turbo_profile_arena_reset((void *)101);
    turbo_profile_snapshot(&s);
    assert(s.valid && s.arena_reclaims == 1 && s.live_data_bytes == 64);
    turbo_profile_arena_reset((void *)102);
    turbo_profile_end(&s);
    assert(s.valid && s.arena_allocations == 2 && s.live_allocations == 0);

    assert(turbo_profile_begin());
    turbo_profile_free((void *)404);
    turbo_profile_end(&s);
    assert(!s.valid && s.unknown_frees == 1);

    assert(turbo_profile_begin());
    turbo_profile_alloc((void *)1, 48, 16, NULL);
    turbo_profile_alloc((void *)1, 48, 16, NULL);
    turbo_profile_end(&s);
    assert(!s.valid && s.errors > 0);

    assert(turbo_profile_begin());
    turbo_profile_alloc((void *)1, SIZE_MAX, 0, NULL);
    turbo_profile_alloc((void *)2, 32, 0, NULL);
    turbo_profile_end(&s);
    assert(!s.valid); /* Overflow must never wrap into a plausible zero. */

    assert(turbo_profile_begin());
    assert(!turbo_profile_begin()); /* Refuse overlapping measurement scopes. */
    turbo_profile_end(&s);
    assert(!s.valid);

    assert(turbo_profile_begin());
    pthread_t threads[4];
    for (uintptr_t i = 0; i < 4; i++)
        assert(pthread_create(&threads[i], NULL, churn, (void *)(i * 10000)) == 0);
    for (int i = 0; i < 4; i++) assert(pthread_join(threads[i], NULL) == 0);
    turbo_profile_end(&s);
    assert(s.valid && s.allocations == 4000 && s.heap_frees == 4000);
    assert(s.total_data_bytes == 128000 && s.total_header_bytes == 64000);
    assert(s.retain_calls == 4000 && s.release_ops == 4000);
    assert(s.live_allocations == 0 && s.live_data_bytes == 0);
    return 0;
}
