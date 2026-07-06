#ifndef LIBTURBO_H
#define LIBTURBO_H

/*
 * Embeddable Turbo JIT C API.
 *
 * The built library artifact is named
 * libturbo_codegen_cranelift.{dylib,so,a}, not libturbo.
 *
 * Runtime faults in Turbo code, including failed assert/assert_eq, divide by
 * zero, integer overflow, array out-of-bounds, and exit(), call process::exit
 * and terminate the host process. They are not catchable through
 * turbo_vm_last_error.
 *
 * All turbo_* calls, including use of a TurboVm handle, are single-thread only;
 * there is no internal synchronization.
 *
 * Multiple TurboVm instances share thread-local and process-global runtime
 * state such as the string arena and HTTP server registry. They are not
 * isolated from each other; use one live VM at a time.
 *
 * turbo_eval runs fn main() synchronously on the calling thread. A blocking
 * main, such as an HTTP server, blocks the host.
 */

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TurboVm TurboVm;

TurboVm *turbo_vm_new(void);
void turbo_vm_free(TurboVm *vm);

const char *turbo_vm_last_error(const TurboVm *vm);
bool turbo_vm_register_host_fn(TurboVm *vm, const char *name, const void *fn_ptr);

bool turbo_eval(TurboVm *vm, const char *source);
bool turbo_call_i64(TurboVm *vm, const char *fn_name, int64_t *out);
const char *turbo_call_str(TurboVm *vm, const char *fn_name);

#ifdef __cplusplus
}
#endif

#endif
