use std::collections::HashMap;
use turbo_ast::*;

#[derive(Debug, Clone)]
pub struct SemaError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for SemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Internal type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    I32,
    I64,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Str,
    Unit,
    /// Type could not be determined (error recovery)
    Error,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::I32 => write!(f, "i32"),
            Ty::I64 => write!(f, "i64"),
            Ty::U32 => write!(f, "u32"),
            Ty::U64 => write!(f, "u64"),
            Ty::F32 => write!(f, "f32"),
            Ty::F64 => write!(f, "f64"),
            Ty::Bool => write!(f, "bool"),
            Ty::Str => write!(f, "str"),
            Ty::Unit => write!(f, "()"),
            Ty::Error => write!(f, "<error>"),
        }
    }
}

impl Ty {
    fn is_integer(&self) -> bool {
        matches!(self, Ty::I32 | Ty::I64 | Ty::U32 | Ty::U64)
    }

    fn is_float(&self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }

    fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }
}

fn resolve_type_expr(te: &TypeExpr) -> Option<Ty> {
    match te {
        TypeExpr::Named(name) => match name.as_str() {
            "i32" => Some(Ty::I32),
            "i64" => Some(Ty::I64),
            "u32" => Some(Ty::U32),
            "u64" => Some(Ty::U64),
            "f32" => Some(Ty::F32),
            "f64" => Some(Ty::F64),
            "bool" => Some(Ty::Bool),
            "str" => Some(Ty::Str),
            _ => None,
        },
        TypeExpr::Unit => Some(Ty::Unit),
    }
}

/// Variable info in scope
#[derive(Debug, Clone)]
struct VarInfo {
    ty: Ty,
    mutable: bool,
}

/// Function signature
#[derive(Debug, Clone)]
struct FnSig {
    params: Vec<(String, Ty)>,
    ret: Ty,
    is_tool: bool,
}

/// Scope for variable tracking
struct Scope {
    vars: HashMap<String, VarInfo>,
}

/// Registered agent info for semantic checking
#[derive(Debug, Clone)]
struct AgentInfo {
    model: String,
    tools: Vec<String>,
    system_prompt: Option<String>,
}

/// Type checker
struct Checker {
    errors: Vec<SemaError>,
    functions: HashMap<String, FnSig>,
    agents: HashMap<String, AgentInfo>,
    scopes: Vec<Scope>,
    /// Return type of the current function being checked
    current_return_type: Ty,
}

