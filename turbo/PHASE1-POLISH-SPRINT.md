# Phase 1 Polish Sprint: Triage Conversation

**Date:** 2026-02-22
**Participants:** Alex (CTO), Sam (CEO)
**Context:** Review of 20 issues found in the Phase 1 Turbo compiler. Decide DO (implement now) or SKIP (defer).

---

## The Conversation

**Sam:** Alright, Alex, we've got twenty items on this list. I want to ship something people can actually try this week. Let's be ruthless. What's critical, what's not, and let's not boil the ocean.

**Alex:** Agreed on the timeline, but I want to be clear up front: if we ship something that segfaults or silently produces wrong answers, we'll lose credibility before we even start. A compiler that lies to you is worse than no compiler at all. Let me walk through the bugs first.

---

### BUGS

**Alex:** Number one. Integer division by zero crashes the entire process. Not a nice error message -- an actual SIGFPE that kills everything. I looked at the codegen. We're emitting a raw `sdiv` instruction through Cranelift with zero guard. This is a one-line fix -- check for zero before the divide, emit a trap or a runtime error.

**Sam:** Yeah, that's a DO. Process crash is unacceptable. Even in an alpha. How long?

**Alex:** Thirty minutes, maybe an hour. We check if the divisor is zero, branch to a trap. Done.

**Sam:** DO. Next.

---

**Alex:** Number two. `let` versus `let mut` -- the parser reads the `mutable` flag perfectly, I can see it in the AST. But the codegen in `compile_stmt` for `Stmt::Let` destructures with `..`, completely ignoring the `mutable` field. And `Expr::Assign` and `Expr::CompoundAssign` never check whether the target was declared mutable. You can reassign immutable variables all day long.

**Sam:** Does the parser already track it?

**Alex:** Yes. The `Stmt::Let` struct has `mutable: bool`. It's literally right there. The codegen just needs to store that flag in the `vars` HashMap and check it on assignment.

**Sam:** Sounds like a couple hours. And this is a core language promise -- immutability by default is on the landing page. DO.

**Alex:** Absolutely DO. If someone writes `let x = 5` and then `x = 10` and we let it through silently, that's a broken contract.

---

**Alex:** Number three. Logical AND and OR don't short-circuit. Look at `compile_binop` -- `BinOp::And` emits `band`, `BinOp::Or` emits `bor`. Both sides are always evaluated. That means `if x != 0 && 100 / x > 5` will divide by zero on the right side even when x is zero.

**Sam:** Hmm. That's actually a correctness issue when combined with bug number one.

**Alex:** Exactly. It's not just a nice-to-have semantic. It makes conditional guards unreliable. The fix is to move AND/OR out of `compile_binop` and into `compile_expr`, using branching -- similar to how we compile `if`. Evaluate left side, conditionally skip right side.

**Sam:** How big is that?

**Alex:** Medium. Maybe two to three hours. We need to create intermediate blocks, branch on the left operand, and phi the result. But we already have that pattern in `compile_if`.

**Sam:** This is a DO. It directly interacts with the division by zero bug. If both are present, users literally cannot write safe guard clauses.

---

**Alex:** Number four. Modulo operator doesn't work for floats. We emit `srem` which is integer-only. Cranelift doesn't have a native float remainder instruction, so we'd need to call out to `fmod` or implement it ourselves.

**Sam:** Is anyone doing float modulo in Phase 1?

**Alex:** Unlikely. Our test files don't use it. Float modulo is a niche operation.

**Sam:** SKIP. We can add it in Phase 2 with a proper math stdlib. If someone hits it, they'll get a codegen error, not silent corruption, right?

**Alex:** Currently it would try to use `srem` on float values and Cranelift would reject it at compile time with an error. So yes, it fails loudly.

**Sam:** Even better. SKIP.

---

**Alex:** Number five. String arithmetic. If someone writes `"hello" + "world"`, we don't have string concatenation. What actually happens is both operands are pointers, and `iadd` does pointer math. So you get some garbage address interpreted as a number, or worse, a valid address pointing at random memory.

**Sam:** That's... terrifying.

**Alex:** It is. Imagine someone trying `"hello" + " world"` because they come from JavaScript or Python. They expect concatenation, they get a segfault or garbage. This is tied to issue six -- no type checking -- but even without a full type checker, we should at minimum reject arithmetic on pointer-typed values in codegen.

**Sam:** So what's the fix?

