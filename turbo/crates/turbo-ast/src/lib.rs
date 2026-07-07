//! Turbo AST — the shared vocabulary for every stage of the compiler.
//!
//! This crate has no logic of its own; it defines the data types that the
//! parser produces, the semantic checker reads, and the codegen consumes.
//! Keeping the AST in its own crate prevents cyclic dependencies between
//! `turbo-parser`, `turbo-sema`, and the codegen backends.
//!
//! # Pipeline position
//!
//! lexer → parser → **AST** → sema → codegen
//!
//! # Key types
//!
//! * [`Module`] — root of every parsed file; contains a `Vec<Spanned<Item>>`.
//! * [`Item`] — top-level: `Function`, `Struct`, `Enum`, `Impl`, `Trait`,
//!   `Agent`, `Import`, `Const`, `Extern`.
//! * [`Expr`] — expression nodes (literals, calls, control flow, etc.).
//! * [`Stmt`] — statement nodes (`let`, `return`, `defer`).
//! * [`TypeExpr`] — type expressions written by the user.
//! * [`Pattern`] — patterns used in `match` arms and destructuring.
//! * [`Span`] / [`Spanned`] — byte-offset spans into the source.
//! * [`errors::ErrorCode`] — the unique error-code enum (`E0001`-`E0515`).
//!
//! Re-exports of `Span`, `Spanned`, and `ErrorCode` make this crate the
//! single source of truth for cross-crate diagnostics.

pub mod errors;
pub mod stdlib_modules;
pub use errors::ErrorCode;

/// A span in source code: byte offset range [start, end).
///
/// # Examples
///
/// ```
/// use turbo_ast::Span;
///
/// let span: Span = 0..5;
/// assert_eq!(span.start, 0);
/// assert_eq!(span.end, 5);
/// ```
pub type Span = std::ops::Range<usize>;

/// A node with source location.
///
/// # Examples
///
/// ```
/// use turbo_ast::Spanned;
///
/// let spanned = Spanned::new(42, 0..2);
/// assert_eq!(spanned.node, 42);
/// assert_eq!(spanned.span, 0..2);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

/// A complete source file.
///
/// # Examples
///
/// ```
/// use turbo_ast::Module;
///
/// let module = Module { items: vec![] };
/// assert!(module.items.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Spanned<Item>>,
}

/// Top-level items
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(FnDef),
    Struct(StructDef),
    Enum(EnumDef),
    Impl(ImplBlock),
    Trait(TraitDef),
    /// Import declaration: `import { name1, name2 } from "path"`
    Import {
        names: Vec<String>,
        path: String,
    },
    /// Constant declaration: `const NAME = value`
    Const(ConstDef),
    /// Extern block: `@unsafe extern "C" { fn foo(...) -> T }`
    Extern(ExternBlock),
}

/// Extern function signature (no body — declared in an `extern "C" { ... }` block)
#[derive(Debug, Clone, PartialEq)]
pub struct ExternFnSig {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
}

/// An `extern "ABI" { ... }` block declaring foreign functions
#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    pub abi: String,
    pub functions: Vec<Spanned<ExternFnSig>>,
}

/// Constant definition: `const NAME: type = expr` or `const NAME = expr`
#[derive(Debug, Clone, PartialEq)]
pub struct ConstDef {
    pub name: String,
    pub ty: Option<Spanned<TypeExpr>>,
    pub value: Spanned<Expr>,
}

/// Impl block: methods attached to a struct type
#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub type_name: String,
    /// Generic type parameters: `impl Pair<T> { ... }`
    pub type_params: Vec<TypeParam>,
    /// If Some, this is a trait impl: `impl TraitName for TypeName { ... }`
    pub trait_name: Option<String>,
    pub methods: Vec<Spanned<FnDef>>,
}

/// Trait definition: `trait Name { fn method(self) -> Type }`
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDef {
    pub name: String,
    pub methods: Vec<TraitMethodSig>,
}

/// Trait method signature (optionally with a default body)
#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethodSig {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
    /// Optional default implementation body
    pub default_body: Option<Spanned<Expr>>,
}

/// Enum variant definition (may carry data)
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariantDef {
    pub name: String,
    pub fields: Vec<Spanned<TypeExpr>>, // empty for unit variants
}

/// Enum (sum type) definition
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<EnumVariantDef>,
    /// Doc comment text (from `///` comments preceding the enum)
    pub doc: Option<String>,
}

impl EnumDef {
    /// Get just the variant names (for backward compatibility)
    pub fn variant_names(&self) -> Vec<String> {
        self.variants.iter().map(|v| v.name.clone()).collect()
    }

    /// Get just the type parameter names
    pub fn type_param_names(&self) -> Vec<String> {
        self.type_params.iter().map(|tp| tp.name.clone()).collect()
    }
}

impl StructDef {
    /// Get just the type parameter names
    pub fn type_param_names(&self) -> Vec<String> {
        self.type_params.iter().map(|tp| tp.name.clone()).collect()
    }
}