impl Checker {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            functions: HashMap::new(),
            agents: HashMap::new(),
            scopes: Vec::new(),
            current_return_type: Ty::Unit,
        }
    }

    fn error(&mut self, message: String, span: Span) {
        self.errors.push(SemaError { message, span });
    }

    // === Scope management ===

    fn push_scope(&mut self) {
        self.scopes.push(Scope { vars: HashMap::new() });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_var(&mut self, name: &str, info: VarInfo, _span: &Span) {
        // Check current scope for redefinition (shadowing is OK across scopes)
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name.to_string(), info);
        }
    }

    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.vars.get(name) {
                return Some(info);
            }
        }
        None
    }

    // === Check module ===

    fn check_module(&mut self, module: &Module) {
        // First pass: register all function signatures and agent declarations
        for item in &module.items {
            match &item.node {
                Item::Function(f) => {
                    if self.functions.contains_key(&f.name) {
                        self.error(
                            format!("function `{}` is already defined", f.name),
                            item.span.clone(),
                        );
                        continue;
                    }

                    let mut params = Vec::new();
                    for param in &f.params {
                        match resolve_type_expr(&param.ty.node) {
                            Some(ty) => params.push((param.name.clone(), ty)),
                            None => {
                                if let TypeExpr::Named(name) = &param.ty.node {
                                    self.error(
                                        format!("unknown type `{name}`"),
                                        param.ty.span.clone(),
                                    );
                                }
                                params.push((param.name.clone(), Ty::Error));
                            }
                        }
                    }

                    let ret = if let Some(ret_type) = &f.return_type {
                        match resolve_type_expr(&ret_type.node) {
                            Some(ty) => ty,
                            None => {
                                if let TypeExpr::Named(name) = &ret_type.node {
                                    self.error(
                                        format!("unknown return type `{name}`"),
                                        ret_type.span.clone(),
                                    );
                                }
                                Ty::Error
                            }
                        }
                    } else {
                        Ty::Unit
                    };

                    self.functions.insert(f.name.clone(), FnSig { params, ret, is_tool: f.is_tool });
                }
                Item::Agent(agent) => {
                    if self.agents.contains_key(&agent.name) {
                        self.error(
                            format!("agent `{}` is already defined", agent.name),
                            item.span.clone(),
                        );
                        continue;
                    }
                    self.agents.insert(agent.name.clone(), AgentInfo {
                        model: agent.model.clone(),
                        tools: agent.tools.clone(),
                        system_prompt: agent.system_prompt.clone(),
                    });
                }
            }
        }

        // Validate agent tool references point to actual tool functions
        for item in &module.items {
            if let Item::Agent(agent) = &item.node {
                for tool_name in &agent.tools {
                    match self.functions.get(tool_name) {
                        Some(sig) => {
                            if !sig.is_tool {
                                self.error(
                                    format!(
                                        "function `{tool_name}` in agent `{}` is not a `tool fn`",
                                        agent.name
                                    ),
                                    item.span.clone(),
                                );
                            }
                        }
                        None => {
                            self.error(
                                format!(
                                    "undefined tool function `{tool_name}` in agent `{}`",
                                    agent.name
                                ),
                                item.span.clone(),
                            );
                        }
                    }
                }
            }
        }

        // Check for main
        if !self.functions.contains_key("main") {
            let span = if module.items.is_empty() {
                0..0
            } else {
                module.items.last().unwrap().span.clone()
            };
            self.error("no `main` function found".to_string(), span);
        }

        // Second pass: check function bodies
        for item in &module.items {
            if let Item::Function(f) = &item.node {
                self.check_function(f);
            }
        }
    }

    fn check_function(&mut self, f: &FnDef) {
        let sig = self.functions.get(&f.name).cloned().unwrap();
        self.current_return_type = sig.ret.clone();

        self.push_scope();

        // Define parameters
        for (name, ty) in &sig.params {
            self.define_var(
                name,
                VarInfo { ty: ty.clone(), mutable: false },
                &(0..0),
            );
        }

        // Check body
        let body_ty = self.check_expr(&f.body);

        // Check return type matches
        if !sig.ret.is_error() && !body_ty.is_error() && sig.ret != Ty::Unit && body_ty != sig.ret {
            self.error(
                format!(
                    "function `{}` should return `{}` but body returns `{}`",
                    f.name, sig.ret, body_ty
                ),
                f.body.span.clone(),
            );
        }

        self.pop_scope();
    }

    // === Expression type checking ===

    fn check_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        match &expr.node {
            Expr::IntLit(_) => Ty::I64,
            Expr::FloatLit(_) => Ty::F64,
            Expr::BoolLit(_) => Ty::Bool,
            Expr::StringLit(_) => Ty::Str,
            Expr::Unit => Ty::Unit,

            Expr::Ident(name) => {
                if let Some(info) = self.lookup_var(name) {
                    info.ty.clone()
                } else {
                    self.error(
                        format!("undefined variable `{name}`"),
                        expr.span.clone(),
                    );
                    Ty::Error
                }
            }

            Expr::BinaryOp { left, op, right } => {
                let lhs = self.check_expr(left);
                let rhs = self.check_expr(right);

                if lhs.is_error() || rhs.is_error() {
                    return Ty::Error;
                }

                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        if !lhs.is_numeric() {
                            self.error(
                                format!("cannot perform arithmetic on `{lhs}`"),
                                left.span.clone(),
                            );
                            return Ty::Error;
                        }
                        if lhs != rhs {
                            self.error(
                                format!("mismatched types in arithmetic: `{lhs}` and `{rhs}`"),
                                expr.span.clone(),
                            );
                            return Ty::Error;
                        }
                        lhs
                    }
                    BinOp::Eq | BinOp::NotEq | BinOp::Less | BinOp::LessEq | BinOp::Greater | BinOp::GreaterEq => {
                        if lhs != rhs {
                            self.error(
                                format!("cannot compare `{lhs}` with `{rhs}`"),
                                expr.span.clone(),
                            );
                            return Ty::Error;
                        }
                        Ty::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        if lhs != Ty::Bool {
                            self.error(
                                format!("expected `bool` in logical operation, found `{lhs}`"),
                                left.span.clone(),
                            );
                        }
                        if rhs != Ty::Bool {
                            self.error(
                                format!("expected `bool` in logical operation, found `{rhs}`"),
                                right.span.clone(),
                            );
                        }
                        Ty::Bool
                    }
                }
            }

            Expr::UnaryOp { op, expr: inner } => {
                let ty = self.check_expr(inner);
                if ty.is_error() {
                    return Ty::Error;
                }
                match op {
                    UnaryOp::Neg => {
                        if !ty.is_numeric() {
                            self.error(
                                format!("cannot negate `{ty}`"),
                                inner.span.clone(),
                            );
                            Ty::Error
                        } else {
                            ty
                        }
                    }
                    UnaryOp::Not => {
                        if ty != Ty::Bool {
                            self.error(
                                format!("cannot apply `!` to `{ty}`, expected `bool`"),
                                inner.span.clone(),
                            );
                            Ty::Error
                        } else {
                            Ty::Bool
                        }
                    }
                }
            }

            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    // Built-in functions
                    if name == "print" || name == "panic" {
                        for arg in args {
                            self.check_expr(arg);
                        }
                        return Ty::Unit;
                    }
                    if name == "assert" {
                        if args.is_empty() {
                            self.error(
                                "assert() requires at least one argument".to_string(),
                                callee.span.clone(),
                            );
                        } else {
                            let cond_ty = self.check_expr(&args[0]);
                            if !cond_ty.is_error() && cond_ty != Ty::Bool {
                                self.error(
                                    format!("assert() condition must be `bool`, found `{cond_ty}`"),
                                    args[0].span.clone(),
                                );
                            }
                            // Optional message argument
                            if args.len() > 1 {
                                self.check_expr(&args[1]);
                            }
                        }
                        return Ty::Unit;
                    }

                    // User-defined function
                    if let Some(sig) = self.functions.get(name).cloned() {
                        if args.len() != sig.params.len() {
                            self.error(
                                format!(
                                    "function `{name}` expects {} argument(s) but {} were given",
                                    sig.params.len(),
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return sig.ret;
                        }

                        for (i, arg) in args.iter().enumerate() {
                            let arg_ty = self.check_expr(arg);
                            let (ref param_name, ref param_ty) = &sig.params[i];
                            if !arg_ty.is_error() && !param_ty.is_error() && arg_ty != *param_ty {
                                self.error(
                                    format!(
                                        "argument `{param_name}` expects `{param_ty}`, found `{arg_ty}`"
                                    ),
                                    arg.span.clone(),
                                );
                            }
                        }

                        sig.ret
                    } else {
                        self.error(
                            format!("undefined function `{name}`"),
                            callee.span.clone(),
                        );
                        Ty::Error
                    }
                } else {
                    self.error(
                        "only named function calls are supported".to_string(),
                        callee.span.clone(),
                    );
                    Ty::Error
                }
            }

            Expr::If { condition, then_branch, else_branch } => {
                let cond_ty = self.check_expr(condition);
                if !cond_ty.is_error() && cond_ty != Ty::Bool {
                    // Allow integer conditions (truthy)
                    if !cond_ty.is_integer() {
                        self.error(
                            format!("if condition must be `bool`, found `{cond_ty}`"),
                            condition.span.clone(),
                        );
                    }
                }

                let then_ty = self.check_expr(then_branch);

                if let Some(else_expr) = else_branch {
                    let else_ty = self.check_expr(else_expr);
                    // If used as expression (both branches must match)
                    if !then_ty.is_error() && !else_ty.is_error() && then_ty != else_ty {
                        // Only warn if both are non-unit (meaning it's used as an expression)
                        if then_ty != Ty::Unit && else_ty != Ty::Unit {
                            self.error(
                                format!(
                                    "if/else branches have different types: `{then_ty}` and `{else_ty}`"
                                ),
                                expr.span.clone(),
                            );
                        }
                    }
                    then_ty
                } else {
                    Ty::Unit
                }
            }

            Expr::Block { stmts, tail_expr } => {
                self.push_scope();

                for stmt in stmts {
                    self.check_stmt(stmt);
                }

                let ty = if let Some(tail) = tail_expr {
                    self.check_expr(tail)
                } else {
                    Ty::Unit
                };

                self.pop_scope();
                ty
            }

            Expr::Assign { target, value } => {
                let val_ty = self.check_expr(value);

                if let Some(info) = self.lookup_var(target).cloned() {
                    if !info.mutable {
                        self.error(
                            format!("cannot assign to immutable variable `{target}` (declare with `let mut` to make mutable)"),
                            expr.span.clone(),
                        );
                    }
                    if !val_ty.is_error() && !info.ty.is_error() && val_ty != info.ty {
                        self.error(
                            format!(
                                "cannot assign `{val_ty}` to variable `{target}` of type `{}`",
                                info.ty
                            ),
                            value.span.clone(),
                        );
                    }
                } else {
                    self.error(
                        format!("undefined variable `{target}`"),
                        expr.span.clone(),
                    );
                }

                Ty::Unit
            }

            Expr::CompoundAssign { target, op, value } => {
                let val_ty = self.check_expr(value);

                if let Some(info) = self.lookup_var(target).cloned() {
                    if !info.mutable {
                        self.error(
                            format!("cannot assign to immutable variable `{target}` (declare with `let mut` to make mutable)"),
                            expr.span.clone(),
                        );
                    }
                    if !val_ty.is_error() && !info.ty.is_error() && val_ty != info.ty {
                        self.error(
                            format!(
                                "cannot apply `{op:?}=` with `{val_ty}` to variable `{target}` of type `{}`",
                                info.ty
                            ),
                            value.span.clone(),
                        );
                    }
                    if !info.ty.is_numeric() && !info.ty.is_error() {
                        self.error(
                            format!("cannot perform arithmetic on `{}`", info.ty),
                            expr.span.clone(),
                        );
                    }
                } else {
                    self.error(
                        format!("undefined variable `{target}`"),
                        expr.span.clone(),
                    );
                }

                Ty::Unit
            }

            Expr::While { condition, body } => {
                let cond_ty = self.check_expr(condition);
                if !cond_ty.is_error() && cond_ty != Ty::Bool {
                    if !cond_ty.is_integer() {
                        self.error(
                            format!("while condition must be `bool`, found `{cond_ty}`"),
                            condition.span.clone(),
                        );
                    }
                }
                self.check_expr(body);
                Ty::Unit
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Let { mutable, name, ty, value } => {
                let val_ty = self.check_expr(value);

                let declared_ty = if let Some(ty_expr) = ty {
                    match resolve_type_expr(&ty_expr.node) {
                        Some(t) => {
                            if !val_ty.is_error() && t != val_ty {
                                self.error(
                                    format!(
                                        "type annotation `{t}` doesn't match value type `{val_ty}`"
                                    ),
                                    ty_expr.span.clone(),
                                );
                            }
                            t
                        }
                        None => {
                            if let TypeExpr::Named(name) = &ty_expr.node {
                                self.error(
                                    format!("unknown type `{name}`"),
                                    ty_expr.span.clone(),
                                );
                            }
                            val_ty.clone()
                        }
                    }
                } else {
                    val_ty
                };

                self.define_var(
                    name,
                    VarInfo { ty: declared_ty, mutable: *mutable },
                    &stmt.span,
                );
            }
            Stmt::Expr(e) => {
                self.check_expr(e);
            }
            Stmt::Return(value) => {
                let ret_ty = if let Some(val) = value {
                    self.check_expr(val)
                } else {
                    Ty::Unit
                };

                if !ret_ty.is_error() && !self.current_return_type.is_error()
                    && self.current_return_type != Ty::Unit && ret_ty != self.current_return_type
                {
                    self.error(
                        format!(
                            "return type `{ret_ty}` doesn't match function return type `{}`",
                            self.current_return_type
                        ),
                        stmt.span.clone(),
                    );
                }
            }
        }
    }
}

