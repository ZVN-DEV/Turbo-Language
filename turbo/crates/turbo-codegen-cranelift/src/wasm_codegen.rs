//! C code generator for WASM compilation.
//!
//! Translates the Turbo AST into C source code, which is then compiled
//! with clang targeting wasm32-wasi. This avoids the limitation that
//! Cranelift does not support wasm32 as a code generation target.
//!
//! The generated C code links against turbo_rt_wasm.c which provides
//! the runtime functions (print, string ops, array ops, etc.).

use crate::closures::find_captures;
use crate::CodegenError;
use std::collections::HashMap;
use std::fmt::Write;
use turbo_ast::*;

/// The C-level signature of a closure: the C types of its user parameters
/// (the hidden leading `void *env` parameter is implicit) and its C return
/// type. Used to emit the correct function-pointer cast at indirect call
/// sites and when a closure is handed to `map`/`filter`.
#[derive(Clone)]
struct ClosureSig {
    /// C type of each user parameter, in order (e.g. `["long long"]`).
    params: Vec<&'static str>,
    /// C return type (e.g. `"long long"`, `"const char*"`, `"char"`).
    ret: &'static str,
}

/// Tracks state during C code generation.
struct CEmitter {
    /// Current indentation level.
    indent: usize,
    /// Counter for generating unique temporary variable names.
    tmp_counter: usize,
    /// Map from struct name to field names (in order).
    struct_fields: HashMap<String, Vec<(String, TypeExpr)>>,
    /// Forward declarations for user functions.
    fn_decls: Vec<String>,
    /// User function definitions (bodies).
    fn_defs: Vec<String>,
    /// Impl method mapping: "TypeName_methodName" -> FnDef
    impl_methods: HashMap<String, Vec<(String, FnDef)>>,
    /// Enum variant info: enum_name -> vec of variant names
    enum_variants: HashMap<String, Vec<String>>,
    /// Variable type tracking: variable name -> simplified type tag
    /// ("str", "int", "float", "bool", "array", "struct", "void*")
    var_types: HashMap<String, String>,
    /// Function return type tracking: function name -> simplified type tag
    fn_return_types: HashMap<String, String>,
    /// Per-scope stack of deferred expressions. Each entry is a scope;
    /// the inner `Vec<Expr>` holds the deferred expressions in the order
    /// they were encountered (they will be emitted in LIFO order at scope
    /// exit, matching the Cranelift backend's semantics).
    defer_stack: Vec<Vec<Expr>>,
    /// Unsupported-construct errors collected during emission. The emitter
    /// keeps producing (syntactically valid) placeholder C so emission does
    /// not panic, but `generate_c` returns the first error instead of the C
    /// so an unsupported program fails to compile rather than miscompiling.
    errors: Vec<CodegenError>,
    /// Monotonic counter for naming lifted closure functions (`__closure_N`).
    closure_counter: usize,
    /// Variable name -> closure signature, for every local bound to a closure
    /// value. Lets a call site recognize `f(args)` as an indirect closure
    /// call (rather than a direct user-function call) and emit the right cast.
    closure_sigs: HashMap<String, ClosureSig>,
}

impl CEmitter {
    fn new() -> Self {
        Self {
            indent: 0,
            tmp_counter: 0,
            struct_fields: HashMap::new(),
            fn_decls: Vec::new(),
            fn_defs: Vec::new(),
            impl_methods: HashMap::new(),
            enum_variants: HashMap::new(),
            var_types: HashMap::new(),
            fn_return_types: HashMap::new(),
            defer_stack: Vec::new(),
            errors: Vec::new(),
            closure_counter: 0,
            closure_sigs: HashMap::new(),
        }
    }

    /// Record an unsupported-construct compile error. `what` names the
    /// construct; `span` (when known) is rendered as a byte range so the CLI
    /// can point near the offending source. The emitter still returns a
    /// placeholder so the C string stays well-formed for the rest of the pass.
    fn record_unsupported(&mut self, what: &str, span: Option<&Span>) {
        let loc = match span {
            Some(s) => format!(" (bytes {}..{})", s.start, s.end),
            None => String::new(),
        };
        self.errors.push(CodegenError {
            code: ErrorCode::E0403,
            message: format!("WASM backend does not support {what}{loc}"),
        });
    }

