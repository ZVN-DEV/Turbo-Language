# Turbo Compiler Error Codes

Every compiler diagnostic includes a unique error code (e.g. `E0100`). Use `turbolang explain E0100` to view the description from the command line.

## Parse Errors (E0001-E0099)

| Code | Description |
|------|-------------|
| E0001 | Unexpected token during parsing |
| E0002 | Expected identifier |
| E0003 | Maximum nesting depth exceeded |
| E0004 | Invalid import syntax |
| E0005 | Invalid string interpolation syntax |
| E0006 | Unexpected end of file |
| E0007 | Invalid attribute |
| E0008 | Invalid expression |

## Type Errors (E0100-E0199)

| Code | Description |
|------|-------------|
| E0100 | Type mismatch |
| E0101 | Cannot perform arithmetic on non-numeric type |
| E0102 | Mismatched types in arithmetic operation |
| E0103 | Cannot compare incompatible types |
| E0104 | Expected `bool` in logical operation |
| E0105 | Cannot negate non-numeric type |
| E0106 | Cannot apply logical not to non-bool type |
| E0107 | If/else branches have different types |
| E0108 | Match arms have different types |
| E0109 | Function return type does not match body |
| E0110 | Type annotation does not match value type |
| E0111 | Cannot assign incompatible type to variable |
| E0112 | Cannot assign incompatible type to struct field |
| E0113 | Cannot assign incompatible type to array element |
| E0114 | Array elements must all have the same type |
| E0115 | Cannot infer type of empty array literal |
| E0116 | If condition must be `bool` |
| E0117 | While condition must be `bool` |
| E0118 | `??` inner type does not match default type |
| E0119 | `??` requires an optional type on the left |
| E0120 | `?` operator requires a Result type |
| E0121 | `?` can only be used in a function returning Result |
| E0122 | Range bounds must be integers |
| E0123 | Array index must be an integer |
| E0124 | Cannot index into non-array type |
| E0125 | Closure body type does not match declared return type |
| E0126 | Cannot infer type of closure parameter |
| E0127 | Unknown closure return type |
| E0128 | Struct equality requires `@derive(Eq)` |
| E0129 | Cannot use ordering comparison on struct type |
| E0130 | Incompatible type in compound assignment |
| E0131 | Type parameter inference conflict |
| E0132 | Pattern type incompatible with match subject |
| E0133 | Builtin function argument type mismatch |
| E0134 | Cannot call method on this type |
| E0135 | Cannot access field on this type |
| E0136 | Expression nesting too deep |
| E0137 | Unsupported compound assignment operator |

## Pattern/Match Errors (E0200-E0299)

| Code | Description |
|------|-------------|
| E0200 | Match expression is not exhaustive |
| E0201 | Match expression has no arms |
| E0202 | Match guard must be `bool` |
| E0203 | Variant destructure binding count does not match field count |
| E0204 | Enum variant requires arguments but none were provided |

## Name Resolution Errors (E0300-E0399)

| Code | Description |
|------|-------------|
| E0300 | Undefined variable |
| E0301 | Undefined function |
| E0302 | Undefined struct |
| E0303 | Undefined enum |
| E0304 | Undefined trait |
| E0305 | Unknown type name |
| E0306 | Struct is already defined |
| E0307 | Enum is already defined |
| E0308 | Function is already defined |
| E0309 | Trait is already defined |
| E0310 | Method is already defined for this type |
| E0311 | Constant is already defined |
| E0312 | Agent is already defined |
| E0313 | Cannot redefine builtin function |
| E0314 | No `main` function found |
| E0315 | Struct has no such field |
| E0316 | Enum has no such variant |
| E0317 | No such method found on type |
| E0318 | Missing field in struct literal |
| E0319 | Unknown derive trait |
| E0320 | Impl block for undefined type |
| E0321 | Undefined tool function referenced in agent |
| E0322 | Function referenced in agent is not a `tool fn` |
| E0323 | Unknown type in trait method signature |
| E0324 | Unknown return type |

## Codegen Errors (E0400-E0499)

| Code | Description |
|------|-------------|
| E0400 | Internal code generation error |
| E0401 | Undefined variable during code generation |
| E0402 | Undefined function during code generation |
| E0403 | Unsupported expression during code generation |
| E0404 | Linker or build tool error |
| E0405 | Cranelift backend error |
| E0406 | Missing function definition |

## Misc Errors (E0500-E0599)

| Code | Description |
|------|-------------|
| E0500 | Wrong number of arguments |
| E0501 | Cannot assign to immutable variable |
| E0502 | Cannot assign to field of immutable variable |
| E0503 | Cannot assign to index of immutable variable |
| E0504 | Test function must have no parameters |
| E0505 | Test function must have no return type |
| E0506 | Constant must be a literal value |
| E0507 | `break` can only be used inside a loop |
| E0508 | `continue` can only be used inside a loop |
| E0509 | Required trait method is not implemented |
| E0510 | Trait impl method return type does not match trait definition |
| E0511 | Agent does not accept field initializers |
| E0512 | Only named function calls are supported |
| E0513 | Builtin function argument count error |
