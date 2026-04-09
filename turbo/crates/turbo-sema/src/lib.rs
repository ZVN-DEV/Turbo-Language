//! Turbo semantic analyzer.
//!
//! The semantic check is the third stage of the compiler pipeline:
//! `lexer` → `parser` → **`sema`** → `codegen`. It walks the AST produced
//! by `turbo_parser` and produces a [`SemaResult`] containing every
//! well-defined [`SemaError`] and [`SemaWarning`] found.
//!
//! The checker is split across four files:
//! * `type_check` — expression / statement / module type checking
//!   (the bulk of the work).
//! * `scope` — lexical scope stack and name resolution.
//! * `exhaustiveness` — match-pattern validity helpers.
//! * This file (`lib.rs`) — shared type definitions ([`Ty`], `Checker`,
//!   helper structs), free helper functions, and the public
//!   [`check`] / [`check_test`] entry points.
//!
//! ```ignore
//! let (tokens, _) = turbo_lexer::tokenize(src);
//! let (module, _) = turbo_parser::parse(tokens);
//! let result = turbo_sema::check(&module);
//! assert!(result.errors.is_empty());
//! ```

use std::collections::HashMap;
use turbo_ast::*;

mod exhaustiveness;
mod scope;
mod suggest;
mod type_check;

use scope::Scope;

#[derive(Debug, Clone)]
pub struct SemaError {
    pub code: ErrorCode,
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for SemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug, Clone)]
pub struct SemaWarning {
    pub code: ErrorCode,
    pub message: String,
    pub span: Span,
}

pub struct SemaResult {
    pub errors: Vec<SemaError>,
    pub warnings: Vec<SemaWarning>,
}

/// Internal type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Str,
    Unit,
    /// Array of a given element type
    Array(Box<Ty>),
    /// Struct type (by name)
    Struct(String),
    /// Enum type (by name)
    Enum(String),
    /// Function type: `fn(params) -> ret`
    Fn(Vec<Ty>, Box<Ty>),
    /// Result type: `Result<ok, err>`
    Result(Box<Ty>, Box<Ty>),
    /// Optional type: `Optional<inner>`
    Optional(Box<Ty>),
    /// `Future<T>` — result of spawn / async function call
    Future(Box<Ty>),
    /// A generic type parameter (e.g., `T`)
    TypeParam(String),
    /// Agent type (by name) — instantiated agent with model/system/tools fields
    Agent(String),
    /// Type could not be determined (error recovery)
    Error,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::I8 => write!(f, "i8"),
            Ty::I16 => write!(f, "i16"),
            Ty::I32 => write!(f, "i32"),
            Ty::I64 => write!(f, "int"),
            Ty::U8 => write!(f, "u8"),
            Ty::U16 => write!(f, "u16"),
            Ty::U32 => write!(f, "u32"),
            Ty::U64 => write!(f, "u64"),
            Ty::F32 => write!(f, "f32"),
            Ty::F64 => write!(f, "float"),
            Ty::Bool => write!(f, "bool"),
            Ty::Str => write!(f, "str"),
            Ty::Unit => write!(f, "()"),
            Ty::Array(inner) => write!(f, "[{}]", inner),
            Ty::Struct(name) => write!(f, "{}", name),
            Ty::Enum(name) => write!(f, "{}", name),
            Ty::Fn(params, ret) => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            Ty::Result(ok, err) => write!(f, "Result<{}, {}>", ok, err),
            Ty::Optional(inner) => write!(f, "{}?", inner),
            Ty::Future(inner) => write!(f, "Future<{inner}>"),
            Ty::TypeParam(name) => write!(f, "{name}"),
            Ty::Agent(name) => write!(f, "{}", name),
            Ty::Error => write!(f, "<error>"),
        }
    }
}

impl Ty {
    pub(crate) fn is_integer(&self) -> bool {
        matches!(
            self,
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }

    pub(crate) fn is_float(&self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }

    pub(crate) fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub(crate) fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }

    /// Check if this type contains `Ty::Error` anywhere in its structure
    /// (e.g., `Optional(Error)`, `Result(Error, Error)`, `Array(Error)`).
    /// Used to suppress cascading diagnostics when the inner error was already reported.
    pub(crate) fn contains_error(&self) -> bool {
        match self {
            Ty::Error => true,
            Ty::Optional(inner) | Ty::Future(inner) | Ty::Array(inner) => inner.contains_error(),
            Ty::Result(ok, err) => ok.contains_error() || err.contains_error(),
            Ty::Fn(params, ret) => {
                ret.contains_error() || params.iter().any(|p| p.contains_error())
            }
            _ => false,
        }
    }
}

pub(crate) fn is_agent_serializable_type(
    ty: &Ty,
    structs: &HashMap<String, StructInfo>,
    enums: &HashMap<String, EnumInfo>,
) -> bool {
    match ty {
        Ty::I8
        | Ty::I16
        | Ty::I32
        | Ty::I64
        | Ty::U8
        | Ty::U16
        | Ty::U32
        | Ty::U64
        | Ty::F32
        | Ty::F64
        | Ty::Bool
        | Ty::Str
        | Ty::Unit => true,
        Ty::Array(inner) | Ty::Optional(inner) => is_agent_serializable_type(inner, structs, enums),
        Ty::Result(ok, err) => {
            is_agent_serializable_type(ok, structs, enums)
                && is_agent_serializable_type(err, structs, enums)
        }
        Ty::Struct(name) => structs.get(name).is_some_and(|info| {
            info.fields
                .iter()
                .all(|(_, field_ty)| is_agent_serializable_type(field_ty, structs, enums))
        }),
        Ty::Enum(name) => enums.get(name).is_some_and(|info| {
            info.variants.iter().all(|(_, field_tys)| {
                field_tys
                    .iter()
                    .all(|field_ty| is_agent_serializable_type(field_ty, structs, enums))
            })
        }),
        Ty::Fn(_, _) | Ty::Future(_) | Ty::TypeParam(_) | Ty::Agent(_) | Ty::Error => false,
    }
}

