# De-Panic Backlog

**A compiler must never panic on user input.** Malformed source must always
surface as a `ParseError` / typed diagnostic and recover — never as an
`unwrap`/`expect`/`panic!`/`unreachable!` abort.

This document inventories the production panic-sites across the workspace,
classifies the parser's sites, and records the worklist for the crates that
are next. It is kept honest by two automated gates:

- `scripts/check_panic_budget.sh` — per-crate ratchet; fails CI if a crate
  gains panic-sites (see [Budgets](#budgets)).
- `turbo-parser`'s proptest + `turbo/fuzz` harness — assert the parser never
  panics on random / turbo-like token streams.

---

## Methodology

Counts are produced by `grep`/`awk` over each crate's `src/` tree (see
`scripts/check_panic_budget.sh` for the exact implementation, which is the
source of truth). A **panic-site** is a source line matching:

| Pattern        | Matches                                                        |
|----------------|---------------------------------------------------------------|
| `.unwrap()`    | bare `unwrap` (NOT `unwrap_or` / `unwrap_or_else` / `unwrap_err`) |
| `.expect("`    | `Result`/`Option::expect` with a message                      |
| `panic!(`      | the `panic!` macro                                            |
| `unreachable!` | the `unreachable!` macro (with or without a message)          |

Test code is excluded **approximately**: for each file only the lines *before*
the first `#[cfg(test)]` attribute are counted, and `tests/` dirs are skipped.

### The `self.expect()` artifact (why the naive count is ~9x too high for the parser)

`turbo-parser` defines its **own** cursor method
`fn expect(&mut self, expected: &Token) -> Result<Span, ParseError>` — proper
error handling used with `?`, **not** a panic. A naive `grep -c '.expect('`
counts all 87 `self.expect(&Token::..)` call-sites as if they were panics.
The refined methodology (requiring `.expect("` — i.e. a string-literal message)
excludes them. This is why turbo-parser drops from **97 naive** to **10 real**
panic-sites. Every other crate's naive count equals its refined count (no other
crate has a method named `expect`).

### Known gaps in the methodology

- **Raw slice indexing** (`v[i]`) can also panic but is too noisy to grep
  reliably (generics, `&Token`, byte-string patterns). The parser's few index
  sites are reviewed individually below instead of counted.
