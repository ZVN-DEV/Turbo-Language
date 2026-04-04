# COW / Silent Data Loss Bug Audit

Date: 2026-04-04
Scope: All collection-mutating builtins, string operations, and their interactions with control flow and scoping.

---

## Root Cause

The `push()` fix (auto-reassign in `compile_stmt` at lib.rs:3984-4001) only covers:
- The `push` builtin specifically (no other COW builtins)
- Simple `Ident(var_name)` as the first argument (not field access or index access)
- Top-level statement position in the SAME basic block (not across control flow boundaries like if/for/match)

---

## CONFIRMED BUGS -- P0 (Silent Data Loss)

### BUG-1: push() inside `if` block = ICE (compiler crash)
```turbo
let mut items = [1, 2, 3]
if true { items.push(4) }   // Cranelift assertion failure (ICE)
print(len(items))
```
When push is the ONLY statement in an if-block with no else, Cranelift panics at `remove_constant_phis.rs:301`.

### BUG-2: push() inside `if/else` = silent no-op
```turbo
let mut items = [1, 2, 3]
if x > 3 { items.push(4) } else { items.push(0) }
print(len(items))           // prints 3, not 4
```
The auto-reassign fires inside the branch block, but the SSA value is lost at the merge point.

### BUG-3: push() inside `for` loop = value lost after loop
```turbo
let mut items = [1, 2, 3]
for i in 0..3 { items.push(i + 10) }
print(len(items))           // prints 3, not 6
```
Inside the loop body, len(items) correctly shows 4, 5, 6. But after the loop exits, the variable reverts to its pre-loop value. The Cranelift phi at exit_block resolves to the entry value.

### BUG-4: push() inside `match` arm = silent no-op
```turbo
let mut items = [1, 2, 3]
match x { 5 => { items.push(4) }, _ => { items.push(0) } }
print(len(items))           // prints 3, not 4
```
Same root cause as if/else -- SSA value lost at merge point.

### BUG-5: push() on struct field = silent no-op
```turbo
let mut b = Bag { items: [1, 2, 3] }
b.items.push(4)             // desugars to push(b.items, 4)
print(len(b.items))         // prints 3, not 4
```
The auto-reassign only handles `Ident(var_name)`, not `FieldAccess`. The new array pointer is discarded.

### BUG-6: push() on nested array = silent no-op
```turbo
let mut matrix = [[1, 2], [3, 4]]
matrix[0].push(5)           // desugars to push(matrix[0], 5)
print(len(matrix[0]))       // prints 2, not 3
```
Same cause as BUG-5 -- auto-reassign doesn't handle `Index` expressions.

### BUG-7: push() on immutable variable = no error
```turbo
let items = [1, 2, 3]       // NOT mut
items.push(4)
print(len(items))           // prints 4 -- silently mutated immutable!
```
The auto-reassign in codegen doesn't check mutability. Sema also doesn't flag push() on immutable arrays.

### BUG-8: map() as statement = silent no-op
```turbo
let mut items = [1, 2, 3]
items.map(|x| { x * 2 })   // result discarded, items unchanged
print(items[0])             // prints 1, not 2
```
`map()` returns a new array. The auto-reassign only covers `push`, not `map`.

### BUG-9: filter() as statement = silent no-op
```turbo
let mut items = [1, 2, 3, 4, 5]
items.filter(|x| { x > 2 })  // result discarded
print(len(items))             // prints 5, not 3
```
Same as BUG-8. `filter()` returns a new array, not covered by auto-reassign.

### BUG-10: filter().map() chain as statement = silent no-op
```turbo
let mut items = [1, 2, 3, 4, 5]
items.filter(|x| { x > 2 }).map(|x| { x * 10 })
print(len(items))             // prints 5
print(items[0])               // prints 1
```
The entire chain result is discarded.

### BUG-11: str.replace() as statement = silent no-op
```turbo
let mut s = "hello world"
s.replace("world", "turbo")   // returns new string, result discarded
print(s)                       // prints "hello world"
```

### BUG-12: str.upper() as statement = silent no-op
```turbo
let mut s = "hello"
s.upper()                      // returns new string, result discarded
print(s)                       // prints "hello"
```

### BUG-13: str.lower() as statement = silent no-op
```turbo
let mut s = "HELLO"
s.lower()                      // prints "HELLO"
```

### BUG-14: str.trim() as statement = silent no-op
```turbo
let mut s = "  hello  "
s.trim()                       // prints "  hello  "
```

### BUG-15: str.repeat() as statement = silent no-op
```turbo
let mut s = "ha"
s.repeat(3)                    // prints "ha", not "hahaha"
```