pub(crate) fn is_agent_structured_decode_supported(
    ty: &Ty,
    structs: &HashMap<String, StructInfo>,
) -> bool {
    match ty {
        Ty::Str | Ty::I64 | Ty::Bool | Ty::F64 => true,
        Ty::Struct(name) => structs.get(name).is_some_and(|info| {
            info.fields
                .iter()
                .all(|(_, field_ty)| is_agent_structured_decode_supported(field_ty, structs))
        }),
        _ => false,
    }
}

/// Extract the integer literal value from an expression, handling both
/// `IntLit(n)` and `UnaryOp { Neg, IntLit(n) }` (which is how `-128` is parsed).
pub(crate) fn extract_int_literal(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::IntLit(n) => Some(*n),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: inner,
        } => {
            if let Expr::IntLit(n) = &inner.node {
                Some(-*n)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if an integer literal value fits in the given target type.
pub(crate) fn int_literal_fits_in_type(n: i64, target: &Ty) -> bool {
    match target {
        Ty::U8 => (0..=255).contains(&n),
        Ty::U16 => (0..=65535).contains(&n),
        Ty::U32 | Ty::U64 => n >= 0,
        Ty::I8 => (-128..=127).contains(&n),
        Ty::I16 => (-32768..=32767).contains(&n),
        _ => true,
    }
}

/// Check if two types are compatible (allowing partial Result types where Error = unknown).
pub(crate) fn types_compatible(expected: &Ty, actual: &Ty) -> bool {
    if expected == actual {
        return true;
    }
    // Result types with Error (unknown) components are compatible with concrete Result types
    match (expected, actual) {
        (Ty::Result(ok1, err1), Ty::Result(ok2, err2)) => {
            let ok_ok = ok1.is_error() || ok2.is_error() || ok1 == ok2;
            let err_ok = err1.is_error() || err2.is_error() || err1 == err2;
            ok_ok && err_ok
        }
        // Optional types with Error (unknown) inner are compatible
        (Ty::Optional(inner1), Ty::Optional(inner2)) => {
            inner1.is_error() || inner2.is_error() || inner1 == inner2
        }
        _ => false,
    }
}

/// Check if a type is safe to use across the C FFI boundary.
pub(crate) fn is_ffi_safe_type(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::I8
            | Ty::I16
            | Ty::I32
            | Ty::I64
            | Ty::U8
            | Ty::U16
            | Ty::U32
            | Ty::U64
            | Ty::F32
            | Ty::F64
            | Ty::Bool
            | Ty::Str
            | Ty::Unit
            | Ty::Error
    )
}

pub(crate) fn resolve_type_expr(
    te: &TypeExpr,
    structs: Option<&HashMap<String, StructInfo>>,
    enums: Option<&HashMap<String, EnumInfo>>,
) -> Option<Ty> {
    resolve_type_expr_with_params(te, structs, enums, &[])
}

pub(crate) fn resolve_type_expr_with_params(
    te: &TypeExpr,
    structs: Option<&HashMap<String, StructInfo>>,
    enums: Option<&HashMap<String, EnumInfo>>,
    type_params: &[String],
) -> Option<Ty> {
    match te {
        TypeExpr::Named(name) => {
            // Check if name is a type parameter first
            if type_params.contains(name) {
                return Some(Ty::TypeParam(name.clone()));
            }
            match name.as_str() {
                "i8" => Some(Ty::I8),
                "i16" => Some(Ty::I16),
                "i32" => Some(Ty::I32),
                "int" | "i64" => Some(Ty::I64),
                "u8" => Some(Ty::U8),
                "u16" => Some(Ty::U16),
                "u32" => Some(Ty::U32),
                "u64" | "usize" => Some(Ty::U64),
                "f32" => Some(Ty::F32),
                "float" | "f64" => Some(Ty::F64),
                "bool" => Some(Ty::Bool),
                "str" => Some(Ty::Str),
                _ => {
                    // Check if it's a struct type
                    if let Some(s) = structs {
                        if s.contains_key(name.as_str()) {
                            return Some(Ty::Struct(name.clone()));
                        }
                    }
                    // Check if it's an enum type
                    if let Some(e) = enums {
                        if e.contains_key(name.as_str()) {
                            return Some(Ty::Enum(name.clone()));
                        }
                    }
                    None
                }
            }
        }
        TypeExpr::Unit => Some(Ty::Unit),
        TypeExpr::Array(inner) => {
            resolve_type_expr(&inner.node, structs, enums).map(|t| Ty::Array(Box::new(t)))
        }
        TypeExpr::FnType { params, ret } => {
            let mut param_tys = Vec::new();
            for p in params {
                match resolve_type_expr(&p.node, structs, enums) {
                    Some(ty) => param_tys.push(ty),
                    None => return None,
                }
            }
            let ret_ty = resolve_type_expr(&ret.node, structs, enums)?;
            Some(Ty::Fn(param_tys, Box::new(ret_ty)))
        }
        TypeExpr::Result { ok_type, err_type } => {
            let ok_ty = resolve_type_expr(&ok_type.node, structs, enums)?;
            let err_ty = resolve_type_expr(&err_type.node, structs, enums)?;
            Some(Ty::Result(Box::new(ok_ty), Box::new(err_ty)))
        }
        TypeExpr::Optional(inner) => {
            resolve_type_expr(&inner.node, structs, enums).map(|t| Ty::Optional(Box::new(t)))
        }
        TypeExpr::Future(inner) => {
            resolve_type_expr(&inner.node, structs, enums).map(|t| Ty::Future(Box::new(t)))
        }
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

/// Function signature
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct FnSig {
    pub(crate) type_params: Vec<String>,
    pub(crate) type_param_bounds: HashMap<String, Vec<String>>,
    pub(crate) params: Vec<(String, Ty)>,
    pub(crate) ret: Ty,
    pub(crate) is_async: bool,
    pub(crate) is_tool: bool,
    pub(crate) is_resource: bool,
    pub(crate) is_prompt: bool,
    pub(crate) is_unsafe: bool,
}

/// Registered agent info for semantic checking
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct AgentInfo {
    pub(crate) model: String,
    pub(crate) tools: Vec<String>,
    pub(crate) resources: Vec<String>,
    pub(crate) prompts: Vec<String>,
    pub(crate) output_ty: Option<Ty>,
    pub(crate) system_prompt: Option<String>,
}

/// Struct field info for the checker
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct StructInfo {
    pub(crate) fields: Vec<(String, Ty)>,
    /// Type parameter names for generic structs
    pub(crate) type_params: Vec<String>,
    /// Derived trait names from `@derive(...)` attribute
    pub(crate) derives: Vec<String>,
}

/// Enum info (variant names + field types)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct EnumInfo {
    /// Variant name -> field types (empty vec for unit variants)
    pub(crate) variants: Vec<(String, Vec<Ty>)>,
    /// Type parameter names for generic enums
    pub(crate) type_params: Vec<String>,
}

impl EnumInfo {
    /// Get just the variant names
    pub(crate) fn variant_names(&self) -> Vec<String> {
        self.variants.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Check if a variant name exists
    pub(crate) fn has_variant(&self, name: &str) -> bool {
        self.variants.iter().any(|(n, _)| n == name)
    }

    /// Get the field types for a variant
    pub(crate) fn variant_fields(&self, name: &str) -> Option<&Vec<Ty>> {
        self.variants
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, fields)| fields)
    }
}

/// Trait definition info for the checker
#[derive(Debug, Clone)]
pub(crate) struct TraitInfo {
    pub(crate) methods: Vec<TraitMethodInfo>,
}

/// Trait method signature info
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TraitMethodInfo {
    pub(crate) name: String,
    pub(crate) params: Vec<(String, Ty)>,
    pub(crate) ret: Ty,
    /// Whether this method has a default implementation in the trait
    pub(crate) has_default: bool,
}

/// Type checker
#[allow(dead_code)]
pub(crate) struct Checker {
    pub(crate) errors: Vec<SemaError>,
    pub(crate) warnings: Vec<SemaWarning>,
    pub(crate) functions: HashMap<String, FnSig>,
    pub(crate) agents: HashMap<String, AgentInfo>,
    pub(crate) structs: HashMap<String, StructInfo>,
    pub(crate) enums: HashMap<String, EnumInfo>,
    /// Methods: type_name -> method_name -> FnSig
    pub(crate) methods: HashMap<String, HashMap<String, FnSig>>,
    /// Trait definitions: trait_name -> TraitInfo
    pub(crate) traits: HashMap<String, TraitInfo>,
    /// Trait implementations: type_name -> set of trait names
    pub(crate) trait_impls: HashMap<String, Vec<String>>,
    /// Module-level constants: name -> type
    pub(crate) constants: HashMap<String, Ty>,
    pub(crate) scopes: Vec<Scope>,
    /// Return type of the current function being checked
    pub(crate) current_return_type: Ty,
    /// Hint for closure parameter types when checking closures passed to map/filter/reduce
    pub(crate) closure_param_hint: Option<Vec<Ty>>,
    /// When true, `main` is not required (used for `turbolang test` mode)
    pub(crate) test_mode: bool,
    /// Whether we are currently checking inside an `@unsafe` function
    pub(crate) in_unsafe_context: bool,
    /// Nesting depth of loops (for break/continue validation)
    pub(crate) loop_depth: usize,
    /// Current recursion depth for expression checking (prevents stack overflow)
    pub(crate) expr_depth: usize,
}

/// Maximum recursion depth for expression/statement checking.
/// Exceeding this produces a sema error instead of a stack overflow.
pub(crate) const MAX_EXPR_DEPTH: usize = 200;

impl Checker {
    pub(crate) fn new() -> Self {
        // Pre-register built-in traits
        let mut traits = HashMap::new();
        traits.insert(
            "Display".to_string(),
            TraitInfo {
                methods: vec![TraitMethodInfo {
                    name: "to_string".to_string(),
                    params: vec![("self".to_string(), Ty::Error)], // self type filled at impl
                    ret: Ty::Str,
                    has_default: false,
                }],
            },
        );

        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            functions: HashMap::new(),
            agents: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            methods: HashMap::new(),
            traits,
            trait_impls: HashMap::new(),
            constants: HashMap::new(),
            scopes: Vec::new(),
            current_return_type: Ty::Unit,
            closure_param_hint: None,
            in_unsafe_context: false,
            test_mode: false,
            loop_depth: 0,
            expr_depth: 0,
        }
    }

    pub(crate) fn error(&mut self, code: ErrorCode, message: String, span: Span) {
        self.errors.push(SemaError {
            code,
            message,
            span,
        });
    }

    pub(crate) fn warn(&mut self, code: ErrorCode, message: String, span: Span) {
        self.warnings.push(SemaWarning {
            code,
            message,
            span,
        });
    }
}