- **Multi-line `.expect(`** where the `(` and the `"message"` land on separate
  lines (rustfmt's choice for long messages) is not matched by the line-based
  regex. One such site exists in the parser (`soft_keyword_ident`, below); it is
  a documented invariant, so the undercount is harmless.

---

## Per-crate counts

Refined = real panic-sites by the methodology above. "This cycle" = the parser
pass in this branch.

| Crate                     | Naive | Refined | Status                                    |
|---------------------------|------:|--------:|-------------------------------------------|
| `turbo-lexer`             |     0 |       0 | clean                                     |
| `turbo-ast`               |     0 |       0 | clean                                     |
| **`turbo-parser`**        |    97 |      10 | **hardened this cycle** (all documented invariants) |
| `turbo-sema`              |    34 |      34 | next cycle                                |
| `turbo-codegen-cranelift` |   282 |     282 | next cycle (largest surface)              |
| `turbo-formatter`         |     1 |       1 | next cycle                                |
| `turbo-cli`               |     4 |       4 | next cycle                                |
| `turbo-lsp`               |     0 |       0 | clean                                     |

> The headline "~170 parser panic-sites / 378 unwrap / 154 expect repo-wide"
> figure that motivated this work was measured with the **naive** grep and is
> dominated by the `self.expect()` artifact. The real parser figure is 10.

---

## turbo-parser — classification & worklist

Classification key:

- **(a) REACHABLE** — reachable from malformed source input; MUST become a
  `ParseError` + recovery.
- **(b) INTERNAL** — genuinely impossible given a prior check in the same
  function; documented with an `expect("invariant: ..")` /
  `unreachable!("invariant: ..")` message. Behaviour unchanged.
- **(c) INVESTIGATE** — needs more analysis.

**Result of this cycle: 0 category-(a) sites.** The parser was already written
defensively — it has full error recovery (`parse_module` skip-to-next-item at
`lib.rs`), guards every `advance()`/index behind a preceding `peek()`, and
routes all malformed input through `ParseError`. Every real panic-site is a
category-(b) internal invariant. This cycle converted each bare site into a
documented one (0 behaviour change on valid input) and removed one raw index in
favour of the panic-free `.get().unwrap_or()` form already used one arm away.

### Sites (all category (b))

| # | File | Site | Category | Justification (why it cannot fire on user input) |
|---|------|------|----------|--------------------------------------------------|
| 1 | `lib.rs` | `advance()` cursor index → `.get(pos).expect("invariant: …")` | (b) | Every caller guards with `peek()`/`matches!(self.peek(), Some(..))` first, so `pos` is always in bounds. A failure means a missing peek-guard (parser bug), not malformed input. Was a bare `self.tokens[self.pos]` index. |
| 2 | `lib.rs` | `soft_keyword_ident(t).expect("invariant: guard checked is_some()")` | (b) | Reached only inside a match arm whose guard is `Self::soft_keyword_ident(t).is_some()`. Was `.unwrap()`. |
| 3 | `lib.rs` | import path: `unreachable!("… Token::String")` | (b) | Outer `match` already matched `Some(Token::String(_))`; `advance()` returns that same token, so the `if let Token::String` re-check always holds. |
| 4–7 | `lib.rs` | `parse_atom` Int/Float/String/Ident literals: `unreachable!("… Token::X")` | (b) | Same peek→advance→destructure invariant, one per literal kind. |
| 8–9 | `lib.rs` | `parse_pattern` Int/String literals: `unreachable!("… Token::X")` | (b) | Same invariant in pattern position. |
| 10–11 | `cow_rewrite.rs` | `rewrite_call_to_assign returned non-Expr` (×2) | (b) | `rewrite_call_to_assign` is a local helper that *always* returns `Stmt::Expr`; called only on AST already confirmed to be a COW-builtin call. Already carried a message before this cycle. |

### Guarded raw-index sites (reviewed, left in place)

Not counted by the regex; individually confirmed safe:

| File | Site | Why safe |
|------|------|----------|
| `lib.rs` | `expect()` — `self.tokens[self.pos].span.clone()` | Two lines under `if let Some(tok) = self.peek()`, so `pos` is in bounds. |
| `lib.rs` | Try-operator `?` — `self.tokens[self.pos].span.clone()` | Guarded by `else if matches!(self.peek(), Some(Token::Question))`. |
| `lib.rs` | enum-item span end | **Converted this cycle** to `.get(pos.saturating_sub(1)).map(..).unwrap_or(start)` to match the struct arm and drop the raw index entirely. |

### Deferred

None for turbo-parser. There are no category-(a) or category-(c) sites; the
crate is fully classified.

---

## Next cycles (counts only — not yet classified)

These crates are out of scope for this pass and are recorded for follow-up.
Each is its own de-panic cycle with the same classify → convert (a) →
document (b) → ratchet-the-budget workflow.

| Crate                     | Refined | Note |
|---------------------------|--------:|------|
| `turbo-codegen-cranelift` |     282 | Largest surface; codegen invariants + FFI. Highest-value next target — user source reaches codegen. |
| `turbo-sema`              |      34 | Type-checker; some sites are `Ty::Error` poison-path invariants, some may be reachable. Needs classification. |
| `turbo-cli`               |       4 | Frontend/driver; likely I/O and arg-handling. |
| `turbo-formatter`         |       1 | Single site. |

---

## Budgets

`scripts/check_panic_budget.sh` enforces a per-crate ceiling equal to the
current actual count. **Budgets ratchet down only** — when you remove a panic,
lower the budget in the same PR. To add a legitimate new panic you must either
prove it's an internal invariant (document it) or convert it; you do **not**
raise the budget to make CI pass.

| Crate                     | Budget |
|---------------------------|-------:|
| `turbo-lexer`             |      0 |
| `turbo-ast`               |      0 |
| `turbo-parser`            |     10 |
| `turbo-sema`              |     34 |
| `turbo-codegen-cranelift` |    282 |
| `turbo-formatter`         |      1 |
| `turbo-cli`               |      4 |
| `turbo-lsp`               |      0 |