**Alex:** Two options. The fast fix: in `compile_binop`, check if either operand is `ptr_type` and emit a codegen error. The real fix: type checking pass, which is issue seven. The fast fix is thirty minutes.

**Sam:** Do the fast fix now. The type checker is a bigger piece. DO -- but the quick guard, not string concatenation.

**Alex:** Agreed. We emit a clear error: "cannot perform arithmetic on strings". DO.

---

**Alex:** Number six. The big one. No type checking at all. You can pass a string to a function expecting an integer. You can add a boolean to a float. You can return the wrong type from a function. Everything just... proceeds. Cranelift does whatever the bits tell it to do.

**Sam:** Give me the honest assessment. How bad is this in practice right now?

**Alex:** In practice, most of our test programs happen to be well-typed because we wrote them carefully. But the moment a real user makes a type error, they get garbage output with no explanation. That's the number one "this language is broken" experience.

**Sam:** And the fix is issue seven -- the semantic analysis pass?

**Alex:** Yes. Issues six and seven are really the same thing. You can't "fix" the lack of type checking without building the type checker.

**Sam:** Let's talk about it as part of issue seven then. But the verdict on issue six as a standalone: it's not a separate fix item, it's the symptom. The cure is issue seven.

**Alex:** Correct. I'll mark six as DO but note it's resolved by implementing seven.

---

### MISSING

**Alex:** Issue seven. Semantic analysis / type checking pass. This is the elephant in the room. Right now the pipeline is: lex -> parse -> codegen. We need: lex -> parse -> **check** -> codegen. The checker would verify types match in binary operations, function arguments match parameter types, return types match declarations, and variables are defined before use.

**Sam:** How big is this?

**Alex:** For Phase 1's limited type system -- i64, f64, bool, str, and unit -- it's manageable. We don't have generics, traits, or user-defined types yet. I'd estimate two to three days for a solid implementation. We walk the AST, infer types for expressions, check them against declarations.

**Sam:** Two to three days is significant. But without it...

**Alex:** Without it, every other fix we do is a band-aid. The division by zero guard? Pointless if someone passes a string where an integer is expected and we happily divide two pointers. The immutability enforcement? Useful, but still incomplete if types are unchecked. The type checker is the foundation.

**Sam:** I hate to say it, but you're right. This is the one thing that separates "toy" from "real." DO. And it's priority one.

**Alex:** Agreed. I'd argue we build a new crate -- `turbo-sema` or `turbo-checker` -- that takes the AST, annotates it with types, and returns errors. Then codegen can trust that everything is well-typed.

**Sam:** DO, top priority.

---

**Alex:** Issue eight. Immutability enforcement. We already said DO for issue two -- that's the codegen side. This is the semantic analysis side. The checker should reject `x = 5` when `x` was declared with `let` (not `let mut`). Since we're building the type checker anyway, we fold this in.

**Sam:** Is this extra work on top of the type checker?

**Alex:** Minimal. When we walk `Assign` and `CompoundAssign` nodes, we check the variable's mutability flag. Fifteen minutes of extra code in the checker. But we should also keep the codegen-level guard from issue two as a defense-in-depth measure.

**Sam:** DO, bundled with the type checker.

---

**Alex:** Issue nine. Variable scope tracking. Right now, variables declared inside a nested block are visible outside it. If you do `if true { let x = 5 }` and then `print(x)`, it works -- x leaked out of the if block. That's wrong. Blocks should introduce a new scope.

**Sam:** Is this hard to fix?

**Alex:** In the type checker, we use a scope stack -- push a new scope when entering a block, pop when leaving. In codegen, we already use a flat `HashMap<String, Variable>`, so we'd need to save/restore the map around blocks. Or switch to a scope chain.

**Sam:** Is this something users will actually hit?

**Alex:** Absolutely. The moment someone accidentally uses a variable from an inner scope, they'll get either a wrong answer or a confusing error. And when we do add loops, loop variables leaking out would be a constant source of bugs.

**Sam:** DO. This is basic scoping. Every language has it.

---

**Alex:** Issue ten. Function redefinition not detected. If you define `fn foo()` twice, the second one silently overwrites the first in the `user_fns` HashMap. No error.

**Sam:** How often will this happen in Phase 1? People are writing small single-file programs.

**Alex:** It's an easy check -- when inserting into the functions map, check if the key already exists. Five lines of code. And it prevents a genuinely confusing experience where your function "isn't working" because you accidentally defined another one with the same name lower in the file.