### BUG-16: str chain as statement = silent no-op
```turbo
let mut s = "  Hello World  "
s.trim().lower()               // prints "  Hello World  "
```

### BUG-17: split() as statement = silent no-op
```turbo
let s = "a,b,c"
s.split(",")                   // returns array, result discarded
print(s)                       // prints "a,b,c"
```

---

## CONFIRMED BUGS -- P1 (Functional but misleading)

### BUG-18: reduce() as statement = no warning
```turbo
items.reduce(0, |acc, x| { acc + x })  // result silently discarded
```
Not a data-loss bug per se (reduce doesn't claim to mutate), but calling reduce without using the result is almost certainly a user error. No warning.

### BUG-19: No "unused return value" warning for pure functions
Sema never warns when a function's return value is discarded in statement position. This enables ALL the above silent no-op bugs.

### BUG-20: `return push(arr, val)` in functions = sema type error
```turbo
fn add_item(arr: [i64]) -> [i64] {
    return push(arr, 99)       // error: body returns ()
}
```
Sema says the function body returns `()` because `return` makes the Block evaluate to Unit, even though the return value matches. Workaround: use `push(arr, 99)` as tail expression (without `return` keyword).

---

## CONFIRMED WORKING

1. **push() at top level** -- `items.push(4)` works correctly in simple sequential code
2. **push() in while loop** -- auto-reassign properly persists across while iterations
3. **push() with let binding** -- `let bigger = items.push(4)` correctly captures new array
4. **push() with explicit reassign** -- `items = push(items, 4)` works everywhere (if, for, match)
5. **map()/filter() with let binding** -- `let doubled = items.map(...)` works
6. **map()/filter() with explicit reassign** -- `items = items.map(...)` works
7. **filter().map() chain with let binding** -- `let result = items.filter(...).map(...)` works
8. **str.replace/upper/lower/trim with let binding** -- `let s2 = s.upper()` works
9. **str.replace/upper/lower/trim with explicit reassign** -- `s = s.upper()` works
10. **Index assignment** -- `arr[i] = x` correctly handles COW and updates variable
11. **Index assignment in for loop** -- `arr[i] = arr[i] * 2` inside for works
12. **Compound index assignment** -- `arr[1] = arr[1] + 5` works
13. **COW isolation (index)** -- `b[0] = 99` doesn't affect original `a`
14. **COW isolation (push)** -- `b.push(4)` doesn't affect original `a`
15. **hashmap_set/remove** -- mutate in-place, work correctly everywhere
16. **hashmap method-style** -- `m.hashmap_set(k, v)` works (in-place mutation)
17. **Pass array to function** -- value semantics work correctly (caller unaffected)
18. **str.trim().lower() chain with assignment** -- works correctly
19. **Multiple push in sequence** -- `items.push(2); items.push(3)` works at top level
20. **push() as tail expression in function** -- `fn f(arr: [i64]) -> [i64] { push(arr, 99) }` works

---

## Recommended Fixes (Priority Order)

### Fix 1: Extend auto-reassign to ALL COW builtins (P0)
In `compile_stmt` (lib.rs:3989), change the check from `fn_name == "push"` to also cover `map`, `filter`, `sort`, `reverse` (when implemented), and any future COW array builtins.

### Fix 2: Fix auto-reassign across control flow (P0)
The current auto-reassign doesn't properly propagate through if/else/for/match. Options:
- **Option A**: Make the auto-reassign work at the Block level (not just Stmt level), by tracking which variables were modified and propagating SSA values at merge points.
- **Option B**: Instead of auto-reassign, make array builtins mutate in-place (change `rt_array_push` to return void and mutate the existing allocation). This eliminates COW entirely for single-ref arrays but requires careful refcount handling.
- **Option C**: Add a sema pass that rewrites `items.push(4)` to `items = push(items, 4)` before codegen. The explicit Assign expression already works correctly across all control flow.

### Fix 3: Extend auto-reassign to FieldAccess/Index targets (P0)
Handle `b.items.push(4)` by detecting the root variable and rewriting the field/index path after the push.

### Fix 4: Add mutability check for push/mutation builtins (P1)
Sema should error when push() is called on an immutable variable.

### Fix 5: Add "unused return value" lint for pure functions (P1)
Warn when map/filter/replace/upper/lower/trim/etc return values are discarded. This catches the entire class of bugs at compile time. Could be a `#[must_use]` attribute or a hardcoded list of pure builtins.

### Fix 6: Fix sema return-type check for blocks ending in `return` (P2)
A function body ending with `return expr` should not report "body returns ()" -- the return type should be checked from the return statement, not the block's own evaluated type.