    /// Short, stable name for an `Expr` variant, used in unsupported-construct
    /// diagnostics so the message identifies the offending construct.
    fn expr_kind_name(expr: &Expr) -> &'static str {
        match expr {
            Expr::Spawn(_) => "spawn expressions",
            Expr::Await(_) => "await expressions",
            Expr::Cast { .. } => "cast (`as`) expressions",
            Expr::Closure { .. } => "closure expressions",
            // NB: `Expr::Match` is fully handled in `emit_expr` (compiled to a
            // nested ternary), so it never reaches this fallback path.
            _ => "this expression",
        }
    }

    /// Push a new defer scope. Call at the start of every block that may
    /// contain `defer` statements.
    fn push_defer_scope(&mut self) {
        self.defer_stack.push(Vec::new());
    }

    /// Pop the current defer scope, returning its deferred expressions
    /// (in LIFO/reverse order — ready to emit at scope exit).
    fn pop_defer_scope(&mut self) -> Vec<Expr> {
        let mut scope = self.defer_stack.pop().unwrap_or_default();
        scope.reverse();
        scope
    }

    /// Emit every currently-live deferred expression (across all active
    /// scopes, innermost first, LIFO within each scope) into `buf`. Used
    /// to run all pending cleanup before an early `return`.
    fn emit_all_deferred_for_return(&mut self, buf: &mut String) {
        // Clone the active scopes so we don't mutate the stack itself —
        // the block they live in still owns them and will emit on normal exit.
        let scopes: Vec<Vec<Expr>> = self.defer_stack.iter().rev().cloned().collect();
        for scope in scopes {
            for expr in scope.into_iter().rev() {
                let e = self.emit_expr(&expr);
                writeln!(buf, "{}{e};", self.indent_str()).unwrap();
            }
        }
    }

    /// Emit the deferred expressions returned by `pop_defer_scope` into
    /// `buf` (they arrive already in LIFO order).
    fn emit_popped_deferred(&mut self, buf: &mut String, deferred: Vec<Expr>) {
        for expr in deferred {
            let e = self.emit_expr(&expr);
            writeln!(buf, "{}{e};", self.indent_str()).unwrap();
        }
    }

    fn fresh_tmp(&mut self) -> String {
        let n = self.tmp_counter;
        self.tmp_counter += 1;
        format!("_t{n}")
    }

    fn indent_str(&self) -> String {
        "    ".repeat(self.indent)
    }

    /// Escape a string for C source code.
    fn escape_c_string(s: &str) -> String {
        let mut result = String::with_capacity(s.len() + 2);
        result.push('"');
        for c in s.chars() {
            match c {
                '\\' => result.push_str("\\\\"),
                '"' => result.push_str("\\\""),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\0' => result.push_str("\\0"),
                c => result.push(c),
            }
        }
        result.push('"');
        result
    }

    /// Map a Turbo TypeExpr to a simplified type tag for variable tracking.
    fn type_expr_to_tag(ty: &TypeExpr) -> String {
        match ty {
            TypeExpr::Named(name) => match name.as_str() {
                "i32" | "i64" | "u32" | "u64" | "int" => "int".to_string(),
                "f32" | "f64" | "float" => "float".to_string(),
                "bool" => "bool".to_string(),
                "str" | "string" | "String" => "str".to_string(),
                _ => "struct".to_string(), // user-defined types
            },
            TypeExpr::Unit => "void".to_string(),
            TypeExpr::Array(_) => "array".to_string(),
            TypeExpr::Optional(_) => "void*".to_string(),
            TypeExpr::Result { .. } => "void*".to_string(),
            _ => "int".to_string(),
        }
    }

    /// Infer a simplified type tag from an expression (best-effort).
    fn infer_type_tag(&self, expr: &Expr) -> String {
        match expr {
            Expr::IntLit(_) => "int".to_string(),
            Expr::FloatLit(_) => "float".to_string(),
            Expr::StringLit(_) | Expr::Interpolation(_) => "str".to_string(),
            Expr::BoolLit(_) => "bool".to_string(),
            Expr::ArrayLit(_) => "array".to_string(),
            Expr::StructLit { .. } => "struct".to_string(),
            Expr::MapLit(_) => "void*".to_string(),
            Expr::Ident(name) => {
                if let Some(tag) = self.var_types.get(name) {
                    tag.clone()
                } else {
                    "int".to_string()
                }
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = &callee.node {
                    // A local bound to a closure: tag follows its return type.
                    if let Some(sig) = self.closure_sigs.get(name) {
                        return Self::c_type_to_tag(sig.ret).to_string();
                    }
                    // Check user-defined function return types first
                    if let Some(tag) = self.fn_return_types.get(name.as_str()) {
                        return tag.clone();
                    }
                    match name.as_str() {
                        "str" | "to_string" | "to_str" | "str_concat" | "str_upper"
                        | "str_lower" | "str_trim" | "str_replace" | "str_char_at"
                        | "str_repeat" | "str_join" | "read_line" | "read_file"
                        | "rt_i64_to_str" | "rt_f64_to_str" | "rt_bool_to_str"
                        | "rt_str_concat" | "json_get" | "json_stringify" | "http_get"
                        | "http_post" => "str".to_string(),
                        "len" | "str_len" | "str_index_of" | "pow" | "hashmap_len" => {
                            "int".to_string()
                        }
                        "sqrt" => "float".to_string(),
                        "str_contains" | "str_starts_with" | "str_ends_with" | "hashmap_has"
                        | "str_eq" => "bool".to_string(),
                        "hashmap_new" | "hashmap_keys" | "str_split" | "array_alloc" | "map"
                        | "filter" => "void*".to_string(),
                        _ => "int".to_string(),
                    }
                } else {
                    "int".to_string()
                }
            }
            Expr::Closure { .. } => "closure".to_string(),
            Expr::BinaryOp { left, op, .. } => {
                match op {
                    BinOp::Eq
                    | BinOp::NotEq
                    | BinOp::Less
                    | BinOp::LessEq
                    | BinOp::Greater
                    | BinOp::GreaterEq
                    | BinOp::And
                    | BinOp::Or => "bool".to_string(),
                    BinOp::Add => {
                        // String concatenation produces string
                        let left_tag = self.infer_type_tag(&left.node);
                        if left_tag == "str" {
                            "str".to_string()
                        } else if left_tag == "float" {
                            "float".to_string()
                        } else {
                            "int".to_string()
                        }
                    }
                    BinOp::Div => {
                        let left_tag = self.infer_type_tag(&left.node);
                        if left_tag == "float" {
                            "float".to_string()
                        } else {
                            "int".to_string()
                        }
                    }
                    _ => {
                        let left_tag = self.infer_type_tag(&left.node);
                        if left_tag == "float" {
                            "float".to_string()
                        } else {
                            "int".to_string()
                        }
                    }
                }
            }
            Expr::If { then_branch, .. } => self.infer_type_tag(&then_branch.node),
            Expr::Block { tail_expr, .. } => {
                if let Some(tail) = tail_expr {
                    self.infer_type_tag(&tail.node)
                } else {
                    "void".to_string()
                }
            }
            Expr::OkExpr(_) | Expr::ErrExpr(_) => "void*".to_string(),
            Expr::SomeExpr(_) | Expr::NoneExpr => "void*".to_string(),
            _ => "int".to_string(),
        }
    }

    /// Determine the right rt_print_* variant based on a type tag.
    fn print_fn_for_tag(tag: &str) -> &'static str {
        match tag {
            "str" => "rt_print_str",
            "float" | "f64" | "f32" => "rt_print_f64",
            "bool" => "rt_print_bool",
            _ => "rt_print_i64",
        }
    }

    /// Convert an expression to a string representation for interpolation,
    /// choosing the right rt_*_to_str based on the inferred type.
    fn expr_to_str_conversion(&self, expr: &Expr, inner_c: &str) -> String {
        let tag = self.infer_type_tag(expr);
        match tag.as_str() {
            "str" => inner_c.to_string(),
            "float" | "f64" | "f32" => format!("rt_f64_to_str({inner_c})"),
            "bool" => format!("rt_bool_to_str({inner_c})"),
            _ => format!("rt_i64_to_str({inner_c})"),
        }
    }

    /// Map a Turbo TypeExpr to a C type string.
    fn type_to_c(ty: &TypeExpr) -> &'static str {
        match ty {
            TypeExpr::Named(name) => match name.as_str() {
                "i32" | "i64" | "u32" | "u64" | "int" => "long long",
                "f32" | "f64" | "float" => "double",
                "bool" => "char",
                "str" | "string" | "String" => "const char*",
                _ => "long long", // structs, enums treated as opaque pointers (i64)
            },
            TypeExpr::Unit => "void",
            TypeExpr::Array(_) => "void*",
            TypeExpr::Optional(_) => "void*",
            TypeExpr::Result { .. } => "void*",
            _ => "long long",
        }
    }

    /// Get the C return type for a function, defaulting to void.
    fn return_type_to_c(ret: &Option<Spanned<TypeExpr>>) -> &'static str {
        match ret {
            Some(t) => Self::type_to_c(&t.node),
            None => "void",
        }
    }

    /// Emit a C expression from a Turbo Expr, returning the C code as a String.
    fn emit_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::IntLit(n) => format!("{n}LL"),
            Expr::FloatLit(f) => {
                if f.fract() == 0.0 {
                    format!("{f}.0")
                } else {
                    format!("{f}")
                }
            }
            Expr::StringLit(s) => Self::escape_c_string(s),
            Expr::BoolLit(b) => {
                if *b {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            }
            Expr::Unit => "0".to_string(),
            Expr::Ident(name) => {
                // Map Turbo identifiers to C, handling reserved words
                match name.as_str() {
                    "true" => "1".to_string(),
                    "false" => "0".to_string(),
                    _ => name.clone(),
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let l = self.emit_expr(&left.node);
                let r = self.emit_expr(&right.node);
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => "%",
                    BinOp::Eq => "==",
                    BinOp::NotEq => "!=",
                    BinOp::Less => "<",
                    BinOp::LessEq => "<=",
                    BinOp::Greater => ">",
                    BinOp::GreaterEq => ">=",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                };
                format!("({l} {op_str} {r})")
            }
            Expr::UnaryOp { op, expr } => {
                let e = self.emit_expr(&expr.node);
                match op {
                    UnaryOp::Neg => format!("(-{e})"),
                    UnaryOp::Not => format!("(!{e})"),
                }
            }
            Expr::Call { callee, args } => self.emit_call(callee, args),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                // For expressions in value position, use ternary if simple,
                // otherwise use a block with a temp var.
                let cond = self.emit_expr(&condition.node);
                let then_c = self.emit_expr(&then_branch.node);
                if let Some(else_b) = else_branch {
                    let else_c = self.emit_expr(&else_b.node);
                    format!("({cond} ? {then_c} : {else_c})")
                } else {
                    // If without else in expression position -- produce 0 for else
                    format!("({cond} ? {then_c} : 0)")
                }
            }
            Expr::Block { stmts, tail_expr } => {
                // Use GCC statement expression extension: ({ stmt; stmt; expr; })
                self.push_defer_scope();
                let mut parts = Vec::new();
                for stmt in stmts {
                    let s = self.emit_stmt_to_string(&stmt.node);
                    if !s.is_empty() {
                        parts.push(s);
                    }
                }
                // For a block in value position, the deferred expressions
                // must run *after* the tail expression is evaluated but
                // *before* the statement-expression yields its value. Stash
                // the tail into a temp, run defers, then yield the temp.
                let deferred = self.pop_defer_scope();
                if let Some(tail) = tail_expr {
                    let e = self.emit_expr(&tail.node);
                    if deferred.is_empty() {
                        parts.push(format!("{e};"));
                    } else {
                        let tmp = self.fresh_tmp();
                        let c_type = self.infer_c_type(&tail.node);
                        parts.push(format!("{c_type} {tmp} = ({c_type})({e});"));
                        for dex in deferred {
                            let d = self.emit_expr(&dex);
                            parts.push(format!("{d};"));
                        }
                        parts.push(format!("{tmp};"));
                    }
                } else {
                    for dex in deferred {
                        let d = self.emit_expr(&dex);
                        parts.push(format!("{d};"));
                    }
                }
                if parts.is_empty() {
                    "0".to_string()
                } else {
                    format!("({{ {} }})", parts.join(" "))
                }
            }
            Expr::Assign { target, value } => {
                let v = self.emit_expr(&value.node);
                format!("({target} = {v})")
            }
            Expr::CompoundAssign { target, op, value } => {
                let v = self.emit_expr(&value.node);
                let op_str = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    BinOp::Mod => "%=",
                    _ => "+=",
                };
                format!("({target} {op_str} {v})")
            }
            Expr::Interpolation(parts) => {
                // Build interpolation via rt_str_concat chains
                let mut result = String::new();
                let mut first = true;
                for part in parts {
                    let piece = match part {
                        InterpolPart::Lit(s) => Self::escape_c_string(s),
                        InterpolPart::Expr(e) => {
                            // Convert expr to string, choosing the right conversion
                            // based on the inferred type of the expression.
                            let inner = self.emit_expr(&e.node);
                            self.expr_to_str_conversion(&e.node, &inner)
                        }
                    };
                    if first {
                        result = piece;
                        first = false;
                    } else {
                        result = format!("rt_str_concat({result}, {piece})");
                    }
                }
                if result.is_empty() {
                    "\"\"".to_string()
                } else {
                    result
                }
            }
            Expr::ArrayLit(elems) => {
                let len = elems.len();
                let tmp = self.fresh_tmp();
                // This needs to be in statement context; for expression context,
                // wrap in a GCC statement expression
                let mut inner = String::new();
                write!(
                    &mut inner,
                    "long long *{tmp} = (long long*)rt_array_alloc({len}LL);"
                )
                .unwrap();
                for (i, elem) in elems.iter().enumerate() {
                    let v = self.emit_expr(&elem.node);
                    write!(
                        &mut inner,
                        " ((long long*){tmp})[1 + {i}] = (long long)({v});"
                    )
                    .unwrap();
                }
                write!(&mut inner, " (void*){tmp};").unwrap();
                format!("({{ {inner} }})")
            }
            Expr::Index { object, index } => {
                let obj = self.emit_expr(&object.node);
                let idx = self.emit_expr(&index.node);
                format!("rt_array_get({obj}, {idx})")
            }
            Expr::FieldAccess { object, field } => {
                let obj = self.emit_expr(&object.node);
                // Determine field index from struct layout
                // For now, use a generic field access via slot offset
                // We'll need to know the struct type to determine the field index
                // Fallback: treat as method-like access
                format!(
                    "((long long*){obj})[{field_idx}]",
                    field_idx = self.get_field_index_str(&obj, field)
                )
            }
            // Method calls are represented as Call { callee: FieldAccess { ... }, args }
            // and handled in emit_call()
            Expr::StructLit { name, fields } => {
                let num_fields = fields.len();
                let tmp = self.fresh_tmp();
                let mut inner = String::new();
                write!(
                    &mut inner,
                    "long long *{tmp} = (long long*)rt_struct_alloc({num_fields}LL);"
                )
                .unwrap();
                // Look up the struct layout to emit fields in the correct order
                if let Some(layout) = self.struct_fields.get(name).cloned() {
                    for (i, (fname, _fty)) in layout.iter().enumerate() {
                        if let Some((_n, val)) = fields.iter().find(|(n, _)| n == fname) {
                            let v = self.emit_expr(&val.node);
                            write!(&mut inner, " {tmp}[{i}] = (long long)({v});").unwrap();
                        }
                    }
                } else {
                    // No layout known -- emit in order given
                    for (i, (_fname, val)) in fields.iter().enumerate() {
                        let v = self.emit_expr(&val.node);
                        write!(&mut inner, " {tmp}[{i}] = (long long)({v});").unwrap();
                    }
                }
                write!(&mut inner, " (void*){tmp};").unwrap();
                format!("({{ {inner} }})")
            }
            Expr::While { condition, body } => {
                let cond = self.emit_expr(&condition.node);
                let body_c = self.emit_expr(&body.node);
                format!("({{ while ({cond}) {{ {body_c}; }} 0; }})")
            }
            Expr::ForIn {
                var_name,
                iterable,
                body,
            } => {
                let iter = self.emit_expr(&iterable.node);
                let body_c = self.emit_expr(&body.node);
                let len_tmp = self.fresh_tmp();
                let i_tmp = self.fresh_tmp();
                format!(
                    "({{ void *_arr = {iter}; long long {len_tmp} = rt_array_len(_arr); \
                    for (long long {i_tmp} = 0; {i_tmp} < {len_tmp}; {i_tmp}++) {{ \
                    long long {var_name} = rt_array_get(_arr, {i_tmp}); {body_c}; }} 0; }})"
                )
            }
            Expr::Range { start, end } => {
                // Ranges are typically only used in for-in loops.
                // Generate an array from start..end
                let s = self.emit_expr(&start.node);
                let e = self.emit_expr(&end.node);
                let tmp = self.fresh_tmp();
                format!(
                    "({{ long long _rs = {s}; long long _re = {e}; \
                    long long _rlen = _re - _rs; \
                    void *{tmp} = rt_array_alloc(_rlen); \
                    for (long long _ri = 0; _ri < _rlen; _ri++) \
                    ((long long*){tmp})[1 + _ri] = _rs + _ri; \
                    {tmp}; }})"
                )
            }
            Expr::Break => "__break__".to_string(), // handled in stmt context
            Expr::Continue => "__continue__".to_string(), // handled in stmt context
            Expr::OkExpr(inner) => {
                let v = self.emit_expr(&inner.node);
                format!("rt_result_ok((long long)({v}))")
            }
            Expr::ErrExpr(inner) => {
                let v = self.emit_expr(&inner.node);
                format!("rt_result_err((long long)({v}))")
            }
            Expr::SomeExpr(inner) => {
                let v = self.emit_expr(&inner.node);
                format!("rt_option_some((long long)({v}))")
            }
            Expr::NoneExpr => "rt_option_none()".to_string(),
            Expr::MapLit(pairs) => {
                let tmp = self.fresh_tmp();
                let mut inner = String::new();
                write!(&mut inner, "void *{tmp} = rt_hashmap_new();").unwrap();
                for (k, v) in pairs {
                    let kc = self.emit_expr(&k.node);
                    let vc = self.emit_expr(&v.node);
                    write!(&mut inner, " rt_hashmap_set({tmp}, {kc}, {vc});").unwrap();
                }
                write!(&mut inner, " {tmp};").unwrap();
                format!("({{ {inner} }})")
            }
            Expr::Match { subject, arms } => {
                // Compile match as nested ternary expressions.
                // Each arm with a literal or wildcard pattern becomes a condition.
                let subj = self.emit_expr(&subject.node);
                let subj_tmp = self.fresh_tmp();
                let mut parts = Vec::new();
                // Store subject in a temp to avoid re-evaluation
                parts.push(format!("long long {subj_tmp} = (long long)({subj});"));
                let ternary = self.emit_match_ternary(&subj_tmp, arms);
                parts.push(format!("{ternary};"));
                format!("({{ {} }})", parts.join(" "))
            }
            Expr::FieldAssign {
                object,
                field,
                value,
            } => {
                let obj = self.emit_expr(&object.node);
                let val = self.emit_expr(&value.node);
                let field_idx = self.get_field_index_str(&obj, field);
                format!("(((long long*){obj})[{field_idx}] = (long long)({val}))")
            }
            Expr::IndexAssign {
                object,
                index,
                value,
            } => {
                let obj = self.emit_expr(&object.node);
                let idx = self.emit_expr(&index.node);
                let val = self.emit_expr(&value.node);
                format!("(rt_array_set({obj}, {idx}, {val}), {val})")
            }
            Expr::Closure {
                params,
                return_type,
                body,
            } => self.emit_closure(params, return_type, body),
            // Catch-all for unsupported expressions: record a hard compile
            // error (surfaced by generate_c) and emit a syntactically valid
            // placeholder so the rest of emission does not panic.
            _ => {
                self.record_unsupported(Self::expr_kind_name(expr), None);
                "0 /* unsupported expr */".to_string()
            }
        }
    }

    // ── Closures ────────────────────────────────────────────────────────
    //
    // A closure value is represented exactly like the native backend: a heap
    // pair `[fn_ptr, env_ptr]` (two `long long` slots via `rt_struct_alloc(2)`).
    // The closure body is lifted to a top-level C function whose first
    // parameter is the captured-environment pointer, followed by the user
    // parameters. clang/LLVM lowers the resulting C function pointers to WASM
    // function-table indices and `call_indirect` automatically, so the WASM
    // side needs no manual function table.
    //
    // Captured variables live in a second heap struct (`rt_struct_alloc(n)`);
    // each capture occupies one `long long` slot. Integer/bool/string/pointer
    // captures round-trip losslessly through that slot via a plain cast; f64
    // captures are stored by bit pattern (union pun) so no precision is lost.

    /// Map a C type string back to the simplified type tag used for print /
    /// interpolation dispatch (inverse of `tag_to_c_type`).
    fn c_type_to_tag(c: &str) -> &'static str {
        match c {
            "const char*" => "str",
            "double" => "float",
            "char" => "bool",
            "void*" => "void*",
            "void" => "void",
            _ => "int",
        }
    }

    /// C type of a closure parameter. Inferred params (e.g. `.map(|x| ...)`)
    /// default to `long long`, matching how array elements are stored.
    fn closure_param_c(p: &Param) -> &'static str {
        match &p.ty.node {
            TypeExpr::Inferred => "long long",
            other => Self::type_to_c(other),
        }
    }

    /// C return type of a closure: the declared return type if present,
    /// otherwise inferred from the body's tail expression (an expression-body
    /// closure such as `(x) => x * 2` returns its tail value).
    fn closure_ret_c(&self, return_type: &Option<Spanned<TypeExpr>>, body: &Expr) -> &'static str {
        if let Some(t) = return_type {
            return Self::type_to_c(&t.node);
        }
        if let Expr::Block {
            tail_expr: Some(tail),
            ..
        } = body
        {
            self.infer_c_type(&tail.node)
        } else {
            "void"
        }
    }

    /// Compute a closure's C signature (parameter + return C types) without
    /// emitting anything. Parameter types are temporarily registered so the
    /// return-type inference can see them.
    fn closure_sig_of(
        &mut self,
        params: &[Param],
        return_type: &Option<Spanned<TypeExpr>>,
        body: &Expr,
    ) -> ClosureSig {
        let param_cs: Vec<&'static str> = params.iter().map(Self::closure_param_c).collect();
        let saved: Vec<(String, Option<String>)> = params
            .iter()
            .map(|p| (p.name.clone(), self.var_types.get(&p.name).cloned()))
            .collect();
        for p in params {
            let tag = match &p.ty.node {
                TypeExpr::Inferred => "int".to_string(),
                other => Self::type_expr_to_tag(other),
            };
            self.var_types.insert(p.name.clone(), tag);
        }
        let ret = self.closure_ret_c(return_type, body);
        for (name, prev) in saved {
            match prev {
                Some(t) => {
                    self.var_types.insert(name, t);
                }
                None => {
                    self.var_types.remove(&name);
                }
            }
        }
        ClosureSig {
            params: param_cs,
            ret,
        }
    }

    /// Resolve the signature of a closure passed as an argument: either a
    /// closure literal or an identifier bound to a closure.
    fn closure_sig_of_arg(&mut self, arg: &Expr) -> Option<ClosureSig> {
        match arg {
            Expr::Closure {
                params,
                return_type,
                body,
            } => Some(self.closure_sig_of(params, return_type, &body.node)),
            Expr::Ident(name) => self.closure_sigs.get(name).cloned(),
            _ => None,
        }
    }

    /// If `value` binds a closure to `name`, record its signature so later
    /// `name(args)` call sites lower to an indirect closure call. Handles both
    /// closure literals and aliasing another closure variable (`let g = f`).
    fn record_closure_binding(&mut self, name: &str, value: &Expr) {
        match value {
            Expr::Closure {
                params,
                return_type,
                body,
            } => {
                let sig = self.closure_sig_of(params, return_type, &body.node);
                self.closure_sigs.insert(name.to_string(), sig);
            }
            Expr::Ident(src) => {
                if let Some(sig) = self.closure_sigs.get(src).cloned() {
                    self.closure_sigs.insert(name.to_string(), sig);
                }
            }
            _ => {}
        }
    }

    /// Emit a lifted top-level C function for a closure and return the C
    /// expression that builds its `[fn_ptr, env_ptr]` pair. Captures are
    /// resolved against the variables currently in scope.
    fn emit_closure(
        &mut self,
        params: &[Param],
        return_type: &Option<Spanned<TypeExpr>>,
        body: &Spanned<Expr>,
    ) -> String {
        let id = self.closure_counter;
        self.closure_counter += 1;
        let name = format!("__closure_{id}");

        // Captures: free variables of the body that exist in the enclosing
        // scope. Resolve each one's tag + C type from the current var_types.
        let outer_vars: Vec<String> = self.var_types.keys().cloned().collect();
        let cap_names = find_captures(params, &body.node, &outer_vars);
        let captures: Vec<(String, String, &'static str)> = cap_names
            .iter()
            .map(|n| {
                let tag = self
                    .var_types
                    .get(n)
                    .cloned()
                    .unwrap_or_else(|| "int".to_string());
                let c = Self::tag_to_c_type(&tag);
                (n.clone(), tag, c)
            })
            .collect();

        let ret_c = self.closure_ret_c(return_type, &body.node);

        // ---- Lifted function definition ----
        let mut param_decls = vec!["void *env".to_string()];
        for p in params {
            let pc = Self::closure_param_c(p);
            param_decls.push(format!("{pc} {}", p.name));
        }
        let params_str = param_decls.join(", ");
        self.fn_decls.push(format!("{ret_c} {name}({params_str});"));

        // Emitting the body clobbers shared emitter state; snapshot + restore.
        let saved_indent = self.indent;
        let saved_var_types = self.var_types.clone();
        let saved_defer = std::mem::take(&mut self.defer_stack);

        for (cn, tag, _c) in &captures {
            self.var_types.insert(cn.clone(), tag.clone());
        }
        for p in params {
            let tag = match &p.ty.node {
                TypeExpr::Inferred => "int".to_string(),
                other => Self::type_expr_to_tag(other),
            };
            self.var_types.insert(p.name.clone(), tag);
        }

        let mut fbody = String::new();
        writeln!(&mut fbody, "{ret_c} {name}({params_str}) {{").unwrap();
        self.indent = 1;
        for (i, (cn, _tag, c)) in captures.iter().enumerate() {
            if *c == "double" {
                // Read the f64 capture back from its stored bit pattern.
                writeln!(
                    &mut fbody,
                    "{}double {cn} = ({{ union {{ double d; long long l; }} _u; \
                     _u.l = (((long long*)env)[{i}]); _u.d; }});",
                    self.indent_str()
                )
                .unwrap();
            } else {
                writeln!(
                    &mut fbody,
                    "{}{c} {cn} = ({c})(((long long*)env)[{i}]);",
                    self.indent_str()
                )
                .unwrap();
            }
        }
        let is_void = ret_c == "void";
        self.emit_block_body(&body.node, &mut fbody, is_void);
        self.indent = 0;
        writeln!(&mut fbody, "}}").unwrap();
        self.fn_defs.push(fbody);

        self.indent = saved_indent;
        self.var_types = saved_var_types;
        self.defer_stack = saved_defer;

        // ---- Pair construction expression ----
        let pair = self.fresh_tmp();
        let mut inner = String::new();
        let env_expr = if captures.is_empty() {
            "(void*)0".to_string()
        } else {
            let env = self.fresh_tmp();
            write!(
                &mut inner,
                "long long *{env} = (long long*)rt_struct_alloc({}LL);",
                captures.len()
            )
            .unwrap();
            for (i, (cn, _tag, c)) in captures.iter().enumerate() {
                if *c == "double" {
                    write!(
                        &mut inner,
                        " {env}[{i}] = ({{ union {{ double d; long long l; }} _u; \
                         _u.d = ({cn}); _u.l; }});"
                    )
                    .unwrap();
                } else {
                    write!(&mut inner, " {env}[{i}] = (long long)({cn});").unwrap();
                }
            }
            format!("(void*){env}")
        };
        write!(
            &mut inner,
            " long long *{pair} = (long long*)rt_struct_alloc(2LL); \
             {pair}[0] = (long long)(&{name}); {pair}[1] = (long long)({env_expr}); \
             (void*){pair};"
        )
        .unwrap();
        format!("({{ {inner} }})")
    }

    /// Emit an indirect call through a closure pair: load `fn_ptr`/`env_ptr`
    /// from the pair, cast `fn_ptr` to the right function-pointer type, and
    /// call it with `env_ptr` as the hidden leading argument.
    fn emit_closure_call(
        &mut self,
        callee_c: &str,
        sig: &ClosureSig,
        arg_strs: &[String],
    ) -> String {
        let tc = self.fresh_tmp();
        let param_sig = if sig.params.is_empty() {
            String::new()
        } else {
            format!(", {}", sig.params.join(", "))
        };
        let cast = format!("{}(*)(void*{})", sig.ret, param_sig);
        let mut call_args = vec![format!("(void*){tc}[1]")];
        call_args.extend(arg_strs.iter().cloned());
        let call = format!("(({cast})({tc}[0]))({})", call_args.join(", "));
        format!("({{ long long *{tc} = (long long*)({callee_c}); {call}; }})")
    }

    /// Emit `map(arr, closure)`: build a new array of the same length, call the
    /// closure on each element via an indirect call, and store the results.
    fn emit_map(&mut self, arr_c: &str, closure_c: &str, sig: &ClosureSig) -> String {
        let arr = self.fresh_tmp();
        let len = self.fresh_tmp();
        let res = self.fresh_tmp();
        let clp = self.fresh_tmp();
        let env = self.fresh_tmp();
        let fnv = self.fresh_tmp();
        let i = self.fresh_tmp();
        let elem = self.fresh_tmp();
        let mapped = self.fresh_tmp();
        let param_c = sig.params.first().copied().unwrap_or("long long");
        let ret_c = sig.ret;
        format!(
            "({{ void *{arr} = ({arr_c}); long long {len} = rt_array_len({arr}); \
             void *{res} = rt_array_alloc({len}); \
             long long *{clp} = (long long*)({closure_c}); \
             void *{env} = (void*){clp}[1]; \
             {ret_c} (*{fnv})(void*, {param_c}) = ({ret_c}(*)(void*, {param_c}))({clp}[0]); \
             for (long long {i} = 0; {i} < {len}; {i}++) {{ \
             {param_c} {elem} = ({param_c})rt_array_get({arr}, {i}); \
             {ret_c} {mapped} = {fnv}({env}, {elem}); \
             rt_array_set({res}, {i}, (long long)({mapped})); }} {res}; }})"
        )
    }

    /// Emit `filter(arr, predicate)`: keep elements for which the predicate is
    /// truthy, packing them into a fresh array and patching its length.
    fn emit_filter(&mut self, arr_c: &str, closure_c: &str, sig: &ClosureSig) -> String {
        let arr = self.fresh_tmp();
        let len = self.fresh_tmp();
        let res = self.fresh_tmp();
        let clp = self.fresh_tmp();
        let env = self.fresh_tmp();
        let fnv = self.fresh_tmp();
        let cnt = self.fresh_tmp();
        let i = self.fresh_tmp();
        let elem = self.fresh_tmp();
        let param_c = sig.params.first().copied().unwrap_or("long long");
        let ret_c = sig.ret;
        format!(
            "({{ void *{arr} = ({arr_c}); long long {len} = rt_array_len({arr}); \
             void *{res} = rt_array_alloc({len}); \
             long long *{clp} = (long long*)({closure_c}); \
             void *{env} = (void*){clp}[1]; \
             {ret_c} (*{fnv})(void*, {param_c}) = ({ret_c}(*)(void*, {param_c}))({clp}[0]); \
             long long {cnt} = 0; \
             for (long long {i} = 0; {i} < {len}; {i}++) {{ \
             long long {elem} = rt_array_get({arr}, {i}); \
             if ((long long)({fnv}({env}, ({param_c}){elem}))) {{ \
             rt_array_set({res}, {cnt}, {elem}); {cnt}++; }} }} \
             ((long long*){res})[0] = {cnt}; {res}; }})"
        )
    }

    fn emit_call(&mut self, callee: &Spanned<Expr>, args: &[Spanned<Expr>]) -> String {
        let fn_name = match &callee.node {
            Expr::Ident(name) => name.clone(),
            Expr::FieldAccess { object, field } => {
                // Static method call like Enum.Variant -- just use the field name
                let obj_name = self.emit_expr(&object.node);
                return format!("{obj_name}_{field}");
            }
            _ => {
                let c = self.emit_expr(&callee.node);
                let arg_strs: Vec<String> = args.iter().map(|a| self.emit_expr(&a.node)).collect();
                return format!("(({c})({}))", arg_strs.join(", "));
            }
        };

        let arg_strs: Vec<String> = args.iter().map(|a| self.emit_expr(&a.node)).collect();
        let args_joined = arg_strs.join(", ");

        // A call to a local bound to a closure value lowers to an indirect
        // call through its [fn_ptr, env_ptr] pair rather than a direct
        // C function call by name.
        if let Some(sig) = self.closure_sigs.get(&fn_name).cloned() {
            return self.emit_closure_call(&fn_name, &sig, &arg_strs);
        }

        // Map built-in functions to runtime calls
        match fn_name.as_str() {
            "map" if args.len() == 2 => match self.closure_sig_of_arg(&args[1].node) {
                Some(sig) if sig.ret != "double" && sig.params.first() != Some(&"double") => {
                    self.emit_map(&arg_strs[0], &arg_strs[1], &sig)
                }
                Some(_) => {
                    self.record_unsupported(
                        "map over float-typed closures (f64 array slots are lossy)",
                        None,
                    );
                    "0 /* unsupported map */".to_string()
                }
                None => {
                    self.record_unsupported("map with a non-closure callback", None);
                    "0 /* unsupported map */".to_string()
                }
            },
            "filter" if args.len() == 2 => match self.closure_sig_of_arg(&args[1].node) {
                Some(sig) if sig.params.first() != Some(&"double") => {
                    self.emit_filter(&arg_strs[0], &arg_strs[1], &sig)
                }
                Some(_) => {
                    self.record_unsupported(
                        "filter over float-typed closures (f64 array slots are lossy)",
                        None,
                    );
                    "0 /* unsupported filter */".to_string()
                }
                None => {
                    self.record_unsupported("filter with a non-closure predicate", None);
                    "0 /* unsupported filter */".to_string()
                }
            },
            "print" => {
                if args.len() == 1 {
                    // Determine which print variant to call based on the
                    // inferred type of the argument expression.
                    let tag = self.infer_type_tag(&args[0].node);
                    let print_fn = Self::print_fn_for_tag(&tag);
                    format!("{print_fn}({args_joined})")
                } else {
                    format!("rt_print_str({args_joined})")
                }
            }
            "println" => {
                if args.is_empty() {
                    "rt_print_str(\"\")".to_string()
                } else {
                    format!("rt_print_str({args_joined})")
                }
            }
            "assert" => {
                if args.len() == 1 {
                    let cond = &arg_strs[0];
                    format!("if (!({cond})) rt_assert_fail(\"assertion failed\")")
                } else if args.len() == 2 {
                    format!("if (!({0})) rt_assert_fail({1})", arg_strs[0], arg_strs[1])
                } else {
                    "rt_assert_fail(\"bad assert\")".to_string()
                }
            }
            "assert_eq" => {
                if args.len() >= 2 {
                    format!("if (({0}) != ({1})) rt_assert_eq_fail(0, rt_i64_to_str({0}), rt_i64_to_str({1}))",
                        arg_strs[0], arg_strs[1])
                } else {
                    "rt_assert_fail(\"bad assert_eq\")".to_string()
                }
            }
            "len" => format!("rt_array_len({args_joined})"),
            "push" => format!("rt_array_push({args_joined})"),
            "str" | "to_string" | "to_str" => {
                if let Some(arg) = args.first() {
                    self.expr_to_str_conversion(&arg.node, &arg_strs[0])
                } else {
                    "rt_i64_to_str(0)".to_string()
                }
            }
            "str_upper" => format!("rt_str_upper({args_joined})"),
            "str_lower" => format!("rt_str_lower({args_joined})"),
            "str_trim" => format!("rt_str_trim({args_joined})"),
            "str_split" => format!("rt_str_split({args_joined})"),
            "str_contains" => format!("rt_str_contains({args_joined})"),
            "str_starts_with" => format!("rt_str_starts_with({args_joined})"),
            "str_ends_with" => format!("rt_str_ends_with({args_joined})"),
            "str_replace" => format!("rt_str_replace({args_joined})"),
            "str_char_at" => format!("rt_str_char_at({args_joined})"),
            "str_index_of" => format!("rt_str_index_of({args_joined})"),
            "str_repeat" => format!("rt_str_repeat({args_joined})"),
            "str_join" => format!("rt_str_join({args_joined})"),
            "str_len" => format!("rt_str_len({args_joined})"),
            "str_concat" => format!("rt_str_concat({args_joined})"),
            "read_line" => "rt_read_line()".to_string(),
            "read_file" => format!("rt_read_file({args_joined})"),
            "write_file" => format!("rt_write_file({args_joined})"),
            "pow" | "math_pow" => format!("rt_pow({args_joined})"),
            "sqrt" | "math_sqrt" => format!("rt_sqrt({args_joined})"),
            "panic" => format!("rt_panic({args_joined})"),
            "hashmap_new" | "HashMap_new" => "rt_hashmap_new()".to_string(),
            "hashmap_set" => format!("rt_hashmap_set({args_joined})"),
            "hashmap_get" => format!("rt_hashmap_get({args_joined})"),
            "hashmap_has" => format!("rt_hashmap_has({args_joined})"),
            "hashmap_len" => format!("rt_hashmap_len({args_joined})"),
            "hashmap_keys" => format!("rt_hashmap_keys({args_joined})"),
            "hashmap_remove" => format!("rt_hashmap_remove({args_joined})"),
            // User-defined function: call directly
            _ => format!("{fn_name}({args_joined})"),
        }
    }

    fn get_field_index_str(&mut self, _obj_expr: &str, field: &str) -> String {
        // Try to find the field index from known struct layouts
        // For now, we can't always determine the struct type at this point,
        // so we'll use a generic approach
        // TODO: Type-aware field resolution
        for fields in self.struct_fields.values() {
            for (i, (fname, _)) in fields.iter().enumerate() {
                if fname == field {
                    return format!("{i}");
                }
            }
        }
        // No known struct has this field. Emitting index 0 here would silently
        // read/write the wrong slot, so fail the compile instead.
        self.record_unsupported(&format!("field access '{field}' (unknown field)"), None);
        format!("0 /* unknown field {field} */")
    }

    /// Emit a match expression as nested ternary operators.
    fn emit_match_ternary(&mut self, subj_tmp: &str, arms: &[MatchArm]) -> String {
        if arms.is_empty() {
            return "0 /* empty match */".to_string();
        }

        let mut result = String::new();
        let mut depth = 0;

        for (i, arm) in arms.iter().enumerate() {
            let body = self.emit_expr(&arm.body.node);
            let is_last = i == arms.len() - 1;

            match &arm.pattern.node {
                Pattern::Wildcard | Pattern::Ident(_) => {
                    // Wildcard or variable binding -- always matches
                    // If it's an Ident binding, we'd ideally bind the variable,
                    // but for expression context we just emit the body.
                    result.push_str(&body);
                    break;
                }
                Pattern::IntLit(n) => {
                    result.push_str(&format!("({subj_tmp} == {n}LL ? {body} : "));
                    depth += 1;
                }
                Pattern::BoolLit(b) => {
                    let cond_val = if *b { "1" } else { "0" };
                    result.push_str(&format!("({subj_tmp} == {cond_val} ? {body} : "));
                    depth += 1;
                }
                Pattern::StringLit(s) => {
                    let escaped = Self::escape_c_string(s);
                    result.push_str(&format!(
                        "(rt_str_eq((const char*){subj_tmp}, {escaped}) ? {body} : "
                    ));
                    depth += 1;
                }
                _ => {
                    // Unsupported pattern type (enum destructure, Ok/Err/Some/None).
                    // Record a hard error so the program fails to compile rather
                    // than silently never matching this arm.
                    self.record_unsupported(
                        "this match pattern (enum/Ok/Err/Some/None destructure)",
                        Some(&arm.pattern.span),
                    );
                    if is_last {
                        result.push_str("0 /* unsupported match pattern */");
                    } else {
                        result.push_str(&format!("(0 /* unsupported match pattern */ ? {body} : "));
                        depth += 1;
                    }
                }
            }

            // If this is the last arm and we haven't hit a wildcard, add a fallback
            if is_last && !matches!(&arm.pattern.node, Pattern::Wildcard | Pattern::Ident(_)) {
                result.push('0');
            }
        }

        // Close all open parens
        for _ in 0..depth {
            result.push(')');
        }

        result
    }

    /// Emit a statement, returning it as a String.
    fn emit_stmt_to_string(&mut self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Let {
                name, value, ty, ..
            } => {
                // Record the variable type for later print/interpolation dispatch
                let type_tag = if let Some(t) = ty {
                    Self::type_expr_to_tag(&t.node)
                } else {
                    self.infer_type_tag(&value.node)
                };
                self.var_types.insert(name.clone(), type_tag);
                self.record_closure_binding(name, &value.node);

                let v = self.emit_expr(&value.node);
                let c_type = if let Some(t) = ty {
                    Self::type_to_c(&t.node)
                } else {
                    // Infer type from value
                    self.infer_c_type(&value.node)
                };
                format!("{c_type} {name} = ({c_type})({v});")
            }
            Stmt::Expr(expr) => match &expr.node {
                Expr::Break => "break;".to_string(),
                Expr::Continue => "continue;".to_string(),
                _ => {
                    let e = self.emit_expr(&expr.node);
                    format!("{e};")
                }
            },
            Stmt::Return(Some(expr)) => {
                let e = self.emit_expr(&expr.node);
                format!("return {e};")
            }
            Stmt::Return(None) => "return;".to_string(),
            Stmt::Defer(expr) => {
                // Record the deferred expression for the innermost scope.
                // It will be emitted in LIFO order at scope exit.
                if let Some(scope) = self.defer_stack.last_mut() {
                    scope.push(expr.node.clone());
                } else {
                    // Defer outside any tracked scope is a programmer error;
                    // drop it with a warning comment so it's auditable in the
                    // emitted C rather than silently swallowed.
                    return "/* defer outside tracked scope — dropped */".to_string();
                }
                String::new()
            }
            Stmt::LetDestructure { fields, value, .. } => {
                let v = self.emit_expr(&value.node);
                let tmp = self.fresh_tmp();
                let mut parts = vec![format!("long long *{tmp} = (long long*)({v});")];
                for (i, field) in fields.iter().enumerate() {
                    parts.push(format!("long long {field} = {tmp}[{i}];"));
                }
                parts.join(" ")
            }
        }
    }

    /// Convert a type tag to a C type string.
    fn tag_to_c_type(tag: &str) -> &'static str {
        match tag {
            "str" => "const char*",
            "float" | "f64" | "f32" => "double",
            "bool" => "char",
            "array" | "struct" | "void*" | "closure" => "void*",
            _ => "long long",
        }
    }

    /// Infer C type from an expression (best-effort).
    fn infer_c_type(&self, expr: &Expr) -> &'static str {
        match expr {
            Expr::IntLit(_) => "long long",
            Expr::FloatLit(_) => "double",
            Expr::StringLit(_) | Expr::Interpolation(_) => "const char*",
            Expr::BoolLit(_) => "char",
            Expr::ArrayLit(_) => "void*",
            Expr::StructLit { .. } => "void*",
            Expr::MapLit(_) => "void*",
            Expr::Ident(name) => {
                // Look up tracked variable type
                if let Some(tag) = self.var_types.get(name) {
                    Self::tag_to_c_type(tag)
                } else {
                    "long long"
                }
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = &callee.node {
                    // A local bound to a closure: C type follows its return type.
                    if let Some(sig) = self.closure_sigs.get(name) {
                        return sig.ret;
                    }
                    // Check user-defined function return types first
                    if let Some(tag) = self.fn_return_types.get(name.as_str()) {
                        return Self::tag_to_c_type(tag);
                    }
                    match name.as_str() {
                        "str" | "to_string" | "to_str" | "str_concat" | "str_upper"
                        | "str_lower" | "str_trim" | "str_replace" | "str_char_at"
                        | "str_repeat" | "str_join" | "read_line" | "read_file"
                        | "rt_i64_to_str" | "rt_f64_to_str" | "rt_bool_to_str"
                        | "rt_str_concat" | "json_get" | "json_stringify" | "http_get"
                        | "http_post" => "const char*",
                        "len" | "str_len" | "str_index_of" | "pow" | "hashmap_len" => "long long",
                        "sqrt" => "double",
                        "str_contains" | "str_starts_with" | "str_ends_with" | "hashmap_has"
                        | "str_eq" => "char",
                        "hashmap_new" | "hashmap_keys" | "str_split" | "array_alloc" | "map"
                        | "filter" => "void*",
                        _ => "long long",
                    }
                } else {
                    "long long"
                }
            }
            Expr::Closure { .. } => "void*",
            Expr::BinaryOp { left, op, .. } => match op {
                BinOp::Eq
                | BinOp::NotEq
                | BinOp::Less
                | BinOp::LessEq
                | BinOp::Greater
                | BinOp::GreaterEq
                | BinOp::And
                | BinOp::Or => "char",
                _ => self.infer_c_type(&left.node),
            },
            Expr::OkExpr(_) | Expr::ErrExpr(_) => "void*",
            Expr::SomeExpr(_) | Expr::NoneExpr => "void*",
            _ => "long long",
        }
    }

    /// Emit a function body (block expression) as C statements.
    /// If `is_void` is true, tail expressions are emitted as statements rather
    /// than return values.
    fn emit_block_body(&mut self, expr: &Expr, buf: &mut String, is_void: bool) {
        match expr {
            Expr::Block { stmts, tail_expr } => {
                self.push_defer_scope();
                for stmt in stmts {
                    self.emit_stmt(buf, &stmt.node, is_void);
                }
                if let Some(tail) = tail_expr {
                    if is_void {
                        // In a void context, emit the tail as a statement,
                        // then emit the scope's deferreds in LIFO order
                        // before falling off.
                        self.emit_stmt(buf, &Stmt::Expr((**tail).clone()), is_void);
                        let deferred = self.pop_defer_scope();
                        self.emit_popped_deferred(buf, deferred);
                    } else {
                        // Value-returning tail: stash into a temp, run the
                        // defers, then `return` the temp. This mirrors the
                        // Cranelift backend's ordering (tail evaluated,
                        // defers run LIFO, function returns the tail value).
                        let e = self.emit_expr(&tail.node);
                        let deferred = self.pop_defer_scope();
                        if deferred.is_empty() {
                            writeln!(buf, "{}return {e};", self.indent_str()).unwrap();
                        } else {
                            let tmp = self.fresh_tmp();
                            let c_type = self.infer_c_type(&tail.node);
                            writeln!(
                                buf,
                                "{}{c_type} {tmp} = ({c_type})({e});",
                                self.indent_str()
                            )
                            .unwrap();
                            self.emit_popped_deferred(buf, deferred);
                            writeln!(buf, "{}return {tmp};", self.indent_str()).unwrap();
                        }
                    }
                } else {
                    // No tail expression — defers just fire before the
                    // block falls off. (Any inner `return` statements have
                    // already flushed deferreds via `emit_all_deferred_for_return`.)
                    let deferred = self.pop_defer_scope();
                    self.emit_popped_deferred(buf, deferred);
                }
            }
            _ => {
                if is_void {
                    let e = self.emit_expr(expr);
                    writeln!(buf, "{}{e};", self.indent_str()).unwrap();
                } else {
                    let e = self.emit_expr(expr);
                    writeln!(buf, "{}return {e};", self.indent_str()).unwrap();
                }
            }
        }
    }

    /// Emit a statement into a buffer.
    /// `is_void` indicates whether we're in a void context (don't return values).
    fn emit_stmt(&mut self, buf: &mut String, stmt: &Stmt, is_void: bool) {
        match stmt {
            Stmt::Let {
                name, value, ty, ..
            } => {
                // Record the variable type for later print/interpolation dispatch
                let type_tag = if let Some(t) = ty {
                    Self::type_expr_to_tag(&t.node)
                } else {
                    self.infer_type_tag(&value.node)
                };
                self.var_types.insert(name.clone(), type_tag);
                self.record_closure_binding(name, &value.node);

                let v = self.emit_expr(&value.node);
                let c_type = if let Some(t) = ty {
                    Self::type_to_c(&t.node)
                } else {
                    self.infer_c_type(&value.node)
                };
                writeln!(
                    buf,
                    "{}{c_type} {name} = ({c_type})({v});",
                    self.indent_str()
                )
                .unwrap();
            }
            Stmt::Expr(expr) => {
                match &expr.node {
                    Expr::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        let cond = self.emit_expr(&condition.node);
                        writeln!(buf, "{}if ({cond}) {{", self.indent_str()).unwrap();
                        self.indent += 1;
                        // Control flow bodies are always void in statement position
                        self.emit_block_body(&then_branch.node, buf, true);
                        self.indent -= 1;
                        if let Some(else_b) = else_branch {
                            writeln!(buf, "{}}} else {{", self.indent_str()).unwrap();
                            self.indent += 1;
                            self.emit_block_body(&else_b.node, buf, true);
                            self.indent -= 1;
                        }
                        writeln!(buf, "{}}}", self.indent_str()).unwrap();
                    }
                    Expr::While { condition, body } => {
                        let cond = self.emit_expr(&condition.node);
                        writeln!(buf, "{}while ({cond}) {{", self.indent_str()).unwrap();
                        self.indent += 1;
                        self.emit_block_body(&body.node, buf, true);
                        self.indent -= 1;
                        writeln!(buf, "{}}}", self.indent_str()).unwrap();
                    }
                    Expr::ForIn {
                        var_name,
                        iterable,
                        body,
                    } => {
                        let len_tmp = self.fresh_tmp();
                        let i_tmp = self.fresh_tmp();
                        let arr_tmp = self.fresh_tmp();
                        // Check if iterable is a range
                        if let Expr::Range { start, end } = &iterable.node {
                            let s = self.emit_expr(&start.node);
                            let e = self.emit_expr(&end.node);
                            writeln!(buf, "{}for (long long {var_name} = {s}; {var_name} < {e}; {var_name}++) {{",
                                self.indent_str()).unwrap();
                        } else {
                            let iter = self.emit_expr(&iterable.node);
                            writeln!(buf, "{}{{", self.indent_str()).unwrap();
                            writeln!(buf, "{}    void *{arr_tmp} = {iter};", self.indent_str())
                                .unwrap();
                            writeln!(
                                buf,
                                "{}    long long {len_tmp} = rt_array_len({arr_tmp});",
                                self.indent_str()
                            )
                            .unwrap();
                            writeln!(buf, "{}    for (long long {i_tmp} = 0; {i_tmp} < {len_tmp}; {i_tmp}++) {{",
                                self.indent_str()).unwrap();
                            writeln!(buf, "{}        long long {var_name} = rt_array_get({arr_tmp}, {i_tmp});",
                                self.indent_str()).unwrap();
                        }
                        self.indent += 1;
                        self.emit_block_body(&body.node, buf, true);
                        self.indent -= 1;
                        writeln!(buf, "{}}}", self.indent_str()).unwrap();
                        if !matches!(&iterable.node, Expr::Range { .. }) {
                            writeln!(buf, "{}}}", self.indent_str()).unwrap();
                        }
                    }
                    Expr::Match { subject, arms } => {
                        // In statement context, compile match as if/else if chain
                        let subj = self.emit_expr(&subject.node);
                        let subj_tmp = self.fresh_tmp();
                        writeln!(buf, "{}{{", self.indent_str()).unwrap();
                        writeln!(
                            buf,
                            "{}    long long {subj_tmp} = (long long)({subj});",
                            self.indent_str()
                        )
                        .unwrap();
                        let mut first = true;
                        for arm in arms {
                            match &arm.pattern.node {
                                Pattern::Wildcard | Pattern::Ident(_) => {
                                    if first {
                                        writeln!(buf, "{}    {{", self.indent_str()).unwrap();
                                    } else {
                                        writeln!(buf, "{}    else {{", self.indent_str()).unwrap();
                                    }
                                    self.indent += 2;
                                    self.emit_block_body(&arm.body.node, buf, true);
                                    self.indent -= 2;
                                    writeln!(buf, "{}    }}", self.indent_str()).unwrap();
                                    break; // wildcard is always last
                                }
                                Pattern::IntLit(n) => {
                                    let keyword = if first { "if" } else { "else if" };
                                    writeln!(
                                        buf,
                                        "{}    {keyword} ({subj_tmp} == {n}LL) {{",
                                        self.indent_str()
                                    )
                                    .unwrap();
                                    self.indent += 2;
                                    self.emit_block_body(&arm.body.node, buf, true);
                                    self.indent -= 2;
                                    writeln!(buf, "{}    }}", self.indent_str()).unwrap();
                                }
                                Pattern::BoolLit(b) => {
                                    let keyword = if first { "if" } else { "else if" };
                                    let cond_val = if *b { "1" } else { "0" };
                                    writeln!(
                                        buf,
                                        "{}    {keyword} ({subj_tmp} == {cond_val}) {{",
                                        self.indent_str()
                                    )
                                    .unwrap();
                                    self.indent += 2;
                                    self.emit_block_body(&arm.body.node, buf, true);
                                    self.indent -= 2;
                                    writeln!(buf, "{}    }}", self.indent_str()).unwrap();
                                }
                                Pattern::StringLit(s) => {
                                    let keyword = if first { "if" } else { "else if" };
                                    let escaped = Self::escape_c_string(s);
                                    writeln!(buf, "{}    {keyword} (rt_str_eq((const char*){subj_tmp}, {escaped})) {{",
                                        self.indent_str()).unwrap();
                                    self.indent += 2;
                                    self.emit_block_body(&arm.body.node, buf, true);
                                    self.indent -= 2;
                                    writeln!(buf, "{}    }}", self.indent_str()).unwrap();
                                }
                                _ => {
                                    // Fail loud: an enum/Ok/Err/Some/None
                                    // destructure in statement-context match is
                                    // not lowered here, so emitting just a
                                    // comment would silently drop the arm.
                                    self.record_unsupported(
                                        "this match pattern (enum/Ok/Err/Some/None destructure)",
                                        Some(&arm.pattern.span),
                                    );
                                    writeln!(
                                        buf,
                                        "{}    /* unsupported match pattern */",
                                        self.indent_str()
                                    )
                                    .unwrap();
                                }
                            }
                            first = false;
                        }
                        writeln!(buf, "{}}}", self.indent_str()).unwrap();
                    }
                    Expr::FieldAssign {
                        object,
                        field,
                        value,
                    } => {
                        let obj = self.emit_expr(&object.node);
                        let val = self.emit_expr(&value.node);
                        let field_idx = self.get_field_index_str(&obj, field);
                        writeln!(
                            buf,
                            "{}((long long*){obj})[{field_idx}] = (long long)({val});",
                            self.indent_str()
                        )
                        .unwrap();
                    }
                    Expr::IndexAssign {
                        object,
                        index,
                        value,
                    } => {
                        let obj = self.emit_expr(&object.node);
                        let idx = self.emit_expr(&index.node);
                        let val = self.emit_expr(&value.node);
                        writeln!(
                            buf,
                            "{}rt_array_set({obj}, {idx}, {val});",
                            self.indent_str()
                        )
                        .unwrap();
                    }
                    Expr::Break => {
                        writeln!(buf, "{}break;", self.indent_str()).unwrap();
                    }
                    Expr::Continue => {
                        writeln!(buf, "{}continue;", self.indent_str()).unwrap();
                    }
                    _ => {
                        let e = self.emit_expr(&expr.node);
                        writeln!(buf, "{}{e};", self.indent_str()).unwrap();
                    }
                }
            }
            Stmt::Return(Some(expr)) => {
                if is_void {
                    let e = self.emit_expr(&expr.node);
                    writeln!(buf, "{}{e};", self.indent_str()).unwrap();
                    // Flush every currently-live deferred expression (LIFO)
                    // before the actual return, matching Cranelift semantics.
                    self.emit_all_deferred_for_return(buf);
                    writeln!(buf, "{}return;", self.indent_str()).unwrap();
                } else {
                    let e = self.emit_expr(&expr.node);
                    // Stash the return value into a temp so the defers can
                    // observe the "return happens last" sequencing without
                    // re-evaluating the expression (which might have side
                    // effects).
                    if self.defer_stack.iter().any(|s| !s.is_empty()) {
                        let tmp = self.fresh_tmp();
                        let c_type = self.infer_c_type(&expr.node);
                        writeln!(
                            buf,
                            "{}{c_type} {tmp} = ({c_type})({e});",
                            self.indent_str()
                        )
                        .unwrap();
                        self.emit_all_deferred_for_return(buf);
                        writeln!(buf, "{}return {tmp};", self.indent_str()).unwrap();
                    } else {
                        writeln!(buf, "{}return {e};", self.indent_str()).unwrap();
                    }
                }
            }
            Stmt::Return(None) => {
                self.emit_all_deferred_for_return(buf);
                writeln!(buf, "{}return;", self.indent_str()).unwrap();
            }
            Stmt::Defer(expr) => {
                // Record the deferred expression onto the innermost scope.
                // Actual emission happens at scope exit (or before `return`).
                if let Some(scope) = self.defer_stack.last_mut() {
                    scope.push(expr.node.clone());
                } else {
                    // Should be unreachable: emit_stmt is only called from
                    // inside a scope pushed by emit_block_body. Leave a
                    // breadcrumb so any future refactor that breaks this
                    // invariant is obvious in the generated C.
                    writeln!(
                        buf,
                        "{}/* defer outside tracked scope — dropped */",
                        self.indent_str()
                    )
                    .unwrap();
                }
            }
            Stmt::LetDestructure { fields, value, .. } => {
                let v = self.emit_expr(&value.node);
                let tmp = self.fresh_tmp();
                writeln!(
                    buf,
                    "{}long long *{tmp} = (long long*)({v});",
                    self.indent_str()
                )
                .unwrap();
                for (i, field) in fields.iter().enumerate() {
                    writeln!(buf, "{}long long {field} = {tmp}[{i}];", self.indent_str()).unwrap();
                }
            }
        }
    }

    /// Emit a function definition.
    fn emit_function(&mut self, fndef: &FnDef, prefix: Option<&str>) {
        let fn_name = if let Some(p) = prefix {
            format!("{p}_{}", fndef.name)
        } else if fndef.name == "main" {
            "turbo_main".to_string()
        } else {
            fndef.name.clone()
        };

        let ret = Self::return_type_to_c(&fndef.return_type);

        // Record function return type for call-site type inference
        if let Some(ret_ty) = &fndef.return_type {
            let ret_tag = Self::type_expr_to_tag(&ret_ty.node);
            self.fn_return_types.insert(fndef.name.clone(), ret_tag);
        }

        // Record parameter types so the body can look them up for print/interpolation
        for p in &fndef.params {
            if p.name != "self" {
                let tag = Self::type_expr_to_tag(&p.ty.node);
                self.var_types.insert(p.name.clone(), tag);
            }
            // Closures as parameters of user-defined functions (higher-order
            // user functions) are out of scope for the WASM backend: the call
            // site can't recover the closure's signature from a `fn(...)` param
            // type. Fail loud here rather than emit a body that calls a
            // non-callable `long long` parameter. Closures handed to the
            // map/filter builtins are fully supported.
            if matches!(p.ty.node, TypeExpr::FnType { .. }) {
                self.record_unsupported(
                    "closures as parameters of user-defined functions \
                     (use map/filter, or call the closure directly)",
                    Some(&p.span),
                );
            }
        }

        let params: Vec<String> = fndef
            .params
            .iter()
            .map(|p| {
                let c_type = Self::type_to_c(&p.ty.node);
                // Handle "self" parameter
                if p.name == "self" {
                    "void *self".to_string()
                } else {
                    format!("{c_type} {}", p.name)
                }
            })
            .collect();

        let params_str = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };

        // Forward declaration
        self.fn_decls
            .push(format!("{ret} {fn_name}({params_str});"));

        // Function body
        let is_void = ret == "void";
        let mut body = String::new();
        writeln!(&mut body, "{ret} {fn_name}({params_str}) {{").unwrap();
        self.indent = 1;
        self.emit_block_body(&fndef.body.node, &mut body, is_void);
        self.indent = 0;
        writeln!(&mut body, "}}").unwrap();
        self.fn_defs.push(body);
    }

    /// Generate the complete C source for a module.
    fn emit_module(&mut self, module: &turbo_ast::Module) -> String {
        // First pass: collect struct layouts and enum variants
        for item in &module.items {
            match &item.node {
                Item::Struct(s) => {
                    let fields: Vec<(String, TypeExpr)> = s
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.node.clone()))
                        .collect();
                    self.struct_fields.insert(s.name.clone(), fields);
                }
                Item::Enum(e) => {
                    let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
                    self.enum_variants.insert(e.name.clone(), variants);
                }
                Item::Impl(imp) => {
                    let type_name = imp.type_name.clone();
                    for method in &imp.methods {
                        let entry = self.impl_methods.entry(type_name.clone()).or_default();
                        entry.push((method.node.name.clone(), method.node.clone()));
                    }
                }
                _ => {}
            }
        }

        // Pre-pass: collect function return types so call sites can infer types
        for item in &module.items {
            if let Item::Function(fndef) = &item.node {
                if let Some(ret_ty) = &fndef.return_type {
                    let ret_tag = Self::type_expr_to_tag(&ret_ty.node);
                    self.fn_return_types.insert(fndef.name.clone(), ret_tag);
                }
            }
        }
        for methods in self.impl_methods.values() {
            for (_method_name, method) in methods {
                if let Some(ret_ty) = &method.return_type {
                    let ret_tag = Self::type_expr_to_tag(&ret_ty.node);
                    self.fn_return_types.insert(method.name.clone(), ret_tag);
                }
            }
        }

        // Collect extern function declarations and return types
        for item in &module.items {
            if let Item::Extern(ext) = &item.node {
                for fn_sig in &ext.functions {
                    let f = &fn_sig.node;
                    if let Some(ret_ty) = &f.return_type {
                        let ret_tag = Self::type_expr_to_tag(&ret_ty.node);
                        self.fn_return_types.insert(f.name.clone(), ret_tag);
                    }
                }
            }
        }

        // Second pass: emit functions
        // Collect all impl methods first, then emit them
        let impl_methods = self.impl_methods.clone();
        for (type_name, methods) in &impl_methods {
            for (_, method) in methods {
                self.emit_function(method, Some(type_name));
            }
        }

        for item in &module.items {
            match &item.node {
                Item::Function(fndef) if !fndef.is_test => {
                    self.emit_function(fndef, None);
                }
                Item::Const(c) => {
                    let v = self.emit_expr(&c.value.node);
                    let c_type = if let Some(t) = &c.ty {
                        Self::type_to_c(&t.node)
                    } else {
                        self.infer_c_type(&c.value.node)
                    };
                    self.fn_decls
                        .push(format!("static {c_type} {} = ({c_type})({v});", c.name));
                }
                _ => {} // structs, enums, etc. handled in first pass
            }
        }

        // Assemble the output
        let mut output = String::with_capacity(8192);
        writeln!(
            &mut output,
            "/* Generated by Turbo Compiler — WASM target */"
        )
        .unwrap();
        writeln!(&mut output).unwrap();

        // Runtime function declarations
        writeln!(&mut output, "/* Runtime function declarations */").unwrap();
        writeln!(&mut output, "void rt_print_str(const char *s);").unwrap();
        writeln!(&mut output, "void rt_print_i64(long long n);").unwrap();
        writeln!(&mut output, "void rt_print_f64(double n);").unwrap();
        writeln!(&mut output, "void rt_print_bool(char b);").unwrap();
        writeln!(&mut output, "void rt_panic(const char *msg);").unwrap();
        writeln!(&mut output, "void rt_assert_fail(const char *msg);").unwrap();
        writeln!(
            &mut output,
            "void rt_assert_eq_fail(long long dummy, const char *left, const char *right);"
        )
        .unwrap();
        writeln!(&mut output, "void rt_div_by_zero(void);").unwrap();
        writeln!(&mut output, "void rt_int_overflow(void);").unwrap();
        writeln!(
            &mut output,
            "const char* rt_str_concat(const char *a, const char *b);"
        )
        .unwrap();
        writeln!(&mut output, "char rt_str_eq(const char *a, const char *b);").unwrap();
        writeln!(&mut output, "long long rt_str_len(const char *s);").unwrap();
        writeln!(&mut output, "void* rt_array_alloc(long long len);").unwrap();
        writeln!(
            &mut output,
            "long long rt_array_get(const void *arr, long long index);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "void* rt_array_set(void *arr, long long index, long long value);"
        )
        .unwrap();
        writeln!(&mut output, "long long rt_array_len(const void *arr);").unwrap();
        writeln!(
            &mut output,
            "void* rt_array_push(void *arr, long long value);"
        )
        .unwrap();
        writeln!(&mut output, "void* rt_struct_alloc(long long num_fields);").unwrap();
        writeln!(&mut output, "const char* rt_i64_to_str(long long n);").unwrap();
        writeln!(&mut output, "const char* rt_f64_to_str(double n);").unwrap();
        writeln!(&mut output, "const char* rt_bool_to_str(char b);").unwrap();
        writeln!(&mut output, "void* rt_result_ok(long long value);").unwrap();
        writeln!(&mut output, "void* rt_result_err(long long value);").unwrap();
        writeln!(&mut output, "long long rt_result_tag(const void *result);").unwrap();
        writeln!(
            &mut output,
            "long long rt_result_value(const void *result);"
        )
        .unwrap();
        writeln!(&mut output, "void* rt_option_some(long long value);").unwrap();
        writeln!(&mut output, "void* rt_option_none(void);").unwrap();
        writeln!(&mut output, "long long rt_option_tag(const void *opt);").unwrap();
        writeln!(&mut output, "long long rt_option_value(const void *opt);").unwrap();
        writeln!(
            &mut output,
            "void* rt_str_split(const char *s, const char *sep);"
        )
        .unwrap();
        writeln!(&mut output, "const char* rt_str_trim(const char *s);").unwrap();
        writeln!(&mut output, "const char* rt_str_upper(const char *s);").unwrap();
        writeln!(&mut output, "const char* rt_str_lower(const char *s);").unwrap();
        writeln!(
            &mut output,
            "char rt_str_starts_with(const char *s, const char *prefix);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "char rt_str_ends_with(const char *s, const char *suffix);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "const char* rt_str_replace(const char *s, const char *from, const char *to);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "const char* rt_str_char_at(const char *s, long long index);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "char rt_str_contains(const char *s, const char *sub);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "const char* rt_str_repeat(const char *s, long long count);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "long long rt_str_index_of(const char *s, const char *sub);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "const char* rt_str_join(const char *arr_ptr, const char *sep);"
        )
        .unwrap();
        writeln!(&mut output, "const char* rt_read_line(void);").unwrap();
        writeln!(&mut output, "const char* rt_read_file(const char *path);").unwrap();
        writeln!(
            &mut output,
            "void rt_write_file(const char *path, const char *content);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "long long rt_pow(long long base, long long exp);"
        )
        .unwrap();
        writeln!(&mut output, "double rt_sqrt(double x);").unwrap();
        writeln!(&mut output, "void* rt_hashmap_new(void);").unwrap();
        writeln!(
            &mut output,
            "void rt_hashmap_set(void *map_ptr, const char *key, const char *value);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "const char* rt_hashmap_get(const void *map_ptr, const char *key);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "char rt_hashmap_has(const void *map_ptr, const char *key);"
        )
        .unwrap();
        writeln!(
            &mut output,
            "long long rt_hashmap_len(const void *map_ptr);"
        )
        .unwrap();
        writeln!(&mut output, "void* rt_hashmap_keys(const void *map_ptr);").unwrap();
        writeln!(
            &mut output,
            "void rt_hashmap_remove(void *map_ptr, const char *key);"
        )
        .unwrap();
        writeln!(&mut output, "void rt_retain(void *data_ptr);").unwrap();
        writeln!(&mut output, "void rt_release(void *data_ptr);").unwrap();
        writeln!(&mut output).unwrap();

        // Extern C function declarations (FFI)
        for item in &module.items {
            if let Item::Extern(ext) = &item.node {
                for fn_sig in &ext.functions {
                    let f = &fn_sig.node;
                    let ret_c = match &f.return_type {
                        Some(t) => Self::type_to_c(&t.node),
                        None => "void",
                    };
                    let params_c: Vec<String> = f
                        .params
                        .iter()
                        .map(|p| format!("{} {}", Self::type_to_c(&p.ty.node), p.name))
                        .collect();
                    let params_str = if params_c.is_empty() {
                        "void".to_string()
                    } else {
                        params_c.join(", ")
                    };
                    writeln!(&mut output, "extern {ret_c} {}({params_str});", f.name).unwrap();
                }
            }
        }
        writeln!(&mut output).unwrap();

        // Forward declarations
        for decl in &self.fn_decls {
            writeln!(&mut output, "{decl}").unwrap();
        }
        writeln!(&mut output).unwrap();

        // Function definitions
        for def in &self.fn_defs {
            writeln!(&mut output, "{def}").unwrap();
        }

        output
    }
}

