#!/usr/bin/env python3
"""check_error_codes.py — every diagnostic must carry a real ErrorCode.

Two passes over every .rs file in turbo-parser/turbo-sema/turbo-codegen-cranelift:

1. **Construction pass.** Every literal `ParseError | SemaError | CodegenError { ... }`
   (and every `Type::new(...)` constructor for those types) must set `code:`
   to a real `ErrorCode::E0NNN` literal — *or* a bare `code` parameter when
   the enclosing fn signature declares `code: ErrorCode` (passthrough helpers
   like `Checker::error`). The walk back to find the enclosing fn is bounded
   by the previous `}` at column 0 so an unrelated helper signature far up
   in the file cannot accidentally allowlist a downstream constructor.
2. **Caller pass.** Every call to a passthrough helper — i.e. any method
   anywhere in the lint's file universe whose signature contains a
   `code: ErrorCode` parameter — must pass an `ErrorCode::E0NNN` literal
   as the first positional argument. Helper discovery is a *first sweep*
   across every linted file: we scan every fn signature for
   `code: ErrorCode` and build a global set of method names, then any
   `self.<name>(...)` or `<recv>.<name>(...)` call site in any file is
   checked for an ErrorCode literal. This generalizes the historical
   hardcoded `{error, warn}` pair so adding a new diagnostic helper
   automatically extends the lint.

   **Name-collision safety.** Helper discovery only emits a method name
   into the global set if *every* fn with that name across the linted
   universe takes `code: ErrorCode`. If any unrelated overload (e.g.
   `Logger::error(&self, msg: &str)`) shares the name, the helper is
   dropped from the set because the caller pass can't distinguish which
   receiver the call site targets without full type inference. This is a
   conservative false-*negative* trade-off: we'd rather miss a check at
   a call site than flag unrelated code. The construction pass still
   catches any bare `code` field inside the real helper's body via
   `fn_takes_code`.

Fallback variants (`Unknown`, `Placeholder`, `Todo`, `Unspecified`,
`Default`) are rejected unconditionally.

Source preprocessing strips line comments, block comments (with nesting),
and string-literal contents (replacing them with spaces of equal length so
byte offsets reported by downstream regexes still line up with the original
file). This eliminates false-positives from things like
`// ParseError { code: ErrorCode::Unknown }` in a comment or
`let s = "ParseError { code: ErrorCode::Unknown }";` in a string literal.

## Conventions enforced by this lint

**ErrorCode must always be the FIRST positional argument of any `::new`
constructor** for ParseError / SemaError / CodegenError. The lint's
`check_new_constructors` check only inspects the first positional arg
of `Type::new(...)` — writing a constructor like
`SemaError::new(span, ErrorCode::E0100, msg)` with the code in position 1
will be rejected as if the ErrorCode were missing entirely. This is a
project convention, enforced here rather than in the compiler, because
canonicalizing the arg order makes diagnostic call sites uniform and the
lint cheap to implement.

Similarly, helper methods with a `code: ErrorCode` parameter must take
that parameter in a fixed position; the caller pass only validates the
FIRST positional arg at each call site.

Exit: 0 = clean, 1 = violation(s), 2 = invocation error.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CRATE_DIRS = [
    REPO_ROOT / "turbo" / "crates" / "turbo-parser",
    REPO_ROOT / "turbo" / "crates" / "turbo-sema",
    REPO_ROOT / "turbo" / "crates" / "turbo-codegen-cranelift",
]
FALLBACKS = ("Unknown", "Placeholder", "Todo", "TODO", "Unspecified", "Default")
FALLBACK_RE = re.compile(r"\bErrorCode::(" + "|".join(FALLBACKS) + r")\b")
REAL_CODE_RE = re.compile(r"\bErrorCode::E\d{4}\b")
CTOR_RE = re.compile(r"\b(ParseError|SemaError|CodegenError)\s*\{")
NEW_CTOR_RE = re.compile(r"\b(ParseError|SemaError|CodegenError)::new\s*\(")
FN_NAME_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*")
DEF_LINE_RE = re.compile(r"\b(pub\s+struct|impl)\b[^;]*\b(ParseError|SemaError|CodegenError)\b")
CODE_FIELD_RE = re.compile(r"\bcode\s*(?::\s*([^\n}]*?))?\s*[,}]")
CODE_PARAM_RE = re.compile(r"\bcode\s*:\s*ErrorCode\b")


def strip_comments_and_strings(text: str) -> str:
    """Replace string-literal contents and comment bodies with spaces.

    Invariant: the returned string has exactly the same length as ``text``
    and identical newline placement, so any byte offset / line number
    derived from the cleaned text refers to the same position in the
    original source. This lets the rest of the lint operate on a sanitized
    view without losing diagnostics. Handles:

    * ``// ...`` line comments (terminated by newline)
    * ``/* ... */`` block comments, including arbitrarily nested ones
      (Rust permits nesting, unlike C)
    * ``"..."`` string literals with ``\\"`` escapes
    * ``r"..."`` raw strings and ``r#"..."#`` / ``r##"..."##`` etc.
    * ``b"..."``, ``br"..."``, ``br#"..."#`` byte-string variants
    * ``'c'`` char literals (treated like a one-char string for safety)

    Quotation marks themselves are preserved so the original tokenization
    of the surrounding code remains intact; only the *contents* between
    the delimiters are blanked.
    """
    out = list(text)
    n = len(text)
    i = 0
    while i < n:
        c = text[i]

        # Line comment: //...\n
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = i
            while j < n and text[j] != "\n":
                out[j] = " "
                j += 1
            i = j
            continue

        # Block comment: /* ... */ with nesting support.
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            out[i] = " "
            out[i + 1] = " "
            j = i + 2
            while j < n and depth > 0:
                if j + 1 < n and text[j] == "/" and text[j + 1] == "*":
                    depth += 1
                    out[j] = " "
                    out[j + 1] = " "
                    j += 2
                    continue
                if j + 1 < n and text[j] == "*" and text[j + 1] == "/":
                    depth -= 1
                    out[j] = " "
                    out[j + 1] = " "
                    j += 2
                    continue
                # Preserve newlines so line counting stays accurate.
                if text[j] != "\n":
                    out[j] = " "
                j += 1
            i = j
            continue

        # Raw string literal: r"...", r#"..."#, br"...", br#"..."#, etc.
        # Detect optional b prefix, then r, then any number of #, then ".
        # Make sure the leading char isn't part of an identifier (e.g.
        # the `r` in `let read = ...` is a variable, not a raw string).
        if (c == "r" or (c == "b" and i + 1 < n and text[i + 1] == "r")) and (
            i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")
        ):
            k = i + (2 if c == "b" else 1)
            hashes = 0
            while k < n and text[k] == "#":
                hashes += 1
                k += 1
            if k < n and text[k] == '"':
                raw_start = k
                # Find closing "<hashes # marks>
                close_seq = '"' + ("#" * hashes)
                end = text.find(close_seq, raw_start + 1)
                if end < 0:
                    # Unterminated raw string — bail out, blank rest.
                    for q in range(raw_start + 1, n):
                        if text[q] != "\n":
                            out[q] = " "
                    i = n
                    continue
                # Blank contents between opening " and closing "
                for q in range(raw_start + 1, end):
                    if text[q] != "\n":
                        out[q] = " "
                i = end + len(close_seq)
                continue

        # Regular string literal: "..." with \\ escapes. Optional b prefix.
        if c == '"' or (c == "b" and i + 1 < n and text[i + 1] == '"'):
            start = i if c == '"' else i + 1
            j = start + 1
            while j < n:
                ch = text[j]
                if ch == "\\" and j + 1 < n:
                    # Blank both the backslash and the escaped char.
                    if text[j] != "\n":
                        out[j] = " "
                    if text[j + 1] != "\n":
                        out[j + 1] = " "
                    j += 2
                    continue
                if ch == '"':
                    break
                if ch != "\n":
                    out[j] = " "
                j += 1
            # Closing quote (or EOF) — leave the quote char alone if present.
            i = j + 1 if j < n else n
            continue

        # Char literal: '\'' or 'c' or '\x41' or lifetime 'a — distinguish
        # by checking that the next non-prefix char is a closing quote
        # within a few chars and not followed by an identifier char.
        if c == "'":
            # Lifetime check: 'ident not followed by another '
            if i + 1 < n and (text[i + 1].isalpha() or text[i + 1] == "_"):
                k = i + 1
                while k < n and (text[k].isalnum() or text[k] == "_"):
                    k += 1
                if k >= n or text[k] != "'":
                    # It's a lifetime, leave alone.
                    i = k
                    continue
            # Otherwise it's a char literal — find the closing '.
            j = i + 1
            while j < n:
                if text[j] == "\\" and j + 1 < n:
                    if text[j] != "\n":
                        out[j] = " "
                    if text[j + 1] != "\n":
                        out[j + 1] = " "
                    j += 2
                    continue
                if text[j] == "'":
                    break
                if text[j] != "\n":
                    out[j] = " "
                j += 1
            i = j + 1 if j < n else n
            continue

        i += 1

    return "".join(out)


def matching(text: str, open_idx: int, oc: str, cc: str) -> int:
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == oc:
            depth += 1
        elif text[i] == cc:
            depth -= 1
            if depth == 0:
                return i
    return -1


def line_of(text: str, idx: int) -> int:
    return text.count("\n", 0, idx) + 1


def skip_generics(text: str, idx: int) -> int:
    """If ``text[idx]`` is ``<``, walk a balanced ``<...>`` block and
    return the index *after* the closing ``>``. Otherwise return ``idx``
    unchanged.

    This exists because `fn foo<T: Trait<U>>(...)` — nested generics —
    defeats a flat regex like ``<[^>]*>``: the inner ``>`` closes the
    match prematurely and the lint then fails to locate the parameter
    list, which silently drops the fn from helper discovery and breaks
    the ``fn_takes_code`` passthrough allowlist.

    The walker counts ``<`` / ``>`` with depth. It is intentionally
    naive: it doesn't try to handle shift operators (``>>``) or turbofish
    ``::<``, because we're scanning *signatures*, not expression bodies,
    and signatures don't produce those tokens in Rust. If the close is
    never found we return -1 so callers can bail out gracefully rather
    than crashing.
    """
    n = len(text)
    if idx >= n or text[idx] != "<":
        return idx
    depth = 0
    j = idx
    while j < n:
        ch = text[j]
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
            if depth == 0:
                return j + 1
        j += 1
    return -1


def iter_fn_heads(text: str, start: int = 0, end: int | None = None):
    """Yield `(name, sig_start, sig_end)` for every fn signature found
    in ``text[start:end]``, where ``sig_start`` is the index of the
    opening ``(`` of the parameter list and ``sig_end`` is the index of
    the matching ``)``.

    Uses `skip_generics` to step over nested generic parameter lists so
    declarations like `fn foo<T: Trait<U>>(...)` are handled correctly.
    A signature whose generics or params are malformed (no matching
    bracket) is silently skipped — the lint is a guardrail, not a full
    parser, and a corrupt source file will already be failing to compile.
    """
    if end is None:
        end = len(text)
    pos = start
    while pos < end:
        m = FN_NAME_RE.search(text, pos, end)
        if not m:
            return
        name = m.group(1)
        after = m.end()
        # Skip whitespace between `fn name` and either `<` or `(`.
        while after < end and text[after].isspace():
            after += 1
        if after < end and text[after] == "<":
            after_gen = skip_generics(text, after)
            if after_gen < 0:
                pos = m.end()
                continue
            after = after_gen
            while after < end and text[after].isspace():
                after += 1
        if after >= end or text[after] != "(":
            pos = m.end()
            continue
        paren_open = after
        paren_close = matching(text, paren_open, "(", ")")
        if paren_close < 0:
            pos = m.end()
            continue
        yield name, paren_open, paren_close
        pos = paren_close + 1


def fn_takes_code(text: str, idx: int) -> bool:
    """True if the nearest enclosing `fn name(...)` before idx declares
    a `code: ErrorCode` parameter, *bounded* by the previous top-level
    item terminator.

    The walk refuses to look past the most recent occurrence of ``\\n}``
    at column 0 (a line that is exactly ``}`` followed by newline / EOF).
    That's the conservative marker for "previous top-level item ended
    here", so a passthrough helper sitting in an unrelated impl block far
    above cannot accidentally allowlist a constructor below it. If no
    such terminator exists we fall back to scanning from the start of
    the file (matches the historical behavior for the first item).

    Uses `iter_fn_heads` so nested generics like
    `fn foo<T: Trait<U>>(...)` are handled correctly.
    """
    # Find the largest `\n}` at column 0 before idx.
    bound = 0
    search_from = 0
    while True:
        nl = text.find("\n}", search_from, idx)
        if nl < 0:
            break
        # The `}` is at position nl + 1; column 0 means the byte after the
        # `}` is either newline / EOF — i.e. the line is exactly `}`.
        end_of_brace = nl + 2
        if end_of_brace == idx or end_of_brace >= len(text) or text[end_of_brace] == "\n":
            bound = end_of_brace
        search_from = nl + 1

    last_open = -1
    last_close = -1
    for _name, paren_open, paren_close in iter_fn_heads(text, bound, idx):
        last_open = paren_open
        last_close = paren_close
    if last_open < 0:
        return False
    return bool(CODE_PARAM_RE.search(text[last_open : last_close + 1]))


def first_arg(args: str) -> str:
    """Return the text of the first top-level positional argument."""
    out, depth = "", 0
    for ch in args:
        if ch in "([{<":
            depth += 1
        elif ch in ")]}>":
            depth -= 1
        elif ch == "," and depth == 0:
            break
        out += ch
    return out.strip()


def discover_helper_methods(text: str) -> tuple[set[str], set[str]]:
    """Scan every fn declared in `text` and classify each bare method
    name by whether it takes a `code: ErrorCode` parameter.

    Returns a pair ``(taking_code, not_taking_code)``:

    * ``taking_code`` — bare method names whose signature contains
      ``code: ErrorCode`` anywhere in the param list. These are
      candidate helpers for the caller pass.
    * ``not_taking_code`` — bare method names whose signature does
      *not* contain a ``code: ErrorCode`` parameter. Call sites of
      these are untouchable by the lint because they are not
      error-code helpers at all.

    The caller pass must only flag a call site when the method name
    is *unambiguously* an error-code helper. If a name appears in
    both sets — e.g. `Checker::error(code: ErrorCode, ...)` and an
    unrelated `Logger::error(&self, msg: &str)` in a different file —
    the lint drops the name from the global helper set because it
    cannot tell from purely textual analysis which overload a given
    `.error(...)` call site targets. This is a conservative
    false-*negative* trade-off: we'd rather miss a check than flag
    unrelated code. The construction pass (`check_constructions`) still
    catches any bare `code` field inside the real helper's body via
    `fn_takes_code`, so most mistakes are still caught.

    Called once per file by main(); results are unioned across files
    so a helper declared in `turbo-sema/src/lib.rs` still catches call
    sites in sibling files like `type_check.rs`.
    """
    taking: set[str] = set()
    not_taking: set[str] = set()
    for name, paren_open, paren_close in iter_fn_heads(text):
        sig = text[paren_open : paren_close + 1]
        if CODE_PARAM_RE.search(sig):
            taking.add(name)
        else:
            not_taking.add(name)
    return taking, not_taking


def check_constructions(text: str, file: Path) -> list[str]:
    bad: list[str] = []
    for m in CTOR_RE.finditer(text):
        ty, brace_open = m.group(1), m.end() - 1
        line_start = text.rfind("\n", 0, m.start()) + 1
        line_end = text.find("\n", m.start())
        line = text[line_start : (line_end if line_end >= 0 else len(text))]
        if DEF_LINE_RE.search(line):
            continue  # `pub struct ParseError {` definition
        close = matching(text, brace_open, "{", "}")
        if close < 0:
            continue
        # Append a `}` sentinel so a single-field literal without a
        # trailing comma — `ParseError { code: ErrorCode::E0001 }` —
        # still gives CODE_FIELD_RE a `}` terminator. The body slice
        # itself doesn't include the closing brace.
        body = text[brace_open + 1 : close] + "}"
        loc = f"{file}:{line_of(text, m.start())}"
        cm = CODE_FIELD_RE.search(body)
        if not cm:
            bad.append(f"FAIL {loc} — {ty} construction missing `code:` field")
            continue
        val = cm.group(1).strip() if cm.group(1) is not None else "code"
        fb = FALLBACK_RE.search(val)
        if fb:
            bad.append(f"FAIL {loc} — {ty} uses fallback ErrorCode::{fb.group(1)}")
            continue
        if REAL_CODE_RE.search(val):
            continue
        if val.rstrip(",").strip() == "code" and fn_takes_code(text, m.start()):
            continue  # passthrough helper like Checker::error
        bad.append(
            f"FAIL {loc} — {ty} `code:` field is not an ErrorCode::E0NNN literal: `{val}`"
        )
    return bad


def check_new_constructors(text: str, file: Path) -> list[str]:
    """Defense-in-depth check for `Type::new(ErrorCode::..., ...)` style
    constructors. The current tree contains zero such call sites
    (verified at audit time), but if anyone adds a `ParseError::new`
    helper later we want it to be held to the same standard as a struct
    literal: first positional argument must be an ErrorCode::E0NNN
    literal, no fallback variants, no bare identifiers.

    NOTE: This check only inspects the FIRST positional argument. That
    enforces the project convention that `ErrorCode` must be the first
    parameter of any `ParseError|SemaError|CodegenError::new(...)`
    constructor. A constructor like
    `SemaError::new(span, ErrorCode::E0100, msg)` — with the code in
    position 1 — will be rejected as if the ErrorCode were missing
    entirely, because `first_arg()` returns `span` and that's not an
    ErrorCode literal. See the module docstring for the rationale.
    """
    bad: list[str] = []
    for m in NEW_CTOR_RE.finditer(text):
        ty, paren_open = m.group(1), m.end() - 1
        close = matching(text, paren_open, "(", ")")
        if close < 0:
            continue
        arg = first_arg(text[paren_open + 1 : close])
        loc = f"{file}:{line_of(text, m.start())}"
        fb = FALLBACK_RE.search(arg)
        if fb:
            bad.append(
                f"FAIL {loc} — {ty}::new() uses fallback ErrorCode::{fb.group(1)}"
            )
            continue
        if not REAL_CODE_RE.search(arg):
            bad.append(
                f"FAIL {loc} — {ty}::new() first argument is not an "
                f"ErrorCode::E0NNN literal: `{arg}`"
            )
    return bad


def check_helper_calls(text: str, file: Path, helpers: set[str]) -> list[str]:
    """For every `<recv>.helper(...)` or `self.helper(...)` call where
    `helper` is in the global helper set, require the first positional
    argument to be an ErrorCode::E0NNN literal. The helper set is built
    by sweeping every linted file once up front so a call site in
    `type_check.rs` is still checked against an `error()` helper
    declared in the sibling `lib.rs`.
    """
    bad: list[str] = []
    if not helpers:
        return bad
    pattern = re.compile(
        r"\.(" + "|".join(re.escape(h) for h in helpers) + r")\s*\("
    )
    for m in pattern.finditer(text):
        method, paren_open = m.group(1), m.end() - 1
        # Skip method *definitions* (they're matched as `\bfn name(`,
        # not `.name(`). The leading `.` in our regex already excludes
        # those, but a method *call* on the `helpers` set could happen
        # to be on a non-self receiver — that's fine, we still check it.
        close = matching(text, paren_open, "(", ")")
        if close < 0:
            continue
        arg = first_arg(text[paren_open + 1 : close])
        loc = f"{file}:{line_of(text, m.start())}"
        fb = FALLBACK_RE.search(arg)
        if fb:
            bad.append(
                f"FAIL {loc} — .{method}() uses fallback ErrorCode::{fb.group(1)}"
            )
        elif not REAL_CODE_RE.search(arg):
            bad.append(
                f"FAIL {loc} — .{method}() first argument is not an "
                f"ErrorCode::E0NNN literal: `{arg}`"
            )
    return bad


def main() -> int:
    for d in CRATE_DIRS:
        if not d.is_dir():
            print(f"error: expected crate dir not found: {d}", file=sys.stderr)
            return 2

    # Build the file list once and read each file once. The cleaned
    # text is reused for both helper discovery and the per-file checks
    # so we don't pay the strip_comments_and_strings cost twice.
    files: list[tuple[Path, str]] = []
    for crate in CRATE_DIRS:
        for rs in sorted((crate / "src").rglob("*.rs")):
            files.append((rs, strip_comments_and_strings(rs.read_text(encoding="utf-8"))))

    # First pass: union helper method names across every linted file.
    # Anything with a `code: ErrorCode` parameter qualifies, so adding
    # a new diagnostic helper extends the lint automatically. We also
    # track the set of method names that appear with a signature that
    # does NOT take `code: ErrorCode` and subtract those — see
    # `discover_helper_methods` for the rationale (name-collision
    # false-positive avoidance).
    taking_code: set[str] = set()
    not_taking_code: set[str] = set()
    for _, text in files:
        taking, not_taking = discover_helper_methods(text)
        taking_code |= taking
        not_taking_code |= not_taking
    helpers = taking_code - not_taking_code

    violations: list[str] = []
    for rs, text in files:
        violations.extend(check_constructions(text, rs))
        violations.extend(check_new_constructors(text, rs))
        violations.extend(check_helper_calls(text, rs, helpers))

    if violations:
        for v in violations:
            print(v, file=sys.stderr)
        print(
            f"\ncheck_error_codes.py: {len(violations)} violation(s) found.",
            file=sys.stderr,
        )
        print(
            "Every diagnostic must carry a unique ErrorCode::E0NNN — "
            "see CONTRIBUTING.md → 'Error Codes and Documentation'.",
            file=sys.stderr,
        )
        return 1
    print("check_error_codes.py: all error constructions carry a concrete ErrorCode.")
    return 0


def _run_checks(text: str, file: Path = Path("<test>")) -> tuple[list[str], set[str]]:
    """Helper shared by the self-tests: runs the full per-file pipeline
    (discovery + construction + caller) against a single in-memory
    source buffer and returns ``(violations, final_helpers)``.
    """
    clean = strip_comments_and_strings(text)
    taking, not_taking = discover_helper_methods(clean)
    helpers = taking - not_taking
    violations: list[str] = []
    violations.extend(check_constructions(clean, file))
    violations.extend(check_new_constructors(clean, file))
    violations.extend(check_helper_calls(clean, file, helpers))
    return violations, helpers


def _run_checks_multi(sources: list[str]) -> list[str]:
    """Multi-file variant: simulates main()'s cross-file helper pass
    over several in-memory source buffers. Returns the flat list of
    violations."""
    cleaned = [
        (Path(f"<test-{i}>"), strip_comments_and_strings(s))
        for i, s in enumerate(sources)
    ]
    taking_code: set[str] = set()
    not_taking_code: set[str] = set()
    for _, clean in cleaned:
        t, nt = discover_helper_methods(clean)
        taking_code |= t
        not_taking_code |= nt
    helpers = taking_code - not_taking_code
    violations: list[str] = []
    for file, clean in cleaned:
        violations.extend(check_constructions(clean, file))
        violations.extend(check_new_constructors(clean, file))
        violations.extend(check_helper_calls(clean, file, helpers))
    return violations


def self_test() -> int:
    """Run the built-in regression tests. Exits non-zero on any failure.

    Run with:
        python3 scripts/check_error_codes.py --self-test

    These are not pytest-based on purpose: the lint must run with just
    a system Python 3, no third-party deps, in CI. Plain asserts and a
    small counter are enough.
    """
    failures = 0

    def check(label: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond:
            print(f"  ok    {label}")
        else:
            failures += 1
            print(f"  FAIL  {label}")
            if detail:
                print(f"        {detail}")

    # ------------------------------------------------------------------
    # 1. Nested generics in fn signature
    # ------------------------------------------------------------------
    # Regression: the old FN_HEAD_RE `<[^>]*>` broke on `<T: Trait<U>>`
    # and `iter_fn_heads` now uses a balanced-angle-brackets walker.
    nested_src = """
