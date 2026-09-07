#ifndef TURBO_ALLOC_PROFILE_H
#define TURBO_ALLOC_PROFILE_H

#include <stddef.h>
#include <stdint.h>

/* Observer ABI only: this does not change the language's allocation header.
 * Data bytes include capacity/terminators/container metadata, not just logical
 * user payload. Foreign allocations and observer storage are outside coverage. */
#define TURBO_PROFILE_FIELDS(X) \
    X(allocations) X(heap_allocations) X(arena_allocations) \
    X(heap_frees) X(arena_reclaims) X(total_data_bytes) X(total_header_bytes) \
    X(live_allocations) X(peak_live_allocations) X(live_data_bytes) \
    X(peak_live_data_bytes) X(live_header_bytes) \
    X(retain_calls) X(release_calls) X(retain_ops) X(release_ops) \
    X(unknown_frees) X(errors) X(peak_observer_bytes)

typedef struct TurboAllocationProfile {
#define TURBO_PROFILE_FIELD(name) uint64_t name;
    TURBO_PROFILE_FIELDS(TURBO_PROFILE_FIELD)
#undef TURBO_PROFILE_FIELD
    int valid;
} TurboAllocationProfile;

enum { TURBO_RETAIN_CALL, TURBO_RELEASE_CALL, TURBO_RETAIN_OP, TURBO_RELEASE_OP };

int turbo_profile_begin(void);
int turbo_profile_start_if_requested(void);
void turbo_profile_alloc(const void *raw, size_t total, size_t header, const void *arena);
void turbo_profile_free(const void *raw);
void turbo_profile_arena_reset(const void *arena);
void turbo_profile_rc(int operation);
void turbo_profile_snapshot(TurboAllocationProfile *out);
void turbo_profile_end(TurboAllocationProfile *out);
void turbo_profile_finish(void);

#endif