/// Struct definition
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    /// Derived trait names from `@derive(Eq, Clone, ...)` attribute
    pub derives: Vec<String>,
    pub fields: Vec<FieldDef>,
    /// Doc comment text (from `///` comments preceding the struct)
    pub doc: Option<String>,
}

/// Struct field definition
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
}

/// A generic type parameter with optional trait bounds
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub bounds: Vec<String>,
}

impl TypeParam {
    pub fn new(name: String) -> Self {
        Self {
            name,
            bounds: Vec::new(),
        }
    }

    pub fn with_bounds(name: String, bounds: Vec<String>) -> Self {
        Self { name, bounds }
    }
}

/// Function definition
#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub is_async: bool,
    /// Whether this function is a `@test fn` (test function)
    pub is_test: bool,
    /// Whether this function is a `@bench fn` (benchmark function).
    /// Like `@test`, this is a marker: the function compiles as an ordinary
    /// function but is also recognized by `turbolang bench` for labeling.
    pub is_bench: bool,
    /// Whether this function is an `@unsafe fn` (can use raw pointer operations)
    pub is_unsafe: bool,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
    pub body: Spanned<Expr>,
    /// Doc comment text (from `///` comments preceding the function)
    pub doc: Option<String>,
}

impl FnDef {
    /// Get just the type parameter names (for codegen compatibility)
    pub fn type_param_names(&self) -> Vec<String> {
        self.type_params.iter().map(|tp| tp.name.clone()).collect()
    }
}

/// Function parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub span: Span,
    /// `true` if declared with `mut` prefix: `fn foo(mut x: T)`.
    /// Makes the parameter binding reassignable inside the function body.
    /// Closure parameters currently always have `mutable: false`.
    pub mutable: bool,
}

/// Type expressions
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Named type: `i32`, `f64`, `bool`, `str`, `()`
    Named(String),
    /// Unit type `()`
    Unit,
    /// Array type: `[T]`
    Array(Box<Spanned<TypeExpr>>),
    /// Function type: `fn(T, T) -> T`
    FnType {
        params: Vec<Spanned<TypeExpr>>,
        ret: Box<Spanned<TypeExpr>>,
    },
    /// Result type: `T ! E`
    Result {
        ok_type: Box<Spanned<TypeExpr>>,
        err_type: Box<Spanned<TypeExpr>>,
    },
    /// Optional type: `T?`
    Optional(Box<Spanned<TypeExpr>>),
    /// `Future<T>` — the result type of an async function / spawn
    Future(Box<Spanned<TypeExpr>>),
    /// `HashMap<K, V>` — a typed generic hash map. `K` (the key) is restricted
    /// by sema to `int` or `str`; `V` (the value) may be any type.
    HashMap(Box<Spanned<TypeExpr>>, Box<Spanned<TypeExpr>>),
    /// Inferred type -- placeholder that sema resolves from calling context
    Inferred,
}

/// Part of a string interpolation
#[derive(Debug, Clone, PartialEq)]
pub enum InterpolPart {
    /// Literal string segment
    Lit(String),
    /// Expression to be evaluated and converted to string
    Expr(Box<Spanned<Expr>>),
}