use std::fmt::Display;
pub struct Checker;
impl Checker {
    pub fn error_nested<T: Display<U>, U>(&mut self, code: ErrorCode, ctx: T, _u: U) {
        self.errors.push(SemaError { code, message: format!("{}", ctx), span: Span::default() });
    }
}
pub fn use_it(c: &mut Checker) {
    c.error_nested(ErrorCode::E0100, "ok", 1);
    c.error_nested(oops_var, "bad", 2);
}
"""
    violations, helpers = _run_checks(nested_src)
    check(
        "nested generics: helper is discovered",
        "error_nested" in helpers,
        f"helpers={sorted(helpers)}",
    )
    check(
        "nested generics: fn_takes_code allowlists passthrough `code` field",
        not any("SemaError `code:`" in v for v in violations),
        f"violations={violations}",
    )
    check(
        "nested generics: bad call site is flagged",
        any(".error_nested()" in v and "oops_var" in v for v in violations),
        f"violations={violations}",
    )
    check(
        "nested generics: good call site is NOT flagged",
        not any(".error_nested()" in v and "E0100" in v for v in violations),
        f"violations={violations}",
    )

    # ------------------------------------------------------------------
    # 2. Helper name collision (single file)
    # ------------------------------------------------------------------
    # Regression: `Logger::error(&str)` next to `Checker::error(ErrorCode,...)`
    # used to false-positive on any `logger.error(...)` call. The fix
    # subtracts name-collided helpers from the global set.
    collision_single = """