**Sam:** Five lines? DO. That's a no-brainer.

---

### NEEDS (Code Quality)

**Alex:** Issue eleven. Better error messages with ariadne. Right now our errors look like `error: expected expression, found end of file at test.tb:3:1`. With ariadne, we'd get pretty-printed errors with source context, underlines, colors. The crate is already in our workspace dependencies -- line 21 of `Cargo.toml`. We're paying for it in compile time but not using it.

**Sam:** How much work?

**Alex:** To wire up ariadne for lex errors and parse errors, maybe half a day. For type checking errors too, another few hours. But it's polish, not correctness.

**Sam:** SKIP for this sprint. Our errors are functional -- they have file, line, column, and a message. Pretty errors are a Phase 2 polish item. We should either use ariadne or remove it from dependencies.

**Alex:** Fair. I'd rather spend that half day on the type checker.

**Sam:** SKIP.

---

**Alex:** Issue twelve. Remove chumsky from workspace dependencies. We switched to a hand-written recursive descent parser but never cleaned up `Cargo.toml`. Chumsky is still listed on line 13. It's dead weight -- extra compile time for nothing.

**Sam:** That's a one-line delete, right?

**Alex:** One line. And it'll speed up clean builds by a few seconds since chumsky pulls in a dependency tree.

**Sam:** DO. Takes ten seconds. No reason not to.

---

**Alex:** Issue thirteen. Short-circuit evaluation for `&&` and `||`. This is the same as issue three. We already said DO.

**Sam:** Duplicate. Already DO. Let's merge it with issue three.

---

### DEVX

**Alex:** Issue fourteen. No `turbo build` command. Right now we only have `turbo run`, which JIT-compiles and immediately executes. There's no way to compile to a binary.

**Sam:** The JIT approach is actually great for the developer experience right now. `turbo run hello.tb` -- done. `turbo build` requires us to figure out object file emission, linking, output paths, all of that. That's a significant chunk of work.

**Alex:** Agreed. Cranelift can emit object files, but wiring that up, calling a system linker, handling platform differences... that's a week of work minimum.

**Sam:** Hard SKIP. Phase 2 or Phase 3. The JIT workflow is fine for now. Honestly, most modern language toolchains lead with `run` anyway -- Deno, Go, etc.

---

**Alex:** Issue fifteen. No `--verbose` or `--debug` flag. Would be useful for us during development -- dump tokens, dump AST, show timing.

**Sam:** How much work?

**Alex:** Trivial. Add a `--verbose` flag to clap, conditionally print the token stream and AST. Maybe an hour.

**Sam:** Actually... this helps *us* debug faster for everything else on this list. If we're building a type checker and debugging codegen, being able to `turbo run --verbose test.tb` and see the AST would save us time.

**Alex:** Good point. It's a force multiplier.

**Sam:** DO. But keep it simple -- just `--verbose` that dumps tokens and AST. No fancy debug infrastructure.

---

**Alex:** Issue sixteen. REPL mode. Interactive Turbo shell.

**Sam:** *laughs* No. SKIP. That's a Phase 3 feature. We don't even have loops yet.

**Alex:** Yeah, a REPL without variables persisting across lines, without loops, without any interactive features... it'd be a bad first impression.

**Sam:** SKIP. Hard skip.

---

### WISH LIST

**Alex:** Issue seventeen. String interpolation. `"Hello, {name}"` instead of string concatenation. This is actually in our design spec.

**Sam:** It's in the spec, but it requires the lexer to parse interpolation segments, the parser to handle embedded expressions, and codegen to allocate and concatenate strings at runtime. That's a multi-day feature.

**Alex:** And we don't even have string concatenation yet. We'd need a runtime string type with allocation.

**Sam:** SKIP. Phase 2. This is a big feature that deserves proper attention, not a rush job.

---

**Alex:** Issue eighteen. While and for loops. The lexer already has `While`, `For`, and `In` tokens. The parser and codegen don't handle them.

**Sam:** How fundamental are loops?

**Alex:** Pretty fundamental. Right now the only way to loop is recursion, which... works, but it's not what anyone expects. Our `recursion.tb` test proves recursion works, but you can't write a simple counter loop.

**Sam:** Counter-argument: we're trying to ship a polished Phase 1, not expand scope. Loops touch the parser and codegen. How long?

