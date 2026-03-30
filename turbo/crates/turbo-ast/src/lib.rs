/// A span in source code: byte offset range [start, end)
pub type Span = std::ops::Range<usize>;

/// A node with source location
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

/// A complete source file
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
    Agent(AgentDef),
    /// Import declaration: `import { name1, name2 } from "path"`
    Import {
        names: Vec<String>,
        path: String,
    },
}

/// Agent definition: a struct-like declaration for AI agents
#[derive(Debug, Clone, PartialEq)]
pub struct AgentDef {
    pub name: String,
    pub model: String,
    pub tools: Vec<String>,
    pub system_prompt: Option<String>,
}

/// Impl block: methods attached to a struct type
#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub type_name: String,
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

/// Trait method signature (no body)
#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethodSig {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
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
    pub variants: Vec<EnumVariantDef>,
}

impl EnumDef {
    /// Get just the variant names (for backward compatibility)
    pub fn variant_names(&self) -> Vec<String> {
        self.variants.iter().map(|v| v.name.clone()).collect()
    }
}

/// Struct definition
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
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
        Self { name, bounds: Vec::new() }
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
    /// Whether this function is a `tool fn` (AI tool with auto-generated schema)
    pub is_tool: bool,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
    pub body: Spanned<Expr>,
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
}

/// Type expressions
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Named type: i32, f64, bool, str, ()
    Named(String),
    /// Unit type ()
    Unit,
    /// Array type: [T]
    Array(Box<Spanned<TypeExpr>>),
    /// Function type: fn(T, T) -> T
    FnType {
        params: Vec<Spanned<TypeExpr>>,
        ret: Box<Spanned<TypeExpr>>,
    },
    /// Result type: T ! E
    Result {
        ok_type: Box<Spanned<TypeExpr>>,
        err_type: Box<Spanned<TypeExpr>>,
    },
    /// Optional type: T?
    Optional(Box<Spanned<TypeExpr>>),
    /// Future<T> — the result type of an async function / spawn
    Future(Box<Spanned<TypeExpr>>),
}

/// Part of a string interpolation
#[derive(Debug, Clone, PartialEq)]
pub enum InterpolPart {
    /// Literal string segment
    Lit(String),
    /// Expression to be evaluated and converted to string
    Expr(Box<Spanned<Expr>>),
}

/// Expressions
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
    /// Index assignment: arr[index] = value
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
    /// Index expression: expr[index]
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
    EnumVariant {
        enum_name: String,
        variant: String,
    },
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
    /// Await expression: await expr
    Await(Box<Spanned<Expr>>),
    /// Spawn expression: spawn expr
    Spawn(Box<Spanned<Expr>>),
}

/// Statements (things that don't produce values in statement position)
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Let binding: let [mut] name [: type] = expr
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
}

/// A match arm: pattern => body
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
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
