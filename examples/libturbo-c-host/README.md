# libturbo C host

This example embeds Turbo as trusted first-party typed scripting from a C host.
The host registers two C callbacks, evaluates Turbo source, then calls Turbo
functions that return `i64` and `str`.

```bash
cargo build -p turbo-codegen-cranelift --manifest-path ../../turbo/Cargo.toml

cc host.c \
  -I ../../turbo/crates/turbo-codegen-cranelift/include \
  -L ../../turbo/target/debug \
  -lturbo_codegen_cranelift \
  -o libturbo-host

DYLD_LIBRARY_PATH=../../turbo/target/debug ./libturbo-host
# Linux: LD_LIBRARY_PATH=../../turbo/target/debug ./libturbo-host
```

Expected output:

```text
answer=42
message=hello Turbo from C host
```

This spike is not a sandbox. It is for trusted scripts embedded in a host
application that controls the source and registered callbacks.