use crate::foo::ErrorCode;
pub struct Checker;
impl Checker {
    pub fn error(&mut self, code: ErrorCode, msg: String) {
        self.errors.push(SemaError { code, message: msg, span: Span::default() });
    }
}
pub struct Logger;
impl Logger {
    pub fn error(&self, msg: &str) { println!("{}", msg); }
}
pub fn do_stuff(logger: &Logger) {
    logger.error("something went wrong");
}
"""
    violations, helpers = _run_checks(collision_single)
    check(
        "collision (single file): `error` dropped from helpers",
        "error" not in helpers,
        f"helpers={sorted(helpers)}",
    )
    check(
        "collision (single file): no false positive on Logger::error call",
        not any(".error()" in v for v in violations),
        f"violations={violations}",
    )

    # ------------------------------------------------------------------
    # 3. Helper name collision (cross-file)
    # ------------------------------------------------------------------
    # Same regression as above but the overloads live in different
    # files — main()'s cross-file merge must still drop the name.
    violations = _run_checks_multi(
        [
            """
use crate::foo::ErrorCode;
pub struct Checker;
impl Checker {
    pub fn error(&mut self, code: ErrorCode, msg: String) {
        self.errors.push(SemaError { code, message: msg, span: Span::default() });
    }
}
""",
            """