**Alex:** `while` is straightforward -- maybe three to four hours. It's just a conditional jump back to the top of a block. `for` with ranges is harder because we'd need range expressions and iterators.

**Sam:** What if we do just `while` and skip `for`?

**Alex:** `while` alone would be a huge usability win. The pattern `let mut i = 0; while i < 10 { ... i += 1 }` covers most use cases.

**Sam:** Hmm. I'm torn. It's scope creep, but loops are so basic...

**Alex:** Here's my argument: without loops, every demo program that needs iteration looks weird. Fibonacci via recursion is fine for a showcase, but "count from 1 to 10" should not require a recursive function.

**Sam:** Fine. DO for `while` only. SKIP `for` until Phase 2. And only after the type checker is done -- I don't want `while` without type checking.

**Alex:** Deal. `while` loops, type checker first.

---

**Alex:** Issue nineteen. `assert` and `panic` built-ins. `assert(condition)` that aborts with an error, `panic("message")` that halts execution.

**Sam:** How would we implement them?

**Alex:** Same pattern as `print` -- built-in functions handled specially in codegen. `panic` calls a runtime function that prints and exits. `assert` checks a condition and calls panic if false. Maybe two hours.

**Sam:** These would be really useful for testing. `assert(add(2, 3) == 5)` is a lot better than eyeballing print output.

**Alex:** Exactly. And it makes our test files self-verifying instead of "look at the output and hope."

**Sam:** DO. This is high leverage for low effort. We can write proper test files that actually fail when things are wrong.

---

**Alex:** Issue twenty. Multiple print arguments. `print("x =", x)` instead of separate print calls.

**Sam:** How hard?

**Alex:** The codegen's `compile_print` currently only handles `args[0]`. We'd need to iterate, print each with a space separator, and print a newline at the end instead of after each argument. Maybe an hour.

**Sam:** SKIP. It's nice but not critical. You can call print multiple times. Let's not touch the print infrastructure when we have bigger fish. Phase 2.

**Alex:** Fair. SKIP.

---

## Final Tally

**Sam:** Let me read back the list.

**Alex:** Go ahead.

**Sam:** Here's what we've got:

