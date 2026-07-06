# libturbo

`libturbo` is the embeddable Turbo JIT surface for trusted first-party scripts.
It lets a native host evaluate Turbo source, register C host functions by name,
and call selected Turbo functions through a small C ABI.

The current spike ships from `turbo-codegen-cranelift` as `rlib`, `cdylib`, and
`staticlib`.

## Build

```bash
cargo build -p turbo-codegen-cranelift --manifest-path turbo/Cargo.toml
```

The C header is:

```text
turbo/crates/turbo-codegen-cranelift/include/libturbo.h
```

## API

```c
typedef struct TurboVm TurboVm;

TurboVm *turbo_vm_new(void);
void turbo_vm_free(TurboVm *vm);

const char *turbo_vm_last_error(const TurboVm *vm);
bool turbo_vm_register_host_fn(TurboVm *vm, const char *name, const void *fn_ptr);

bool turbo_eval(TurboVm *vm, const char *source);
bool turbo_call_i64(TurboVm *vm, const char *fn_name, int64_t *out);
const char *turbo_call_str(TurboVm *vm, const char *fn_name);
```

`turbo_eval` lexes, parses, type-checks, JIT-compiles, and evaluates the source.
If the module defines `fn main()`, `main` must be zero-argument and return unit;
it is run after compilation. Host functions must be registered before
`turbo_eval` and declared from Turbo with `@unsafe extern "C"`.

`turbo_call_i64` and `turbo_call_str` currently accept zero-argument Turbo
functions with explicit `-> i64`/`-> int` or `-> str` return types. The returned
string pointer is owned by the VM and remains valid until the next string result
or `turbo_vm_free`.

On failure, calls return `false` or `NULL`; use `turbo_vm_last_error` for the
diagnostic.

## Example

See `examples/libturbo-c-host`. It demonstrates:

- C registering `host_add(i64, str) -> i64`.
- C registering `host_greet(str) -> str`.
- Turbo calling those host functions from an `extern "C"` block.
- C calling Turbo `answer() -> i64` and `message() -> str`.

## Sandbox Feasibility

Go/no-go for this spike: **go for trusted first-party scripts, no-go for
untrusted third-party scripts**.

The current JIT links the standard runtime symbols into every compiled module,
including file, environment, process execution, HTTP, and server helpers. A real
sandbox needs a capability-shaped runtime symbol table, tests proving denied
capabilities fail closed, and probably a separate VM profile that omits ambient
I/O by construction. That is a follow-up design and implementation lane, not
part of this spike.
