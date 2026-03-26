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

/// Function definition
#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
    pub body: Spanned<Expr>,
}

/// Function parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub span: Span,
}

/// Type expressions (Phase 1: just basic types)
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Named type: i32, f64, bool, str, ()
    Named(String),
    /// Unit type ()
    Unit,
    /// Array type: [T]
    Array(Box<Spanned<TypeExpr>>),
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
