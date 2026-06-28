/*
 * turbo_rt_guards.h — shared size-overflow / allocation-cap guards.
 *
 * Both turbo_rt.c (native AOT runtime) and turbo_rt_wasm.c (WASM/WASI
 * runtime) include this header so the hardening stays in lockstep. The two
 * runtimes were hand-maintained in parallel and the WASM copy previously
 * drifted out of sync, missing overflow checks the native side had. Keeping
 * the math in one place prevents that class of drift.
 *
 * The helpers are `static inline` so each translation unit gets its own
 * copy with no link-time symbol; unused ones are dropped by the compiler.
 *
 * Header-size note: the native runtime uses a 16-byte refcount+capacity
 * header in front of array/struct data; the WASM runtime uses an 8-byte
 * refcount header. The array-length bound therefore takes the header size
 * as a parameter so each runtime passes its own value, while the actual
 * overflow arithmetic stays shared.
 */
#ifndef TURBO_RT_GUARDS_H
#define TURBO_RT_GUARDS_H

#include <stddef.h>
#include <stdint.h>

/* Practical allocation cap: 256 MB. Totals beyond this almost always indicate
 * a bug or an attack and would exhaust memory; both runtimes refuse them.
 * Mirrors the cap used by the Rust JIT runtime helpers. */
#define TURBO_RT_MAX_ALLOC_BYTES (256ULL * 1024ULL * 1024ULL)

/* Returns 1 if `a * b` does not overflow size_t, 0 otherwise.
 * Used to guard `total = a * b` allocations on both wasm32 (32-bit size_t)
 * and 64-bit hosts. */
static inline int rt_mul_size_fits(size_t a, size_t b) {
    if (b == 0) return 1;
    return a <= SIZE_MAX / b;
}

/* Returns 1 if `a * b + add` does not overflow size_t, 0 otherwise.
 * `add` reserves room for a trailing NUL or a fixed-size header. */
static inline int rt_mul_add_size_fits(size_t a, size_t b, size_t add) {
    if (!rt_mul_size_fits(a, b)) return 0;
    return a * b <= SIZE_MAX - add;
}

/* Array length bound shared by array alloc / COW copy / push / split.
 *
 * The Turbo array allocation layout is:
 *   total = header_bytes + 8 (in-band length field) + len * 8 (elements)
 * This returns 1 iff that total fits in size_t (and len is non-negative),
 * turning an adversarial or miscomputed length into a clean abort at the
 * call site instead of an undersized allocation + heap overflow. */
static inline int rt_array_len_fits_hdr(long long len, size_t header_bytes) {
    if (len < 0) return 0;
    return rt_mul_add_size_fits((size_t)len, 8u, header_bytes + 8u);
}

#endif /* TURBO_RT_GUARDS_H */