/// Run semantic analysis on a module. Returns errors found.
pub fn check(module: &Module) -> Vec<SemaError> {
    let mut checker = Checker::new();
    checker.check_module(module);
    checker.errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(source: &str) -> Vec<SemaError> {
        let (tokens, _) = turbo_lexer::tokenize(source);
        let (module, _) = turbo_parser::parse(tokens);
        check(&module)
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
    fn test_string_arithmetic() {
        assert_has_error(
            r#"fn main() { let x = "a" + "b" }"#,
            "cannot perform arithmetic on `str`",
        );
    }

    #[test]
    fn test_undefined_variable() {
        assert_has_error(
            "fn main() { print(x) }",
            "undefined variable `x`",
        );
    }

    #[test]
    fn test_immutable_assignment() {
        assert_has_error(
            "fn main() { let x = 1\n x = 2 }",
            "cannot assign to immutable variable `x`",
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
            "argument `x` expects `i64`, found `str`",
        );
    }

    #[test]
    fn test_return_type_mismatch() {
        assert_has_error(
            r#"fn foo() -> i64 { "hello" }"#,
            "should return `i64` but body returns `str`",
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
        assert_has_error(
            "fn foo() { }",
            "no `main` function found",
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
            "argument `q` expects `str`, found `i64`",
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
}
