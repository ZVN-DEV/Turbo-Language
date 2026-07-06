#ifndef LIBTURBO_H
#define LIBTURBO_H

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