/// Generate C source code from a Turbo AST module.
///
/// Returns `Err` if the program uses a construct the WASM backend cannot
/// lower (unsupported expression, unsupported match pattern, or an unknown
/// struct field). Previously these emitted a literal `0`, which silently
/// miscompiled the program; failing here surfaces a real `CodegenError` to
/// the CLI instead.
pub fn generate_c(module: &turbo_ast::Module) -> Result<String, CodegenError> {
    let mut emitter = CEmitter::new();
    let c = emitter.emit_module(module);
    if let Some(err) = emitter.errors.into_iter().next() {
        return Err(err);
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_to_c(source: &str) -> String {
        let (tokens, lex_errors) = turbo_lexer::tokenize(source);
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        let (module, parse_errors) = turbo_parser::parse(tokens);
        assert!(parse_errors.is_empty(), "parse errors: {:?}", parse_errors);
        generate_c(&module).expect("wasm codegen produced an unsupported-construct error")
    }

    /// Like `compile_to_c` but returns the `Result`, so a test can assert that
    /// an unsupported construct fails loud instead of miscompiling.
    fn try_compile_to_c(source: &str) -> Result<String, CodegenError> {
        let (tokens, lex_errors) = turbo_lexer::tokenize(source);
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        let (module, parse_errors) = turbo_parser::parse(tokens);
        assert!(parse_errors.is_empty(), "parse errors: {:?}", parse_errors);
        generate_c(&module)
    }

    /// Extract the body of `turbo_main` (the renamed `main`) from the
    /// emitted C — keeps assertions focused on the relevant fragment.
    fn turbo_main_body(c: &str) -> String {
        let marker = "turbo_main(void) {";
        let start = c.find(marker).expect("turbo_main not emitted");
        let rest = &c[start + marker.len()..];
        // Match braces to find the body end.
        let mut depth = 1usize;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return rest[..i].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("unterminated turbo_main body in emitted C");
    }

    #[test]
    fn defer_basic_lifo_order() {
        let c = compile_to_c(
            r#"
fn main() {
    print("start")
    defer print("deferred 1")
    defer print("deferred 2")
    print("middle")
    print("end")
}
"#,
        );
        let body = turbo_main_body(&c);

        // All four user prints must appear.
        for needle in [
            "\"start\"",
            "\"middle\"",
            "\"end\"",
            "\"deferred 1\"",
            "\"deferred 2\"",
        ] {
            assert!(
                body.contains(needle),
                "expected `{}` in emitted body:\n{}",
                needle,
                body
            );
        }

        // Ordering: the body prints in source order, then defers fire in
        // LIFO, so "deferred 2" must come before "deferred 1", and both
        // must come after "end".
        let pos_end = body.find("\"end\"").expect("missing end");
        let pos_d1 = body.find("\"deferred 1\"").expect("missing deferred 1");
        let pos_d2 = body.find("\"deferred 2\"").expect("missing deferred 2");
        assert!(pos_end < pos_d2, "end must precede deferred 2");
        assert!(
            pos_d2 < pos_d1,
            "LIFO: deferred 2 must precede deferred 1 in emitted C (got d2={} d1={})",
            pos_d2,
            pos_d1
        );

        // And the old "not supported" placeholder must be gone.
        assert!(
            !c.contains("defer not supported"),
            "stale placeholder still emitted:\n{}",
            c
        );
    }

    #[test]
    fn defer_fires_before_early_return() {
        let c = compile_to_c(
            r#"
fn main() {
    defer print("cleanup")
    print("before")
    return
    print("after")
}
"#,
        );
        let body = turbo_main_body(&c);

        // The deferred print must appear between "before" and the return.
        let pos_before = body.find("\"before\"").expect("missing before");
        let pos_cleanup = body.find("\"cleanup\"").expect("missing cleanup");
        let pos_return = body.find("return;").expect("missing return");
        assert!(
            pos_before < pos_cleanup,
            "defer must fire after preceding stmts"
        );
        assert!(
            pos_cleanup < pos_return,
            "defer must fire before the return (got cleanup={} return={})",
            pos_cleanup,
            pos_return
        );
    }

    #[test]
    fn defer_runs_at_fallthrough() {
        // No explicit return — defer still fires at block end.
        let c = compile_to_c(
            r#"
fn main() {
    print("a")
    defer print("z")
    print("b")
}
"#,
        );
        let body = turbo_main_body(&c);
        let pos_b = body.find("\"b\"").expect("missing b");
        let pos_z = body.find("\"z\"").expect("missing z");
        assert!(pos_b < pos_z, "defer must fire after tail statements");
    }

    #[test]
    fn float_string_paths_use_float_runtime_conversion() {
        let c = compile_to_c(
            r#"
fn main() {
    let radius = 5.0
    let area = 3.14159 * radius * radius
    print(area)
    print("area={area}")
    print(to_str(area))
}
"#,
        );
        let body = turbo_main_body(&c);

        assert!(
            body.contains("rt_print_f64(area);"),
            "print(float) must use the float print runtime:\n{}",
            body
        );
        assert!(
            body.contains("rt_str_concat(\"area=\", rt_f64_to_str(area))"),
            "float interpolation must format through rt_f64_to_str:\n{}",
            body
        );
        let float_to_str_calls = body.matches("rt_f64_to_str(area)").count();
        assert!(
            float_to_str_calls >= 2,
            "interpolation and to_str(float) must both use rt_f64_to_str (got {float_to_str_calls}):\n{}",
            body
        );
        assert!(
            !body.contains("rt_i64_to_str(area)"),
            "float values must not be formatted through integer conversion:\n{}",
            body
        );
    }

    #[test]
    fn closure_direct_lifts_function_and_builds_pair() {
        let c = compile_to_c(
            r#"
fn main() {
    let twice = (x: i64) => x * 2
    print(twice(5))
}
"#,
        );
        // The closure body is lifted to a top-level function taking the env
        // pointer first, then the user parameter.
        assert!(
            c.contains("long long __closure_0(void *env, long long x)"),
            "closure must be lifted to a top-level function:\n{}",
            c
        );
        let body = turbo_main_body(&c);
        // A [fn_ptr, env_ptr] pair is allocated and populated.
        assert!(
            body.contains("rt_struct_alloc(2LL)") && body.contains("(long long)(&__closure_0)"),
            "closure value must be a {{fn_ptr, env_ptr}} pair:\n{}",
            body
        );
        // The call site loads the pair and calls through a function pointer.
        assert!(
            body.contains("(long long(*)(void*, long long))"),
            "call site must cast and call the closure indirectly:\n{}",
            body
        );
    }

    #[test]
    fn closure_capture_builds_env_and_reads_it_back() {
        let c = compile_to_c(
            r#"
fn main() {
    let n = 10
    let add = (x: i64) => x + n
    print(add(5))
}
"#,
        );
        // The lifted function reads the captured variable from the env slot.
        assert!(
            c.contains("long long n = (long long)(((long long*)env)[0]);"),
            "captured variable must be loaded from the environment:\n{}",
            c
        );
        let body = turbo_main_body(&c);
        // The capture site allocates a one-slot env and stores `n` into it.
        assert!(
            body.contains("rt_struct_alloc(1LL)"),
            "env struct for one capture must be allocated:\n{}",
            body
        );
        assert!(
            body.contains("[0] = (long long)(n);"),
            "captured `n` must be stored into the env:\n{}",
            body
        );
    }

    #[test]
    fn closure_str_capture_uses_str_runtime() {
        let c = compile_to_c(
            r#"
fn main() {
    let prefix = "Hello"
    let greet = |name: str| -> str { "{prefix}, {name}!" }
    print(greet("world"))
}
"#,
        );
        // String capture is read back as a const char*.
        assert!(
            c.contains("const char* prefix = (const char*)(((long long*)env)[0]);"),
            "string capture must round-trip as const char*:\n{}",
            c
        );
        // print(greet(...)) must dispatch to the string print runtime, proving
        // the closure's return type is tracked.
        let body = turbo_main_body(&c);
        assert!(
            body.contains("rt_print_str("),
            "a str-returning closure call must print via rt_print_str:\n{}",
            body
        );
    }

    #[test]
    fn map_lowers_to_indirect_call_loop() {
        let c = compile_to_c(
            r#"
fn main() {
    let nums = [1, 2, 3]
    let doubled = map(nums, (x) => x * 2)
    print(doubled[0])
}
"#,
        );
        let body = turbo_main_body(&c);
        assert!(
            body.contains("rt_array_alloc(") && body.contains("rt_array_set("),
            "map must allocate a result array and store into it:\n{}",
            body
        );
        assert!(
            body.contains("(long long(*)(void*, long long))"),
            "map must call the closure indirectly per element:\n{}",
            body
        );
        assert!(
            c.contains("long long __closure_0(void *env, long long x)"),
            "the map callback must be lifted to a function:\n{}",
            c
        );
    }

    #[test]
    fn filter_lowers_to_predicate_loop() {
        let c = compile_to_c(
            r#"
fn main() {
    let nums = [1, 2, 3, 4]
    let evens = filter(nums, (x) => x % 2 == 0)
    print(len(evens))
}
"#,
        );
        let body = turbo_main_body(&c);
        // filter keeps a running count and patches the result array length.
        assert!(
            body.contains("rt_array_set(") && body.contains("[0] = "),
            "filter must pack survivors and patch the length slot:\n{}",
            body
        );
        // The predicate returns bool -> char.
        assert!(
            c.contains("char __closure_0(void *env, long long x)"),
            "the filter predicate must be lifted with a char (bool) return:\n{}",
            c
        );
    }

    #[test]
    fn higher_order_user_fn_param_fails_loud() {
        // Closures handed to *user-defined* higher-order functions are out of
        // scope (the call site can't recover the closure signature from a
        // `fn(...)` parameter type) and must fail loud rather than miscompile.
        let err = try_compile_to_c(
            r#"
fn apply(f: fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    print(apply((x: i64) => x + 1, 5))
}
"#,
        )
        .expect_err("higher-order user-function parameters must be unsupported");
        assert_eq!(err.code, ErrorCode::E0403);
        assert!(
            err.message.contains("closures as parameters"),
            "diagnostic should name the unsupported construct: {}",
            err.message
        );
    }
}