pub struct Logger;
impl Logger {
    pub fn error(&self, msg: &str) { println!("{}", msg); }
}
pub fn do_stuff(logger: &Logger) { logger.error("boom"); }
""",
        ]
    )
    check(
        "collision (cross-file): no false positive on Logger::error call",
        not any(".error()" in v for v in violations),
        f"violations={violations}",
    )

    # ------------------------------------------------------------------
    # 4. Isolated helper (no collision) still gets checked
    # ------------------------------------------------------------------
    clean_helper = """
use crate::foo::ErrorCode;
pub struct Checker;
impl Checker {
    pub fn error(&mut self, code: ErrorCode, msg: String) {
        self.errors.push(SemaError { code, message: msg, span: Span::default() });
    }
}
pub fn caller(c: &mut Checker) {
    c.error(some_runtime_code, "bad".to_string());
    c.error(ErrorCode::E0100, "good".to_string());
    c.error(ErrorCode::Unknown, "fallback bad".to_string());
}
"""
    violations, helpers = _run_checks(clean_helper)
    check(
        "isolated helper: `error` is in helper set",
        "error" in helpers,
        f"helpers={sorted(helpers)}",
    )
    check(
        "isolated helper: runtime arg is flagged",
        any("some_runtime_code" in v for v in violations),
        f"violations={violations}",
    )
    check(
        "isolated helper: fallback variant is flagged",
        any("fallback ErrorCode::Unknown" in v for v in violations),
        f"violations={violations}",
    )
    check(
        "isolated helper: literal ErrorCode::E0100 is NOT flagged",
        not any("E0100" in v for v in violations),
        f"violations={violations}",
    )

    # ------------------------------------------------------------------
    # 5. `::new` first-arg convention
    # ------------------------------------------------------------------
    # Wrong-order: span first, then ErrorCode. Must be flagged.
    new_wrong_order = """