| # | Issue | Decision | Why |
|---|-------|----------|-----|
| 1 | Division by zero crashes process | **DO** | Process crash is unacceptable even in alpha. One-hour fix with a zero-check guard before `sdiv`. |
| 2 | Mutable bindings not enforced | **DO** | Core language promise (immutability by default) is broken. Parser already tracks `mutable` flag; codegen just ignores it. |
| 3 | Logical AND/OR don't short-circuit | **DO** | Correctness issue -- guard clauses like `x != 0 && 100/x > 5` will crash. Interacts directly with bug #1. |
| 4 | Modulo unsupported for floats | **SKIP** | Niche operation, fails loudly at compile time (Cranelift rejects it), not silent corruption. Phase 2 with math stdlib. |
| 5 | String arithmetic does pointer math | **DO** | Silent garbage/segfault when users try `"a" + "b"`. Quick guard: reject arithmetic on pointer-typed values in codegen. |
| 6 | No type checking (garbage on wrong types) | **DO** | Resolved by implementing #7. Not a separate work item -- this is the symptom, #7 is the cure. |
| 7 | Semantic analysis / type checking pass | **DO** | Foundation of correctness. Without it, every other fix is a band-aid. Covers type mismatches, return types, argument types. Top priority. |
| 8 | Immutability enforcement (sema side) | **DO** | Bundled with type checker (#7). Fifteen minutes of extra code -- check mutability flag on assignment nodes. |
| 9 | Variable scope tracking | **DO** | Nested block variables leak out. Basic scoping is expected by every programmer. Needed before loops (#18). |
| 10 | Function redefinition not detected | **DO** | Five-line check when inserting into function map. Prevents genuinely confusing silent overwrites. |
| 11 | Better error messages (ariadne) | **SKIP** | Polish, not correctness. Current errors are functional (file:line:col + message). Time better spent on type checker. |
| 12 | Remove chumsky from workspace deps | **DO** | One-line delete. Dead dependency adding compile time. No reason not to. |
| 13 | Short-circuit evaluation | **DO** | Duplicate of #3. Already decided DO. |
| 14 | `turbo build` command | **SKIP** | Requires object file emission, linking, platform handling. JIT via `turbo run` is the right UX for now. Phase 2/3. |
| 15 | `--verbose` / `--debug` flag | **DO** | Force multiplier for debugging everything else on this list. Trivial to add (clap flag + conditional AST dump). |
| 16 | REPL mode | **SKIP** | No loops, no persistent state, no interactive features. Would be a bad first impression. Phase 3. |
| 17 | String interpolation | **SKIP** | Multi-day feature requiring lexer, parser, and runtime string allocation work. Deserves proper attention in Phase 2. |
| 18 | While loops | **DO** (`while` only) | Loops are too fundamental to skip. `while` is straightforward (conditional back-jump). `for` deferred to Phase 2. |
| 19 | Assert/panic built-ins | **DO** | High leverage, low effort. Makes test files self-verifying. Same pattern as `print` -- built-in function in codegen. |
| 20 | Multiple print arguments | **SKIP** | Nice convenience but not critical. Call print multiple times. Phase 2. |

---

## DO NOW: Implementation Priority Order

**Alex:** Here's my proposed order, and the reasoning:

**Sam:** Let's hear it.

**Alex:** We build bottom-up. Foundation first, then the things that depend on it.

### Priority 1: Foundations (Day 1)

1. **#12 -- Remove chumsky from workspace deps**
   Ten seconds. Clean up. Gets it off the table.

2. **#15 -- Add `--verbose` flag**
   One hour. We need this to debug everything that follows. Dump tokens, dump AST, show stage timings.

3. **#10 -- Detect function redefinition**
   Five-line guard. Quick win that prevents confusing behavior during all subsequent testing.

### Priority 2: Safety Guards (Day 1-2)

4. **#1 -- Division by zero guard**
   One hour. Emit a check before `sdiv`/`srem`. Branch to trap on zero. Process-crash-level bugs ship first.

5. **#5 -- String arithmetic guard**
   Thirty minutes. Check for pointer-type operands in `compile_binop`, emit codegen error. Prevents silent garbage.

6. **#2 -- Immutability enforcement (codegen side)**
   Two hours. Store mutability flag in vars HashMap. Check on `Assign`/`CompoundAssign`. Defense in depth before the type checker exists.

### Priority 3: The Type Checker (Day 2-4)

7. **#7 + #6 + #8 -- Semantic analysis pass**
   Two to three days. New `turbo-sema` crate. Walk the AST, infer expression types, check:
   - Binary ops have matching/compatible types
   - Function arguments match parameter types
   - Return values match declared return type
   - Variables defined before use
   - Assignments only to mutable bindings (#8)
   This is the big one. Everything else is better once this exists.

8. **#9 -- Variable scope tracking**
   Bundled with #7. Scope stack in the checker, scope-aware variable map in codegen. Half day.

### Priority 4: Correctness (Day 4-5)

9. **#3 + #13 -- Short-circuit evaluation for && / ||**
   Three hours. Move AND/OR out of `compile_binop` into `compile_expr`. Use branching pattern from `compile_if`. Depends on the type checker being done so we can trust operand types.

### Priority 5: Features (Day 5-6)

10. **#18 -- While loops** (while only, no for)
    Three to four hours. Parser: recognize `while condition { body }`. Codegen: loop header block, condition check, body block, back-edge jump. Depends on scope tracking (#9) being done.

11. **#19 -- Assert/panic built-ins**
    Two hours. Runtime functions `rt_panic(msg)` and `rt_assert(cond, msg)`. Wire into codegen like `print`. Last because it benefits from everything above being stable.

---

**Sam:** That's a clean six-day sprint. Thirteen items get done, seven get deferred. The deferred items are either pure polish (#11 ariadne, #20 multi-print), large features that deserve their own sprint (#14 build command, #16 REPL, #17 string interpolation), or edge cases that fail loudly (#4 float modulo).

**Alex:** And at the end of this sprint, the Turbo compiler will: type-check programs before running them, enforce immutability, handle scoping correctly, support while loops, have short-circuit logic, and never crash on division by zero. That's a real compiler.

**Sam:** Ship it. Let's go.

---

## Summary Statistics

- **Total issues reviewed:** 20
- **DO NOW:** 13 (items 1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13, 15, 18, 19)
- **SKIP (deferred):** 7 (items 4, 11, 14, 16, 17, 20)
- **Estimated sprint duration:** 6 working days
- **Unique work items:** 11 (after merging duplicates: 3+13, 6+7+8)