/// Run semantic analysis on a module. Returns errors found.
pub fn check(module: &Module) -> SemaResult {
    let mut checker = Checker::new();
    checker.check_module(module);
    SemaResult {
        errors: checker.errors,
        warnings: checker.warnings,
    }
}

/// Run semantic analysis in test mode (no `main` required). Returns errors found.
pub fn check_test(module: &Module) -> SemaResult {
    let mut checker = Checker::new();
    checker.test_mode = true;
    checker.check_module(module);
    SemaResult {
        errors: checker.errors,
        warnings: checker.warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(source: &str) -> Vec<SemaError> {
        let (tokens, _) = turbo_lexer::tokenize(source);
        let (module, _) = turbo_parser::parse(tokens);
        check(&module).errors
    }

    fn assert_no_errors(source: &str) {
        let errors = check_source(source);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    fn assert_has_error(source: &str, expected_msg: &str) {
        let errors = check_source(source);
        assert!(
            errors.iter().any(|e| e.message.contains(expected_msg)),
            "Expected error containing '{}', got: {:?}",
            expected_msg,
            errors
        );
    }

    #[test]
    fn test_valid_program() {
        assert_no_errors(r#"fn main() { print("hello") }"#);
    }

    #[test]
    fn test_valid_arithmetic() {
        assert_no_errors("fn main() { let x = 10\n let y = 20\n let z = x + y }");
    }

    #[test]
    fn test_type_mismatch_arithmetic() {
        assert_has_error(
            "fn main() { let x = true\n let y = x + 1 }",
            "cannot perform arithmetic on `bool`",
        );
    }

    #[test]
    fn test_string_concat_ok() {
        assert_no_errors(r#"fn main() { let x = "a" + "b" }"#);
    }

    #[test]
    fn test_string_subtraction_rejected() {
        assert_has_error(
            r#"fn main() { let x = "a" - "b" }"#,
            "cannot perform arithmetic on `str`",
        );
    }

    #[test]
    fn test_undefined_variable() {
        assert_has_error("fn main() { print(x) }", "undefined variable `x`");
    }

    #[test]
    fn test_immutable_assignment() {
        assert_has_error(
            "fn main() { let x = 1\n x = 2 }",
            "cannot assign to immutable variable",
        );
    }

    #[test]
    fn test_mutable_assignment_ok() {
        assert_no_errors("fn main() { let mut x = 1\n x = 2 }");
    }

    #[test]
    fn test_function_wrong_args() {
        assert_has_error(
            "fn add(a: i64, b: i64) -> i64 { a + b }\nfn main() { add(1) }",
            "expects 2 argument(s) but 1 were given",
        );
    }

    #[test]
    fn test_function_wrong_type() {
        assert_has_error(
            r#"fn double(x: i64) -> i64 { x * 2 }
fn main() { double("hello") }"#,
            "argument `x` expects `int`, found `str`",
        );
    }

    #[test]
    fn test_return_type_mismatch() {
        assert_has_error(
            r#"fn foo() -> i64 { "hello" }"#,
            "should return `int` but body returns `str`",
        );
    }

    #[test]
    fn test_duplicate_function() {
        assert_has_error(
            "fn foo() { }\nfn foo() { }\nfn main() { }",
            "function `foo` is already defined",
        );
    }

    #[test]
    fn test_scope_isolation() {
        assert_has_error(
            "fn main() { if true { let x = 1 }\n print(x) }",
            "undefined variable `x`",
        );
    }

    #[test]
    fn test_type_annotation_mismatch() {
        assert_has_error(
            r#"fn main() { let x: i32 = "hello" }"#,
            "type annotation `i32` doesn't match value type `str`",
        );
    }

    #[test]
    fn test_valid_if_else_expression() {
        assert_no_errors("fn main() { let x = if true { 1 } else { 2 } }");
    }

    #[test]
    fn test_if_else_branch_mismatch() {
        assert_has_error(
            r#"fn main() { let x = if true { 1 } else { "hello" } }"#,
            "if/else branches have different types",
        );
    }

    #[test]
    fn test_no_main() {
        assert_has_error("fn foo() { }", "no `main` function found");
    }

    // === Task 1: Builtin function shadowing ===

    #[test]
    fn test_shadow_builtin() {
        assert_has_error("fn print() { }\nfn main() { }", "cannot redefine builtin");
    }

    #[test]
    fn test_shadow_builtin_panic() {
        assert_has_error(
            "fn panic() { }\nfn main() { }",
            "cannot redefine builtin function `panic`",
        );
    }

    #[test]
    fn test_shadow_builtin_assert() {
        assert_has_error(
            "fn assert(x: bool) { }\nfn main() { }",
            "cannot redefine builtin function `assert`",
        );
    }

    // === Task 2: Builtin argument count validation ===

    #[test]
    fn test_print_too_many_args() {
        assert_has_error(
            r#"fn main() { print("a", "b") }"#,
            "print() takes at most 1 argument, got 2",
        );
    }

    #[test]
    fn test_print_zero_args_ok() {
        assert_no_errors("fn main() { print() }");
    }

    #[test]
    fn test_print_one_arg_ok() {
        assert_no_errors(r#"fn main() { print("hello") }"#);
    }

    #[test]
    fn test_panic_too_many_args() {
        assert_has_error(
            r#"fn main() { panic("a", "b") }"#,
            "panic() takes at most 1 argument, got 2",
        );
    }

    #[test]
    fn test_assert_too_many_args() {
        assert_has_error(
            r#"fn main() { assert(true, "msg", "extra") }"#,
            "assert() takes at most 2 arguments, got 3",
        );
    }

    #[test]
    fn test_assert_one_arg_ok() {
        assert_no_errors("fn main() { assert(true) }");
    }

    #[test]
    fn test_assert_two_args_ok() {
        assert_no_errors(r#"fn main() { assert(true, "ok") }"#);
    }

    // === Task 3: Integer literal coercion ===

    #[test]
    fn test_unsigned_literal_assignment() {
        assert_no_errors("fn main() { let x: u32 = 5 }");
    }

    #[test]
    fn test_u64_literal_assignment() {
        assert_no_errors("fn main() { let x: u64 = 100 }");
    }

    #[test]
    fn test_i32_literal_assignment() {
        assert_no_errors("fn main() { let x: i32 = 42 }");
    }

    #[test]
    fn test_negative_literal_to_unsigned_rejected() {
        assert_has_error(
            "fn main() { let x: u32 = -1 }",
            "type annotation `u32` doesn't match value type",
        );
    }

    #[test]
    fn test_string_to_u32_still_rejected() {
        assert_has_error(
            r#"fn main() { let x: u32 = "hello" }"#,
            "type annotation `u32` doesn't match value type `str`",
        );
    }

    // === Match exhaustiveness ===

    #[test]
    fn test_match_int_without_wildcard() {
        assert_has_error(
            "fn main() { let x = 1\n match x { 1 => print(1) } }",
            "match is not exhaustive",
        );
    }

    #[test]
    fn test_match_int_with_wildcard_ok() {
        assert_no_errors("fn main() { let x = 1\n match x { 1 => print(1)\n _ => print(0) } }");
    }

    #[test]
    fn test_match_enum_missing_variant() {
        assert_has_error(
            r#"type Color { Red, Green, Blue }
fn main() {
    let c = Color.Red
    match c {
        Red => print(1)
        Green => print(2)
    }
}"#,
            "match is not exhaustive",
        );
    }

    #[test]
    fn test_match_enum_all_variants_ok() {
        assert_no_errors(
            r#"type Color { Red, Green, Blue }
fn main() {
    let c = Color.Red
    match c {
        Red => print(1)
        Green => print(2)
        Blue => print(3)
    }
}"#,
        );
    }

    #[test]
    fn test_match_enum_with_wildcard_ok() {
        assert_no_errors(
            r#"type Color { Red, Green, Blue }
fn main() {
    let c = Color.Red
    match c {
        Red => print(1)
        _ => print(0)
    }
}"#,
        );
    }

    #[test]
    fn test_match_bool_not_exhaustive() {
        assert_has_error(
            "fn main() { let x = true\n match x { true => print(1) } }",
            "match is not exhaustive",
        );
    }

    #[test]
    fn test_match_bool_exhaustive_ok() {
        assert_no_errors(
            "fn main() { let x = true\n match x { true => print(1)\n false => print(0) } }",
        );
    }

    // === Generics ===

    #[test]
    fn test_generic_identity_int() {
        assert_no_errors("fn identity<T>(x: T) -> T { x }\nfn main() { let r = identity(42) }");
    }

    #[test]
    fn test_generic_identity_str() {
        assert_no_errors(
            r#"fn identity<T>(x: T) -> T { x }
fn main() { let r = identity("hello") }"#,
        );
    }

    #[test]
    fn test_generic_identity_bool() {
        assert_no_errors("fn identity<T>(x: T) -> T { x }\nfn main() { let r = identity(true) }");
    }

    #[test]
    fn test_generic_first() {
        assert_no_errors("fn first<T>(a: T, b: T) -> T { a }\nfn main() { let r = first(1, 2) }");
    }

    #[test]
    fn test_generic_type_mismatch() {
        assert_has_error(
            r#"fn first<T>(a: T, b: T) -> T { a }
fn main() { first(1, "hello") }"#,
            "type parameter `T` inferred as `int` but argument has type `str`",
        );
    }

    #[test]
    fn test_generic_wrong_arg_count() {
        assert_has_error(
            "fn identity<T>(x: T) -> T { x }\nfn main() { identity(1, 2) }",
            "expects 1 argument(s) but 2 were given",
        );
    }

    #[test]
    fn test_async_fn_valid() {
        assert_no_errors(
            "async fn compute(x: i64) -> i64 { x * x + 1 }\nasync fn main() { let r = await compute(5)\n print(r) }",
        );
    }

    #[test]
    fn test_async_fn_return_type_mismatch() {
        assert_has_error(
            r#"async fn bad() -> i64 { "hello" }
fn main() { }"#,
            "should return `int` but body returns `str`",
        );
    }

    #[test]
    fn test_spawn_creates_future() {
        // spawn wraps result in Future<T>, await unwraps it
        assert_no_errors(
            "async fn square(x: i64) -> i64 { x * x }\nasync fn main() { let f = spawn square(3)\n let r = await f\n print(r) }",
        );
    }

    #[test]
    fn test_await_passthrough_non_future() {
        // await on a non-future type just passes through
        assert_no_errors(
            "fn compute(x: i64) -> i64 { x + 1 }\nfn main() { let r = await compute(5)\n print(r) }",
        );
    }

    #[test]
    fn test_tool_fn_valid() {
        assert_no_errors(
            r#"tool fn search(q: str) -> str { "results" }
fn main() { search("hello") }"#,
        );
    }

    #[test]
    fn test_tool_fn_type_checking() {
        assert_has_error(
            r#"tool fn search(q: str) -> str { "results" }
fn main() { search(42) }"#,
            "argument `q` expects `str`, found `int`",
        );
    }

    #[test]
    fn test_agent_valid() {
        assert_no_errors(
            r#"tool fn search(q: str) -> str { "r" }
tool fn calc(x: i64) -> i64 { x * 2 }
agent Helper {
    model: "claude-sonnet"
    tools: [search, calc]
    system: "You help."
}
fn main() { search("hi") }"#,
        );
    }

    #[test]
    fn test_agent_undefined_tool() {
        assert_has_error(
            r#"agent Helper {
    model: "test"
    tools: [nonexistent]
}
fn main() { }"#,
            "undefined tool function `nonexistent`",
        );
    }

    #[test]
    fn test_agent_non_tool_function() {
        assert_has_error(
            r#"fn helper(x: i64) -> i64 { x }
agent Bot {
    model: "test"
    tools: [helper]
}
fn main() { }"#,
            "is not a `tool fn`",
        );
    }

    #[test]
    fn test_duplicate_agent() {
        assert_has_error(
            r#"tool fn t(x: i64) -> i64 { x }
agent A {
    model: "test"
    tools: [t]
}
agent A {
    model: "test"
    tools: [t]
}
fn main() { }"#,
            "agent `A` is already defined",
        );
    }

    #[test]
    fn test_optional_none_no_error_leak() {
        // Regression: `let x: i64? = none` should not produce an error
        // containing `<error>?` (internal type representation leaking).
        assert_no_errors("fn main() { let x: i64? = none }");
    }

    #[test]
    fn test_optional_none_no_error_in_message() {
        // If an error IS produced, it must never contain `<error>`
        let errors = check_source("fn main() { let x: i64? = none }");
        for e in &errors {
            assert!(
                !e.message.contains("<error>"),
                "Internal type `<error>` leaked into error message: {}",
                e.message
            );
        }
    }

    #[test]
    fn test_optional_none_return_no_error_leak() {
        // Returning `none` from a function that returns `i64?` should not
        // produce `<error>?` in any diagnostic message.
        assert_no_errors("fn get_val() -> i64? { none }\nfn main() { let x = get_val() }");
    }

    // === Unused variable warnings (E0515) ===

    fn check_warnings(source: &str) -> Vec<SemaWarning> {
        let (tokens, _) = turbo_lexer::tokenize(source);
        let (module, _) = turbo_parser::parse(tokens);
        check(&module).warnings
    }

    fn assert_has_warning(source: &str, expected_msg: &str) {
        let warnings = check_warnings(source);
        assert!(
            warnings.iter().any(|w| w.message.contains(expected_msg)),
            "Expected warning containing '{}', got: {:?}",
            expected_msg,
            warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
        );
    }

    fn assert_no_warnings(source: &str) {
        let warnings = check_warnings(source);
        assert!(
            warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_unused_param_no_warning() {
        // Function parameters should NOT produce unused variable warnings
        assert_no_warnings("fn foo(x: i64) -> i64 { 42 }\nfn main() { foo(1) }");
    }

    #[test]
    fn test_unused_variable_underscore_suppressed() {
        // Variables starting with _ should not trigger unused warnings
        assert_no_warnings("fn foo(_x: i64) -> i64 { 42 }\nfn main() { foo(1) }");
    }

    #[test]
    fn test_used_variable_no_warning() {
        assert_no_warnings("fn add(a: i64, b: i64) -> i64 { a + b }\nfn main() { add(1, 2) }");
    }

    #[test]
    fn test_unused_let_binding_warning() {
        assert_has_warning("fn main() { let y = 42 }", "unused variable `y`");
    }

    #[test]
    fn test_used_let_binding_no_warning() {
        assert_no_warnings(
            r#"fn main() { let y = 42
            print(y) }"#,
        );
    }

    #[test]
    fn test_unused_closure_param_no_warning() {
        // Closure parameters should NOT produce unused variable warnings
        assert_no_warnings("fn main() { let f = (x: i64) -> i64 => { 42 }\nf(1) }");
    }

    #[test]
    fn test_unused_for_var_no_warning() {
        // For-in loop variables should NOT produce unused variable warnings
        assert_no_warnings("fn main() { for i in range(0, 3) { print(\"hello\") } }");
    }

    // =========================================================================
    // P1 Task 8: Behavioral regression tests — targeted error codes + edge cases.
    // These exist to ensure every ErrorCode path has at least one test that
    // flips if the code path breaks. Grouped by ErrorCode category.
    // =========================================================================

    /// Assert at least one error with the given ErrorCode is produced.
    fn assert_has_code(source: &str, expected: ErrorCode) {
        let errors = check_source(source);
        assert!(
            errors.iter().any(|e| e.code == expected),
            "Expected error with code {:?}, got: {:?}",
            expected,
            errors
                .iter()
                .map(|e| (e.code, &e.message))
                .collect::<Vec<_>>()
        );
    }

    // ----- Match exhaustiveness (E0200 / E0201 / E0202) ----------------------
    // Note: E0201 (empty match arms) is unreachable from source today — the
    // parser accepts `match x { _ => 0 }` and rejects truly empty arm lists.
    // Wildcard-only matches are covered by `test_match_int_with_wildcard_ok`.

    #[test]
    fn test_match_guard_non_bool_flags_e0202() {
        // A match guard that isn't a bool expression should be rejected.
        assert_has_code(
            r#"fn main() {
                let x = 1
                match x {
                    n if 42 => print(1)
                    _ => print(0)
                }
            }"#,
            ErrorCode::E0202,
        );
    }

    #[test]
    fn test_match_bool_missing_true_not_exhaustive() {
        assert_has_code(
            "fn main() { let b = false\n match b { false => print(1) } }",
            ErrorCode::E0200,
        );
    }

    #[test]
    fn test_match_enum_data_variant_exhaustive_ok() {
        // Enum with data carrying variants — covering all variants should be OK.
        assert_no_errors(
            r#"type Shape {
                Circle(f64),
                Square(f64)
            }
            fn main() {
                let s = Shape.Circle(1.0)
                match s {
                    Circle(r) => print(r)
                    Square(s) => print(s)
                }
            }"#,
        );
    }

    #[test]
    fn test_match_int_guard_then_wildcard_ok() {
        // Guards + wildcard should satisfy exhaustiveness.
        assert_no_errors(
            "fn main() { let x = 1\n match x { n if n > 0 => print(1)\n _ => print(0) } }",
        );
    }

    #[test]
    fn test_match_subject_type_incompatible_pattern() {
        // Matching a bool value against an integer literal pattern.
        assert_has_code(
            "fn main() { let b = true\n match b { 1 => print(1)\n _ => print(0) } }",
            ErrorCode::E0132,
        );
    }

    // ----- FFI-safety (E0305 ish / extern) -----------------------------------

    #[test]
    fn test_extern_fn_with_array_param_rejected() {
        // Array<T> is not FFI-safe; should produce an error mentioning FFI-safe.
        assert_has_error(
            r#"@unsafe
extern "C" {
    fn bad(xs: [i32]) -> i32
}
fn main() { }"#,
            "not FFI-safe",
        );
    }

    #[test]
    fn test_extern_fn_with_struct_param_rejected() {
        assert_has_error(
            r#"struct Point { x: i32, y: i32 }
@unsafe
extern "C" {
    fn bad(p: Point) -> i32
}
fn main() { }"#,
            "not FFI-safe",
        );
    }

    #[test]
    fn test_extern_fn_with_enum_return_rejected() {
        assert_has_error(
            r#"type Color { Red, Green, Blue }
@unsafe
extern "C" {
    fn bad() -> Color
}
fn main() { }"#,
            "not FFI-safe",
        );
    }

    #[test]
    fn test_extern_fn_with_scalar_types_ok() {
        // i64, f64, bool, str are all FFI-safe per is_ffi_safe_type.
        assert_no_errors(
            r#"@unsafe
extern "C" {
    fn good1(x: i64) -> i64
    fn good2(f: f64) -> f64
    fn good3(b: bool) -> bool
    fn good4(s: str) -> i32
}
fn main() { }"#,
        );
    }

    // ----- Generic / type-param inference (E0131) ----------------------------

    #[test]
    fn test_generic_bool_int_mismatch_inference_conflict() {
        // Inference binds T=bool from first arg, then int for second argument.
        // The sema currently reports this via E0100 ("type parameter ... inferred as ...").
        assert_has_error(
            r#"fn pair<T>(a: T, b: T) -> T { a }
fn main() { pair(true, 1) }"#,
            "type parameter `T` inferred as",
        );
    }

    #[test]
    fn test_generic_matches_nested_inference_ok() {
        // Two generic params, each inferred independently.
        assert_no_errors(
            r#"fn both<A, B>(a: A, b: B) -> A { a }
            fn main() { both(1, "hi") }"#,
        );
    }

    // ----- Shadowing / scoping (E0300 etc.) ----------------------------------

    #[test]
    fn test_let_shadowing_same_scope_ok() {
        // Re-binding via `let` should be allowed (shadowing).
        assert_no_errors(
            r#"fn main() {
                let x = 1
                let x = "hello"
                print(x)
            }"#,
        );
    }

    #[test]
    fn test_inner_scope_cannot_leak_variable() {
        assert_has_code("fn main() { { let x = 1 }\n print(x) }", ErrorCode::E0300);
    }

    #[test]
    fn test_nested_if_scope_leaks_rejected() {
        assert_has_code(
            r#"fn main() {
                if true { let y = 10 }
                print(y)
            }"#,
            ErrorCode::E0300,
        );
    }

    #[test]
    fn test_while_scope_leaks_rejected() {
        assert_has_code(
            r#"fn main() {
                let mut i = 0
                while i < 3 { let tmp = i\n i += 1 }
                print(tmp)
            }"#,
            ErrorCode::E0300,
        );
    }

    // ----- did-you-mean suggestions (E0300) ----------------------------------

    #[test]
    fn test_did_you_mean_typo_suggestion() {
        // If a close candidate exists, the message should include `did you mean`.
        let errors = check_source("fn main() { let counter = 0\n print(cuonter) }");
        let found = errors
            .iter()
            .any(|e| e.code == ErrorCode::E0300 && e.message.contains("did you mean"));
        assert!(
            found,
            "Expected a did-you-mean suggestion for `cuonter`, got: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_no_did_you_mean_when_distant() {
        // Totally unrelated identifier should NOT produce a suggestion.
        let errors = check_source("fn main() { print(zzzzzzzzzzzzz) }");
        assert!(
            !errors
                .iter()
                .any(|e| e.code == ErrorCode::E0300 && e.message.contains("did you mean")),
            "Unexpected did-you-mean: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    // ----- Numeric coercion (signed/unsigned, float/int) ---------------------

    #[test]
    fn test_float_to_int_annotation_rejected() {
        assert_has_code("fn main() { let x: i64 = 3.14 }", ErrorCode::E0110);
    }

    #[test]
    fn test_int_to_float_annotation_rejected() {
        assert_has_code("fn main() { let x: f64 = 42 }", ErrorCode::E0110);
    }

    #[test]
    fn test_mixed_i32_i64_arithmetic_rejected() {
        assert_has_code(
            r#"fn main() {
                let a: i32 = 1
                let b: i64 = 2
                let c = a + b
            }"#,
            ErrorCode::E0102,
        );
    }

    #[test]
    fn test_mixed_u32_i32_arithmetic_rejected() {
        assert_has_code(
            r#"fn main() {
                let a: u32 = 1
                let b: i32 = 2
                let c = a + b
            }"#,
            ErrorCode::E0102,
        );
    }

    // ----- Immutability errors (E0501 / E0502 / E0503) -----------------------

    #[test]
    fn test_immutable_struct_field_assign_rejected() {
        assert_has_code(
            r#"struct Point { x: i64, y: i64 }
            fn main() {
                let p = Point { x: 1, y: 2 }
                p.x = 10
            }"#,
            ErrorCode::E0502,
        );
    }

    #[test]
    fn test_immutable_array_index_assign_rejected() {
        assert_has_code(
            r#"fn main() {
                let arr = [1, 2, 3]
                arr[0] = 99
            }"#,
            ErrorCode::E0503,
        );
    }

    #[test]
    fn test_mutable_struct_field_assign_ok() {
        assert_no_errors(
            r#"struct Point { x: i64, y: i64 }
            fn main() {
                let mut p = Point { x: 1, y: 2 }
                p.x = 10
                print(p.x)
            }"#,
        );
    }

    // ----- Break/continue outside loop (E0507 / E0508) -----------------------

    #[test]
    fn test_break_outside_loop_flagged() {
        assert_has_code("fn main() { break }", ErrorCode::E0507);
    }

    #[test]
    fn test_continue_outside_loop_flagged() {
        assert_has_code("fn main() { continue }", ErrorCode::E0508);
    }

    #[test]
    fn test_break_inside_while_ok() {
        assert_no_errors("fn main() { while true { break } }");
    }

    #[test]
    fn test_continue_inside_while_ok() {
        assert_no_errors("fn main() { let mut i = 0\n while i < 10 { i += 1\n continue } }");
    }

    // ----- Try / Result / Optional edges (E0119 / E0120 / E0121) -------------

    #[test]
    fn test_try_in_non_result_fn_rejected() {
        // `?` is only allowed in a function that returns a Result.
        assert_has_code(
            r#"fn get() -> i64 ! str { ok(1) }
fn main() { let x = get()? }"#,
            ErrorCode::E0121,
        );
    }

    #[test]
    fn test_try_on_non_result_rejected() {
        // Applying `?` to a non-Result expression.
        assert_has_code(
            r#"fn bad() -> i64 ! str {
    let x = 1?
    ok(x)
}
fn main() { }"#,
            ErrorCode::E0120,
        );
    }

    #[test]
    fn test_null_coalesce_on_non_optional_rejected() {
        assert_has_code("fn main() { let x = 1 ?? 2 }", ErrorCode::E0119);
    }

    // ----- Undefined identifiers (E0301 / E0302 / E0303) ---------------------

    #[test]
    fn test_undefined_function_e0301() {
        assert_has_code("fn main() { mystery_fn(1, 2) }", ErrorCode::E0301);
    }

    #[test]
    fn test_undefined_struct_literal_e0302() {
        assert_has_code(
            "fn main() { let p = Nonexistent { x: 1 } }",
            ErrorCode::E0302,
        );
    }

    // ----- Duplicate definitions (E0306..E0312) ------------------------------

    #[test]
    fn test_duplicate_struct_e0306() {
        assert_has_code(
            r#"struct Foo { x: i32 }
            struct Foo { y: i32 }
            fn main() { }"#,
            ErrorCode::E0306,
        );
    }

    #[test]
    fn test_duplicate_enum_e0307() {
        assert_has_code(
            r#"type Color { Red }
            type Color { Blue }
            fn main() { }"#,
            ErrorCode::E0307,
        );
    }

    // ----- If/while condition type errors (E0116 / E0117) --------------------

    #[test]
    fn test_if_non_bool_condition_rejected() {
        // Integers are allowed (truthy); a string is not.
        assert_has_code("fn main() { if \"hi\" { print(\"x\") } }", ErrorCode::E0116);
    }

    #[test]
    fn test_while_non_bool_condition_rejected() {
        assert_has_code(
            "fn main() { while \"hi\" { print(\"x\") } }",
            ErrorCode::E0117,
        );
    }

    // ----- Unused variable warning (E0515) -----------------------------------

    #[test]
    fn test_unused_variable_emits_warning_code() {
        let warnings = check_warnings("fn main() { let x = 10 }");
        assert!(
            warnings.iter().any(|w| w.code == ErrorCode::E0515),
            "Expected E0515 warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_let_shadowing_no_false_unused_on_used_inner() {
        // Shadowing into a used variable should not produce an unused warning
        // on the inner binding (the inner `x` IS read via print).
        let warnings = check_warnings(
            r#"fn main() {
                let x = 1
                let x = 2
                print(x)
            }"#,
        );
        let inner_reported = warnings
            .iter()
            .filter(|w| w.code == ErrorCode::E0515 && w.message.contains("`x`"))
            .count();
        // We expect at most one unused warning (for the original outer x that
        // gets shadowed before use). Reporting more than one would be a bug.
        assert!(
            inner_reported <= 1,
            "Shadowed `x` should not produce duplicate unused warnings, got: {:?}",
            warnings
        );
    }

    // ----- Range / indexing (E0122 / E0123 / E0124) --------------------------

    #[test]
    fn test_array_index_non_integer_rejected() {
        assert_has_code(
            r#"fn main() {
                let a = [1, 2, 3]
                let x = a["zero"]
            }"#,
            ErrorCode::E0123,
        );
    }

    #[test]
    fn test_index_non_array_rejected() {
        assert_has_code("fn main() { let x = 1\n let y = x[0] }", ErrorCode::E0124);
    }

    // ----- Field access / method call on wrong type (E0134 / E0135) ---------

    #[test]
    fn test_field_access_on_int_rejected() {
        assert_has_code("fn main() { let x = 1\n print(x.foo) }", ErrorCode::E0135);
    }

    // ----- Struct literal missing field (E0318) ------------------------------

    #[test]
    fn test_struct_literal_missing_field() {
        assert_has_code(
            r#"struct Point { x: i64, y: i64 }
            fn main() { let p = Point { x: 1 } }"#,
            ErrorCode::E0318,
        );
    }

    // ----- Test function constraints (E0504 / E0505) -------------------------

    #[test]
    fn test_test_fn_with_params_rejected() {
        assert_has_code(
            "@test fn my_test(x: i64) { }\nfn main() { }",
            ErrorCode::E0504,
        );
    }

    #[test]
    fn test_test_fn_with_return_type_rejected() {
        assert_has_code(
            "@test fn my_test() -> i64 { 1 }\nfn main() { }",
            ErrorCode::E0505,
        );
    }

    // ----- Redefinition of builtins (E0313) ----------------------------------

    #[test]
    fn test_redefine_print_rejected_with_code() {
        assert_has_code("fn print() { }\nfn main() { }", ErrorCode::E0313);
    }

    #[test]
    fn test_redefine_len_rejected_with_code() {
        assert_has_code("fn len() -> i64 { 0 }\nfn main() { }", ErrorCode::E0313);
    }

    // ----- Array element type uniformity (E0114) -----------------------------

    #[test]
    fn test_mixed_array_elements_rejected() {
        assert_has_code(r#"fn main() { let a = [1, "two", 3] }"#, ErrorCode::E0114);
    }

    #[test]
    fn test_empty_array_no_annotation_rejected() {
        assert_has_code("fn main() { let a = [] }", ErrorCode::E0115);
    }

    // ----- Logical-not / negate (E0105 / E0106) ------------------------------

    #[test]
    fn test_logical_not_on_int_rejected() {
        assert_has_code("fn main() { let x = !42 }", ErrorCode::E0106);
    }

    #[test]
    fn test_negate_bool_rejected() {
        assert_has_code("fn main() { let x = -true }", ErrorCode::E0105);
    }
}