pub fn wrong_order() {
    SemaError::new(span.clone(), ErrorCode::E0100, "msg".to_string());
}
"""
    violations, _ = _run_checks(new_wrong_order)
    check(
        "::new arg-order convention: wrong order is flagged",
        any("SemaError::new()" in v for v in violations),
        f"violations={violations}",
    )
    # Right-order: ErrorCode first. Must NOT be flagged.
    new_right_order = """
pub fn right_order() {
    SemaError::new(ErrorCode::E0100, span.clone(), "msg".to_string());
}
"""
    violations, _ = _run_checks(new_right_order)
    check(
        "::new arg-order convention: right order is NOT flagged",
        not violations,
        f"violations={violations}",
    )

    # ------------------------------------------------------------------
    # 6. Flat generics still work (no regression)
    # ------------------------------------------------------------------
    flat_src = """
pub struct Checker;
impl Checker {
    pub fn error<T>(&mut self, code: ErrorCode, ctx: T) {
        self.errors.push(SemaError { code, message: String::new(), span: Span::default() });
    }
}
"""
    _, helpers = _run_checks(flat_src)
    check(
        "flat generics: helper still discovered",
        "error" in helpers,
        f"helpers={sorted(helpers)}",
    )

    # ------------------------------------------------------------------
    # 7. Non-crashing behavior on malformed signatures
    # ------------------------------------------------------------------
    # Unterminated generic — should NOT crash the lint.
    malformed = """