/// Expressions.
///
/// # Examples
///
/// ```
/// use turbo_ast::{Expr, Spanned};
///
/// let lit = Expr::IntLit(42);
/// let spanned = Spanned::new(lit, 0..2);
/// assert_eq!(spanned.node, Expr::IntLit(42));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer literal
    IntLit(i64),
    /// Float literal
    FloatLit(f64),
    /// String literal
    StringLit(String),
    /// Boolean literal
    BoolLit(bool),
    /// Unit value ()
    Unit,
    /// Variable reference
    Ident(String),
    /// Binary operation: left op right
    BinaryOp {
        left: Box<Spanned<Expr>>,
        op: BinOp,
        right: Box<Spanned<Expr>>,
    },
    /// Unary operation: op expr
    UnaryOp {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },
    /// Explicit type cast: `expr as Type`. Used for numeric conversions
    /// (int↔int widths, int↔float, signed↔unsigned). The result type is the
    /// target type written after `as`.
    Cast {
        expr: Box<Spanned<Expr>>,
        ty: Spanned<TypeExpr>,
    },
    /// Function call: callee(args...)
    Call {
        callee: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },
    /// If expression: if cond { then } else { else_ }
    If {
        condition: Box<Spanned<Expr>>,
        then_branch: Box<Spanned<Expr>>,
        else_branch: Option<Box<Spanned<Expr>>>,
    },
    /// Block expression: { stmts... expr? }
    Block {
        stmts: Vec<Spanned<Stmt>>,
        /// The final expression (return value of the block), if any
        tail_expr: Option<Box<Spanned<Expr>>>,
    },
    /// Assignment: name = value
    Assign {
        target: String,
        value: Box<Spanned<Expr>>,
    },
    /// Compound assignment: name += value, etc.
    CompoundAssign {
        target: String,
        op: BinOp,
        value: Box<Spanned<Expr>>,
    },
    /// Field assignment: obj.field = value
    FieldAssign {
        object: Box<Spanned<Expr>>,
        field: String,
        value: Box<Spanned<Expr>>,
    },
    /// Index assignment: `arr[index] = value`
    IndexAssign {
        object: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
        value: Box<Spanned<Expr>>,
    },
    /// While loop: while condition { body }
    While {
        condition: Box<Spanned<Expr>>,
        body: Box<Spanned<Expr>>,
    },
    /// For-in loop: for name in iterable { body }
    ForIn {
        var_name: String,
        iterable: Box<Spanned<Expr>>,
        body: Box<Spanned<Expr>>,
    },
    /// Range expression: start..end (exclusive)
    Range {
        start: Box<Spanned<Expr>>,
        end: Box<Spanned<Expr>>,
    },
    /// Array literal: [expr, expr, ...]
    ArrayLit(Vec<Spanned<Expr>>),
    /// Index expression: `expr[index]`
    Index {
        object: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
    /// Struct literal: Name { field: value, ... }
    StructLit {
        name: String,
        fields: Vec<(String, Spanned<Expr>)>,
    },
    /// Field access: expr.field
    FieldAccess {
        object: Box<Spanned<Expr>>,
        field: String,
    },
    /// Enum variant: EnumName.VariantName
    EnumVariant { enum_name: String, variant: String },
    /// Match expression
    Match {
        subject: Box<Spanned<Expr>>,
        arms: Vec<MatchArm>,
    },
    /// String interpolation: "text {expr} text"
    Interpolation(Vec<InterpolPart>),
    /// Closure / lambda: |params| -> ret { body }
    Closure {
        params: Vec<Param>,
        return_type: Option<Spanned<TypeExpr>>,
        body: Box<Spanned<Expr>>,
    },
    /// Ok constructor: ok(value)
    OkExpr(Box<Spanned<Expr>>),
    /// Err constructor: err(value)
    ErrExpr(Box<Spanned<Expr>>),
    /// Some constructor: some(value)
    SomeExpr(Box<Spanned<Expr>>),
    /// None literal
    NoneExpr,
    /// Null coalescing: expr ?? default
    NullCoalesce {
        value: Box<Spanned<Expr>>,
        default: Box<Spanned<Expr>>,
    },
    /// Optional chaining: expr?.field — unwraps optional, accesses field, re-wraps in optional
    OptionalChain {
        object: Box<Spanned<Expr>>,
        field: String,
    },
    /// Await expression: await expr
    Await(Box<Spanned<Expr>>),
    /// Spawn expression: spawn expr
    Spawn(Box<Spanned<Expr>>),
    /// Try operator: expr? — unwraps Ok, propagates Err
    Try(Box<Spanned<Expr>>),
    /// If-let pattern matching: `if let some(v) = expr { ... } else { ... }`
    IfLet {
        pattern: Box<Spanned<Pattern>>,
        value: Box<Spanned<Expr>>,
        then_branch: Box<Spanned<Expr>>,
        else_branch: Option<Box<Spanned<Expr>>>,
    },
    /// Break out of the innermost loop
    Break,
    /// Continue to the next iteration of the innermost loop
    Continue,
    /// Map literal: {"key": "value", ...}
    MapLit(Vec<(Spanned<Expr>, Spanned<Expr>)>),
}

/// Statements (things that don't produce values in statement position)
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Let binding: `let [mut] name [: type] = expr`
    Let {
        mutable: bool,
        name: String,
        ty: Option<Spanned<TypeExpr>>,
        value: Spanned<Expr>,
    },
    /// Expression used as a statement
    Expr(Spanned<Expr>),
    /// Return statement
    Return(Option<Spanned<Expr>>),
    /// Defer statement: `defer expr` — runs expr at end of enclosing block (LIFO)
    Defer(Spanned<Expr>),
    /// Struct destructuring: `let [mut] { field1, field2 } = struct_expr`
    LetDestructure {
        mutable: bool,
        fields: Vec<String>,
        value: Spanned<Expr>,
    },
}

/// A match arm: `pattern [if guard] => body`
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
    pub guard: Option<Spanned<Expr>>,
    pub body: Spanned<Expr>,
}

/// Pattern in a match expression
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Named pattern (variant name or variable binding)
    Ident(String),
    /// Wildcard pattern _
    Wildcard,
    /// Integer literal pattern
    IntLit(i64),
    /// Boolean literal pattern
    BoolLit(bool),
    /// String literal pattern
    StringLit(String),
    /// ok(binding) pattern
    Ok(String),
    /// err(binding) pattern
    Err(String),
    /// some(binding) pattern
    Some(String),
    /// none pattern
    None,
    /// Variant destructure pattern: VariantName(binding1, binding2, ...)
    VariantDestructure {
        variant: String,
        bindings: Vec<String>,
    },
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    And,
    Or,
}

impl BinOp {
    /// Operator precedence (higher = binds tighter)
    pub fn precedence(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::NotEq => 3,
            BinOp::Less | BinOp::LessEq | BinOp::Greater | BinOp::GreaterEq => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 6,
        }
    }
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}
