#!/usr/bin/env bash
#
# check_panic_budget.sh — panic-site ratchet for the Turbo compiler.
#
# A compiler must never panic on user input. This script counts the
# production panic-sites in each crate and fails if any crate exceeds its
# budget. Budgets are a ratchet: they only ever go DOWN. When you remove a
# panic, lower the budget in the same PR so it can't creep back.
#
# Run from the repo root:
#     bash scripts/check_panic_budget.sh
#
# No dependencies beyond grep/awk (POSIX). See docs/DEPANIC-BACKLOG.md for the
# full inventory, classification, and worklist.
#
# --- Methodology (must match docs/DEPANIC-BACKLOG.md) -------------------------
# A "panic-site" is a source line matching:
#     .unwrap()      bare unwrap  (unwrap_or / unwrap_or_else / unwrap_err are
#                                  NOT matched: the regex requires literal "()")
#     .expect("      Result/Option::expect with a message
#     panic!(        the panic! macro
#     unreachable!   the unreachable! macro (with or without a message)
#
# Test code is excluded approximately: for each file we count only the lines
# BEFORE the first `#[cfg(test)]` attribute, and we skip any `tests/` dir.
#
# NOTE: `.expect("` deliberately does NOT match turbo-parser's own
# `self.expect(&Token::..)` cursor method (that takes a Token, not a string
# literal, and returns a Result used with `?` — it is proper error handling,
# not a panic). A naive `grep -c '.expect('` over turbo-parser overcounts by
# ~9x for exactly this reason.
#
# KNOWN GAP: raw slice indexing (`v[i]`) can also panic but is too noisy to
# grep reliably (generics, `&Token`, etc.), so it is not counted here. The
# parser's few index sites are individually reviewed in the backlog doc.
# -----------------------------------------------------------------------------

set -euo pipefail

# Resolve repo root as the parent of this script's dir, so it works from anywhere.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$REPO_ROOT/turbo/crates"

# Per-crate budgets. Set to the post-conversion ACTUAL count. Lower, never raise.
# (Associative arrays need bash 4+; fall back to parallel arrays for macOS bash 3.)
CRATE_NAMES=(
  turbo-lexer
  turbo-ast
  turbo-parser
  turbo-sema
  turbo-codegen-cranelift
  turbo-formatter
  turbo-cli
  turbo-lsp
)
CRATE_BUDGETS=(
  0    # turbo-lexer
  0    # turbo-ast
  10   # turbo-parser   (all documented internal invariants — see backlog doc)
  34   # turbo-sema     (next cycle)
  282  # turbo-codegen-cranelift (next cycle — largest surface)
  1    # turbo-formatter
  4    # turbo-cli
  0    # turbo-lsp
)

PANIC_REGEX='\.unwrap\(\)|\.expect\("|panic!\(|unreachable!'

# Count production panic-sites in one crate's src/ tree.
count_crate() {
  local crate_src="$1"
  local total=0
  [ -d "$crate_src" ] || { echo 0; return; }
  while IFS= read -r f; do
    # Only the lines before the first #[cfg(test)] (approx test exclusion).
    local n
    n=$(awk '/#\[cfg\(test\)\]/{exit} {print}' "$f" | grep -cE "$PANIC_REGEX" || true)
    total=$((total + n))
  done < <(find "$crate_src" -name '*.rs')
  echo "$total"
}

echo "Panic-site budget check (production sites; see docs/DEPANIC-BACKLOG.md)"
echo "-----------------------------------------------------------------------"
printf '%-28s %8s %8s   %s\n' "crate" "actual" "budget" "status"

fail=0
i=0
for crate in "${CRATE_NAMES[@]}"; do
  budget="${CRATE_BUDGETS[$i]}"
  actual="$(count_crate "$CRATES_DIR/$crate/src")"
  if [ "$actual" -gt "$budget" ]; then
    status="OVER (+$((actual - budget)))"
    fail=1
  elif [ "$actual" -lt "$budget" ]; then
    status="under — lower the budget!"
  else
    status="ok"
  fi
  printf '%-28s %8s %8s   %s\n' "$crate" "$actual" "$budget" "$status"
  i=$((i + 1))
done

echo "-----------------------------------------------------------------------"
if [ "$fail" -ne 0 ]; then
  cat <<'EOF'

FAIL: a crate exceeds its panic-site budget.

A compiler must never panic on user input. Do NOT raise the budget to make
this pass. Instead, for each new panic-site:
  * If it is REACHABLE from malformed source, convert it to a ParseError /
    proper error return with an existing turbo-ast::ErrorCode and recover.
  * If it is a genuine INTERNAL invariant, document WHY with
    .expect("invariant: ...") or unreachable!("invariant: ...").
Then keep the budget where it is (or lower it). Budgets ratchet down only.
See docs/DEPANIC-BACKLOG.md for the classification method and worklist.
EOF
  exit 1
fi

echo "OK: all crates within budget."