pub fn broken<T: Trait<U, code: ErrorCode) {
    SemaError { code, message: String::new(), span: Span::default() }
}
"""
    try:
        _run_checks(malformed)
        crashed = False
    except Exception as e:
        crashed = True
        crash_detail = f"{type(e).__name__}: {e}"
    check(
        "malformed signature: lint does not crash",
        not crashed,
        crash_detail if crashed else "",
    )

    # ------------------------------------------------------------------
    # 8. Construction pass still catches Unknown / missing code
    # ------------------------------------------------------------------
    bad_ctor = """
pub fn bad() {
    let e = SemaError { code: ErrorCode::Unknown, message: String::new(), span: Span::default() };
    let f = SemaError { message: String::new(), span: Span::default() };
}
"""
    violations, _ = _run_checks(bad_ctor)
    check(
        "construction: Unknown fallback is flagged",
        any("fallback ErrorCode::Unknown" in v for v in violations),
        f"violations={violations}",
    )
    check(
        "construction: missing code field is flagged",
        any("missing `code:` field" in v for v in violations),
        f"violations={violations}",
    )

    print()
    if failures:
        print(f"self-test: {failures} failure(s)")
        return 1
    print("self-test: all checks passed")
    return 0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        sys.exit(self_test())
    sys.exit(main())
