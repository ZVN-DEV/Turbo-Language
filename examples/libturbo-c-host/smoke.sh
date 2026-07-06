#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
target_dir="$repo_root/turbo/target/debug"
include_dir="$repo_root/turbo/crates/turbo-codegen-cranelift/include"
work_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

cargo build -p turbo-codegen-cranelift --manifest-path "$repo_root/turbo/Cargo.toml"

cc \
  "$script_dir/host.c" \
  -I"$include_dir" \
  -L"$target_dir" \
  -lturbo_codegen_cranelift \
  -o "$work_dir/libturbo-c-host-smoke"

output="$(
  DYLD_LIBRARY_PATH="$target_dir${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}" \
  LD_LIBRARY_PATH="$target_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$work_dir/libturbo-c-host-smoke"
)"

expected='answer=42
message=hello Turbo from C host'

if [[ "$output" != "$expected" ]]; then
  printf 'unexpected libturbo C host output\nexpected:\n%s\nactual:\n%s\n' "$expected" "$output" >&2
  exit 1
fi

printf '%s\n' "$output"
